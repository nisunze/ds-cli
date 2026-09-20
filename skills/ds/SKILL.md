---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover one live command, follow its contract, hand over at a named boundary, report confirmed gaps. Required before every DS task."
---

# Work through `ds`

Everything you learn about or do to the stack passes through `ds`. Never
substitute an API, desktop bridge, store, parser, repository, or skill-local
program. Report capabilities proven absent through `ds`.

Use `--output json` for agent calls.

## Find one command

Establish the installed surface, then walk the tiers; each is small and names
the next. Memory is not evidence.

```
ds --version
ds doctor --output json
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

Search is lexical: try domain and product vocabulary before declaring a gap.
Use help for readable contracts, capabilities for machine selection.

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
and follow the device-link contract from `auth.link.begin`; launch the matching
installed lane only where that contract requires it. CLI and map lane/principal
must match — a mismatch is a refusal, never permission to borrow credentials,
projects, or lanes.

## Through MCP

The broad server exposes `ds_catalog` and chapter routers: select from the
catalogue, `describe`, then invoke with declared arguments. Set envelope
`confirm: true` only when required. A typed profile advertises leaf tools
instead. Branch on the DS envelope, follow typed remedies, and use
`ds-mcp-host` for installation and profile selection.

## Where `ds` stops, and who continues

`ds` owns DS data and DS effects. Four continuations are outside it. Hand over
only on the condition that selects one, name that handover, then return to `ds`
with the result:

- Native PLS-CADD — the model must be opened, solved or visually accepted:
  `ds` writes and reads workspaces, never drives that UI.
- A document renderer — a reviewed draft must become DOCX/PDF: `ds` authors
  and lints the text, installed document tools typeset it.
- Third-party GIS and recorders — geometry is edited interactively or motion
  captured: `ds` serves layers, tiles and still evidence only.
- The operator — the effect needs authority `ds` will not grant: approval,
  credentials, an OS install, a deploy, or a refusal's remedy. Report the
  refusal code with that remedy; never route around it.

A stop is not a gap: name it, and what it needs.

## When `ds` cannot

After ruling out a stop, and trying alternate vocabulary, discover feedback:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Submit one non-secret sighting with expected behavior, evidence, impact and
acceptance. Never create a gap file, call the API, or bypass `ds`.

## Route to a narrower skill

- Project and data — `ds-project-context` (active project), `ds-assets`
  (documents), `ds-survey-lifecycle` (coverage, capture, forms),
  `ds-dirty-categories` (category seeds), `ds-cloud-datasets` (parcels a
  corridor crosses, customers in a boundary; seed per project).
- Maps — `ds-map-composition` (print hierarchy, relief), `ds-map-local-data`
  (temporary layers, viewport), `ds-style-composite` (two-field cartography).
- Design and delivery — `ds-lv-design-revision`, `ds-pls-cadd-terrain-roundtrip`,
  `ds-report-consumption`, `ds-boq-staking-table`, `ds-boq-combined-report`.
- Surface and backlog — `ds-mcp-host`, `ds-workstation-setup`,
  `ds-feedback-triage`.

These assume this one; do not load them for ordinary discovery.

Stops at: the four continuations named above, each on its own condition.
