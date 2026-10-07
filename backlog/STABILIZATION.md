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
| ds-cli | `DS_REPORT_BIN=<reporter target>/debug/ds-report DS_SOLAR_BIN=<ds-solar target>/debug/ds-solar cargo test --locked --workspace --all-features --no-fail-fast` | 107 suites green; the MCP skill check was fixed in `4e466494` |
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
2. **Session refresh:** `refreshGridSessionState` then re-reads the snapshot,
   persists the session state and schedules a working checkpoint
   (`session.ts:684-695`).
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
   - clone the whole session for a dry run;
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
5. **Persist off the interaction path.** Checkpoint and persist debounced, not
   on every edit.
6. **CLI fixes:**
   - store a migrated working copy once in schema 5, using the existing
     migration (no new bump);
   - compute line angles only for the selected structures;
   - use a lighter dry-run probe than a whole-session clone.
7. **One path for batches.** Retype one, a range or a selection through the
   same single transaction, followed by the same scoped recompute.

**Acceptance:** on this model, in a release build, retyping 618 shows the new
type and the updated usage in well under a second, with no whole-model package
round trip. Whole-model analysis stays available on demand.

### 5. Bug evidence found during the trace

- **CLI edits drop library pins.** The persist path in
  `ds-cli/crates/ds-cli-dsgrid/src/mutation.rs` packs with
  `library_pins: Vec::new()` and `library_needs: Vec::new()`. Every CLI edit
  (retype, describe, …) silently removes the model's interoperability-library
  pins. Carry them through, as model composition should too (`.bak` hole 3).
- **The CLI cannot co-work with the dev Desktop.**
  - `dsgrid.structure.retype` has no `--target`, so the CLI cannot edit the
    model the Desktop has open.
  - `dsgrid.model.list` reaches only the stable and canary catalogues, not the
    dev Desktop's `local` lane.
  - `desktop.status` and `map.profile.view` report the open model's id but not
    its name.
  - `map.profile.view --target desktop:<id>` does report the selection
    correctly: structure 618.

### 6. Right dock: Issues opens uninvited and keeps the focus

Restored: this item was dropped from this file without a destination.

**Owner:**

- The Engineering issues drawer "is coming uninvited too much".
- Opening Properties docks it, but Issues stays active. The panel the user
  opened last must be the active one.

**Cause:**

- Every native Profile run with analysis publishes an issue target
  (`ds-web/src/lib/desktop/cli-profile-native.ts:57`).
- `MapRightDock.svelte:74` then switches the dock to Issues
  (`if (showIssues && !previousIssues) active = 'issues'`).
- The tab rule tracks only Issues and Work appearing, never Properties.

**Needed:**

- Keep the analysis result current without opening the drawer.
- Open it only on the user's action (the Issues button,
  `GridProfilePanel.svelte:726`, or a finding link).
- Respect a closed drawer until the user reopens it.
- Make the last-opened tab (Properties, Issues or Work) the active one.

### 7. One colour code for usage %

Restored: this item was dropped from this file without a destination.

- **Seen:** structure 5 (`l-s-S800-stay.012`) on the oracle copy. The Profile
  drawing shows 3.4% in green; the toolbar badge shows the same 3.4% on amber.
- **Cause:** the badge picks its colour in TypeScript from `engineering_status`
  (`ds-web/src/lib/components/grid/GridProfileContextBar.svelte:27-29`). The
  drawing uses the engine's colour role.
- **Needed:** the badge takes the engine's colour role; the UI does not decide
  colours.
- **Open choice:** the band thresholds themselves stay the owner's decision
  (OPEN-X.md "Usage-% colour bands").

### 8. Investigate uneven structure results across models

Restored: this item was dropped from this file without a destination.

- **Seen:** the oracle reference copy shows usage % per structure and
  "Analysis complete". Other models stop at an unresolved
  `capacity_case_binding` / capacity, or at "Engineering issue unsupported".
- **Ask:**
  1. For each model, list through `ds` which structures lack a result and why.
  2. Test whether attaching the common cloud interoperability library (the
     157-member `pls-interoperability-canonical-candidate`) reaches the oracle
     copy's completeness.
  3. Report which gaps need source evidence instead. Never invent capacities.
- **Intersects:** OPEN.md §12 structure usage, `df1f53ae`, `908c07c1`,
  `4ba8ab15`; SEEDING "Saved scaffold trust states" and "Shared-library
  capacity tables" (`3acfd343`).

### 9. Owner direction: externalize DS Grid libraries

Restored: this item was dropped from this file without a destination.

**Direction:** many models refer to the same external libraries, such as
criteria (CRI) and structures, so one library update reaches every model that
uses it. The cloud interoperability library sits beside DS Grid's own library:
PLS-CADD first, possibly Camelia or Jove later.

**What exists today:** each model carries exact immutable pins and a cached
copy of each pinned release (ds-network `c2a17b70`, `b442dd6c`; kernel
`3bda1f1`, `9c44615`), and §18 forbids head or basename fallback.

**Before any code, bring the owner a design that answers:**

1. How an update propagates while each model revision keeps its exact release
   (an auditable re-pin with an impact report).
2. What happens to results that an update changes.
3. How the Desktop and Server behave offline.
4. How third-party adapters map onto the library without becoming the
   authority.
5. How references avoid another breaking `.dsgrid` schema bump (PRINCIPLES
   §2: schema 5 is final).

### 10. Owner wording check: MV plan key

Restored: this item was dropped from this file without a destination.

The MV plan key now prints the held legend titles (ds-network-reporter
`3748e48`). For example, it shows "New LV cables / Existing LV lines / Existing
MV lines" instead of "Context LV / Existing LV / Existing MV". If the owner
wants the old words, author them as governed legend titles in the MV context
template.

## Reference: landed on 7 October, don't redo

ds-network `720f4184` (one compact attachment box per structure), `516e29c0`;
ds-network-reporter `ade0aa2`, `d0db433`, `e24602e`, `3748e48`, `6147edc`,
`ac7e740`, `b28c844`; ds-cli `31e3c1b4`, `af9b7621`, `4e466494`; ds-web
`be5be7445`; kernel `3e23208`.
