---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover one live command, follow its contract, and report confirmed gaps. Required before every DS task."
---

# Work through `ds`

Everything you learn about or do to the stack passes through `ds`. Never
substitute an API, desktop bridge, store, parser, repository, or skill-local
program. If live discovery proves the task absent, report that through `ds`.

Use `--output json` for agent calls.

## Establish the installed surface

```
ds --version
ds doctor --output json
```

These live results are the installation's identity and availability; memory or
a skill from another build is not evidence.

## Find one command

Walk the tiers; each is small and names the next:

```
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

Search is lexical. Try the likely domain and product vocabulary before deciding
a capability is absent; narrow broad matches and never choose by rank alone.

Compact discovery never replaces `ds --help`, `ds <domain> --help`, or
`ds <domain> <command> --help`. Use help for the readable contract and
`capabilities` for bounded machine-readable selection.

## Read, then invoke, the live contract

```
ds capabilities <command-id> --output json
```

The descriptor is authoritative. Inspect availability, authority, effect,
confirmation and refusals; use only declared inputs. Pass `--yes` only when the
user authorizes that exact effect and scope.

Follow the returned remedy and next action. If multiple desktop builds make
pairing ambiguous, select the candidate whose profile matches the established
`ds` and retain it for the session. Never repeat a non-retryable call unchanged,
switch identity/project to force success, or reconstruct a refused answer.

## Through MCP

The broad server exposes `ds_catalog` plus chapter routers. Use the catalogue,
call the selected chapter with `operation: "describe"`, then invoke through the
same chapter with descriptor-conforming `arguments`. Put `confirm: true` only
at the chapter envelope and only when the exact contract requires it.

A typed role profile instead advertises its leaf tools directly. In either
shape, branch on the unchanged DS envelope and follow typed remedies. Read
`ds-mcp-host` for installation and profile selection; never invent a generated
tool name or use an omitted profile command.

## When `ds` cannot

After checking the likely domain and alternate vocabulary, discover feedback:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Submit one bounded, non-secret sighting: expected behavior, live evidence,
impact and observable acceptance. Never create a gap file, call the API, or
bypass `ds`.

## Route to a narrower skill when one fits

- `ds-project-context` — active project and state boundary.
- `ds-map-local-data` — temporary layers, semantic panel staging, and still PNG evidence.
- `ds-lv-design-revision` — revise one transformer's LV design safely.
- `ds-pls-cadd-terrain-roundtrip` — PLS-CADD route and terrain delivery.
- `ds-style-composite` — colour/icon/size, a second field, and line/fill cartography.
- `ds-report-consumption` — obtain and read delivered workbooks.
- `ds-qgis-print-delivery` — governed multi-layout print delivery.
- `ds-boq-staking-table` — LV/MV staking tables against a BOQ.
- `ds-boq-combined-report` — the combined workbook against a project BOQ.
- `ds-mcp-host` — compact chapters and typed MCP profiles.
- `ds-workstation-setup` — prerequisites, component provenance, and safe setup planning.
- `ds-feedback-close` — close backlog reports this session has fixed.
- `ds-survey-lifecycle` — Form Factory, project-form settings, reusable templates, and create-from-template without a map.

Those skills assume this one. Do not load them for ordinary discovery.
