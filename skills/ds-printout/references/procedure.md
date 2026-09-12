# Printout procedure — the ordered steps, by command id

The role in `SKILL.md` says what a finished print set must satisfy. This file
says the order the work happens in and which `ds` command performs each step,
so a plain request ("print the district map with our title block") does not
cost a fresh discovery of the sequence. Every id below is a live command: read
its exact request shape, refusals and authority with `ds capabilities <id>`
before calling it; never copy a schema from here.

Two hosts run this procedure, and the id's authority says which:

- **Server / headless** — `headless_*` and `none` ids. They restore the native
  user (`auth.status`, `auth.project.use`) and run on this machine's holdings.
  This is the production route: a signed-in `ds` on the Linux server or any
  workstation, no application open, offline-first — reads never reach a
  provider, and an unheld layer is omitted and named rather than fetched.
- **Desktop / paired** — `desktop_user`, `desktop_pairing` and the `project`
  ids that still run through the paired application. Use them only for the
  steps marked *desktop-only* below; never open a desktop to do what a headless
  id already does.

Each step: **what it answers · command · the receipt to keep · the hazard.**

## 0. Fix the target

- Which user, lane and project the work binds to · `auth.status --lane <lane>`
  (native user, device, fenced selected project) and `auth.project.use
  --project <id>` to select · keep `data.auth_context.selected_project` · the
  *requested* project wins; a desktop's map project is irrelevant to headless
  steps. On a paired step also read `desktop.status`.

## 1. Know what there is to print

- The printable inventory · `report.transformers --lane <lane>` (86 rows for a
  project with 86 active transformers) and, for lifecycle, `design.transformer.
  inventory` · names, kinds, `total` · `combined_transformer`, `mv_data`,
  `collisions` are project rows, not transformers; resolve every `omitted`
  before claiming "all".

## 2. Hold the project's context on this machine

Context is machine-local. Two families, seeded differently, both through the
same door:

- What the project declares and holds · `data.project-cache.status --lane
  <lane> [--dataset <id>]` · per dataset `seeded`, `ready`, `feature_count`,
  requested vs completed coverage, `stale`, `last_error`; `catalog.read` says
  whether the reference catalogue answered · **downloaded bytes prove
  nothing**: completed coverage does; `seeded` is not `ready`; a catalogue
  that cannot be read still lists held rooms.
- Acquire what is missing · `data.project-cache.seed --lane <lane> [--dataset
  <id>] --yes` · per dataset `clusters`, `acquired`, `covered`,
  `feature_count`, `error` · **national catalogue layers** (roads, rivers,
  boundaries, schools, health facilities, churches, settlements, offices,
  peaks — what ds-brain publishes for the country) are installed on the
  machine once from their verified bundle and subset per project;
  **buildings and contours** are acquired per project through ds-brain's
  `query_print_context`. Re-running over unchanged design acquires nothing —
  that is the idempotence proof. One dataset's failure is its own row; the run
  continues. A row without a published bundle refuses
  `reference_bundle_unavailable` and is never fetched from BigQuery.
- Seed on the first print instead · `report.project.export … --seed` runs the
  same acquisition for each printed transformer's own cluster before it
  renders, so the first print request seeds and later runs acquire nothing.
  Without `--seed` a print never reaches a provider.
