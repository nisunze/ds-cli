# `ds workstation` — reference

Tier-4 reference. `ds workstation <command> --help` is the live contract.

## Scope

This interim domain is the safe half of workstation bootstrap: local discovery,
data-driven platform plans, bounded verification, component provenance, and
receipts. It has no install/configure command and performs no network access or
settings mutation. That omission is deliberate until the owning Windows host
has produced install/uninstall/reinstall lifecycle evidence.

| Command | Effect | What it proves |
|---|---|---|
| `status` | discovery | PATH executables, bounded version probes, receipt state, and separate shell facts |
| `components` | discovery | purpose, provenance, local state, and acquisition policy |
| `plan` | proposal | ordered policy and authorization boundaries; `mutated` is always false |
| `verify` | read-only | executable/version or governed receipt/file hashes, never more |

## Components

- `libreoffice` — required for document/report work. The executable/version
  probe is not a headless document-conversion or lifecycle proof.
- `qgis` — optional GIS desktop prerequisite. Installation always needs an
  explicit request for that run.
- `git-bash` — Windows-only. A detected suitable copy is preserved. Native
  macOS/Linux shells are reported instead of pretending Git Bash applies.
- `rwanda-reference` — optional governed data. Installed state requires a
  receipt under the platform component root and matching SHA-256 for each
  relative file.

`DS_WORKSTATION_COMPONENT_ROOT` may point verification at a staged governed
component store. It does not authorize creation or acquisition.

## LibreOffice provisional fallback

The preferred Windows route remains the platform package manager. Only a
verified package-manager transport failure may reach the fallback. The future
implementation must obtain current candidates from The Document Foundation's
official Metalink response, accept only a listed URL, and require its published
SHA-256 to match the completed installer before execution. No version, mirror,
or historical failure is universal policy.

The separate lifecycle proof must still cover package registration, executable
version, harmless headless conversion, repeat install, task-owned cleanup, and
install/uninstall/reinstall behavior. Until then the plan says
`deferred_pending_lifecycle_proof`.

## Shell meanings

`status.shells` keeps PATH Bash, active shell, VS Code's Windows default
profile, Windows Terminal's default profile, and DS process execution separate.
DS uses direct typed process execution, not a caller-selected general-purpose
shell. Any future Git Bash configuration must target exactly VS Code or Windows
Terminal, merge only the required key, preserve unrelated JSONC content, and
leave Remote-SSH on the remote native shell.

## Refusals and gaps

- `workstation_platform_unsupported` — browser/unsupported host.
- `workstation_component_unknown` — id outside the governed catalogue.
- `workstation_plan_invalid` — component/platform/target combination is not a
  real supported intent.

A confirmed missing lifecycle is reported through the existing
`ds feedback submit` contract. It is not implemented with an API, shell script,
or skill-local downloader.
