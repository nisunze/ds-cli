---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover one live command, follow its contract, and report confirmed gaps. Required before every DS task."
---

# Work through `ds`

Everything you learn about or do to the stack passes through `ds`. Never
substitute an API, desktop bridge, store, parser, repository, or skill-local
program. Report capabilities proven absent through `ds`.

Use `--output json` for agent calls.

## Establish the installed surface

```
ds --version
ds doctor --output json
```

These results identify the installed surface; memory is not evidence.

## Find one command

Walk the tiers; each is small and names the next:

```
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

Search is lexical. Try domain and product vocabulary before declaring a gap.
Use help for readable contracts and capabilities for machine selection.

## Read, then invoke, the live contract

```
ds capabilities <command-id> --output json
```

Inspect availability, authority, effect, confirmation and refusals. Use only
declared inputs; pass `--yes` only for the user's exact authorized effect.

Follow returned remedies. Pair only with the desktop profile matching `ds`.
Never repeat non-retryable calls, switch identity/project to force success, or
reconstruct a refused answer.

## Recover headless identity

When signed out or password login is rejected, do not loop it. Discover `auth`
and follow device-link contracts from `auth.link.begin`. If required and in
scope, launch the matching installed lane, then approve and complete as
described. CLI and map lane/principal must match; mismatch is a refusal, never
permission to borrow credentials, projects, or lanes.

## Through MCP

The broad server exposes `ds_catalog` and chapter routers. Select from the
catalogue, `describe`, then invoke through that chapter with declared arguments.
Set envelope `confirm: true` only when required.

A typed profile advertises leaf tools. Always branch on the DS envelope and
follow typed remedies. Use `ds-mcp-host` for installation/profile selection.

## When `ds` cannot

After checking the likely domain and alternate vocabulary, discover feedback:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Submit one non-secret sighting with expected behavior, evidence, impact and
acceptance. Never create a gap file, call the API, or bypass `ds`.

## Route to a narrower skill when one fits

- `ds-project-context` — active project and state boundary.
- `ds-map-local-data` — temporary map layers and viewport.
- `ds-lv-design-revision` — revise one transformer's LV design safely.
- `ds-pls-cadd-terrain-roundtrip` — PLS-CADD route and terrain delivery.
- `ds-style-composite` — two-field cartography.
- `ds-report-consumption` — obtain and read delivered workbooks.
- `ds-qgis-print-delivery` — governed multi-layout print delivery.
- `ds-boq-staking-table` — staking tables against a BOQ.
- `ds-boq-combined-report` — combined workbook against a BOQ.
- `ds-mcp-host` — compact chapters and typed MCP profiles.
- `ds-workstation-setup` — prerequisites, component provenance, and safe setup planning.
- `ds-feedback-close` — close backlog reports this session has fixed.

Those skills assume this one. Do not load them for ordinary discovery.
