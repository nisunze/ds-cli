# Working in `ds-cli` — the skills tree

**Read [`CLAUDE.md`](CLAUDE.md) first. It is the normative document for this
repository and every rule in it applies here.**

This file used to be a verbatim copy of it plus four short additions, which
meant every rule had two homes and one of them was always the stale one — the
same "second description" failure the CLI itself exists to avoid. So it is now
only the part that is additionally true because this repository also owns the
native agent skills.

## What this repository additionally owns

This repository also owns the canonical native Agent Skills under `skills/`
and their ownership-safe installers under `scripts/`. Those documents teach
agents when to use the executable; they never implement or duplicate a command.

## Progressive disclosure applies to a skill too

The same rule applies to skills. Native agent discovery sees only each skill's
short frontmatter description; a full `SKILL.md` is loaded only after it is
selected. Keep command flags, schemas, enums, output contracts and refusals in
the live `Command` declaration and discover them with `ds capabilities`.
Compact discovery never replaces root, domain, or command help. Both are
first-class projections of the same declaration and both must remain complete.

## Verification

Everything under *Verification* in `CLAUDE.md` applies unchanged. Two further
commands must pass, and CI runs them too:

```bash
python3 scripts/check.py
bash scripts/test-install-skills.sh
```

`check.py` is the only content gate on the skills tree: it holds each skill's
description and body inside their conditional-load budgets, refuses a
skill-local executable, and scans for a route around `ds` or a local gap
ledger.

## Additionally rejected

Beyond the list in `CLAUDE.md`:

- A skill-local executable, copied CLI contract, or direct API route.
- A local gap Markdown ledger. Agents report verified gaps through
  `ds feedback submit`, which reaches the same backlog as the app's `fb`
  shortcut, and close what they have fixed with `ds feedback close` — the
  same governed triage call, needing the same platform capability.
