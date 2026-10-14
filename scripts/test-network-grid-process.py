#!/usr/bin/env python3
"""Prove CLI/MCP Grid transport against an exact offline native Network artifact.

Uses public commands and immutable owner fixtures. Development identities remain
unqualified; this regression does not grant installation or release trust.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import select
import shutil
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('ds', 'engine', 'fixtures', 'baseline', 'trace-helper', 'evidence'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--fixture-exe', required=True, type=Path, help='built protocol-only process fixture; no engineering acceptance is claimed from it')
    args = parser.parse_args()
    ds, engine, fixtures, baseline = [p.resolve(strict=True) for p in (args.ds, args.engine, args.fixtures, args.baseline)]
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('owning_host_gate', args.trace_helper.resolve(strict=True))
    gate = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gate)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)
    os.environ['TMPDIR'] = str(evidence)
    os.environ.pop('DS_SRE_REPORTING', None)
    model = fixtures / 'model.dsgrid'
    before = {'ds': digest(ds), 'engine': digest(engine), 'model': digest(model)}
    cases = []

    def traced(label, command, success=True, code=None):
        completed = gate.trace(command, evidence, label)
        envelope = json.loads(completed.stdout)
        if success:
            assert completed.returncode == 0 and envelope['status'] == 'ok', envelope
        else:
            assert completed.returncode != 0 and envelope['error']['code'] == code, envelope
        counts = gate.classify_network_trace(evidence / (label + '.network.trace'))
        cases.append({'case': label, 'exit_code': completed.returncode, 'network_counts': counts})
        return envelope

    def cli(label, *words, **expected):
        return traced(label, [str(ds), *words, '--output', 'json'], **expected)

    descriptor = cli('run-discovery', 'capabilities', 'dsgrid.run')['data']['command']
    assert descriptor['authority'] == 'none' and descriptor['effect'] == 'read_only'
    assert gate.classify_network_trace(evidence / 'run-discovery.network.trace')['local_rust_exec_socketpairs'] == 0, 'discovery must not probe any addon'
    for label, words in [
        ('plan', ['dsgrid', 'run', '--model', str(model), '--operation', 'project_plan']),
        ('cheap-inspect', ['dsgrid', 'inspect', '--model', str(model)]),
        ('full-inspect', ['dsgrid', 'inspect', '--model', str(model), '--include', 'tables', '--include', 'members', '--include', 'library', '--include', 'extent', '--include', 'authored-revision']),
    ]:
        answer = cli(label, *words)
        expected = json.loads((baseline / (label + '-qualified.stdout')).read_text())
        assert answer == expected, f'{label}: installed pre-extraction projection changed'
        expected_calls = 3 if label == 'full-inspect' else 2
        assert gate.classify_network_trace(evidence / (label + '.network.trace'))['local_rust_exec_socketpairs'] == expected_calls, f'{label}: duplicate or absent native calls'
    assert json.loads((evidence / 'cheap-inspect.stdout').read_text())['data']['decoded'] is False
    assert json.loads((evidence / 'full-inspect.stdout').read_text())['data']['decoded'] is True
    # Catalog serves this explicit regression consumer; no polling or project read.
    catalog = cli('catalog', 'dsgrid', 'describe', '--kind', 'operations')['data']['entries']
    admitted = [row['id'] for row in catalog if row['effect'] in ('read', 'solve', 'propose') and not row['journaled']]
    assert len(admitted) == 39, admitted
    workers = len(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else os.cpu_count()
    for index, operation in enumerate(admitted):
        params = {}
        if operation in ('plan_whole_model_spotting', 'plan_optimum_spotting_batch'):
            # A fixed host allocation avoids comparing time-varying RAM admission.
            params = {'memory_budget_bytes': 104857600, 'max_workers': 1}
        params_path = evidence / f'params-{index}.json'
        params_path.write_text(json.dumps(params))
        request = evidence / f'request-{index}.json'
        request.write_text(json.dumps({'schema':'ds.grid.native-query/v1', 'source':{'package_sha256': before['model']}, 'operation_id':operation, 'params':params}))
        out = evidence / f'owner-{index}'
        out.mkdir()
        direct = gate.trace([str(engine), 'grid-query', '--request', str(request), '--model', str(model), '--out', str(out), '--workers', str(workers)], evidence, f'owner-{index}')
        result = gate.trace([str(ds), 'dsgrid', 'run', '--model', str(model), '--operation', operation, '--params', str(params_path), '--limit', '10000', '--output', 'json'], evidence, f'query-{index}')
        owner = json.loads(direct.stdout)
        envelope = json.loads(result.stdout)
        if direct.returncode == 0:
            assert result.returncode == 0, envelope
            complete = json.loads((out / 'query.json').read_text())
            assert envelope['data']['result'] == complete['result'], operation
            assert envelope['data']['source']['authored_revision'] == complete['source']['expected_revision']
            assert envelope['data']['source']['package_sha256'] == 'sha256:' + before['model']
            assert envelope['data']['staged'] is False and envelope['data']['persisted'] is False
        else:
            refusal = envelope['error']
            assert result.returncode != 0 and refusal['code'] == owner['code'], (operation, owner, envelope)
            assert refusal.get('detail') == owner.get('detail'), (operation, owner, envelope)
            assert refusal.get('remedy') == owner.get('remedy'), (operation, owner, envelope)
            assert refusal['class'] == ('invalid_input' if owner.get('invalidInput') else 'failed')
        cases.append({'case':'owning-query-' + operation, 'owner_exit_code':direct.returncode, 'cli_exit_code':result.returncode,
            'network_counts':gate.classify_network_trace(evidence / f'query-{index}.network.trace')})

    bad_params = evidence / 'unknown-params.json'
    bad_params.write_text('{"unowned":true}')
    cli('unknown-params', 'dsgrid', 'run', '--model', str(model), '--operation', 'project_plan', '--params', str(bad_params), success=False, code='params_invalid')
    cli('unknown-operation', 'dsgrid', 'run', '--model', str(model), '--operation', 'unowned', success=False, code='unknown_operation')
    cli('journaled-operation', 'dsgrid', 'run', '--model', str(model), '--operation', 'create_alignment', success=False, code='operation_not_read_only')
    missing = evidence / 'absent-network'
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(missing)
    cli('missing-addon', 'dsgrid', 'run', '--model', str(model), '--operation', 'project_plan', success=False, code='network_engine_missing')
    assert gate.classify_network_trace(evidence / 'missing-addon.network.trace')['local_rust_exec_socketpairs'] == 0, 'missing addons must not fall through to another engine'
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)

    wires = []
    for number, (command, arguments) in enumerate([
        ('dsgrid.run', {'model':str(model), 'operation':'project_plan'}),
        ('dsgrid.inspect', {'model':str(model), 'include':['library','extent','authored-revision']}),
        ('dsgrid.run', {'model':str(model), 'operation':'project_plan', 'params':str(bad_params)}),
    ], 1):
        wires.append({'jsonrpc':'2.0','id':number,'method':'tools/call','params':{'name':'ds_grid_model','arguments':{'operation':'invoke','command':command,'arguments':arguments}}})
    trace_path = evidence / 'mcp.network.trace'
    completed = subprocess.run(['strace','-f','-qq','-o',str(trace_path),'-e','trace=network','-e','inject=socket:error=EPERM','--',str(ds),'mcp','serve'],
        input=''.join(json.dumps(wire)+'\n' for wire in wires), text=True, capture_output=True, timeout=120)
    (evidence / 'mcp.stdout').write_text(completed.stdout)
    (evidence / 'mcp.stderr').write_text(completed.stderr)
    assert 'ptrace' not in completed.stderr
    counts = gate.classify_network_trace(trace_path)
    assert counts['outgoing_network_syscall_attempts'] == 0, counts
    responses = {row['id']: row['result']['structuredContent'] for line in completed.stdout.splitlines() if (row := json.loads(line)).get('id') in (1,2,3)}
    assert responses[1] == json.loads((evidence / 'plan.stdout').read_text())
    assert responses[2]['data']['decoded'] is True
    assert responses[2]['data']['library'] == json.loads((evidence / 'full-inspect.stdout').read_text())['data']['library']
    assert responses[3]['error'] == json.loads((evidence / 'unknown-params.stdout').read_text())['error']
    cases.append({'case':'mcp-native-query-inspection-refusal-parity', 'network_counts':counts})
    # A controlled protocol fixture holds its terminal receipt for one second.
    # This proves the real CLI forwards progress while its child remains alive,
    # independent of whether a small engineering fixture finishes in microseconds.
    live_fixture = evidence / ('live-progress.exe' if os.name == 'nt' else 'live-progress')
    shutil.copy2(args.fixture_exe.resolve(strict=True), live_fixture)
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(live_fixture)
    live_trace = evidence / 'live-progress.network.trace'
    process = subprocess.Popen(['strace','-f','-qq','-o',str(live_trace),'-e','trace=network','-e','inject=socket:error=EPERM','--',
        str(ds),'dsgrid','run','--model',str(model),'--operation','project_plan','--progress','--output','json'],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    assert select.select([process.stderr], [], [], 10)[0], 'no live progress before the deadline'
    first = process.stderr.readline()
    assert json.loads(first)['progress'] == {'fixture':'before-terminal'}
    assert process.poll() is None, 'progress was buffered until process completion'
    assert not select.select([process.stdout], [], [], 0)[0], 'terminal answer preceded progress'
    stdout, stderr = process.communicate(timeout=20)
    assert process.returncode == 0 and json.loads(stdout)['status'] == 'ok'
    (evidence / 'live-progress.stdout').write_text(stdout)
    (evidence / 'live-progress.stderr').write_text(first + stderr)
    live_counts = gate.classify_network_trace(live_trace)
    assert live_counts['outgoing_network_syscall_attempts'] == 0, live_counts
    cases.append({'case':'real-cli-live-progress-before-terminal-protocol-fixture', 'network_counts':live_counts})
    os.environ['DS_NETWORK_ENGINE_BIN'] = str(engine)
    assert before == {'ds':digest(ds),'engine':digest(engine),'model':digest(model)}, 'captured artifacts or source changed'
    (evidence / 'acceptance.json').write_text(json.dumps({'artifacts':before, 'development_only':True, 'owning_operations':len(admitted), 'cases':cases}, indent=2)+'\n')
    print(f'Grid native CLI/MCP acceptance passed: {len(cases)} cases, {len(admitted)} owner operations, zero outgoing calls')


if __name__ == '__main__':
    main()
