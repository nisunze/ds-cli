#!/usr/bin/env python3
"""Regression checks for the skill validator's per-skill budgets."""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


CHECK = Path(__file__).with_name("check.py")


def write_skill(root: Path, name: str, description: str) -> None:
    skill = root / "skills" / name
    (skill / "agents").mkdir(parents=True, exist_ok=True)
    (skill / "SKILL.md").write_text(
        f"---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n",
        encoding="utf-8",
        newline="\n",
    )
    (skill / "agents" / "openai.yaml").write_text(
        f"interface:\n  display_name: {name}\n",
        encoding="utf-8",
        newline="\n",
    )


def run_check(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(root / "scripts" / "check.py")],
        text=True,
        capture_output=True,
        check=False,
    )


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="ds-skill-check-") as temporary:
        root = Path(temporary)
        (root / "scripts").mkdir()
        shutil.copyfile(CHECK, root / "scripts" / "check.py")

        # Seven individually bounded descriptions deliberately exceed 2,048
        # characters in aggregate. Skill discovery is conditional, so growth
        # in the number of skills must not become one global release ceiling.
        for index in range(7):
            name = "ds" if index == 0 else f"skill-{index}"
            write_skill(root, name, f"Route {name}: " + ("x" * 340))
        many = run_check(root)
        if many.returncode != 0:
            raise AssertionError(
                "individually bounded skills failed as an aggregate:\n"
                + many.stdout
                + many.stderr
            )

        write_skill(root, "skill-1", "x" * 513)
        overlong = run_check(root)
        if overlong.returncode == 0 or "description over 512 chars (513)" not in overlong.stdout:
            raise AssertionError(
                "one overlong skill description was not refused by name:\n"
                + overlong.stdout
                + overlong.stderr
            )

    print("skill check per-skill budget contract passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