- *Desktop-only*: `desktop.printing.seed-context` (survey layers, local
  layers, the project's DS Grid MV models) and `desktop.data.rwanda.install`
  (the villages DSAB the Rwanda engine stamps with; headless export reads it
  from the shared root or `--admin-bounds`).

## 3. Templates: adopt from global, customise for the project

- What layouts exist · `report.layout.list --scope global` and `--scope
  project` · setup ids, names, revisions.
- Read one · `report.layout.get --scope <scope> --id <setup>` · the layout and
  its **revision** · every save needs that revision; read again after any
  failure.
- Adopt a global template into the project · `report.layout.copy --request
  <file>` · the new project setup, its revision, pinned source lineage · the
  global is never changed; A0 and A3 are separate setups.
- Learn the grammar before editing · `report.layout.schema`, `map.print.schema
  --section <request|layout|edit|outputs>` · an unknown property is a
  validation error, not an ignored option.
- Customise · `report.layout.edit --request <file>` per intent (title block,
  logo asset, rect moves, tables, legend, context layers, paper), then
  `report.layout.save --scope project --request <file>` with the expected
  revision · the saved revision · a refused save names the field
  (`print_layout_invalid`); if the field is valid the deployed validator is
  older than this client and must be redeployed — never strip a valid field.
- Styling is global, not per project · the layer's governed style through
  `style.read`, `style.dimension.plan/set` (second field → halo, opacity,
  size; e.g. river `type` → width), `style.cartography.plan/set` (line type,
  casing, hatch), then `style.print.plan/create` for the `_print` clone every
  print template inherits · published style receipts · plan first, set with
  identical arguments.

## 4. Decide what the project produces

- The output policy · `report.project.settings --lane <lane>` · `outputs`,
  `papers` (one per named PDF layout), `ready`, else the named refusal.
- Save the selection · `report.project.outputs.set --selection <file> --lane
  <lane> --yes` (`ds.design-output-selection/v1`: prints by layout id, then
  kmz/shp/xlsx) · `saved`, `parameter`, placements · it refuses
  `print_setup_not_adopted` for a setup the project does not hold; publish or
  copy the setup first.
- *Desktop-only*: `desktop.printing.prepare` does layout + selection +
  per-transformer overrides in one transaction; per-transformer overrides have
  no headless twin yet.

## 5. Prove one sheet before the batch

- One real sheet through the production route · `report.project.export
  --transformer <name> --out-dir <dir> --lane <lane> [--seed]` · the batch
  receipt, and per row `artifacts`, `print_context.{sha256,layers,omitted}`;
  `context.selected_layers`, `context.warnings`, `context.notes` ·
  `omitted` names selected layers with no features in the extent or not held;
  `print_context_unsupported` is a survey/local-layer source (desktop-only);
  `admin_bounds_unavailable` wants the villages asset (`--admin-bounds`).
- Look at it · the files are local: rasterise
  `<out-dir>/<transformer>/<transformer>.<setup>.pdf` and inspect whole page
  and crops per `acceptance.md`; `report-run.json` beside it records
  `print_context_sha256` and the layers · a render that succeeded is not
  visual QA.
- A document-only proof · `report.layout.render --request <file>` · proves
  the layout, nothing about seeding, settings or publication.

## 6. District MV and custom-area maps — *desktop-only today*

- `desktop.printing.custom.area`, `map.print.schema --section request`,
  `desktop.printing.map.export`, `desktop.printing.map.list`,
  `desktop.printing.map.attach` · unchanged; the server has no map-export
  owner yet, say so rather than substituting a transformer sheet.

## 7. Produce the batch

- Transformer sheets · `report.project.export --out-dir <dir> --lane <lane>
  [--concurrency <n>] [--seed]` (every active transformer, or repeat
  `--transformer`) · `<out-dir>/report-batch.json` and one
  `<transformer>/report-run.json` per completed row · build the expected
  (transformer × layout × format) matrix first and reconcile every receipt to
  a row; a duplicate can hide a missing one; the command exits non-zero only
  when nothing completed.
- Publication · add `--publish` (release reporter only) · rows gain the
  durable Server-sync queue identity, `publication.state =
  queued_for_server_sync` · queued is not published; the native Server sync
  pump (`ds server serve`) transfers committed batches, and the shared head
  revision is the proof.
- The combined and compounded deliverables · `report.project.scope` →
  `report.project.compounded --yes` → `report.project.archives` · packages
  individual reports that already exist in the cloud registry; it computes
  none.
- Local packaging · `report.bundle --request <file>` · ZIP path, SHA-256.

## 8. Deliver and verify

- Files to the requested destination · copy from `<out-dir>`; keep the
  SHA-256 from `report-run.json` beside each file.
- Publication state · `server.activity` (the server's Sync Center activity),
  `desktop.sync.status` / `desktop.sync.published` on a desktop · head
  revision and `updated_at`.
- Reviewed cartographic pages into report delivery · `map.design.attach-print`.

## 9. Record and report gaps

- Keep the delivery record (`delivery-record.md`) current at every phase
  boundary; it is the resume aid after an interruption or a redeploy.
- A verified gap goes to `feedback.submit`, once, with the receipt that shows
  it — never a parallel ledger. (`feedback.submit` is desktop-paired today;
  on a server, record the receipt for the next paired session.)
