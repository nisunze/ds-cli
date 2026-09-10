#!/usr/bin/env python3
"""Exercise the workspace boundary without touching the real workspace."""

import subprocess
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


CHECK = Path(__file__).with_name("check-workspace-root.py")


class WorkspaceRootTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ds-workspace-layout-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def run_check(self, *options):
        return subprocess.run([sys.executable, str(CHECK), str(self.root), *options],
                              text=True, capture_output=True, check=False)

    def test_layout_accepts_repositories_worktrees_and_containers(self):
        for name in ("AGENTS.md", "PLAN.md", "README.md"):
            (self.root / name).write_text("", encoding="utf-8")
        for name in ("_shared", "_discardable", "_worktrees", "_deprecated_stack",
                     ".github", ".release", "ds-system"):
            (self.root / name).mkdir()
        for name in ("ds-cli", "simple_notes"):
            (self.root / name / ".git").mkdir(parents=True)
        (self.root / "ds-network").mkdir()
        (self.root / "ds-network" / ".git").write_text("gitdir: elsewhere\n")
        # Contents of the archive and excluded stack are deliberately irrelevant.
        (self.root / "_deprecated_stack" / "do-not-read.bin").write_bytes(b"\xff")
        self.assertEqual(self.run_check().returncode, 0)

    def test_loose_files_are_reported_and_preserved(self):
        path = self.root / "huye_probe.py"
        path.write_bytes(b"private evidence\r\n")
        result = self.run_check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(path.name, result.stdout)
        self.assertNotIn("private evidence", result.stdout)
        self.assertEqual(path.read_bytes(), b"private evidence\r\n")

    def test_unowned_directories_and_hidden_scratch_are_rejected(self):
        for name in ("outputs", "node_modules", "__pycache__", ".codex-tmp", "ds-dump"):
            (self.root / name).mkdir()
        result = self.run_check()
        self.assertEqual(result.returncode, 1)
        for name in ("outputs", "node_modules", "__pycache__", ".codex-tmp", "ds-dump"):
            self.assertIn(name, result.stdout)

    def test_allowed_name_with_wrong_type_is_rejected(self):
        (self.root / "README.md").mkdir()
        (self.root / "_shared").write_text("not a directory")
        self.assertEqual(self.run_check().returncode, 1)

    def test_retired_checkouts_cannot_return_to_active_root(self):
        for name in ("ds-mcp", "ds-cli-skills"):
            (self.root / name / ".git").mkdir(parents=True)
        result = self.run_check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("ds-mcp (retired", result.stdout)
        self.assertIn("ds-cli-skills (retired", result.stdout)

    def test_unmanaged_parent_is_skipped_only_when_requested(self):
        (self.root / "other-project").mkdir()
        self.assertEqual(self.run_check("--if-managed").returncode, 0)
        self.assertEqual(self.run_check().returncode, 1)

    def test_managed_parent_is_checked(self):
        (self.root / "AGENTS.md").write_text("<!-- ds-workspace-root:v1 -->\n")
        (self.root / "debug.log").touch()
        self.assertEqual(self.run_check("--if-managed").returncode, 1)

    def test_existing_validation_command_enforces_managed_root(self):
        (self.root / "AGENTS.md").write_text("<!-- ds-workspace-root:v1 -->\n")
        repo = self.root / "ds-cli"
        (repo / ".git").mkdir(parents=True)
        (repo / "scripts").mkdir()
        (repo / "skills").mkdir()
        for name in ("check.py", "check-workspace-root.py"):
            shutil.copyfile(CHECK.with_name(name), repo / "scripts" / name)
        command = [sys.executable, str(repo / "scripts" / "check.py")]
        clean = subprocess.run(command, text=True, capture_output=True, check=False)
        self.assertEqual(clean.returncode, 0, clean.stdout + clean.stderr)
        (self.root / "probe.json").write_text("{}")
        dirty = subprocess.run(command, text=True, capture_output=True, check=False)
        self.assertEqual(dirty.returncode, 1)
        self.assertIn("probe.json", dirty.stdout)


if __name__ == "__main__":
    unittest.main()
