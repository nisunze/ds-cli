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

What the machine holds for one project (sync store rows, sealed report
batches, survey photos waiting or synced, verified downloads) is
`ds workstation local-data status --project <id> --output json`; only its
`cleanable[]` stores may be removed, with `ds workstation local-data clean
--project <id> --yes`.

## Apply only the proven exact actions

After explicit user intent, review the live install descriptor. Native Windows
LibreOffice uses its fixed package identity:

```text
ds capabilities workstation.install --output json
ds workstation install --component libreoffice --approval interactive --yes --output json
```

Keep the user present for UAC; never bypass it. The command is idempotent,
verifies registration/version/headless conversion, and records task ownership
only when it installed the package. LibreOffice needs no separate MCP.

Linux Server and Linux Desktop share one host tiling toolchain. The browser and
Windows use their declared online tiling route and must not be sent through the
Linux installer. On Linux, install the kernel-pinned Tippecanoe and PMTiles pair
through the governed command while the user can answer sudo:

```text
ds workstation plan --component tippecanoe --platform linux --output json
ds capabilities workstation.install --output json
ds workstation install --component tippecanoe --approval interactive --yes --output json
ds workstation verify --component tippecanoe --output json
```

For local report finishing, use the same plan/install/verify sequence with
`pandoc`. On Linux use `--approval interactive`; on Windows the platform may
show UAC. Linux LibreOffice is also available through this path when absent:

```text
ds workstation plan --component pandoc --platform current --output json
ds workstation install --component pandoc --approval interactive --yes --output json
ds workstation verify --component pandoc --output json
```

Do not install these during Server publication. Package deployment completes
first; host prerequisites are a separate idempotent workstation operation.

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

Git Bash acquisition remains unimplemented and fails closed. Discovery or
planning is not permission. Detect a suitable Git Bash before configuration and never reinstall or remove a
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

Stops at: the operating system — an install needing administrator rights is run
by the operator; `ds` plans and verifies it.
