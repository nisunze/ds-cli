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

Establish the installed surface, then walk the tiers; each names the next.
Memory is not evidence.

```
ds --version
ds doctor --output json
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

Try domain and product terms before declaring a gap.

## Read, then invoke, the contract

```
ds capabilities <command-id> --output json
```

Inspect availability, authority, effect, confirmation and refusals. Use only
declared inputs; `--yes` only for the user's exact authorized effect.

Follow returned remedies. Pair only with the desktop profile matching `ds`.
Never repeat non-retryable calls, switch identity/project to force success, or
reconstruct a refused answer.

## Recover identity

Signed out (`headless_signed_out`): run `ds account connect` (MCP: the
`account.connect` tool), have the person approve it in their signed-in DS
GridDesign Desktop under Account > Link a trusted device, then run it again.
That is the only sign-in; never ask for an address or a secret. CLI and map
lane/principal must match; a mismatch is a refusal, never permission to borrow
credentials, projects or lanes.

## Through MCP

The broad server exposes `ds_catalog` and chapter routers: select from the
catalogue, `describe`, then invoke with declared arguments. Set envelope
`confirm: true` only when required. A typed profile advertises leaf tools
instead. Branch on the DS envelope, follow typed remedies; `ds-mcp-host`
covers installation and profile selection.

## Where `ds` stops, and who continues

`ds` owns DS data and DS effects. Four continuations are outside it. Hand over
only on the condition that selects one, name the handover, return with the
result:

- Native PLS-CADD — the model must be opened, solved or visually accepted:
  `ds` writes and reads workspaces, never drives that UI.
- A document renderer — a reviewed draft must become DOCX/PDF: `ds` authors
  and lints the text, installed document tools typeset it.
- Third-party GIS and recorders — interactive geometry edits or motion
  capture: `ds` serves layers, tiles and still evidence only.
- The operator — the effect needs authority `ds` will not grant: approval,
  credentials, an OS install, a deploy, a refusal's remedy. Report the refusal
  code with that remedy; never route around it.

## When `ds` cannot

After ruling out a stop and trying other vocabulary, discover feedback:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Submit one non-secret sighting with expected behavior, evidence, impact and
acceptance. Never create a gap file, call the API or bypass `ds`.

## Narrower skills

- Project and data — `ds-project-context` (active project), `ds-assets`
  (documents), `ds-survey-lifecycle` (coverage, capture, forms),
  `ds-dirty-categories` (category seeds), `ds-cloud-datasets` (parcels,
  customers in a boundary; seeded per project).
- Maps — `ds-map-composition` (print hierarchy, relief), `ds-map-local-data`
  (temporary layers, viewport), `ds-style-composite` (two-field cartography).
- Design and delivery — `ds-grid-spotting`, `ds-lv-design-revision`,
  `ds-pls-cadd-terrain-roundtrip`, `ds-pls-cadd-backup-delivery`,
  `ds-pls-cadd-native-dialogs`, `ds-report-consumption`,
  `ds-boq-staking-table`, `ds-boq-combined-report`.
- Surface and backlog — `ds-mcp-host`, `ds-workstation-setup`,
  `ds-feedback-triage`.

They assume this one; load none for ordinary discovery.

Stops at: the four continuations above, each on its own condition.
