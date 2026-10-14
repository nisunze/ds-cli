#!/usr/bin/env python3
"""Owning host-regression gate: outgoing networking is distinct from exec IPC.

Compute executables retain their stricter zero-network-syscall gate. Rust's
pre_exec supervision uses a local SOCK_SEQPACKET pair to acknowledge exec;
only that exact constructor and its fd-scoped empty EOF read are admitted here.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess

NETWORK = re.compile(r"(?:^|\s)(?:socket|socketpair|connect|bind|listen|accept|accept4|sendto|recvfrom|sendmsg|recvmsg|sendmmsg|recvmmsg|getsockname|getpeername|setsockopt|getsockopt|shutdown)\(")
EXEC_PAIR = re.compile(r"^(\d+)\s+socketpair\(AF_UNIX, SOCK_SEQPACKET\|SOCK_CLOEXEC, 0, \[(\d+), (\d+)\]\) = 0$")
EXEC_EOF = re.compile(r'^([0-9]+)\s+recvfrom\(([0-9]+), "", 8, 0, NULL, NULL\) = 0$')
TOKIO_WAKE = re.compile(r"^\d+\s+socketpair\(AF_UNIX, SOCK_STREAM\|SOCK_CLOEXEC\|SOCK_NONBLOCK, 0, \[\d+, \d+\]\) = 0$")


def classify_network_trace(path: Path) -> dict:
    endpoints: set[tuple[int, int]] = set()
    pairs = eof_reads = wake_pairs = 0
    attempts = []
    for line in path.read_text().splitlines():
        pair = EXEC_PAIR.fullmatch(line)
        if pair:
            pid, first, second = map(int, pair.groups())
            endpoints.update(((pid, first), (pid, second)))
            pairs += 1
            continue
        eof = EXEC_EOF.fullmatch(line)
        if eof and tuple(map(int, eof.groups())) in endpoints:
            eof_reads += 1
            continue
        if TOKIO_WAKE.fullmatch(line):
            wake_pairs += 1
            continue
        if NETWORK.search(line):
            attempts.append(line)
    return {
        "outgoing_network_syscall_attempts": len(attempts),
        "local_rust_exec_socketpairs": pairs,
        "local_rust_exec_empty_eof_reads": eof_reads,
        "local_tokio_wake_socketpairs": wake_pairs,
        "unrecognized_network_syscalls": attempts,
    }


def trace(command: list[str], scratch: Path, label: str) -> subprocess.CompletedProcess:
    scratch.mkdir(parents=True, exist_ok=True)
    path = scratch / (label + ".network.trace")
    if path.exists():
        raise RuntimeError(f"Trace already exists: {path}; use a fresh label")
    environment = os.environ.copy()
    environment["TMPDIR"] = str(scratch)
    for name in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
        environment[name] = "http://127.0.0.1:1"
    environment["NO_PROXY"] = environment["no_proxy"] = ""
    completed = subprocess.run([
        "strace", "-f", "-qq", "-o", str(path), "-e", "trace=network",
        "-e", "inject=socket:error=EPERM", "--", *command,
    ], env=environment, capture_output=True, text=True, timeout=7200)
    (scratch / (label + ".stdout")).write_text(completed.stdout)
    (scratch / (label + ".stderr")).write_text(completed.stderr)
    if not path.exists() or "ptrace" in completed.stderr:
        raise RuntimeError("Syscall tracing unavailable; this is not a pass")
    counts = classify_network_trace(path)
    (scratch / (label + ".network-counts.json")).write_text(json.dumps(counts, indent=2) + "\n")
    if counts["outgoing_network_syscall_attempts"]:
        raise RuntimeError(f"{label} attempted outgoing networking; inspect {path}")
    return completed
