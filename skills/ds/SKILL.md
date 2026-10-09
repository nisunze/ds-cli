---
name: ds
description: "Use deployed `ds` as the sole DS interface: discover one live command, follow its contract, hand over at a named boundary, and report friction, failures and gaps as you go, unasked. Required before every DS task."
---

# Work through `ds`

Use `ds` for DS data/effects; never substitute APIs, bridges, stores,
parsers, repositories or skill-local programs. Use `--output json` for agents.

## Report as you go

Outside coding, file a `feedback.submit` sighting (expected behavior, evidence,
impact, acceptance) at once, unasked, when:

1. a command failed, crashed or refused past its remedy;
2. finding it took a second search, or the user's words missed it;
3. help, refusal or skill text disagreed with what it did;
4. output was wrong, incomplete, surprising or needed post-processing;
5. a step was slow or needed a workaround;
6. a capability was missing;
7. an idea would have saved the user steps.

One sighting per distinct gap; repeats merge, so never filing is the failure.
Ask no permission; include no secrets or customer data. End by listing
the report ids, one per line.

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

Signed out (`headless_signed_out`): run `ds account connect` (MCP:
`account.connect`), have the person approve it in their signed-in DS
GridDesign Desktop under Account > Link a trusted device, then run it again.
Only this signs in; never request addresses or secrets, or borrow credentials,
projects or lanes; match the CLI/map lane and principal.

## Through MCP

`ds_catalog`, then a chapter router: select, `describe`, invoke; `confirm: true`
only when required. Setup and typed profiles: `ds-mcp-host`.

## Where `ds` stops, and who continues

Name the boundary and return with its result:

- Native PLS-CADD — the model must be solved or accepted as the authority:
  edit in DS Grid, export, let PLS-CADD verify; `ds` never drives that UI.
- A document renderer — a reviewed draft must become DOCX/PDF: `ds` authors
  and lints the text, installed document tools typeset it.
- Desktop map and screen recorders — interactive geometry or motion: `ds` serves layers, tiles and still evidence only.
- The operator — the effect needs authority `ds` will not grant: approval,
  credentials, install, deploy or refusal remedy. Report the code and remedy.

## When `ds` cannot

Rule out a stop, try other vocabulary, then report as you go. You may
filter/compare `ds` files in a disposable step; name it in the sighting. Never
improvise DS data/effects or bypass `ds` with a gap file or API call.

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

Stops at: PLS-CADD, a document renderer, Desktop or operator (above).
