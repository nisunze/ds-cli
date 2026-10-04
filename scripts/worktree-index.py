#!/usr/bin/env python3
"""Compact live worktree queue. Completed source history belongs in Git."""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

WORKSPACE = Path('/home/magese/data-solutions')
STATES = ('working', 'gating', 'ready_for_integration', 'integrating',
          'followup', 'blocked', 'needs_recovery', 'artifact_preserved',
          'protected', 'consumed')


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def compact(entry):
    fields = ('owner', 'purpose', 'state', 'next_action', 'unlanded', 'dirty',
              'commits', 'gates', 'receipts')
    return {key: entry[key] for key in fields if key in entry}


def update_counts(entry, observation):
    rows = observation.get('repositories', {}).values()
    entry['unlanded'] = sum(row.get('commits_not_in_observed_origin_run') or 0 for row in rows)
    entry['dirty'] = any(not row['head_sha'] or row.get('tracked_changes') or
                         row.get('untracked_source') for row in rows)


def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True, stderr=subprocess.PIPE).strip()


def evidence(filename):
    p = Path(filename).resolve(strict=True)
    assert p.is_file(), f'not a receipt file: {p}'
    with p.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    return {'path': str(p), 'sha256': digest}


def observe(folder):
    repos = {}
    for p in sorted(folder.iterdir()):
        if not p.is_dir() or not (p / '.git').exists():
            continue
        try:
            head = git(p, 'rev-parse', '--verify', 'HEAD')
        except subprocess.CalledProcessError:
            repos[p.name] = {'path': str(p), 'head_sha': None,
                            'attention': 'Git directory has no committed HEAD; ownership audit required',
                            'tracked_changes': []}
            continue
        try:
            origin = git(p, 'rev-parse', 'origin/run')
        except subprocess.CalledProcessError:
            origin = None
        untracked = git(p, 'ls-files', '--others', '--exclude-standard').splitlines()
        source = [name for name in untracked if name.split('/', 1)[0] not in
                  ('out', 'target', 'node_modules', '.ds-run-cache', '.svelte-kit')]
        repos[p.name] = {'path': str(p), 'head_sha': head,
                        'branch': git(p, 'branch', '--show-current') or '(detached)',
                        'origin_run_sha_observed': origin,
                        'commits_not_in_observed_origin_run': int(git(p, 'rev-list', '--count', f'{origin}..{head}')) if origin else None,
                        'tracked_changes': git(p, 'status', '--porcelain', '--untracked-files=no').splitlines(),
                        'untracked_source': source}
    return {'observed_at_utc': utc(), 'exists': True, 'repositories': repos}


