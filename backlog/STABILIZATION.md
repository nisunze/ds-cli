# Stabilization

For Codex, on canonical `run`. Opened 2026-10-07 by the owner after the 7
October test sweep. There is no deployment pressure.

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

The owner puts requests here. The coding agent decides where each one belongs
and moves it there. Already moved:

- the PLS-CADD `.bak` export holes → OPEN.md §11 `e258b287`;
- Profile lasso removal and two-way tracking → OPEN.md §14;
- the selection-aware context menu → OPEN.md §23.

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
3. **Auto analysis:** the revision change re-offers auto analysis
   (`GridProfilePanel.svelte:1210-1226`, on by default since 2026-10-05). It
   calls `executeNativeProfileCommand` (`ds-web/src/lib/grid/profile-native.ts`),
   which:
   - runs `client.checkpointDsgrid()`, a full package emit;
   - sends `package_bytes: Array.from(bytes)`, a JSON array of 1.4 million
     numbers, over Tauri IPC;
   - has the native side decode and validate the whole package, rebuild the
     Profile scene for every band, and run the whole-model engineering
     analysis;
   - returns the full scene and up to 2,000 issues per kind, which are then
     re-installed in the worker.
4. **CLI path** (`ds-cli/crates/ds-cli-dsgrid/src/structure/retype.rs`,
   `mutation.rs`):
   - decode the whole package; schema-4 copies re-run the 4 → 5 migration and
     its verification on every open;
   - open a session;
   - run `line_angles_by_structure` over every structure;
   - apply, checkpoint, then re-emit the whole package with all its assets,
     hash it and write it.

**Patch direction, smallest first:**

1. **Send the edit, not the package.** Keep a native session per open model and
   revision, and replay the journaled command on it. Stop calling
   `checkpointDsgrid()` and sending `Array.from(bytes)` on every Profile
   refresh. At minimum, use binary IPC instead of a JSON number array.
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
   Profile selection (see OPEN.md §14, two-way tracking).
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

## Reference: landed on 7 October, don't redo

ds-network `720f4184` (one compact attachment box per structure), `516e29c0`;
ds-network-reporter `ade0aa2`, `d0db433`, `e24602e`, `3748e48`, `6147edc`,
`ac7e740`, `b28c844`; ds-cli `31e3c1b4`, `af9b7621`, `4e466494`; ds-web
`be5be7445`; kernel `3e23208`.
