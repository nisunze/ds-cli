#!/usr/bin/env python3
"""Prove one existing revision-gated Grid apply through the offline owner.

Only immutable synthetic fixtures and private regression outputs are consumed.
"""
import argparse
import ast
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('ds', 'engine', 'legacy-engine', 'model', 'fixtures', 'baseline', 'trace-helper', 'evidence'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    ds, engine, legacy, model, fixtures, baseline = [path.resolve(strict=True) for path in (args.ds, args.engine, args.legacy_engine, args.model, args.fixtures, args.baseline)]
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('owning_host_gate', args.trace_helper.resolve(strict=True))
    gate = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gate)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)
    os.environ['TMPDIR'] = str(evidence)
    os.environ.pop('DS_SRE_REPORTING', None)
    before = {name: digest(path) for name, path in [('ds', ds), ('network', engine), ('model', model)]}
    envelope = fixtures / 'envelope.json'
    cases = []

    def trace(label, words, input_text=None):
        path = evidence / (label + '.network.trace')
        raw_path = evidence / (label + '.syscall.trace')
        assert not path.exists() and not raw_path.exists(), path
        environment = os.environ.copy()
        for name in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy'):
            environment[name] = 'http://127.0.0.1:1'
        environment['NO_PROXY'] = environment['no_proxy'] = ''
        result = subprocess.run([
            'strace', '-f', '-qq', '-s', '4096', '-o', str(raw_path),
            '-e', 'trace=network,execve', '-e', 'inject=socket:error=EPERM', '--',
            str(ds), *words,
        ], env=environment, input=input_text, text=True, capture_output=True, timeout=120)
        (evidence / (label + '.stdout')).write_text(result.stdout)
        (evidence / (label + '.stderr')).write_text(result.stderr)
        assert raw_path.exists() and 'ptrace' not in result.stderr, result.stderr
        # Exec and its parent acknowledgement can interleave in strace. Join
        # only matching PID/syscall fragments; retain unfinished calls so the
        # owning network classifier still refuses unrecognized attempts.
        completed, pending = [], {}
        for line in raw_path.read_text().splitlines():
            unfinished = re.fullmatch(r'(\d+)\s+(\w+)\(.* <unfinished \.\.\.>', line)
            resumed = re.fullmatch(r'(\d+)\s+<\.\.\. (\w+) resumed>(.*)', line)
            if unfinished:
                assert unfinished[1] not in pending, line
                pending[unfinished[1]] = (unfinished[2], line.split(' <unfinished ...>')[0])
            elif resumed:
                operation, beginning = pending.pop(resumed[1])
                assert operation == resumed[2], line
                completed.append(beginning + resumed[3])
            else:
                completed.append(line)
        completed.extend(beginning for operation, beginning in pending.values())
        path.write_text('\n'.join(completed) + '\n')
        counts = gate.classify_network_trace(path)
        assert counts['outgoing_network_syscall_attempts'] == 0, counts
        tasks = {}
        for line in path.read_text().splitlines():
            match = re.fullmatch(r'\d+\s+execve\(("(?:\\.|[^"\\])*"), \[(.*?)\], .*\)\s+= 0', line)
            if match and ast.literal_eval(match[1]) == str(environment['DS_NETWORK_ENGINE_BIN']):
                argv = [ast.literal_eval(value) for value in re.findall(r'"(?:\\.|[^"\\])*"', match[2])]
                assert len(argv) >= 2, argv
                tasks[argv[1]] = tasks.get(argv[1], 0) + 1
        counts['network_engine_tasks'] = tasks
        (evidence / (label + '.network-counts.json')).write_text(json.dumps(counts, indent=2) + '\n')
        return result, counts

    def cli(label, *words, success=True, code=None, calls=3):
        result, counts = trace(label, [*words, '--output', 'json'])
        answer = json.loads(result.stdout)
        if success:
            assert result.returncode == 0 and answer['status'] == 'ok', answer
        else:
            assert result.returncode != 0 and answer['error']['code'] == code, answer
        assert counts['local_rust_exec_socketpairs'] == calls, (label, counts)
        assert counts['network_engine_tasks'] == {
            0: {},
            2: {'build-info': 1, 'grid-inspect': 1},
            3: {'build-info': 1, 'grid-inspect': 1, 'grid-apply': 1},
        }[calls], (label, counts)
        cases.append({'case': label, 'exit_code': result.returncode, 'network_counts': counts})
        return answer

    descriptor = cli('metadata-discovery', 'capabilities', 'dsgrid.apply', calls=0)['data']['command']
    assert descriptor['effect'] == 'local_file_write' and descriptor['authority'] == 'none'
    preview = cli('preview', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(envelope), '--dry-run')
    assert preview == json.loads((baseline / 'preview.json').read_text())
    destination = evidence / 'model-applied.dsgrid'
    applied = cli('apply', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(envelope), '--out', str(destination))
    expected = json.loads((baseline / 'apply.json').read_text())
    expected['data']['artifact']['path'] = str(destination)
    assert applied == expected
    assert destination.read_bytes() == (fixtures / 'model-applied.dsgrid').read_bytes()
    assert applied['data']['persisted'] is True and preview['data']['persisted'] is False
    cli('output-exists', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(envelope), '--out', str(destination), success=False, code='output_exists', calls=0)
    cli('output-required', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(envelope), success=False, code='output_required', calls=0)
    stale = json.loads(envelope.read_text())
    stale['expected_revision'] = 'rev:' + '0' * 64
    stale_path = evidence / 'stale.json'
    stale_path.write_text(json.dumps(stale))
    answer = cli('stale-revision', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(stale_path), '--out', str(evidence / 'refused.dsgrid'), success=False, code='revision_conflict')
    assert answer['error']['class'] == 'conflict' and not (evidence / 'refused.dsgrid').exists()
    assert answer['error']['detail']['actual_revision'] == preview['data']['source']['authored_revision']
    bad = evidence / 'invalid.json'
    bad.write_text('{}')
    cli('invalid-envelope', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(bad), '--dry-run', success=False, code='envelope_invalid')
    admin = json.loads(envelope.read_text())
    admin['command'] = {'command_kind': 'refresh_rwanda_admin', 'authority_sha256': 'unverified', 'rows': [], 'preview_token': 'unverified', 'accept_existing_conflicts': False}
    admin_path = evidence / 'admin.json'
    admin_path.write_text(json.dumps(admin))
    cli('admin-authority', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(admin_path), '--dry-run', success=False, code='admin_authority_required')
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(legacy)
    cli('incompatible-apply-capability', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(envelope), '--dry-run', success=False, code='network_engine_incompatible', calls=2)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(evidence / 'absent-network')
    cli('missing-addon', 'dsgrid', 'apply', '--model', str(model), '--envelope', str(envelope), '--dry-run', success=False, code='network_engine_missing', calls=0)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)
    wires = []
    for number, (operation, arguments) in enumerate([
        ('describe', {}),
        ('invoke', {'model': str(model), 'envelope': str(envelope), 'dry-run': True}),
        ('invoke', {'model': str(model), 'envelope': str(stale_path), 'dry-run': True}),
    ], 1):
        wires.append({'jsonrpc': '2.0', 'id': number, 'method': 'tools/call', 'params': {'name': 'ds_grid_model', 'arguments': {'operation': operation, 'command': 'dsgrid.apply', 'arguments': arguments}}})
    result, counts = trace('mcp', ['mcp', 'serve'], ''.join(json.dumps(wire) + '\n' for wire in wires))
    assert result.returncode == 0 and 'ptrace' not in result.stderr, result.stderr
    responses = {row['id']: row['result']['structuredContent'] for line in result.stdout.splitlines() if (row := json.loads(line)).get('id') in (1, 2, 3)}
    assert responses[1]['data']['command'] == descriptor
    assert responses[2] == preview
    assert responses[3]['error'] == answer['error']
    assert counts['outgoing_network_syscall_attempts'] == 0 and counts['local_rust_exec_socketpairs'] == 6, counts
    assert counts['network_engine_tasks'] == {'build-info': 2, 'grid-inspect': 2, 'grid-apply': 2}, counts
    cases.append({'case': 'mcp-discovery-preview-conflict', 'network_counts': counts})
    assert before == {name: digest(path) for name, path in [('ds', ds), ('network', engine), ('model', model)]}
    (evidence / 'acceptance.json').write_text(json.dumps({'development_only': True, 'matching_artifacts': before, 'cases': cases, 'project_data_reads': 0, 'installed_release_acceptance': False}, indent=2) + '\n')
    print(json.dumps({'actual_cases': len(cases), 'outgoing_calls': 0, 'unchanged_source': True}))


if __name__ == '__main__':
    main()