def write_atomic(index, value):
    # The temporary file is on the same physical disk as the index, not /tmp.
    fd, temporary = tempfile.mkstemp(prefix='.gittrees-index-', dir=index.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            stream.write('{\n')
            for key in ('schema_version', 'updated_at_utc', 'cap'):
                stream.write(f'  {json.dumps(key)}: {json.dumps(value[key])},\n')
            stream.write('  "entries": {\n')
            rows = sorted(value['entries'].items())
            for i, (name, entry) in enumerate(rows):
                comma = ',' if i + 1 < len(rows) else ''
                stream.write(f'    {json.dumps(name)}: {json.dumps(entry, ensure_ascii=False)}{comma}\n')
            stream.write('  },\n  "other_folders": ')
            stream.write(json.dumps(value['other_folders'], ensure_ascii=False))
            stream.write('\n}\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, index)
        directory = os.open(index.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', nargs='?', default='report', choices=['scan', 'record', 'summary', 'report'])
    parser.add_argument('--index', type=Path, default=WORKSPACE / 'git_trees/gittrees-index.json')
    parser.add_argument('--lane')
    parser.add_argument('--owner')
    parser.add_argument('--purpose')
    parser.add_argument('--state', choices=STATES)
    parser.add_argument('--next-action')
    parser.add_argument('--commit', action='append', default=[], metavar='REPO=SHA')
    parser.add_argument('--gate', action='append', default=[], metavar='RECEIPT')
    parser.add_argument('--receipt', action='append', default=[])
    parser.add_argument('--main', action='store_true', help='Main may assign recovery/follow-up ownership.')
    args = parser.parse_args()
    assert not args.index.is_symlink()
    index = args.index.resolve()
    assert index.parent.is_dir()
    with (index.parent / '.gittrees-index.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        raw = index.read_bytes() if index.exists() else b''
        previous = json.loads(raw) if raw.strip() else {}
        assert previous.get('schema_version', 1) == 1
        document = {'schema_version': 1, 'cap': previous.get('cap', 4),
                    'entries': {name: compact(entry) for name, entry in previous.get('entries', {}).items()
                                if entry.get('state') not in ('consumed', 'artifact_preserved', 'protected')},
                    'other_folders': {'evidence': [], 'protected': []}}
        entries = document['entries']
        if args.action == 'scan':
            assert args.main, 'inventory belongs to main'
        # Counts are refreshed on every operation; detailed inventories stay local.
        for folder in sorted(index.parent.iterdir()):
                if not folder.is_dir() or folder.name.startswith('.'):
                    continue
                # The protected mirror is recorded by name; never inspect it.
                if folder.name == 'github-exit':
                    document['other_folders']['protected'].append(folder.name)
                    continue
                observation = observe(folder)
                if not observation['repositories']:
                    document['other_folders']['evidence'].append(folder.name)
                    entries.pop(folder.name, None)
                    continue
                entry = entries.setdefault(folder.name, {
                    'owner': 'unassigned', 'purpose': 'ownership audit required',
                    'state': 'needs_recovery',
                    'next_action': 'main assigns a named owner; preserve all source and evidence'})
                update_counts(entry, observation)
                if entry['state'] == 'ready_for_integration' and (entry['dirty'] or any(
                        observation['repositories'].get(repo, {}).get('head_sha') != sha
                        for repo, sha in entry.get('commits', {}).items())):
                    entry.update(state='blocked', next_action='Source changed after gates; rerun owning gates')
        for name, entry in entries.items():
            if not (index.parent / name).exists():
                entry.update(state='blocked', next_action='Missing path: main verifies landing before forgetting')
        if args.action == 'record':
            assert args.lane and re.fullmatch(r'[A-Za-z0-9_-]+', args.lane)
            assert args.lane != 'github-exit', 'protected mirror'
            assert args.owner and args.state and args.next_action
            folder = index.parent / args.lane
            entry = entries.get(args.lane, {})
            assert args.main or entry.get('owner') in (None, 'unassigned', args.owner), 'another owner holds this tree'
            receipts = [evidence(p) for p in args.receipt]
            gates = [evidence(p) for p in args.gate]
            commits = {}
            for item in args.commit:
                repo, sha = item.split('=', 1)
                assert re.fullmatch(r'[A-Za-z0-9_-]+', repo) and re.fullmatch(r'[a-f0-9]{40}', sha)
                commits[repo] = sha
            if args.state == 'consumed':
                assert args.main and not folder.exists() and receipts
                landed = commits or entry.get('commits', {})
                assert landed, 'consumption requires recorded landed commits'
                for repo, sha in landed.items():
                    canonical = index.parent.parent / repo
                    for ref in ('run', 'origin/run'):
                        subprocess.run(['git', '-C', str(canonical), 'merge-base', '--is-ancestor', sha, ref], check=True)
                entries.pop(args.lane, None)
            else:
                assert folder.is_dir(), 'live tree path missing'
                observation = observe(folder)
                assert observation['repositories'], 'evidence directories are not worktrees'
                for repo, sha in commits.items():
                    assert repo in observation['repositories']
                    assert git(folder / repo, 'rev-parse', sha + '^{commit}') == sha
                if args.state == 'ready_for_integration':
                    assert gates and commits, 'ready requires exact commits and gate receipts'
                    assert all(row['head_sha'] for row in observation['repositories'].values()), 'ready tree has a repository without committed HEAD'
                    assert all(not row['tracked_changes'] for row in observation['repositories'].values()), 'ready tree has uncommitted tracked source'
                    assert all(not row.get('untracked_source') for row in observation['repositories'].values()), 'ready tree has uncommitted new source'
                    assert all(observation['repositories'][repo]['head_sha'] == sha for repo, sha in commits.items()), 'ready commit must match gated HEAD'
                update_counts(entry, observation)
            entry.update({'owner': args.owner,
                          'purpose': args.purpose or entry.get('purpose') or 'purpose required',
                          'state': args.state, 'next_action': args.next_action})
            if commits:
                entry['commits'] = commits
            if gates:
                entry['gates'] = gates
            if receipts:
                entry['receipts'] = receipts
            if args.state != 'consumed':
                entries[args.lane] = compact(entry)
        summary = {'live_git_folders': len(entries),
            'cap': document['cap'],
            'over_cap': len(entries) > document['cap'],
            'active': [k for k, e in entries.items() if e['state'] in ('working', 'gating', 'ready_for_integration', 'integrating', 'followup')],
            'ready_for_main': [k for k, e in entries.items() if e['state'] == 'ready_for_integration'],
            'needs_owner': [k for k, e in entries.items() if e['owner'] == 'unassigned'],
            'recovery': [k for k, e in entries.items() if e['state'] in ('needs_recovery', 'blocked')],
            'other_folders': document['other_folders']}
        if args.action not in ('summary', 'report'):
            document['updated_at_utc'] = utc()
            write_atomic(index, document)
        if args.action == 'report':
            print(f'Live Git folders: {summary["live_git_folders"]} / cap {summary["cap"]}')
            for name in summary['active']:
                e = entries[name]
                print(f'{name}: {e["purpose"]} — {e["state"]} ({e["owner"]})')
            print(f'Recovery: {len(summary["recovery"])} folders; ready to integrate: {len(summary["ready_for_main"])}')
            print(f'Evidence only: {len(summary["other_folders"]["evidence"])}; protected: {len(summary["other_folders"]["protected"])}')
        else:
            print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
