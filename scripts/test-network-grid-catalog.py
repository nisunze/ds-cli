#!/usr/bin/env python3
"""Prove Grid discovery follows the captured offline Network component.

This consumes synthetic owner metadata only. It grants no install/release trust.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('ds', 'engine', 'legacy-engine', 'baseline', 'trace-helper', 'evidence'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    ds, engine, legacy, baseline = [path.resolve(strict=True) for path in (args.ds, args.engine, args.legacy_engine, args.baseline)]
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('owning_host_gate', args.trace_helper.resolve(strict=True))
    gate = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gate)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)
    os.environ['TMPDIR'] = str(evidence)
    os.environ.pop('DS_SRE_REPORTING', None)
    cases = []

    def cli(label, *words, success=True, code=None, calls=2):
        result = gate.trace([str(ds), *words, '--output', 'json'], evidence, label)
        envelope = json.loads(result.stdout)
        if success:
            assert result.returncode == 0 and envelope['status'] == 'ok', envelope
        else:
            assert result.returncode != 0 and envelope['error']['code'] == code, envelope
        counts = gate.classify_network_trace(evidence / (label + '.network.trace'))
        assert counts['local_rust_exec_socketpairs'] == calls, (label, counts)
        cases.append({'case': label, 'exit_code': result.returncode, 'network_counts': counts})
        return envelope

    descriptor = cli('metadata-discovery', 'capabilities', 'dsgrid.describe', calls=0)['data']['command']
    assert descriptor['effect'] == 'discovery' and descriptor['authority'] == 'none'
    for kind in ('operations', 'commands', 'projections', 'types'):
        index = cli(kind, 'dsgrid', 'describe', '--kind', kind)
        assert index['data'] == json.loads((baseline / (kind + '.json')).read_text())['data']
        entry = (baseline / (kind + '-id.txt')).read_text()
        detail = cli(kind + '-descriptor', 'dsgrid', 'describe', '--kind', kind, '--id', entry)
        assert detail['data'] == json.loads((baseline / (kind + '-descriptor.json')).read_text())['data']
    unknown = cli('unknown-descriptor', 'dsgrid', 'describe', '--id', 'unknown', success=False, code='unknown_descriptor')
    assert unknown['error']['class'] == 'invalid_input'
    assert len(unknown['error']['detail']['ids']) == 40
    assert unknown['error']['detail']['withheld'] > 0
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(legacy)
    cli('incompatible-catalog-capability', 'dsgrid', 'describe', success=False, code='network_engine_incompatible', calls=1)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(evidence / 'absent-network')
    cli('missing-addon', 'dsgrid', 'describe', success=False, code='network_engine_missing', calls=0)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)

    wires = []
    calls = [
        ('describe', 'dsgrid.describe', {}),
        ('invoke', 'dsgrid.describe', {'kind': 'operations'}),
        ('invoke', 'dsgrid.describe', {'kind': 'types', 'id': (baseline / 'types-id.txt').read_text()}),
    ]
    for number, (operation, command, arguments) in enumerate(calls, 1):
        wires.append({'jsonrpc': '2.0', 'id': number, 'method': 'tools/call', 'params': {'name': 'ds_grid_model', 'arguments': {'operation': operation, 'command': command, 'arguments': arguments}}})
    trace = evidence / 'mcp.network.trace'
    result = subprocess.run(['strace', '-f', '-qq', '-o', str(trace), '-e', 'trace=network', '-e', 'inject=socket:error=EPERM', '--', str(ds), 'mcp', 'serve'], input=''.join(json.dumps(wire) + '\n' for wire in wires), text=True, capture_output=True, timeout=120)
    (evidence / 'mcp.stdout').write_text(result.stdout)
    (evidence / 'mcp.stderr').write_text(result.stderr)
    assert result.returncode == 0 and 'ptrace' not in result.stderr, result.stderr
    responses = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    assert len(responses) == len(wires), responses
    envelopes = [response['result']['structuredContent'] for response in responses]
    assert envelopes[0]['data']['command'] == descriptor
    assert envelopes[1]['data'] == json.loads((baseline / 'operations.json').read_text())['data']
    assert envelopes[2]['data'] == json.loads((baseline / 'types-descriptor.json').read_text())['data']
    counts = gate.classify_network_trace(trace)
    assert counts['outgoing_network_syscall_attempts'] == 0 and counts['local_rust_exec_socketpairs'] == 4, counts
    cases.append({'case': 'mcp-metadata-and-catalog', 'native_calls': 4, 'network_counts': counts})
    control = subprocess.run(['strace', '-f', '-qq', '-o', str(evidence / 'socket-control.network.trace'), '-e', 'trace=network', '-e', 'inject=socket:error=EPERM', '--', sys.executable, '-c', 'import socket; socket.socket(socket.AF_INET,socket.SOCK_STREAM)'], capture_output=True, text=True)
    control_counts = gate.classify_network_trace(evidence / 'socket-control.network.trace')
    assert control.returncode != 0 and control_counts['outgoing_network_syscall_attempts'] == 1, control_counts
    acceptance = {'development_only': True, 'matching_artifacts': {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [('ds', ds), ('network', engine), ('legacy_network', legacy)]}, 'cases': cases, 'socket_negative_control': control_counts, 'project_data_reads': 0, 'installed_release_acceptance': False}
    (evidence / 'acceptance.json').write_text(json.dumps(acceptance, indent=2) + '\n')
    print(json.dumps({'actual_cases': len(cases), 'outgoing_calls': 0, 'project_data_reads': 0}))


if __name__ == '__main__':
    main()
