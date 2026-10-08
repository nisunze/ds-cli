---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover one live command, follow its contract, hand over at a named boundary, report confirmed gaps. Required before every DS task."
---

# Work through `ds`

Use `ds` for DS data/effects; never substitute APIs, bridges, stores,
parsers, repositories or skill-local programs.

Use `--output json` for agents.

Coding sessions: fix findings or record unfinished work only in the owning
backlogs (OPEN.md code, OPEN-X.md decisions, SEEDING.md operations). Never
submit feedback during coding; remove entries after proof.

## Find one command

```
ds --version
ds doctor --output json
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

## Read, then invoke, the contract

```
ds capabilities <command-id> --output json
```

Inspect availability, authority, effects and refusals. Use declared inputs;
`--yes` only for the user's authorized effect.

Follow remedies; pair only with the matching `ds` desktop profile.
Never repeat non-retryable calls, switch identity/project to force success, or
reconstruct a refused answer.

## Recover identity

Signed out (`headless_signed_out`): run `ds account connect` (MCP: the
`account.connect` tool), have the person approve it in their signed-in DS
GridDesign Desktop under Account > Link a trusted device, then run it again.
Only this signs in; never request addresses or secrets. Match CLI/map lane and principal;
never borrow credentials, projects or lanes.

## Through MCP

Use `ds_catalog` and chapter routers: select, `describe`, invoke. Set
`confirm: true` only when required. Typed profiles advertise leaf tools.
Follow DS envelopes and remedies; see `ds-mcp-host` for installation.

## Where `ds` stops, and who continues

Name the applicable boundary and return with its result:

- Native PLS-CADD — the model must be solved or accepted as the authority:
  edit in DS Grid, export, let PLS-CADD verify; `ds` never drives that UI.
- A document renderer — a reviewed draft must become DOCX/PDF: `ds` authors
  and lints the text, installed document tools typeset it.
- Desktop map and screen recorders — interactive geometry or motion: `ds` serves layers, tiles and still evidence only.
- The operator — the effect needs authority `ds` will not grant: approval,
  credentials, install, deploy or refusal remedy. Report the code and remedy.

## When `ds` cannot

Outside coding, rule out a stop, try other vocabulary, then discover feedback:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Submit one non-secret sighting: expected behavior, evidence, impact, acceptance.
You may filter/compare `ds` files in a disposable step; name it in the sighting.
Never improvise DS data/effects or bypass `ds` with a gap file or API call.

## Narrower skills

- Project and data — `ds-project-context` (context), `ds-project-work` (tasks and records), `ds-assets`
  (documents), `ds-survey-lifecycle` (coverage, capture, forms),
  `ds-dirty-categories` (category seeds), `ds-cloud-datasets` (parcels,
  customers in a boundary; seeded per project).
- Maps — `ds-map-composition` (print hierarchy, relief), `ds-map-local-data`
  (temporary layers, viewport), `ds-style-composite` (two-field cartography).
- Local geometry — `ds-vector-tools` (GeoJSON measurement, zones, sampling, crossings).
- Terrain — `ds-terrain-sampling` (native sampled curves),
  `ds-mv-corridor-ground` (sparse MV corridor seeds and bounded footprints).
- Design and delivery — `ds-grid-spotting`, `ds-lv-design-revision`,
  `ds-lv-voltage-drop`, `ds-pls-cadd-terrain-roundtrip`,
  `ds-pls-cadd-backup-delivery`, `ds-pls-cadd-native-dialogs`,
  `ds-report-consumption`, `ds-boq-staking-table`, `ds-boq-combined-report`.
- Surface and backlog — `ds-mcp-host`, `ds-workstation-setup`,
  `ds-feedback-triage`.

Stops at: PLS-CADD, a document renderer, Desktop or operator; see boundaries above.
