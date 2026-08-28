#!/usr/bin/env python3
"""Validate this repository's native agent skills.

Checks structure and context budgets, rejects skill-local executables and
scans for routes around `ds` or local gap ledgers. Deterministic, with no
dependencies beyond the standard library. Exit 1 on any failure.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAX_DESCRIPTION_CHARS = 512
MAX_ENTRY_SKILL_BYTES = 4 * 1024
MAX_SKILL_BYTES = 8 * 1024
MAX_ALL_DESCRIPTIONS = 2 * 1024
# Tokens that only appear when something routes around `ds`, or tells the
# reader to switch off a platform security control to run something. A skill
# that has to disable Windows' execution policy is asking an agent to trust an
# unsigned script of unverified provenance, which no skill here needs.
BYPASS = re.compile(
    r"\b(curl|wget|Invoke-WebRequest|Invoke-RestMethod|gcloud|firebase|bq|psql|sqlite3)\b"
    r"|https?://(127\.0\.0\.1|localhost)|\blocalhost:\d+|\bfetch\("
    r"|ExecutionPolicy\s+Bypass"
)
# Credential shapes. Every entry names a class, and a hit reports the file and
# the class only — never the matched text. A gate that echoes the secret it
# found has published it a second time, in a build log that is easier to read
# than the repository.
SECRETS = (
    ("google api key", re.compile(r"AIza[0-9A-Za-z_-]{35}")),
    ("github token", re.compile(r"\bgh[pousr]_[0-9A-Za-z]{36,}")),
    ("openai-style api key", re.compile(r"\bsk-[A-Za-z0-9]{20,}")),
    ("slack token", re.compile(r"\bxox[baprs]-[0-9A-Za-z-]{10,}")),
    ("json web token", re.compile(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.")),
    ("private key block", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("service account key", re.compile(r'"type"\s*:\s*"service_account"')),
)
DUPLICATED_FRONT_DOOR_CONTRACT = re.compile(
    r"\b(unexpected_operand|desktop_pairing|desktop_user|local_file_write|artifact_write|global_write)\b"
    r"|\bExit codes?:"
)
LOCAL_GAP_LEDGER = re.compile(
    r"\b(?:write|create|record|append|save|file)\b[^\n]{0,80}\bgaps?/"
    r"|DSCLI-GAP-|gaps/README\.md",
    re.I,
)
FAIL = []


def frontmatter(text, where):
    m = re.match(r"---\n(.*?)\n---\n", text, re.S)
    if not m:
        FAIL.append(f"{where}: missing frontmatter")
        return {}
    out = {}
    for line in m.group(1).splitlines():
        k, _, v = line.partition(":")
        if _:
            out[k.strip()] = v.strip().strip('"')
    return out


def shipped_text(path):
    """One shipped file's text, or None where it is not text.

    A NUL in the first block, or bytes that are not UTF-8, means an image or an
    archive: scanning its mojibake would produce noise, not findings.
    """
    data = path.read_bytes()
    if b"\x00" in data[:8192]:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def check_skill(d):
    md = d / "SKILL.md"
    where = md.relative_to(ROOT)
    if not md.exists():
        FAIL.append(f"{where}: missing")
        return
    text = md.read_text(encoding="utf-8")
    fm = frontmatter(text, where)
    if fm.get("name") != d.name:
        FAIL.append(f"{where}: name '{fm.get('name')}' != directory '{d.name}'")
    desc = fm.get("description", "")
    if not desc:
        FAIL.append(f"{where}: empty description")
    elif len(desc) > MAX_DESCRIPTION_CHARS:
        FAIL.append(f"{where}: description over {MAX_DESCRIPTION_CHARS} chars ({len(desc)})")
    byte_count = len(text.encode("utf-8"))
    budget = MAX_ENTRY_SKILL_BYTES if d.name == "ds" else MAX_SKILL_BYTES
    if byte_count > budget:
        FAIL.append(f"{where}: {byte_count} bytes exceeds its {budget}-byte conditional-load budget")
    if not (d / "agents" / "openai.yaml").exists():
        FAIL.append(f"{where}: missing agents/openai.yaml")
    if "\r" in text:
        FAIL.append(f"{where}: CRLF line endings")
    scripts = d / "scripts"
    if scripts.exists() and any(path.is_file() for path in scripts.rglob("*")):
        FAIL.append(f"{where}: skill-local executables are forbidden; invoke ds directly")
    if d.name == "ds":
        for n, line in enumerate(text.splitlines(), 1):
            if DUPLICATED_FRONT_DOOR_CONTRACT.search(line):
                FAIL.append(
                    f"{where}:{n}: duplicates the live ds contract: {line.strip()[:80]}"
                )
    # The installer copies the whole skill directory, so the scan walks the
    # whole skill directory. A non-recursive `references/*.md` scan left
    # `references/sub/notes.md`, any `.txt`, and every `agents/` file shipping
    # unread.
    for path in sorted(p for p in d.rglob("*") if p.is_file()):
        text = shipped_text(path)
        if text is None:
            continue
        where_file = path.relative_to(ROOT)
        for n, line in enumerate(text.splitlines(), 1):
            if BYPASS.search(line):
                FAIL.append(
                    f"{where_file}:{n}: routes around ds or disables a security control: "
                    f"{line.strip()[:80]}"
                )
            if LOCAL_GAP_LEDGER.search(line):
                FAIL.append(f"{where_file}:{n}: creates a local gap ledger: {line.strip()[:80]}")
            for label, pattern in SECRETS:
                if pattern.search(line):
                    # Path and class only. The value is never echoed.
                    FAIL.append(
                        f"{where_file}:{n}: possible {label}; remove it from the skill "
                        "and rotate the value"
                    )


def main():
    skills = sorted(p for p in (ROOT / "skills").iterdir() if p.is_dir())
    for d in skills:
        check_skill(d)
    descriptions = []
    for d in skills:
        text = (d / "SKILL.md").read_text(encoding="utf-8")
        descriptions.append(frontmatter(text, (d / "SKILL.md").relative_to(ROOT)).get("description", ""))
    description_chars = sum(map(len, descriptions))
    if description_chars > MAX_ALL_DESCRIPTIONS:
        FAIL.append(
            f"skill discovery metadata is {description_chars} chars; budget is {MAX_ALL_DESCRIPTIONS}"
        )
    if (ROOT / "gaps").exists():
        FAIL.append("gaps/: local gap ledgers are forbidden; use `ds feedback submit`")
    print(f"{len(skills)} skills checked; {description_chars} discovery characters")
    for f in FAIL:
        print("FAIL", f)
    return 1 if FAIL else 0


if __name__ == "__main__":
    sys.exit(main())
