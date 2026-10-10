---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover a command, follow its contract, name handover boundaries and report gaps. Required before every DS task."
---

# Work through `ds`

Use `ds` for DS data/effects; never substitute APIs, bridges, stores,
parsers, repositories or skill-local programs. Use `--output json` for agents.

## Report as you go

Outside coding, immediately file `feedback.submit` with expected behavior,
evidence, impact and acceptance for failures past their remedy, a second
discovery search, missed user vocabulary, contract disagreements, wrong or
incomplete output, post-processing, slow steps, workarounds, missing controls
or ideas that save steps. One sighting per gap; repeats merge. Ask no permission;
include no secrets or customer data. End with report ids, one per line.

Coding sessions: never submit feedback; fix findings or record unfinished work
in the owning backlogs (OPEN.md code, OPEN-X.md decisions, SEEDING.md
operations); remove entries after proof.

## Find one command

```
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
`--yes` only for the user's authorized effect. Follow remedies; pair only with
the matching desktop profile. Never repeat non-retryable calls, switch
identity/project to force success, or reconstruct a refused answer.

## Recover identity

For `headless_signed_out`, run `ds account connect` (MCP: `account.connect`).
The person approves in signed-in Desktop > Account > Link a trusted device;
run it again. Never request secrets or borrow credentials, projects or lanes.
Match the CLI/map lane and principal.

## Through MCP

`ds_catalog`, then a chapter router: select, `describe`, invoke; `confirm: true`
only when required. Setup and typed profiles: `ds-mcp-host`.

## Where `ds` stops, and who continues

Name the boundary and return its result:

- Native PLS-CADD — for model solving or acceptance, edit in DS Grid, export
  and let PLS-CADD verify; `ds` never drives that UI.
- A document renderer — a reviewed draft must become DOCX/PDF: `ds` authors
  and lints the text, installed document tools typeset it.
- Desktop and screen recorders — for interactive geometry or motion;
  `ds` serves layers, tiles and still evidence only.
- The operator — for approval, credentials, install, deploy or refusal remedy
  beyond `ds` authority. Report the code and remedy.

## When `ds` cannot

Rule out a stop, try other vocabulary, then report. You may filter/compare
`ds` files in a disposable step; name it in the sighting. Never bypass `ds`.

## Narrower skills

- Project and data — `ds-project-context`, `ds-project-work`, `ds-assets`,
  `ds-survey-lifecycle`, `ds-dirty-categories`, `ds-cloud-datasets` (parcels,
  customers in a boundary).
- Maps — `ds-map-composition`, `ds-map-local-data`, `ds-style-composite`.
- Geometry and terrain — `ds-vector-tools`, `ds-terrain-sampling`,
  `ds-mv-corridor-ground`.
- Design and delivery — `ds-grid-spotting`, `ds-lv-design-revision`,
  `ds-lv-voltage-drop`, `ds-pls-cadd-terrain-roundtrip`,
  `ds-pls-cadd-backup-delivery`, `ds-pls-cadd-native-dialogs`,
  `ds-report-consumption`, `ds-boq-staking-table`, `ds-boq-combined-report`.
- Surface and backlog — `ds-mcp-host`, `ds-workstation-setup`,
  `ds-feedback-triage`.

## Demand-driven reads

Read project data only for navigation, explicit refresh or an actual consumer.
No polling. Pushes invalidate retained state without fetching unused data or
checking freshness. Reuse admitted local snapshots at the shared freshness
boundary; preserve authorization, revision and dirty-work fences. Notifications
retain their minimal push contract. In coding, prove read counts and the unused
data negative case.

Stops at: PLS-CADD, a document renderer, Desktop or operator (above).
