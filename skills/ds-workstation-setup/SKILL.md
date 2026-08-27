---
name: ds-workstation-setup
description: "Inspect, plan, verify, and safely install proven DS workstation prerequisites and governed Rwanda reference components."
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

## Apply only the proven exact actions

The native Windows LibreOffice lifecycle is proven. After explicit user intent,
review the live install descriptor and use its fixed package identity:

```text
ds capabilities workstation.install --output json
ds workstation install --component libreoffice --approval interactive --yes --output json
```

Keep the user present for UAC; never bypass it. The command is idempotent,
verifies registration/version/headless conversion, and records task ownership
only when it installed the package. LibreOffice needs no separate MCP.

When a task needs Rwanda village boundaries, the explicit acquisition command
uses the fixed official NISR 2022 Open Data layer and writes a
provenance/version/license/SHA-256 receipt:

```text
ds workstation install --component rwanda-reference --yes --output json
```

To select an already-defined suitable Git Bash profile in VS Code:

```text
ds workstation configure --component git-bash --target vscode --yes --output json
```

QGIS and Git Bash acquisition remain unimplemented and fail closed; do not
install a third-party QGIS MCP. Discovery or planning is not permission.
Detect a suitable Git Bash before configuration and never reinstall or remove a
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
