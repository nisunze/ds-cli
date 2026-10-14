"""Adversarial controls for the host's offline proof classifier."""
import tempfile
import unittest
from pathlib import Path

from native_host_network_trace import classify_network_trace


class TraceControls(unittest.TestCase):
    def classify(self, text):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace"
            path.write_text(text)
            return classify_network_trace(path)

    def test_interleaved_exec_eof_does_not_hide_outgoing_socket(self):
        result = self.classify(
            '10 socketpair(AF_UNIX, SOCK_SEQPACKET|SOCK_CLOEXEC, 0, [9, 10]) = 0\n'
            '10 recvfrom(9 <unfinished ...>\n'
            '20 socket(AF_INET, SOCK_STREAM, IPPROTO_IP) = -1 EPERM\n'
            '10 <... recvfrom resumed>, "", 8, 0, NULL, NULL) = 0\n'
        )
        self.assertEqual(result["local_rust_exec_empty_eof_reads"], 1)
        self.assertEqual(result["outgoing_network_syscall_attempts"], 1)

    def test_empty_read_on_unadmitted_descriptor_is_not_exec(self):
        result = self.classify(
            '10 recvfrom(9 <unfinished ...>\n'
            '10 <... recvfrom resumed>, "", 8, 0, NULL, NULL) = 0\n'
        )
        self.assertEqual(result["outgoing_network_syscall_attempts"], 1)
        self.assertEqual(result["local_rust_exec_empty_eof_reads"], 0)

    def test_split_connect_is_still_an_outgoing_attempt(self):
        result = self.classify(
            '10 socket(AF_INET, SOCK_STREAM, IPPROTO_IP) = 3\n'
            '10 connect(3 <unfinished ...>\n'
            '10 <... connect resumed>, {sa_family=AF_INET}, 16) = -1 EPERM\n'
        )
        self.assertEqual(result["outgoing_network_syscall_attempts"], 2)

    def test_unmatched_resumed_connect_fails_closed(self):
        result = self.classify('10 <... connect resumed>, {sa_family=AF_INET}, 16) = 0\n')
        self.assertEqual(result["outgoing_network_syscall_attempts"], 1)

    def test_unfinished_read_fails_closed(self):
        result = self.classify('10 recvfrom(9 <unfinished ...>\n')
        self.assertEqual(result["outgoing_network_syscall_attempts"], 1)


if __name__ == "__main__":
    unittest.main()
