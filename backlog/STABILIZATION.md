# Stabilization

For Codex, on canonical `run`. Opened 2026-10-07 by the owner after the 7
October test sweep. There is no deployment pressure.

**Priority (owner, 2026-10-07):** this file is the owner's current priority
order. Take its items before the rows of [ORDER.md](ORDER.md); ORDER.md rows that
link here are worked from here.

**Scope:** this work may touch any other backlog surface it intersects:
[OPEN.md](OPEN.md) §2 formatting and release gates, §11 PLS exchange, §12
structure usage, §18 libraries, §23 JSON UI, §25 CLI reliability; and
[SEEDING.md](SEEDING.md) §18. When a slice fulfils part of one of those
entries, update or delete that entry instead of duplicating it here. Delete
each item below once it is done, as for every backlog file.

**How to work it:** one small slice at a time. Prove it with the owning tests,
then commit to `run` in its owning repository. Never use a worktree.

Review requests and incoming findings live in [BUG-EVIDENCE.md](BUG-EVIDENCE.md).
The coding agent re-reads both files between slices.

## Owner rulings that apply

- **No more breaking schema bumps:** follow [PRINCIPLES.md §2](PRINCIPLES.md#2-how-to-work-an-entry).
- **Never work in worktrees.** Before any release, run the owning full suites
  on canonical `run`.
- **ds-server keeps only the canary package.** The stable `ds-cli-headless`
  package was uninstalled; only canary 0.1.6 and its service remain.
- **National ID is not urgent and gets no code now.** See the
  [OPEN.md §2](OPEN.md#2-green-run-and-release-gates) note: DS handles national
  IDs, and the `a69402e` restriction came from the datasolutions.rw product
  context.

## 2. Suites to prove on settled tips

| Suite | Command | Last result (7 October) |
| --- | --- | --- |
| ds-network-reporter | `cargo test --locked --no-fail-fast` | All green |
| ds-cli | `DS_REPORT_BIN=<reporter target>/debug/ds-report DS_SOLAR_BIN=<ds-solar target>/debug/ds-solar cargo test --locked --workspace --all-features --no-fail-fast` | Full settled-tip run: 1,841 tests across 108 targets green (15 explicitly ignored); workspace/all-targets/all-features Clippy also green. |
| ds-network | `cargo test --locked --workspace --no-fail-fast` | Full settled-tip run: three known private-corpus failures in two targets; all other targets passed. Fixes remain in OPEN.md §11 |
| ds-solar | `cargo test --locked --workspace` | Full settled-tip run: 427 tests across 58 targets green. |

- The three private-corpus tests are `conversion_planner::corpus_planner_bak_matches_direct_ingest_model`, `conversion_planner::corpus_planner_normalizes_characterized_19_01_source_to_16_81` and `ds-grid-wasm import_convert::corpus_huye_backup_imports_through_the_wasm_handle`.
- The stable `/usr/bin/ds-report` is uninstalled on ds-server, so point
  `DS_REPORT_BIN` and `DS_SOLAR_BIN` at freshly built engines.
- All five lockfiles are current (`cargo metadata --locked`).

## 3. Release pins, last

Once the slices above have landed:

- set `ds-cli/pins/ds-command-kernel.rev` to the kernel `HEAD`, exactly;
- point `ds-cli/pins/ds-client-core.rev` at a commit whose native crates
  (`ds-client-core`, `ds-report-host`, `ds-sync-runtime`, `ds-layer-store`,
  `ds-project-data`, `ds-edge-authority`) are tree-identical to `HEAD`;
- set `ds-cli/pins/ds-network.rev` to the ds-network `HEAD`;
- in `ds-network-reporter/pins/native-build-sources.json`, set the kernel and
  ds-network entries to the same tips (ds-solar stays `bdae6aa`);
- prove with `cd ds-cli && scripts/with-network-deps.sh true`;
- then run `cd ds-web && bash -l -c './linux-server-deploy.sh prod --check'`.
  It builds and publishes nothing; the version gate passes (source 0.1.7,
  production `0.1.7-r86`). Publishing is the owner's decision.

## Owner requests, investigations and bug evidence (2026-10-07)

The owner puts requests here. Related OPEN.md and OPEN-X.md work is grouped
below (A–D, moved 2026-10-07) so it is fixed together with these items.

### 4. Swift editing: retyping one structure, a range or a selection must feel instant

**Owner:** a retype takes far too long. Retype one, retype a range, retype the
selection and similar edits must be fluid, or the field team will not use
editing. The owner suspects the whole model is recomputed on every edit; the
trace below confirms it.

**Measured** on ds-server, 7 October:

- Builds: debug, because it is the only `ds` that reads today's schema-5
  packages. Installed canary 0.1.6 refuses them, so no release timing exists
  yet.
- Model: Nyamagabe MV Model 1, "2026-10-02 oracle reference copy" (stable-lane
  copy `local-aad500c1e08847db`, revision 8, 1.4 MB, schema 4), structure 618.
- Writes went to scratch packages only; no working copy changed.

| Step | Time |
| --- | --- |
| CLI `dsgrid structure retype` dry run (any type; nothing written) | 6.0–6.2 s, 180 MB, one core |
| CLI write 618 → `j-w-S255.012`, from the stored copy (schema 4) | 7.3 s |
| CLI write 618 → `j-w-S325.014`, from a schema-5 package | 3.9 s |
| `dsgrid validate` (open and validate only), schema 4 / schema 5 | 5.5 s / 1.9 s (`inspect` alone: 0.1 s) |
| Whole-model `engineering_issue_layer` (the Desktop's analysis request) | 5.6 s |

**Trace: every edit reprocesses the whole model several times.**

1. **Desktop apply:** `GridProfileRetypePicker.svelte` →
   `gridCommitStructureRetype` (`ds-web/src/lib/grid/session.ts:749`) applies
   the edit in the worker, which is cheap.
3. **Auto analysis:** revision changes re-offer auto analysis (on by default).
   The native session refresh still rebuilds the Profile scene for every band
   and runs default screening without the receipt-advanced demand cache before
   the cached issue layer. It returns the full scene and bounded issues for
   worker installation. Localize scene preparation and share native demands.
4. **CLI path** (`ds-cli/crates/ds-cli-dsgrid/src/structure/retype.rs`,
   `mutation.rs`):
   - decode the whole package; schema-4 copies re-run the 4 → 5 migration and
     its verification on every open;
   - open a session;
   - run `line_angles_by_structure` over every structure;
   - apply, checkpoint, then re-emit the whole package with all its assets,
     hash it and write it.

**Patch direction, smallest first:**

2. **Scope the recompute.** A retype touches one structure: recompute its
   incident tension sections, those sections' structure usage and the
   clearance of the adjacent spans. Reuse cached results elsewhere, keyed by
   per-section or per-structure input digests (the engine already has a
   per-revision `engineering_input_root`).
3. **Rebuild only the edited bands.** Rebuild only the Profile band(s) that
   contain the edited structures; keep the rest of the scene.
4. **Background analysis.** Auto analysis stays on (owner, 2026-10-05), but it
   runs scoped and in the background. It never blocks the next edit, and it
   never opens the Issues drawer (item 6).
6. **CLI fixes:**
   - store a migrated working copy once in schema 5, using the existing
     migration (no new bump);
   - compute line angles only for the selected structures;
7. **One path for batches.** Retype one, a range or a selection through the
   same single transaction, followed by the same scoped recompute.

**Acceptance:** on this model, in a release build, retyping 618 shows the new
type and updated usage in well under 0.5 seconds, with no whole-model package
round trip. Edits stay dirty in RAM until explicit Save. Recalculation reaches
incident and adjacent tension sections and their affected supports, spans and
bands, stopping propagation at affected tension-section ends during interaction.
Wider sway/downstream propagation waits for reconciliation; whole-model analysis
stays available on demand.

### 5. Bug evidence found during the trace

- **The CLI cannot co-work with the dev Desktop.**
  - `dsgrid.structure.retype` has no `--target`, so the CLI cannot edit the
    model the Desktop has open.
  - `dsgrid.model.list` reaches only the stable and canary catalogues, not the
    dev Desktop's `local` lane.
  - `desktop.status` and `map.profile.view` report the open model's id but not
    its name.
  - `map.profile.view --target desktop:<id>` does report the selection
    correctly: structure 618.

### 11. Intermittent: a selected ground point shows no highlight and no vertical handle (unproven)


**Update (owner, 2026-10-07):** the likely cause is the Profile warm-up window; see §12.
Not reproducible after 13:25 UTC. The owner reports it working again and
suspects the debug state.

**Evidence (7 October):**

- **13:22 screenshot** (dev Desktop, "Nyamagabe MV Model 1 - structure usage
  oracle test copy v1", edit mode):
  - Properties shows ground point `ter-423815a32fed4309-4029` (elevation
    2067.902344, GROUND).
  - The Profile neither highlights it nor shows its vertical-move handle.
- **13:24:26:** `ds map profile view --target desktop:5d53513e…` reports
  structure `str-423815a32fed4309-5` as the Profile selection, not that ground
  point. Properties and the Profile selection disagreed.
- **13:25:17:** the selection is `ter-423815a32fed4309-3370`
  (`kind: terrain_points`), with edit mode on and ground points visible. The
  owner reports it working.
- **What the code requires:**
  - The highlight (`ds-web/src/lib/grid/profile-draw.ts:946-958`) and the
    handle (`GridProfileCanvas.svelte:172-189`) need the selected id to be in
    the scene's `band.terrain_points`.
  - The handle also needs edit mode, visible ground points, `scene.axis_pin`,
    a finite elevation and `movingGroundPoint` false.
  - `GridProfilePanel.svelte:1686-1714` wires all of these unconditionally.
- **Build state:** the dev Desktop's native binary was built at 12:06 UTC
  (debug), while its TypeScript hot-reloads. Since then, kernel `4258972`
  (Profile symbol vocabulary), ds-network `70d36346` and ds-web `0b3794b9c`
  have landed, so it ran a mixed state.

**Hypotheses (none executed):**

1. Properties showed a Plan-selected ground point while the Profile selection
   still held structure 5. The Plan/Properties selection diverged from the
   Profile selection (see group A below, two-way tracking).
2. A `movingGroundPoint` left set by an interrupted ground move hides the
   handle. This would not explain the missing highlight.
3. The mixed native/TypeScript dev build.

**Next check:** on a freshly restarted or release Desktop, select a ground
point first in the Plan, then in the Profile. Read `ds map profile view` after
each click, and confirm that the selection kind and id match a point in the
Profile scene.

### 12. Profile warm-up: early selections are floppy; animate the status button during any computation

**Owner, 2026-10-07:** right after the Profile opens, the first selections are
"floppy", as if it needs to warm up. This is what blocked moving the ground
point (§11). Every computation must animate the toolbar's status button.

**Evidence** (code; not timed in the UI):

- **One queue.** Opening the Profile queues, one after the other on the same
  model client (`nativeProfileCommand` serializes per client,
  `ds-web/src/lib/grid/profile-native.ts:44-49`):
  - the native `project`: a full `checkpointDsgrid()` emit, a JSON
    `Array.from` IPC transfer, a full decode, and the whole scene;
  - then the automatic whole-model analysis
    (`GridProfilePanel.svelte:1210-1226`).
- **Selection before the first scene.** Until the first native scene exists,
  case-layer requests refuse with
  `profile_case_base_unavailable: draw the native Profile first`
  (`nativeProfileCaseCommand`, `profile-native.ts:58`). Selection-dependent
  reads (selected usage, scene ids for highlights and handles) wait, or miss.
- **Comparable costs** (§4, debug builds): a full open and validate takes
  1.9–5.5 s; the whole-model issue analysis takes 5.6 s.
- **Status button.** The toolbar's leftmost button spins only for edit
  reconciliation (`reconciliationStatus.phase === 'pending'`,
  `GridProfilePanel.svelte:1455-1470`). Scene building (`loading`), the
  automatic analysis, case layers and usage screening animate nothing.

**Needed:**

- One busy signal fed by every native Profile computation: project,
  analysis, case layer, usage and reconciliation. The status button animates
  while any of them is in flight, and its tooltip names it.
- Selection and handles work immediately from the Plan and scene data already
  shown. Heavy analysis runs in the background and never blocks selection
  (§4 and the edits-in-RAM ruling in BUG-EVIDENCE.md).
- **Reproduce:** restart the Desktop, open the Profile, and click a ground
  point within the first seconds. Read `ds map profile view` repeatedly to
  time when the selection, the scene and the analysis settle.

### 13. Print templates: wrong MV/Overview grouping, missing Front matter, transformer-only subject

**Owner, 2026-10-07:**

- MV Sheets → Network should list plan-and-profile A3 landscape only.
- Overview Sheets should hold the maps.
- A **Front matter** category is missing even though its pages are defined.
  Front matter can select DS Grid MV model(s).
- The catalogue and previews already work in the CLI. The Desktop should only
  list, group and render them from that JSON, writing little beyond small pen
  settings. Delete the TypeScript that does more than that.

**Evidence** (read-only; data from installed `ds-canary report layout list
--scope project --project wnmajona_edcl_rural_electrification_template`):

- **Data.** The project holds 12 custom layouts.
  - The two district maps are stored as `mv_model / network`:
    - `nyamagabe-district-mv-a3-core-v0` (A3 portrait, rev `38eabe5e…`);
    - `nyaruguru-district-mv-a3-landscape-core-v0` (A3 landscape, rev
      `f4564fbd…`).

    Both are inherited from `arjgpydw_nyam`. This is why they show under MV
    Sheets → Network.
  - `project-overview-a0/a3` are `project / overview`. `transformer-focus-a0`
    and `a3-landscape-rwanda-project-lv` are `lv_transformer / network`.
  - Seven layouts are unclassified. No plan-and-profile layout is among the 12.
  - The plan-and-profile A3 landscape sheet isn't a Layout at all. It is a
    reporter engine configuration, `ds.grid-plan-profile-print/v3`, rendered
    by `report.plan-profile-config` (`ds-cli/docs/reference/report.md:1150`).
    `catalogue::Location` has only `layout` and `governed_html`
    (`catalogue.rs:40-53`), so the catalogue cannot list it.
- **Grouping follows stored classification only.**
  - `template_catalogue` groups by `classification` alone
    (`src/printing/catalogue.rs:258-340`). Names and paper never classify
    (`docs/printing.md:28-37`).
  - Nothing restricts `mv_model / network` to an A3 landscape plan-and-profile
    sheet. `set_classification` accepts any pair.
- **Front matter is defined but never listed.**
  - `Purpose` has four values: `lv_transformer`, `mv_model`, `project`,
    `voltage_drop` (`catalogue.rs:14-19`). The front-matter roles (Cover,
    Naming, SheetIndex, KeyMap, KeyPlan, Legend, GeneralNotes, LegendNotes,
    Revisions) exist only as roles under one of those purposes
    (`catalogue.rs:241-249`).
  - The governed front-matter pages come from `report.standard.list` as
    `kind: standard`. The catalogue drops every one of them:
    `Some("standard") => continue` (`catalogue.rs:109`). The contract says
    "Governed booklet documents retain their existing consumer"
    (`docs/printing.md:71`).
- **The preview subject is always an LV transformer.**
  - A preview is "one transformer rendered" (`docs/printing.md:537`).
  - `printing::preview::plan` refuses a non-transformer with
    `print_preview_not_a_transformer` (`src/printing/preview.rs:150-160`).
  - The Desktop picker lists only printable transformers
    (`ds-web/src/lib/components/printing/PrintingSetup.svelte:246-254`). In
    the owner's screenshot, an MV template previews against LV transformer
    `agahehe`.
  - No subject exists for a DS Grid MV model or a map extent.
- **The Desktop does more than read.**
  - It reads layouts and governed pages straight from ds-brain:
    `/api/v1/printing` (`ds-web/src/lib/printing/shared.ts:153`) and
    `/api/v1/printing/standard` (`ds-web/src/lib/printing/governed.ts:18`).
  - It runs its own cache and read plan, then sends the rows through WASM
    `template_catalogue` (`ds-web/src/lib/printing/catalogue.ts:25`).
  - It orchestrates transformer previews in the 2,117-line
    `PrintingSetup.svelte` (`:469-547`).
  - No `ds report layout catalogue` (grouped JSON) command was found in
    `ds-cli/docs/reference/report.md`. The CLI has `report.layout.list`,
    `report.standard.list` and `report.layout.render`, plus `report.project.export
    --preview-layout` for previews.

**Needed** (one slice each; prove each slice through `ds` before the next):

1. **Kernel: Front matter purpose.**
   - Add `front_matter` to `Purpose`, labelled "Front matter", holding the
     front-matter roles.
   - Project the governed `kind: standard` pages under it instead of skipping
     them.
   - The change is additive. Check older readers against PRINCIPLES §2: no
     breaking bump.
2. **Kernel: subject follows purpose.**
   - `lv_transformer` previews against one transformer.
   - `mv_model` and `front_matter` preview against one or more DS Grid MV
     models.
   - `project` previews against a map or project extent.
   - `preview::plan` takes the subject and refuses a mismatch with a keyed
     reason. This replaces the transformer-only rule.
3. **Kernel: MV Network admission.**
   - Give the catalogue a storage location for the plan-and-profile print
     configuration, so it can be listed under MV → Network.
   - `mv_model / network` admits only that A3 landscape sheet.
     `set_classification` refuses anything else, and the catalogue flags
     stored mismatches.
4. **CLI/MCP.** Add one grouped catalogue read and a subject-aware preview, so
   the Desktop and agents read the same JSON.
5. **Desktop.**
   - The Print templates panel renders that JSON: groups, entries, subjects
     and preview pages.
   - Delete the replaced TypeScript: the direct ds-brain reads, the
     catalogue cache and read plan, and the transformer-only subject logic.
   - Keep pen-setting writes.
6. **Data (seeding, not code).** Reclassify the two district maps in the
   Rwanda Template to `project / overview` with `report.layout.edit`
   `set_classification` and `report.layout.update`. The Template's seeding
   does this (`SEEDING.md`).

**Proof:**

- `ds report layout list` and the new catalogue read show:
  - the district maps under Overview;
  - the front-matter pages under Front matter;
  - only plan-and-profile A3 landscape under MV → Network.
- An MV preview chooses a DS Grid MV model and never `agahehe`.

### 14. Print previews miss map context

**Owner, 2026-10-07:** printing previews are missing context. Take this
together with §13 and group E.

**Evidence** (code; not reproduced, because the installed canary `ds` predates
the current layout contract, see below):

- **TypeScript gathers preview context per transformer.**
  - The Desktop preview calls
    `prepareDraftTransformerPrintContext(project, uid, transformer, room.layers, draft)`
    (`ds-web/src/lib/components/printing/PrintingSetup.svelte:550`). This runs
    `collectTransformerPrintContext`
    (`ds-web/src/lib/reports/transformer-print-context.ts:192-268`), a
    TypeScript loop that fetches each context layer.
  - The kernel only lists which layers print and sets
    `acquire_missing: online` (`src/printing/mod.rs:3073-3084`).
- **Context is clipped to one transformer's design plus 200 m.**
  - Catalog layers become a project subset bounded by the transformer's
    design and `buffer_m ?? 200`
    (`transformer-print-context.ts:52-54, 82, 225-232`).
  - DS Grid MV, Open Buildings and contours go through
    `retainedDerivedPrintContext(..., transformer, design, ...)` (`:255`).
  - A district or overview sheet covers far more than that buffer. Its
    context outside the buffer is never staged. This compounds §13: these
    templates preview against an LV transformer (`agahehe`).
- **Failures degrade silently into warnings.**
  - Any catalog or derived context error becomes a warning, and the preview
    renders without that layer. Only survey and local layers stop the preview
    (`transformer-print-context.ts:257-263`).
  - The warnings are appended to the preview status (`PrintingSetup.svelte:570`).
  - Offline, an unheld catalog layer fails with "This layer is not available
    offline" (`:242`).
- **Two contract sentences to reconcile.**
  - `docs/printing.md:282` says "a read or preview never acquires providers".
  - `docs/printing.md:389-391` lets `preview_context_preparation` prepare
    missing inputs online, before the read-only render. The kernel and
    TypeScript follow the second sentence.

**Needed:**

1. **Kernel: context extent follows the §13 subject and the sheet.** The
   context extent is the map viewport's coverage for that subject:
   - one transformer's design plus buffer;
   - the MV model's corridor;
   - the project or district extent.

   Never use a fixed transformer buffer for MV or overview sheets.
2. **Native: one context acquisition behind `ds`.** The CLI's export path and
   the Desktop preview call the same native preparation, which returns layers
   plus a typed omission per missing layer. Delete the TypeScript loop and
   its gateway reads (PRINCIPLES §5; OPEN §23).
3. **Preview status: name every omission.** List each missing layer with its
   reason: hidden by the layout, unheld offline, empty in the extent, or
   failed. A degraded preview is labelled degraded, never shown as complete.
4. Reconcile the two `docs/printing.md` sentences in the same slice.

**Reproduce once a current canary is installed:**

1. Run `ds report project export --preview-layout` for
   `nyaruguru-district-mv-a3-landscape-core-v0` and for `project-overview-a3`
   in `wnmajona_edcl_rural_electrification_template`.
2. Compare the staged context layers and omissions with the sheet's map
   coverage.

**Tool note:** installed `ds-canary` (2026-09-29) lists the 12 layouts, but
`report layout get` refuses 10 of them: "the authentication or project
response did not match its closed contract". Debug `ds` builds route native
API calls to the local Go authority (ds-cli `980462e7`), so they can't read
the cloud project either.

## Moved from OPEN.md and OPEN-X.md: related work, fix together (owner, 2026-10-07)

These entries share code with the stabilization items above. Fix each group in
the same slices as its related item, so nothing is read or fixed twice. Owner
decisions keep their **Input needed** and **Boundary**.

### A. Profile editing, selection and docking (with §4, §11, §12 and BUG-EVIDENCE.md retype/closure)

- **One Plan selection and independent two-way tracking** (owner, 2026-10-07): remove the non-working Profile lasso and its native contracts, `ds-grid-engine/src/profile_lasso.rs`/descriptor/pick-index hooks, WASM binding/tests, web overlay/binder/protocol and Profile-only adaptive-runner tests. Keep the Plan/map lasso, field/layer filters, Within/Intersects/Outside predicates and native composition. Audit shared `profile_hit`, `lasso.ts`, `lasso-run.ts` and `ds-canvas/src/selection.rs` consumers before deletion. Preserve Profile-click navigation to Plan; selecting a displayed element in Plan must navigate Profile. Expose independent "Plan follows Profile" and "Profile follows Plan" switches through the kernel/CLI, then render them. Reconcile the earlier `c28ee1a5` Outside implementation; the Plan predicate remains. Prove selection identity/revision isolation, both direction switches and removal of obsolete discovery/contracts.

- **Native selected-case Profile binding**: complete criterion/case/weight-basis transport, separate scene-cache identities and native/Web bindings for the selected display case. Paint usage, failures, uplift and clearance findings from the Rust projection; screening percentages must retain their declared limited scope. Current runtime/Windows acceptance belongs in SEEDING.md, not a repeated implementation-history paragraph.

- **Show loading, uplift and failures in Profile for the selected case, as PLS-CADD does** (owner, 2026-10-02; DS Grid compute track): a criterion-set/case (and weight-basis) selector drives the drawn `display_case` and a per-case `structure_usage_screening`; each structure gets a source-backed usage-% label (owner-defined colour bands are separated into [OPEN-X.md](OPEN-X.md#14-profile-display-and-supervised-editing)), uplift is marked from `uplift_case_present`/`uplift_failure`, and clearance violations are drawn at their station and elevation. Profile paints none of these today; only the selected structure's screening % shows. `profileAtlasSceneJson` accepts `display_case` but ds-web never sends it; `clearance_report` cannot run for one case (no case ids in `ClearanceReportOptions`) and its findings carry no scene coordinates; `conductor_spans[].clearance` follows the display case but is untyped and unpainted in ds-web.

- Make selection, hover, range, drag, and cross-alignment switching responsive on a large real model; measure the slow path. Use Rust hit/pick/snap rules, with ground-point collision priority, a hover cursor, sensible overlap cycling, and free select/zoom in Plan and Profile. Prove popout fit stable on the live profile.

- Keep one universal, resizable, flat Properties dock for Plan, Profile, and tables. Useful editable facts come first, immutable IDs last. Remove duplicate compact dialogs. Use a pen for edit context; show model name, revision, dirty/save/discard state only while editing. Model cloud-save belongs in the model menu; continuous working-head synchronization must not bump the intentional named version.

- Move a structure instantly within legal station bounds, with a centered hitable slider and no filled ring. Warn before crossing a neighbor or moving an angle duty off the route angle; imported supports outside their alignment need explicit native findings and review, never silent snapping. Retype inline with current type selected and searchable assembly-first choices; the Profile source now shows exact native unresolved-attachment findings after an accepted draft retype, but repair controls and installed/live proof remain. Keep spotting/stringing revisions local to affected angle and tension intervals and terminate each spotting session cleanly.

- Complete the CLI's revision-pinned Profile edit bridge for a full human/CLI edit cycle without inferring or overwriting the person's choice. Native camera/selection paths are source-tested. An incompatible retype yields exact unresolved attachments, but the CLI's package-backed copy cannot persist that invalid draft (`package_emit_failed`); wire the new pinned native draft checkpoint through the local catalogue while keeping strict export/publication gates. Keep cloud working revisions synchronized without a version bump during ordinary iterations; make bumping a version intentional, and never upload local-dev compute artifacts. Implement scoped proposals, explicit decision/checkpoint handling and an HTML modification-note asset; prove revision fences and refusal paths with fixtures. The human demonstration, chosen changes and review of the resulting notes are in [OPEN-X.md](OPEN-X.md#14-profile-display-and-supervised-editing).

- `d8529cff` (minor): Retype keeps material-specific staking facts (E-STEEL on a wooden H-pole).

- **Selection-aware context menu across editing contexts** (owner, 2026-10-07): extend LV design’s selection filter/actions to DS Grid model and general geographic editing. The active editing context selects applicable actions through declared kernel/CLI contracts; Web/Desktop render that JSON. Replace the current Grid right-click menu limited to Measure/Exit/New/Fix Popup. Trace `ds-web/src/lib/components/map/SelectionFilterDialog.svelte` and existing LV/native selection contracts; preserve the same selection context and exact project/model/revision fences across CLI and UI.

- **Nyamagabe LV transformer editing opens a generic map menu** (`d654bfcc`): prove the affected installed session can select the design transformer with a cold index, then move, edit, save and reload it without changing another transformer.

Owner decisions (from OPEN-X.md §14):

- **Usage-% colour bands** (owner request, 2026-10-02): no bands are defined for per-case Profile labels. **Input needed:** choose thresholds and colours, and confirm their meaning for limited-scope screening versus qualified capacity. **Boundary:** seeding semantic colour bands. Native case transport, labels, uplift/clearance findings and UI binding continue in [OPEN.md block 14](OPEN.md#14-plan-and-profile-authoring); do not invent safety bands or hide an unknown verdict.

- **Human/CLI edit demonstration and chosen changes**: the requested full edit cycle includes a person demonstrating edits, deciding which scoped proposals to accept and reviewing modification notes. **Input needed:** that demonstration and the intended edits on an identified model/revision. **Boundary:** applying the person’s choices and marking notes reviewed. Agents first finish and prove pinned draft checkpoints, proposal/decision handling, export gates and HTML note generation in [OPEN.md block 14](OPEN.md#14-plan-and-profile-authoring); ordinary code testing is their responsibility.

### B. PLS-CADD `.bak` export (with outputs/pls-bak-export-plan-20261007/PLS-BAK-EXPORT-PLAN.md and BUG-EVIDENCE.md NUM)

- `e258b287` (blocker): PLS export plan accepts unsupported edited model but convert cannot emit backup. Complete the model-menu `.bak` export vertical in the seven bounded slices diagnosed in `outputs/pls-bak-export-plan-20261007/PLS-BAK-EXPORT-PLAN.md`: engine plan-time library admission and unpinned/qualification warnings; truthful CLI failed/partial outcomes; Restore/report gates; composition pin preservation or refusal; native flow with pinned-plan UI preview/apply; shared version intents; exact per-pin cloud state. This advances the existing §11, §18, §23 and §25 items without another gap list. Each slice needs owning tests and a real filed-model replay through `ds` before landing; byte verification never implies native Restore or solver approval.

- `4af28da3` (minor): Model 2 package cannot export: 'preserved PLS source version could not be inspected'.

- `8f6e3ff1` (major): Alignment-revised .dsgrid cannot return to PLS-CADD: export refuses on pre-existing PLS-POLE 20.01 library members, sync has no writer for the edit class.

### C. Library identity and external libraries (with §5 and BUG-EVIDENCE.md pin evidence)

- **External libraries and explicit impact-plan apply**: implement the approved propagation contract in `ds-network/docs/contracts/dsgrid-plscadd-library-boundary.md` ("External references and explicit propagation"). Keep immutable exact release/member pins in each model revision, separate governed follower metadata, read-only impact plans and explicit authorized apply with expected-head fences and honest partial receipts. Mark old results stale in the new revision, recompute headlessly and expose changed results for review without granting engineering approval. Remove duplicated full library payloads from normal model packages through the exact shared Desktop/Server release cache; offline missing/corrupt dependencies refuse. Keep third-party adapters as evidence translators and evolve compatibly inside final schema 5. Prove native resolution, refusal, project/revision isolation and CLI/MCP request/result parity before UI bindings. This broad implementation continues here after the owner approved explicit impact-plan apply on 2026-10-07.

- **Governed model-library adoption and UI**: bind the native `library.model.create/attach/clone/detach/show` workflow to the model view and exact governed cloud releases. The local provider-neutral lifecycle, offline exact release cache, semantic/digest fences, retained native bytes, and distinct library adoption/detach cloud publication revisions have landed (`ds-network c2a17b70/b442dd6c/b06f5931/48c7915d`, `ds-cli 962c39e8`, kernel `3bda1f1`). Complete standalone model-extracted library publication, project-authorized cross-project adoption and readable incoming/outgoing source history; local capture or a cache does not establish a cloud review state. Keep explicit project and expected-head fences, and bind UI actions to these native contracts.

- Record each incoming/imported and outgoing/exported member and model as exact immutable evidence with the chosen library release, mapping, warnings, and project context. A future `.bak` example upload should offer a clear default-on consent control with a visible off choice before cloud transfer; preserve project and account authorization. Compare source, DS Grid translation, and outgoing native state without silently treating a round trip as solver approval. Keep full project manipulation history readable without mixing customers' data or giving the web an active-project assumption on the server.

- **Governed trust and export admission in the model view**: complete cloud qualification/state and UI evidence on top of the landed exact pinned-byte gate and CLI/MCP member report. `exact_native_bytes` and `managed_export_allowed` currently describe local immutable byte coverage, not cloud registration, native case applicability or engineering approval. Route, terrain and typed project settings report `retained_project_evidence`; feature-code native definitions remain required and unknown native dependencies fail as `no_cloud_equivalent`. The blind cached-library export now emits a verified 15-member backup without granting solver approval. Promoted managed exports still need the exact governed release and qualified scope; no head/basename/scaffold fallback. Also close the native structure-substitution dependency materialization gap: swapping the recovered three models currently leaves 29 unresolved references. Preserve every unrelated source byte and refuse conflicting or unsourced dependencies; do not repair this by manual file copying. Characterize the donor CRI positive-tension rows with blank wire selectors in the native adapter before admitting them as reusable criteria; do not infer all-wire semantics or weaken the criterion invariant.

### D. Release sources and gates (with §2 and §3)

- **Native source pins and integration**: reconcile any still-unpushed repository changes and update `ds-network-reporter/pins/native-build-sources.json` to the exact published native sources before packaging. Recheck the remaining repositories before changing pins.

- **Remaining source release gates**: recheck the recorded Web navigation/design/version tests, CLI bridge-parity harness, Solar Dockerfile dependency omissions, reporter `golden_artifacts` / `printing_examples` and reporter Clippy. Native checkpoint validation also reproduces 8 existing ds-io and 28 ds-grid-engine Clippy errors (collapsible conditions, type complexity and argument counts); repair these in their own slices. The changed ds-grid-wasm crate and checkpoint regression pass strict owning Clippy with dependencies excluded; the broader log is `ds-network/out/stabilization/wasm-pins-clippy.log`. Repair only still-failing assertions or owning code; remove each claim after its gate passes.

### E. Printing templates, previews and booklets (with §13 and §14)

Moved from OPEN.md §2, §4, §5 and §7. Operational print seeding stays in
[SEEDING.md](SEEDING.md) §4–§6. Combined-output entries stay in OPEN.md §7.

- **Printing toolbar test**: `src/tests/printing-toolbar.test.ts` fails to locate the toolbar closing markup in current `PrintingSetup.svelte`. Repair the test boundary and verify narrow-width controls; `TemplateCatalogue.browser.test.ts` passes.

- **KMZ style resolution**: move the 30 Grid and 47 transformer KMZ tuples from `ds_kml::governed_document` into the sole governed style resolver and its declarative seed.

- **Absent-field country defaults**: remove implicit Rwanda/EDCL print defaults from owning code after checking stored project layouts. Require authored country/branding metadata; issue-date/version values, distinct LV context pens and A3 overview seeding belong in [SEEDING.md](SEEDING.md#4-print-templates-styles-and-seed).

- **MV booklet owner integration** (`f0e9c2ed`, `290bf404`, `46f54c67`): complete any missing native cover → naming → key-map → plan/profile integration that still needs the ds-work Python bridge. Resolve project-native PLS types to exact canonical DS members or return missing mappings. Runtime-gate the structure-description offset correction and restore held-model preview/background preparation on new installations. Final page composition and real project delivery belong in [SEEDING.md](SEEDING.md#5-print-sheet-content-and-legibility).

- **Print sets are too large** (`e2cd543e`): Gisagara's 96-page A3 MV booklet is 481 MB and 86-page A0 LV combined PDF is 714 MB. Reduce repeated vector resources and paths while preserving legible schedules, linework, pagination and supported georeferencing; keep any raster context settings in project-owned output JSON.

- **MV map-inputs repeatedly reads the whole project** (`f48bd93c`): each Northern Hub plan/profile capture rereads 180 transformer contexts and takes about six minutes; the command does not read native server state, so a state-dir flag alone cannot cure it. Add bounded, digest-pinned source reuse and a named failure for stalled acquisition. The delivery runner's pinned district-capture reuse must become a governed DS path.

- `5828fac5` (minor): A0 contour index labels fail renderer with text paper wider than frame.

- `778a9f4d` (minor): Plan & profile: labels after an elevation break drawn in the top band with leader lines crossing the scale bar.

- `8de3e8e0` (major): Batch transformer export needs a declarative JSON job for layout, naming and packaging.

- `88c03fbd` (major): Desktop printing export rejects current reporter result field.

## Reference: landed on 7 October, don't redo

ds-network `720f4184` (one compact attachment box per structure), `516e29c0`;
ds-network-reporter `ade0aa2`, `d0db433`, `e24602e`, `3748e48`, `6147edc`,
`ac7e740`, `b28c844`; ds-cli `31e3c1b4`, `af9b7621`, `4e466494`; ds-web
`be5be7445`; kernel `3e23208`.
