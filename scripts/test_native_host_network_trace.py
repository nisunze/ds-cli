#!/usr/bin/env python3
"""Adversarial trace classifier tests; never relax the compute executable gate."""
from pathlib import Path
import tempfile
import unittest

from native_host_network_trace import classify_network_trace


class HostTraceTests(unittest.TestCase):
    def count(self, lines):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace"
            path.write_text("\n".join(lines))
            return classify_network_trace(path)

    def test_exact_local_exec_constructor_and_its_eof(self):
        result = self.count([
            "10 socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0, [9, 10]) = 0",
            '10 recvfrom(9, "", 8, 0, NULL, NULL) = 0',
        ])
        self.assertEqual(result["outgoing_network_syscall_attempts"], 0)
        self.assertEqual(result["local_rust_exec_socketpairs"], 1)
        self.assertEqual(result["local_rust_exec_empty_eof_reads"], 1)

    def test_eof_requires_the_same_process_and_admitted_fd(self):
        result = self.count([
            "10 socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0, [9, 10]) = 0",
            '11 recvfrom(9, "", 8, 0, NULL, NULL) = 0',
            '10 recvfrom(12, "", 8, 0, NULL, NULL) = 0',
        ])
        self.assertEqual(result["outgoing_network_syscall_attempts"], 2)

    def test_payload_or_different_receive_is_not_exec_eof(self):
        result = self.count([
            "10 socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0, [9, 10]) = 0",
            '10 recvfrom(9, "payload", 8, 0, NULL, NULL) = 7',
            '10 recvfrom(9, "", 4, 0, NULL, NULL) = 0',
        ])
        self.assertEqual(result["outgoing_network_syscall_attempts"], 2)

    def test_unix_connections_and_internet_calls_always_refuse(self):
        result = self.count([
            "10 socket(AF_INET, SOCK_STREAM, 0) = -1 EPERM (Operation not permitted)",
            '10 connect(9, {sa_family=AF_INET, sin_port=htons(443)}, 16) = -1 EPERM',
            "10 socket(AF_UNIX, SOCK_STREAM, 0) = -1 EPERM",
            '10 connect(9, {sa_family=AF_UNIX, sun_path="/run/service.sock"}, 32) = -1 EPERM',
        ])
        self.assertEqual(result["outgoing_network_syscall_attempts"], 4)

    def test_only_the_existing_tokio_local_wake_pair_is_admitted(self):
        result = self.count([
            "10 socketpair(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC|SOCK_NONBLOCK, 0, [9, 10]) = 0",
            "10 socketpair(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC, 0, [9, 10]) = 0",
        ])
        self.assertEqual(result["local_tokio_wake_socketpairs"], 1)
        self.assertEqual(result["outgoing_network_syscall_attempts"], 1)


if __name__ == "__main__":
    unittest.main()
