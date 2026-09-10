# Printout procedure — the ordered steps, by command id

The role in `SKILL.md` says what a finished print set must satisfy. This file
says the order the work happens in and which `ds` command performs each step,
so a plain request ("print the district map with our title block") does not
cost a fresh discovery of the sequence. Every id below is a live command: read
its exact request shape, refusals and authority with `ds capabilities <id>`
before calling it; never copy a schema from here. Ids whose authority is
`desktop_user` or `project` run through the paired, signed-in DS GridDesign;
`headless_*` and `none` ids need no application at all. The migration is moving
the printing steps to headless ids; when a headless twin exists on the
installed `ds`, prefer it.

Each step: **what it answers · command · the receipt to keep · the hazard.**

## 0. Fix the target

- Which project, lane and application state the work binds to ·
  `desktop.status` (paired app: lane, project, active design context) and
  `auth.project.status` (the CLI's fenced selected project) · keep both ids ·
  when they differ the *requested* project wins; switch the map only with
  `desktop.project.switch` and only for a map-owned step, never because data
  exists elsewhere.

## 1. Know what there is to print

- The printable inventory and whether each room is held locally ·
  `desktop.printing.transformers --project <id>`; headless twin `design.status`
  (rows as the service holds them) and `design.transformer.inventory`
  (active/retired/deleted) · names, kinds, cache/version rows, `omitted` ·
  resolve `omitted` before claiming "all"; `combined_transformer`, `mv_data`,
  `collisions` are project rows, not transformers.
- The rooms themselves, held before any render · `design.transformer.download`
  (repeat `--transformer`, or omit for every active transformer) ·
  downloaded / already local / dirty preserved / failed, per name · a dirty
  room is kept even with `--force`; a failed name is a missing matrix row until
  it succeeds.

## 2. Seed context from the global data

- What reference data is installed on this machine ·
  `desktop.data.rwanda.status` · per-dataset install and publication state.
- What is published globally when something is missing · `tile.global.list`,
  `tile.global.catalog` (sources not yet published; `tile.global.generate` then
  `tile.global.status` to publish, an owner decision) · resource ids · install
  by the opaque resource id from status, not the human name.
- Bring it onto the machine · `desktop.data.rwanda.install [--resource <id>]`
  (unselected: everything ≤ 500 MiB; larger datasets need explicit
  `--resource`) · compressed/expanded digests, index sizes · a skipped bundle
  never blocks printing; it becomes a named context warning on the page.
- What the project already holds for its own design extent ·
  `data.project-cache.status --project <id>` · requested vs completed coverage,
  feature counts, stale flags · **downloaded bytes prove nothing**: completed
  coverage does; coverage under a replaced source version reads stale.
- Acquire only what is missing · `data.project-cache.seed --project <id>
  [--dataset <id>] --yes` (derives coverage from every transformer's design
  extent, buffers by project policy, fuses clusters, spends provider cost) ·
  clusters, acquired vs held coverage, digests · re-running over unchanged
  design acquires nothing; that is the proof it is idempotent.
- Per transformer, the print-time context · `desktop.printing.seed-context
  --transformer <name>` (missing indexed datasets, current MV models, derived
  buildings/contours for the selected setups) · `complete`, warnings, cached
  feature counts per layer · export runs this automatically when online;
  offline export reads held data and names missing coverage — so an explicit
  seed is how you make an offline batch complete.

## 3. Templates: adopt from global, customise for the project

- What layouts exist · `report.layout.list --scope global` (published samples)
  and `--scope project` (this project's setups; needs the fenced selected
  project) · setup ids, names, revisions.
- Read one · `report.layout.get --scope <global|project> --id <setup>` · the
  layout and its **revision** · every later save needs that revision
  (optimistic concurrency); read again after any failure.
- Adopt a global template into the project · `report.layout.copy --request
  <file>` (global adoption, global promotion or same-library duplicate, one
  Brain transaction) · the new project setup, its revision, pinned source
  lineage · the global is never changed; A0 and A3 are separate setups, keep
  the canonical map id while iterating so replacing one preserves the other.
- Learn the grammar before editing · `report.layout.schema` and
  `map.print.schema --section <request|layout|edit|outputs>` · the closed
  intent vocabulary · an unknown property is a validation error, not an
  ignored option.
- Customise · `report.layout.edit --request <file>` (validate and apply ONE
  intent: title-block text, logo asset, rect moves, tables, legend, context
  layers, paper) repeated per intent, then `report.layout.save --scope project
  --request <file>` (publish with the expected revision) or
  `report.layout.update` · the saved revision · title-block wording comes from
  sourced facts (`ds assets` for the client's logos and documents), never from
  an example drawing's approvals; a PLS-CADD sheet is evidence for names, not
  a layout to copy.
- Paper-specific styling · `style.print.plan` → `style.print.create --ref
  <layer>` (a create-only `_print` clone) then `style.label.plan/set`,
  `style.appearance.plan/set`, `style.cartography.plan/set` on the `_print`
  ref, with `--paper` where offered · published style receipts · plan first,
  set with identical arguments; an existing `_print` style is never
  overwritten.

## 4. Decide what the project produces

- The project's saved output policy · `desktop.printing.settings --project
  <id>` (headless twin `report.project.settings`) · authored setting, planned
  outputs, template papers, `ready` and, when not ready, the named refusal ·
  `ready:false` is a preparation state to fix, not a reason to render a proof
  and call it done.
- Save layout + selection + per-transformer overrides · `desktop.printing.prepare
  --request <file> --yes` (project, layout, expectedRevision — empty to create
  — and a `ds.design-output-selection/v1` paper-by-format matrix; headless
  twin for the selection alone: `report.project.outputs.set --selection <file>
  --yes`) · setup id/revision and the output matrix, `ready=true` · **prepare
  can save the recipe and then fail at context staging**: read the setup and
  revision back before resuming, never replay a create.

## 5. Prove one sheet before the batch

- A document proof with no project preparation · `report.layout.render
  --request <file>` (native: layout + held GeoJSON → PDF/SVG/PNG/JPEG) · the
  file and its digest · proves the layout only; it validates nothing about
  seeding, settings or publication.
- One real sheet through the production route · `desktop.printing.export
  --project <id> --transformer <name> [--selection <file>]` · per-output
  filename, format, size, SHA-256, recorded layout/paper/orientation/
  dimensions, context warnings · `printing_execution_disabled` means a Cloud
  Run reporter answered — use the native desktop route; paper comes from the
  receipt, never the filename.
- Look at it · `desktop.printing.artifact.read` (verify committed bytes
  against the sealed receipt) then `desktop.printing.artifact.copy --out
  <file>`; rasterise the PDF locally and inspect whole page and crops per
  `acceptance.md` · the copy path and what was inspected · a render that
  succeeded is not visual QA.

## 6. District MV and custom-area maps

- The exact area · `desktop.printing.custom.area --project <id> --id <map>
  --sector <code>… [--geometry]` (1–8 connected sectors through the national
  boundary authority) · sorted codes, area SHA-256, `connected=true`.
- The request grammar · `map.print.schema --section request` (paper A0|A3,
  DPI 72–1200, landscape only, combined or per-area pages).
- Render locally · `desktop.printing.map.export --request <file>` · canonical
  filename, path, SHA-256, page count, source inventory, warnings ·
  incomplete inputs are *warned*, not refused — read the warnings before
  calling the page complete.
- What is retained and what is published · `desktop.printing.map.list`, then
  `desktop.printing.map.attach --project <id> --filename <name>` · `attached`
  or `pending` · pending is not published.

## 7. Produce the batch

- Transformer sheets · `desktop.printing.export --project <id> --transformer
  <a> --transformer <b>…` (selected outputs overwritten, unselected artifacts
  kept with their provenance) · one receipt per transformer · build the
  expected (transformer × layout × format) rows first and reconcile every
  receipt to a row; a duplicate can hide a missing one.
- The combined and compounded deliverables · `report.project.scope` (who is
  in and why others are out) → `report.project.compounded --yes`
  (server-composed ZIP with a registry row) → `report.project.archives` ·
  requested vs achieved archive layout · retired transformers are never in
  scope; a heavy composition legitimately runs to its full deadline.
- Local packaging · `report.bundle --request <file>` · ZIP path, SHA-256,
  entry count · existing output is never overwritten.

## 8. Deliver and verify

- Files to the requested destination · `desktop.printing.artifact.copy` per
  expected row · destination paths with SHA-256.
- Publication state · `desktop.sync.status`, `desktop.sync.published
  --project <id> --operation <op>` (the shared head revision), and
  `desktop.sync.retry --yes` for a recoverable failure · head revision and
  `updated_at` · a local file is not synced; a pending transfer is not
  published; the head revision is the proof.
- Reviewed cartographic pages into report delivery ·
  `map.design.attach-print --path <pdf> --map-family … --layout … --paper-size
  … --orientation … [--page-role …]` · durable artifact reference.

## 9. Record and report gaps

- Keep the delivery record (`delivery-record.md`) current at every phase
  boundary; it is the resume aid after an interruption.
- A verified gap in a command goes to `feedback.submit`, once, with the
  receipt that shows it — never a parallel ledger.
