---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover one live command, follow its contract, hand over at a named boundary, report confirmed gaps. Required before every DS task."
---

# Work through `ds`

Use `ds` for DS data and effects. Do not substitute an API, desktop bridge,
store, parser, repository or skill-local program.

Use `--output json` for agents.

Coding sessions: fix findings or record unfinished work only in the owning
backlogs (OPEN.md code, OPEN-X.md decisions, SEEDING.md operations). Never
submit feedback during coding; remove entries after proof.

## Find one command

Check installed discovery; memory is not evidence.

```
ds --version
ds doctor --output json
ds capabilities --output json
ds capabilities <domain> --output json
ds capabilities --search '<words>' --output json
```

Try domain terms first.

## Read, then invoke, the contract

```
ds capabilities <command-id> --output json
```

Inspect availability, authority, effects and refusals. Use declared inputs;
`--yes` only for the user's authorized effect.

Follow returned remedies. Pair only with the desktop profile matching `ds`.
Never repeat non-retryable calls, switch identity/project to force success, or
reconstruct a refused answer.

## Recover identity

Signed out (`headless_signed_out`): run `ds account connect` (MCP: the
`account.connect` tool), have the person approve it in their signed-in DS
GridDesign Desktop under Account > Link a trusted device, then run it again.
Only this signs in; never ask for an address or secret. CLI and map
lane/principal must match. A mismatch is a refusal; never borrow credentials,
projects or lanes.

## Through MCP

Use `ds_catalog` and chapter routers: select, `describe`, then invoke declared
arguments. Set envelope `confirm: true` only when required. Typed profiles
advertise leaf tools. Follow DS envelopes and remedies; `ds-mcp-host` covers
installation and profiles.

## Where `ds` stops, and who continues

Four tasks need another tool. Hand over only when one applies, name it, and
return with its result:

- Native PLS-CADD — the model must be solved or accepted as the authority:
  edit in DS Grid, export, let PLS-CADD verify; `ds` never drives that UI.
- A document renderer — a reviewed draft must become DOCX/PDF: `ds` authors
  and lints the text, installed document tools typeset it.
- The DS GridDesign Desktop map and screen recorders — interactive geometry
  drawing or motion capture: `ds` serves layers, tiles and still evidence only.
- The operator — the effect needs authority `ds` will not grant: approval,
  credentials, an OS install, a deploy, a refusal's remedy. Report the refusal
  code with that remedy; never route around it.

## When `ds` cannot

Outside coding sessions, after ruling out a stop and trying other vocabulary,
discover feedback:

```
ds capabilities --search feedback --output json
ds capabilities feedback.submit --output json
```

Submit one non-secret sighting with expected behavior, evidence, impact and
acceptance. To finish, you may filter, reshape or compare files produced or read
through `ds` in a disposable local step; name it in the sighting. Never improvise
DS data or effects or route around `ds` with a gap file or API call.

## Narrower skills

- Project and data — `ds-project-context` (context), `ds-project-work` (tasks and records), `ds-assets`
  (documents), `ds-survey-lifecycle` (coverage, capture, forms),
  `ds-dirty-categories` (category seeds), `ds-cloud-datasets` (parcels,
  customers in a boundary; seeded per project).
- Maps — `ds-map-composition` (print hierarchy, relief), `ds-map-local-data`
  (temporary layers, viewport), `ds-style-composite` (two-field cartography).
- Local geometry — `ds-vector-tools` (native GeoJSON measurement, zones,
  points along lines and line crossings).
- Design and delivery — `ds-grid-spotting`, `ds-lv-design-revision`,
  `ds-lv-voltage-drop`, `ds-pls-cadd-terrain-roundtrip`,
  `ds-pls-cadd-backup-delivery`, `ds-pls-cadd-native-dialogs`,
  `ds-report-consumption`, `ds-boq-staking-table`, `ds-boq-combined-report`.
- Surface and backlog — `ds-mcp-host`, `ds-workstation-setup`,
  `ds-feedback-triage`.

Stops at: the four continuations above, each on its own condition.
