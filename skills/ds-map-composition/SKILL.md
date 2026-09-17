---
name: ds-map-composition
description: Compose and refine DS transformer sheets, custom-area maps, district MV and project atlases through CLI/MCP, including per-transformer print exceptions.
---

# Compose DS engineering maps

Use the `ds` skill first. Discover the live printing, layout, style and map
preview contracts. Their declarations own command inputs and schemas; this
skill owns the cartographic reasoning. Do not require repository access.

Start with the live published default for the requested paper and layout family.
Read its complete layout and revision, compare the project customization, and
apply only the requested changes to a project-owned copy. An earlier preview or
local draft is not a default. Preserve the default furniture arrangement unless
the user changes it; visual dissatisfaction is a reason to return to that baseline.
For transformer sheets, read [default-based sheet placement](references/transformer-sheets.md)
before authoring A0/A3 furniture. For one crowded or unusually shaped transformer,
read [outlier adjustments](references/outlier-transformers.md); keep its exception
separate from the project default.

Read [composition guidance](references/composition.md) when choosing hierarchy,
relief, landmarks, labels or page furniture. It provides references and review
criteria, not a fixed layer whitelist or a particular project's defaults.
For one project on one page — a district sheet at 1:30 000–1:110 000 — read
[the district sheet](references/district-sheet.md): what it shows and hides,
corner-snapped furniture, scale-only overrides over governed pens, and the
headless assembly of designs, the DS Grid model and the holdings' context.

## Work from evidence

- Establish the intended reader, geographic scope, paper formats and engineering
  purpose from the request. Inspect available source inventories and canonical
  attributes through declared DS commands before authoring the recipe.
- Separate source completeness from visual emphasis. A muted or hidden class is
  an authored presentation choice; missing source data is a preparation failure.
  Never call downloaded bytes a validated model, or a locally produced PDF synced.
- Author print styles, layer ordering, label priorities, physical dimensions and
  context requests in the supported recipe. Keep live-map styles independent.
  Reuse a project recipe, or a per-transformer exception, instead of hardcoding
  a town, paper size or list of landmarks into the engine.
- Preserve model angle points, transformer roles and canonical numbering. Verify
  topology/numbering with the network owner's commands. Do not derive replacement
  numbers in a print recipe or mistake feature IDs for engineering numbers.
- Generate locally, inspect the complete page and readable detail crops, then
  revise the recipe. Sample geographically diverse sheets and dense outliers.
  Verify every page in a district pair; names alone do not prove boundary identity.
- Reprint selected canonical formats while retaining unselected artifacts. Open
  the application PDF preview and check publication separately through the
  declared attachment/sync channel. Use exact receipt identities when delivering.

## Missing controls

Search the live catalog's printing, layout and geographic-data vocabulary before
claiming a control is unavailable. Use the typed feedback workflow for a confirmed
missing control. Never simulate an unsupported relief option with invented JSON,
a screen capture presented as vector engineering output, or undocumented access.
A renderer capability is not an end-to-end CLI capability until acquisition,
recipe authoring, offline retention, rendering and preview all support it.
