# `ds workstation` — reference

Tier-4 reference. `ds workstation <command> --help` is the live contract.

## Scope

This domain provides local discovery, data-driven plans, bounded verification,
component provenance/receipts, and exact proven installation/configuration
paths. All other acquisition and settings paths fail closed.

| Command | Effect | What it proves |
|---|---|---|
| `status` | discovery | PATH executables, bounded version probes, receipt state, and separate shell facts |
| `components` | discovery | purpose, provenance, local state, and acquisition policy |
| `plan` | proposal | ordered policy and authorization boundaries; `mutated` is always false |
| `install` | machine write | Windows LibreOffice or the governed official NISR Rwanda component |
| `configure` | machine write | existing suitable Git Bash profile selected as VS Code's Windows default only |
| `verify` | read-only | executable/version, LibreOffice headless conversion, or receipt/file hashes |

## Components

- `libreoffice` — required for document/report work. Verification includes a
  harmless task-owned HTML-to-PDF conversion and exact temporary cleanup.
- `qgis` — optional GIS desktop prerequisite. Installation always needs an
  explicit request for that run.
- `git-bash` — Windows-only. A detected suitable copy is preserved. Native
  macOS/Linux shells are reported instead of pretending Git Bash applies.
- `rwanda-reference` — governed official NISR Village Boundary 2022 Open Data.
  Acquisition uses bounded, low-bandwidth-tolerant pages with no geometry
  simplification and commits source-precision GeoJSON only with source,
  version, license, ownership, and matching SHA-256 receipt.

`DS_WORKSTATION_COMPONENT_ROOT` may point verification at a staged governed
component store. It does not authorize creation or acquisition.

## LibreOffice lifecycle and fallback

The preferred Windows route remains the platform package manager. Only a
verified package-manager transport failure may reach the fallback. The future
implementation must obtain current candidates from The Document Foundation's
official Metalink response, accept only a listed URL, and require its published
SHA-256 to match the completed installer before execution. No version, mirror,
or historical failure is universal policy.

The owning native Windows proof covered package registration, executable
version, harmless headless conversion, idempotence, task-owned cleanup, and
uninstall/reinstall behavior. `install` implements only the fixed package
identity, records ownership only for its own new install, and never removes a
pre-existing installation. LibreOffice has no separate MCP.

## Shell meanings

`status.shells` keeps PATH Bash, active shell, VS Code's Windows default
profile, Windows Terminal's default profile, and DS process execution separate.
DS uses direct typed process execution, not a caller-selected general-purpose
shell. `configure` supports only VS Code and an already-defined suitable Git
Bash profile. It merges one key, preserves unrelated JSONC, and leaves Windows
Terminal and Remote-SSH unchanged.

## Refusals and gaps

- `workstation_platform_unsupported` — browser/unsupported host.
- `workstation_component_unknown` — id outside the governed catalogue.
- `workstation_plan_invalid` — component/platform/target combination is not a
  real supported intent.
- `workstation_mutation_unsupported` — the exact mutation is not implemented.
- `workstation_dataset_acquisition_failed` — the official NISR component could
  not be fetched, validated, or committed.
- `workstation_settings_unsafe` — Git Bash/profile/settings evidence is not
  strong enough for the conservative merge.

A confirmed missing lifecycle is reported through the existing
`ds feedback submit` contract. It is not implemented with an API, shell script,
or skill-local downloader.
