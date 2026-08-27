---
name: ds-workstation-setup
description: "Inspect and plan DS workstation prerequisites: LibreOffice, QGIS, Git Bash/default terminals, and governed Rwanda reference components."
metadata:
  ds-chapters: workstation
---

# Prepare a DS workstation

Use the `ds` skill first. All DS setup discovery and planning flows through the
installed `ds`; never replace a missing command with package-manager, browser,
API, registry, settings-file, or download automation.

## Start without changing the machine

```text
ds --version
ds doctor --output json
ds capabilities --search 'workstation prerequisite component' --output json
ds capabilities workstation.status --output json
ds workstation status --output json
```

Read the exact live descriptor before the next call. Use
`ds workstation components --output json` for provenance and receipt state,
`ds workstation plan --component <id> --platform <platform> --output json` for
a no-side-effect plan, and `ds workstation verify --component <id> --output
json` only on the host that owns the component.

## Keep interim proof boundaries visible

The current surface inspects, plans, and verifies bounded evidence. It does not
install packages, acquire reference data, or change terminal settings. Treat
LibreOffice installer/fallback details and headless document conversion as
provisional until the dedicated Windows install/uninstall/reinstall proof is
landed. An executable/version result is not that lifecycle proof.

QGIS installation and Rwanda reference-data acquisition each require a new,
explicit user request. Discovery or planning is not permission. Detect a
suitable Git Bash before planning installation and never reinstall or remove a
pre-existing copy. Cleanup may name only files recorded as task-owned by the
same governed run.

Read [references/components.md](references/components.md) only when choosing a
component path. Read
[references/windows-shells.md](references/windows-shells.md) only for Windows
Git Bash/default-profile intent.

## Route remaining gaps

When live discovery proves that install/configure or lifecycle proof is still
absent, discover `feedback.submit` and send one bounded observation through
`ds feedback submit`. Do not invent a workaround or claim an unrun proof.
