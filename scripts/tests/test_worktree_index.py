"""Exercise registry ownership, concurrent updates and safe forgetting on disk."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'worktree-index.py'


class RegistryTests(unittest.TestCase):
    def setUp(self):
        root = Path(os.environ['WORKTREE_INDEX_TEST_ROOT'])
        root.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix='registry-', dir=root)
        self.workspace = Path(self.temporary.name)
        self.trees = self.workspace / 'git_trees'
        self.trees.mkdir()
        self.index = self.trees / 'gittrees-index.json'
        self.repo = self.workspace / 'ds-cli'
        self.git('init', '-b', 'run', str(self.repo), cwd=self.workspace)
        self.git('config', 'user.email', 'registry-test@example.invalid')
        self.git('config', 'user.name', 'Registry Test')
        (self.repo / 'source').write_text('initial\n')
        self.git('add', 'source')
        self.git('commit', '-m', 'fixture')
        self.git('update-ref', 'refs/remotes/origin/run', 'HEAD')
        self.sha = self.git('rev-parse', 'HEAD').stdout.strip()
        self.receipt = self.workspace / 'gate.json'
        self.receipt.write_text('{"status":"PASS"}\n')

    def tearDown(self):
        self.temporary.cleanup()

    def git(self, *args, cwd=None):
        return subprocess.run(['git', *args], cwd=cwd or self.repo, text=True,
                              capture_output=True, check=True)

    def lane(self, name):
        path = self.trees / name / 'ds-cli'
        self.git('worktree', 'add', '-b', name, str(path), 'run')
        return path

    def command(self, *args):
        return ['python3', str(SCRIPT), '--index', str(self.index), *args]

    def run_tool(self, *args, success=True):
        result = subprocess.run(self.command(*args), text=True, capture_output=True)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def record(self, name, owner='worker', **kwargs):
        args = ['record', '--lane', name, '--owner', owner,
                '--state', kwargs.pop('state', 'working'), '--next-action', 'Gate then integrate']
        for key, value in kwargs.items():
            args.extend(['--' + key.replace('_', '-'), value])
        return self.run_tool(*args)

    def data(self):
        return json.loads(self.index.read_text())

    def test_compact_live_overview_has_no_events_or_inventories(self):
        self.lane('one')
        (self.trees / 'evidence-only').mkdir()
        (self.trees / 'github-exit').mkdir()
        self.record('one', purpose='Prints')
        data = self.data()
        self.assertNotIn('events', data)
        self.assertNotIn('observation', data['entries']['one'])
        self.assertEqual(data['other_folders'], {'evidence': ['evidence-only'], 'protected': ['github-exit']})
        self.assertIn('Live Git folders: 1 / cap 4', self.run_tool().stdout)

    def test_concurrent_workers_do_not_overwrite_each_other(self):
        for name in ('one', 'two', 'three', 'four'):
            self.lane(name)
        processes = [subprocess.Popen(self.command('record', '--lane', name, '--owner', name,
                     '--state', 'working', '--next-action', 'Integrate'), stdout=subprocess.PIPE,
                     stderr=subprocess.PIPE, text=True) for name in ('one', 'two', 'three', 'four')]
        for process in processes:
            output, error = process.communicate(timeout=30)
            self.assertEqual(process.returncode, 0, output + error)
        self.assertEqual({name: row['owner'] for name, row in self.data()['entries'].items()},
                         {name: name for name in ('one', 'two', 'three', 'four')})

    def test_worker_cannot_take_another_workers_tree(self):
        self.lane('one')
        self.record('one', owner='first')
        self.run_tool('record', '--lane', 'one', '--owner', 'second', '--state', 'working',
                      '--next-action', 'Take tree', success=False)
        self.assertEqual(self.data()['entries']['one']['owner'], 'first')

    def test_ready_requires_clean_exact_gated_head(self):
        path = self.lane('one')
        self.record('one')
        (path / 'source').write_text('dirty\n')
        self.run_tool('record', '--lane', 'one', '--owner', 'worker', '--state', 'ready_for_integration',
                      '--next-action', 'Integrate', '--commit', 'ds-cli=' + self.sha,
                      '--gate', str(self.receipt), success=False)
        self.git('add', 'source', cwd=path)
        self.git('commit', '-m', 'new source', cwd=path)
        self.run_tool('record', '--lane', 'one', '--owner', 'worker', '--state', 'ready_for_integration',
                      '--next-action', 'Integrate', '--commit', 'ds-cli=' + self.sha,
                      '--gate', str(self.receipt), success=False)

    def test_consumed_entry_is_forgotten_only_after_landing_and_removal(self):
        path = self.lane('one')
        self.record('one', commit='ds-cli=' + self.sha)
        args = ['record', '--lane', 'one', '--owner', 'worker', '--state', 'consumed',
                '--main', '--next-action', 'Finished', '--receipt', str(self.receipt)]
        self.run_tool(*args, success=False)
        self.git('worktree', 'remove', str(path))
        path.parent.rmdir()
        self.run_tool(*args)
        self.assertEqual(self.data()['entries'], {})
        self.assertNotIn('events', self.data())
        self.assertEqual(self.git('rev-parse', self.sha).stdout.strip(), self.sha)

    def test_missing_unlanded_tree_stays_a_warning(self):
        path = self.lane('one')
        (path / 'source').write_text('unlanded\n')
        self.git('add', 'source', cwd=path)
        self.git('commit', '-m', 'unlanded', cwd=path)
        head = self.git('rev-parse', 'HEAD', cwd=path).stdout.strip()
        self.record('one', commit='ds-cli=' + head)
        self.git('worktree', 'remove', str(path))
        path.parent.rmdir()
        self.run_tool('scan', '--main')
        self.assertEqual(self.data()['entries']['one']['state'], 'blocked')
        self.run_tool('record', '--lane', 'one', '--owner', 'worker', '--state', 'consumed',
                      '--main', '--next-action', 'Forget', '--receipt', str(self.receipt), success=False)
        self.assertIn('one', self.data()['entries'])


if __name__ == '__main__':
    unittest.main()
