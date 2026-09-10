#!/usr/bin/env python3
"""Read-only layout gate for a DS multi-repository workspace; never recurses."""

import argparse
from pathlib import Path


MARKER = "<!-- ds-workspace-root:v1 -->"
FILES = {"AGENTS.md", "PLAN.md", "README.md"}
RETIRED = {"ds-cli-skills", "ds-mcp"}
DIRECTORIES = {
    ".git", ".github", ".release", "_shared", "_discardable", "_worktrees",
    "_deprecated_stack", "ds-system",
}


def findings(root: Path) -> list[str]:
    errors = []
    for entry in sorted(root.iterdir(), key=lambda path: path.name):
        if entry.name in RETIRED:
            errors.append(entry.name + " (retired; canonical owner is ds-cli)")
            continue
        if entry.name in FILES and entry.is_file():
            continue
        if entry.name in DIRECTORIES and entry.is_dir():
            continue
        if entry.name == "simple_notes" or entry.name.startswith("ds-"):
            # A worktree has a .git file; a normal checkout has a directory.
            # Do not walk repository contents or inspect excluded repositories.
            if entry.is_dir() and (entry / ".git").exists():
                continue
        errors.append(entry.name)
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace", nargs="?", type=Path,
                        default=Path(__file__).resolve().parents[2])
    parser.add_argument("--if-managed", action="store_true",
                        help="Skip parents without the opt-in AGENTS.md marker")
    args = parser.parse_args()
    root = args.workspace.resolve()
    if not root.is_dir():
        parser.error(f"workspace is not a directory: {root}")
    if args.if_managed:
        agents = root / "AGENTS.md"
        if not agents.is_file() or MARKER not in agents.read_text(encoding="utf-8"):
            print("Workspace root check skipped: parent has no managed-layout marker")
            return 0
    errors = findings(root)
    for name in errors:
        print(f"FAIL workspace root: {name}")
    if errors:
        print("Put reusable source/docs in the owning repository, evidence in "
              "_shared/<task>/, and scratch in _discardable/<task>/.")
        print("No files were changed. See docs/development/workspace-layout.md.")
        return 1
    print("Workspace root layout passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
