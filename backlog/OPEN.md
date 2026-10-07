# Open code changes

> **Moved 2026-10-07:** this backlog now lives in `ds-command-kernel/backlog/`. The old `ds-work/backlog/` no longer exists: edit these files here, commit them to ds-command-kernel `run`, and stage only `backlog/`. ds-work holds working sessions, not coding lists.

Unfinished code that an agent can implement and prove without further owner input. The backlog is declarative: it lists only what is known not to work, and an entry is removed the moment it is handled ([PRINCIPLES.md §0](PRINCIPLES.md#0-the-backlog-is-declarative-owner-2026-10-07)). The standing owner rulings — the backlog split, how to work an entry, priorities, reporting, kernel/CLI/UI, engineering and country neutrality — are in [PRINCIPLES.md](PRINCIPLES.md); the start order is [ORDER.md](ORDER.md). Owner decisions are in [OPEN-X.md](OPEN-X.md); template building and seeding that need no code change, plus deployment and acceptance, in [SEEDING.md](SEEDING.md).

<!-- toc -->

| Area | Remaining entries |
| --- | ---: |
| [1. Source integration and release code](#1-source-integration-and-release-code) | 3 |
| [2. Green `run` and release gates](#2-green-run-and-release-gates) | 2 |
| [3. Template creation code](#3-template-creation-code) | 2 |
| [4. Print template and style code](#4-print-template-and-style-code) | 0 |
| [5. Print renderer and booklet code](#5-print-renderer-and-booklet-code) | 0 |
| [7. Report compute, export and combined output](#7-report-compute-export-and-combined-output) | 12 |
| [8. Sync, publication and edge outbox](#8-sync-publication-and-edge-outbox) | 4 |
| [9. LV processing, sizing and headless intake](#9-lv-processing-sizing-and-headless-intake) | 13 |
| [10. Voltage drop](#10-voltage-drop) | 1 |
| [11. PLS-CADD exchange round trip](#11-pls-cadd-exchange-round-trip) | 9 |
| [12. DS Grid engineering (Codex main)](#12-ds-grid-engineering-codex-main) | 11 |
| [13. Spotting (Codex main)](#13-spotting-codex-main) | 8 |
| [14. Plan and Profile authoring](#14-plan-and-profile-authoring) | 1 |
| [15. Project delivery code](#15-project-delivery-code) | 2 |
| [16. Map data, datasets and screen styling](#16-map-data-datasets-and-screen-styling) | 9 |
| [17. Project Work and Assets](#17-project-work-and-assets) | 7 |
| [18. Global DS Grid library](#18-global-ds-grid-library) | 1 |
| [19. Country neutrality and duplicated rules](#19-country-neutrality-and-duplicated-rules) | 2 |
| [21. Windows release code](#21-windows-release-code) | 6 |
| [22. Operations product code](#22-operations-product-code) | 2 |
| [23. Kernel controls, JSON UI and TypeScript retirement](#23-kernel-controls-json-ui-and-typescript-retirement) | 5 |
| [24. Owner feature requests (CLI-first, after stabilization)](#24-owner-feature-requests-cli-first-after-stabilization) | 4 |
| [25. CLI reliability reporting and proactive feedback (priority)](#25-cli-reliability-reporting-and-proactive-feedback-priority) | 7 |
<!-- /toc -->

## 1. Source integration and release code

- **In progress:** `wasm-size` (take back the network WASM growth).

- **Security `c9df4098`**: yes, short-lived signed URLs via ds-brain replace Firebase download-token URLs (code tonight; deploy with ds-brain).

- **Move ds-network to Polars 0.55** (owner, 2026-10-07): `ds-network` pins `polars`, `polars-arrow` and `polars-io` at exactly 0.53.0 (workspace `Cargo.toml:74-76`), while ds-solar is already on 0.55.2. Rust 0.x minor versions break APIs. `polars-arrow` writes the `.dsgrid` Arrow IPC tables, so prove the emitted packages are byte-identical before and after: no package format change (schema 5 is final). Then run the ds-network, ds-cli and reporter suites and check the WASM size (`wasm-size`). Polars 2.0 is the Python release and does not apply.

## 2. Green `run` and release gates

- **Native engine strict Clippy gate**: `cargo clippy --locked -p ds-grid-engine --all-targets --no-deps -- -D warnings` remains blocked by baseline owning diagnostics: collapsible conditions, large error/enum variants, argument counts and presentation borrow/style issues. Current library gate reports 27 diagnostics; exact log: `ds-network/out/stabilization/profile-reference-reuse-clippy.log`. Repair coherent owning slices and rerun library-test diagnostics after the library gate admits; retain native error JSON and engineering behavior.

- **Stabilization after the 7 October test sweep** (owner, 2026-10-07): the full list is in [STABILIZATION.md](STABILIZATION.md). It may touch any other backlog surface it intersects.

- **National ID and phone in external reports: not urgent, no code now** (owner, 2026-10-07): DS does handle national IDs. Reporter `a69402e` hides `nid`, `national_id` and phone from every external report projection, but that restriction came from the datasolutions.rw product context and was not meant for everything. Kernel `3e23208` overstated it as a general "personal identity never prints" rule, and reporter `ac7e740` aligned the XLSX and as-built schedule tests with `a69402e`, so the former XLSX test failure is green. Implement no identity removal or restoration until the owner scopes it; correct the `3e23208` wording when this is taken up.

## 3. Template creation code

- **Template creation fixtures and UI binding**: complete representative transformer-outlier fixtures and any missing selectable-copy UI binding over the declared CLI contract. Keep explicit template identity and copy choices in the owner; Tchad layout contents and project parameters belong in [SEEDING.md](SEEDING.md#3-template-projects).

- **Typed single-sheet configuration copy** (`d76f6e55`): expose a reviewable plan/apply operation with source READ and destination WRITE authorization. Installed stable exposes `layer_mapping` in `arjgpydw_survey_test` but omits it from `wnmajona_edcl_rural_electrification_template`; existing-sheet editing refuses that absent destination sheet. Validate admitted sheet identity/schema, create only the selected sheet, preserve unrelated destination sheets and read back the result. Prove an existing `cust_category` copy and unknown-sheet refusal as controls. Do not relax arbitrary-sheet validation or substitute whole-network configuration copying.

## 4. Print template and style code

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group E (printing templates, previews and booklets).

## 5. Print renderer and booklet code

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group E (printing templates, previews and booklets).

## 7. Report compute, export and combined output

- **Product gaps left by the ds-work script removal** (old drivers: `git show f3809be:skills/_scripts/<name>.py`): (1) run a download plan: fetch the listed artefacts byte-exact with a sha256 manifest (`ds design download plan` only previews; was `fetch-download-plan.py`); (2) find a room's district from its artefacts when `design tag enrich-preview` returns `not_located` (was `find-districts-from-artefacts.py`); (3) MV/LV orphan join: MV tips without a transformer and transformers without MV, within a tolerance (was `mv-lv-orphans.py`).

- **Combined reports can publish incomplete output** (`4151b6b7`): a `combined_inputs_not_current` refusal must leave no externally visible partial archive. Desktop and headless combined outputs must agree on MV content and explicit project context (`a9b76d47`, `155d4782`, `70f507fc`, `72155a40`, `655b5a0d`).

- **Northern Hub true combined output is unavailable** (`42f3abd1`): the available combined transformer task emits workbook/geospatial data but no A0/A3 PDFs, while `report project combined` is a compounded ZIP. Expose and prove true combined A0/A3 PDFs and workbook without presenting a compounded archive as combined.

- **Model and transformer retirement lack complete restoration** (`b1b3eec5`, `81ae43eb`): verify exact backup before retirement, expose a governed CLI restore and backup read, and refuse tombstones without a recoverable backup.

- `74bdd848` (blocker): Headless export computed 113 valid reports, but none reached cloud publication; the project remained stale and Combined Report empty.

- `56749e69` (blocker): Desktop report rejects reporter output containing survey_layers_omitted.

- `f6597311` (major): map design batch report: CLI advertises --file-level/--combine-per-group the Desktop rejects, and it needs an unprepared report_archive grouping.

- `da000524` (minor): Export delivery needs a JSON manifest with hash verification and destination receipts.

- `69f60e82` (major): Combined report waits on slow serial publication of all transformer reports.

- `1849f4a3` (major): Group successful report warnings so engineering blockers stay actionable.

- `c9df4098` (major): design download plan lists artefact URLs but no ds verb executes it; every listed report and drafting artefact is readable by URL without any credential.

- Never-landed (owner 2026-10-02, land adapted to run): Generic archive-group readers: kernel, ds-cli (890bac3) and ds-web (54a65730) read the `group_count`/`groups` ds-brain writes, not `district_count` only.

## 8. Sync, publication and edge outbox

- `fd5bce6a` (blocker): Gisagara A0 contour reports cannot replace shared project publications.

- `d50cc39d` (blocker): Edge compute without a visible, self-healing publication: 593 sealed report artifacts (379 MB) stuck in the Desktop pending-upload outbox behind a dead-process lock since 2026-09-18; nobody was told.

- Expose cloud-conflict review as bounded JSON for both DS Grid and LV, with exact local/base/cloud identity, before/after facts, complete change counts, explicit omitted details and useful resolution choices. Existing Grid CLI comparison lists changed IDs but lacks field values; the Grid menu lists revision/digest pins without a content review. Preserve working copies and current head/digest overwrite guards. Advice must distinguish two-way differences from a merge proved against an actual base; provide the same native review facts through CLI/MCP and the JSON view.

- **Bounded cloud-head reads**: the affected `fill_in_gishubi` cloud-head read produced no output for more than ten minutes. Provide a bounded result or named stalled-acquisition refusal while preserving retained local artifacts. Reproduction receipts: `ds-command-kernel/_discardable/artifact-inventory-refresh/root-installed-publications-stalled.json`; publication recovery is tracked in [SEEDING.md](SEEDING.md#8-sync-publication-and-edge-outbox).

## 9. LV processing, sizing and headless intake

- `8b2d9545` (major): LV spans longer than the cable REG maximum are never flagged, and generated poles target max plus tolerance (47 m for 3x70, REG 44 m); EDCL rejected three designs on span length.

- `38bd5091` (major): LV batch process changes approved poles and cable sizes despite preservation settings.

- `4bad5ef3` (major): LV processing always renumbers LV line numbers; no setting keeps supplied line_number.

- `4070556f` (major): Sizing: frozen or existing transformers keep a saved DB cable, LV breaker and MV fuse that is never checked against their size, so mismatched protection ships silently.

- `571c4b57` (major): Sizing: minimum_tr_size raises transfo_size but DB cable, LV breaker and MV fuse stay on the smaller demand row, so issued reports break REG Tables 12 and 13.

- `dcacdb90` (minor): Sizing: only the 30 kV MV fuse is ever used; mv_fuze_15_kv_amp in transfo_sizes is ignored, so 15 kV networks get about half the REG fuse rating.

- `00ad9848` (major): Legacy saved LV designs need a non-destructive topology analysis for voltage-drop reports. Full processing changes approved assignments, pole identities, spans and cable sizes; keep saved design bytes intact or refuse where connections cannot be reconstructed.

- `007c1624` (major): Legacy (unfenced) transformer rooms cannot be read headlessly: lv project-export refuses, features select scans 0, location path resolves nothing, no headless migrate.

- `6ed42257` (blocker): No headless way to save a supplied LV design as a new transformer version without reprocessing it.

- `2adfa7d2` (major): Gisagara cannot publish lv_poles standards columns through known-columns route.

- `7f5ac170` (major): No migration from the deprecated struct_type 'Existing' marker to existing_status across a project's transformers.

- `83f3712b` (major): Supplied shapefiles with 10-character DBF field names are not mapped back to canonical LV fields.

- `14fd08e8` (major): Headless LV intake cannot upload to Northern Hub.

## 10. Voltage drop

- **Analysis/Reporter input binding**: make producer topology, saved voltage-drop JSON and Reporter use the same graph/input digest, with typed stale/missing refusals or a reviewable difference. Complete any missing direct-artifact discovery or saved-JSON-only report bindings. Source reproduction, paper QA and file replacement belong in [SEEDING.md](SEEDING.md#10-voltage-drop).

## 11. PLS-CADD exchange round trip

- **PLS engineering fidelity** (`4ed6b202`, `ef2478d4`, `34ad2bea`, `0a653e07`, `9817e17e`): the native round trip must retain connected T-offs, use section ruling spans for clearance, reject Method-2 uplift without a negative weight branch, and preserve valid cable and criteria records. Prove each against its filed PLS acceptance before issuing strength results.

- **PLS restore acceptance** (`1086e058`, `6f346aa3`): handle the catalogued Save changes exit prompt, then compare every restored file with the backup; a dialog count alone must not report success.

- **Import validation misses reversed routes** (`41176f22`): the model lacks typed source or tap roles. Govern the source relation so validation flags reversed alignments without false positives.

- **Old `.dsgrid` revisions**: refused with a named code once the old readers go; `ds grid model upgrade` republishes every published head.

- **8. PLS-CADD comparison harness and exchange fidelity**: a repeatable harness (DS Grid vs PLS-CADD on the pinned revisions, through the installed `ds pls desktop` verbs) whose outputs are committed tests. Keep the exchange suite green (612 ds-grid-exchange, 432 ds-io, 488 ds-network at `f1a8e02`). Exchange items: `7a836b7a`/`9f2316b3`/`6405185d` Repair Wizard; `e258b287`; `f27c69f1` Model 2 merges 76 of 709 sections; `732fcd12` two criterion sets; `3724e4ce` ACSR member; `91331096` Method-2 signed wind cases; `9817e17e` export re-authors CRI/cable files; `0a653e07` ACSR cable extra line; `de849b9b` 1 m alignment gap; `7ce604c5` duplicate weather labels; `8f312f23` immutable source provenance; `b3bdc857`, `b1a2b25c`, `2b8cd4ae`, `ec73ef89`, `c45dc437`/`e37d99d5`, `90cf2311`.

- **Restore failure reproduction** (`7a836b7a`): repair the affected DS-authored backup only from its exact source and native Restore evidence. Include the known clean Gisagara round-trip as a positive control; relative `FILENAME=.\` headers alone are not an established Repair Wizard trigger.

- **Restore path handling** (`9f2316b3`): verify the filed project `.str`/`.cri` rebasing failure on the affected source before changing writers. Keep clean native positive controls and source bytes; do not preserve the disproved blanket claim that every relative member header causes repair.

- `c45dc437` (major): A characterized PLS-CADD 19.01 DON v68 backup cannot normalize to 16.81: direct v68 comment writing is rightly refused, while deferring it exposes a 12-to-15 criterion-case binding mismatch. Preserve the native writer refusal and prove stage-two parity, restored workspace identity, and the 16.20 negative control.

- `90cf2311` (minor): Whole-model PLS workspace export of Model 2 refuses on earthing before any cable or capacity edit can be proved.

## 12. DS Grid engineering (Codex main)

Rules: [PRINCIPLES.md §6](PRINCIPLES.md#6-ds-grid-engineering).

- **1. Loads and strength (structure usage)**: applied loads and reactions per load case and weather case; Method 1 and Method 2 usage from interaction diagrams; governing case, margin and loads per structure. Implement and test source-backed loads, usage and exact case binding; report measured PLS differences. Approval of an unspecified engineering acceptance tolerance is separated into [OPEN-X.md](OPEN-X.md#12-engineering-acceptance-and-material-authority). Items: `3d2ddd7a` Method 2 usage in DS Grid; `df1f53ae` Model 2 interaction artifacts unverified; `4ff54666` 334,508% usage on stayed steel; `4ba8ab15` section-boundary structures take one side (wind span ~0.5x PLS); guys/stays and foundations (14 m burial 2.0 m).

- **2. Uplift**: suspension uplift detected from the vertical load sign per case. No false flags. Items: `523a7f6d` flags every -0 suspension from the EDT ray end; `4ba8ab15` 115 spurious uplifts (Gisagara calibration). Acceptance: the uplift set equals PLS-CADD's on the reference models.

- **3. Stringing and sag-tension**: ruling span, AutoSag equivalence, weather cases, creep as a temperature shift (client: 15 C). Items: `e343d5ac` DS stringing ~27% tighter than PLS AutoSag (828 section violations); `6226c0bb` and `3d1c735c` creep shift field; `c496feb0` and `59d38d86` OPGW sections split at suspensions. Prove sag/tension computation and imported criteria per span and case; report measured differences. The unspecified acceptance tolerance is in [OPEN-X.md](OPEN-X.md#12-engineering-acceptance-and-material-authority).

- **4. Clearances**: ground, crossings, buildings and cleared forest (contract decision 16), per weather case; the checked clearance is ground offset plus the code extra (`7bc05e22`, computation side; the drawing belongs to the print slice). MV voltage classes from the model/project (15 kV ADERM, not 30 kV everywhere: country scout #33); the structure rules standard per project, not the REG v7 default (#35). Prove violating spans and source-backed clearance values against PLS-CADD. Approval of an unspecified numeric acceptance tolerance is in [OPEN-X.md](OPEN-X.md#12-engineering-acceptance-and-material-authority).

- **5. Terrain and profile geometry**: `d8863709` profile atlas threads off-centreline terrain into the ground line; `8a8b86ae` fill a ground-survey gap from the governed DEM; `41176f22` and `8363a4fc` alignment direction and reversal.

- **Existing conductor counts as new**: tension sections have no typed existing/new status (only structures carry `StakingStatus`), so staking/BOQ include existing conductor and prints cannot style it (2026-09-25 school-tap findings, confirmed in the model). Add a typed section status carried by the atlas, the print and the BOQ.

- `df1f53ae` (major): Every Method-2 interaction artifact of Model 2 is unverified, so no steel pole can ever be capacity-screened.

- `6226c0bb` (major): Cable creep as a temperature shift (client: 15 C) cannot be authored in DS; export writes the proportional creep model with no shift.

- `3d1c735c` (major): Cable creep temperature shift has no canonical field; nominal modulus disagrees with curves and GTP. Implement exact field/provenance transport and expose the conflict; do not choose a replacement material law. Unresolved source precedence is in [OPEN-X.md](OPEN-X.md#12-engineering-acceptance-and-material-authority).

- `e343d5ac` (major): DS stringing ignores the imported AutoSag criteria: sections ~27% tighter than PLS AutoSag, 828 section violations.

- **Qualified structure usage contracts**: support exact native realization, declared weight-span and longitudinal-capacity conventions, complete criterion roles, structural dead loads, signed foundation/anchorage resistance and source-backed load transfer. Expose governing named cases, margins and truthful unknown/partial verdicts in Properties/Issues. Test selected-support vectors against full analysis before qualification; do not manufacture an overall capacity percentage from incomplete inputs. Native comparison and data qualification are in [SEEDING.md](SEEDING.md#12-ds-grid-engineering-codex-main).

## 13. Spotting (Codex main)

- **Whole-model spotting receipt cannot apply** (`7a78525b`): a valid 233 MiB receipt exceeds the 16 MiB parameter bound because of rejection diagnostics. Apply its verified layout without another planning run.

- **6. Spotting on a laptop** (i7-1355U, 12 threads, 15.6 GB; no server capacity): bounded, progressive and parallel within a memory budget (`6b597153`: A28 ran 26 min without a plan; `358e926f`); spotting that uses interaction-diagram strength (`908c07c1`); legality (`fb163694` forbidden 10-60 degree band, `3acfd343` capacity tables without a straight line, `5d0fd02b` between-angle policy); receipts that apply (`7a78525b`); rule-driven retypes and strain splits (`5c7152e3`); the post-processing loop S325->S255 with NG healing (`50ab45f5`); duty profiles (`778bb573`). Acceptance: a whole reference model spots on the laptop within a stated time and memory budget, with quality measured against the optimum.

- `449b4f36-abca-4f73-a54f-8b08897b0bda` (major): Gisagara Model 3 spotting estimates ground witnesses from structure-base stations while materialized phase clearance uses offset attachment endpoints; the same ground, 59.347441828465996 m ruling span and AutoSag state produce 6.03195269148 m planned clearance versus 5.721074724451 m actual clearance (0.310878 m difference). Align the planner's station mapping with the native attachment geometry and prove all-phase parity; five native A140 support insertions remove the observed draft violations without qualifying final structural strength. Evidence: `out/gisagara-m3-terrain-20261005/rebuilt-model3/planner-materialized-clearance-feedback.json` and `route0-corrected-native-receipt.json`.

- `778bb573` (blocker): author_design_policy requires structure duty profiles whose row shape is undiscoverable; Model 2 cannot get the project policy.

- `5d0fd02b` (major): Imported .012 span-limit tables carry an uncharacterized between-angle policy, so a Method-2 steel pole is unplaceable off a knot. Trace native evidence, expose the policy/provenance through the CLI and test exact-knot and unknown-policy refusals. Unresolved engineering interpretation is in [OPEN-X.md](OPEN-X.md#13-between-angle-policy); do not invent interpolation to make a plan feasible.

- `fb163694` (major): Respot apply creates single poles in the forbidden 10-60 deg band and sections without a criterion set.

- `908c07c1` (major): Optimum spotting cannot use interaction-diagram structure strength: every candidate capacity_indeterminate, plan infeasible (Rev A).

- **Bounded spotting and scoped refinement** (`6b597153`): complete truthful CPU/memory/work admission, safe cancellation and progress/remaining-time projection where still absent. Support granular alignment/interval/pass reruns and partial refinement while evaluating all required conductor, attachment and governing-case facts for the final result. Compact rough-search hints must not replace full-layout engineering coverage. Preserve unaffected intervals, declared capacity/ground rules and provisional/unknown status; measure actual missing behaviour before more optimization. The accepted 17-minute baseline and installed stage/ETA proof are operational acceptance in SEEDING.md.

## 14. Plan and Profile authoring

### Profile-only continuation: atomic editing and intentional calculation

Owner scope, 2026-10-07: make Profile precise, continuously usable and fast,
driven by native CLI/MCP controls. Work only its owning engine/runtime, result
queries, CLI transports and Web/Desktop bindings. Defer unrelated stabilization,
printing/templates, NUM conversion, library curation and release pins. The
remaining Profile package branch and print-scale screening item below are
secondary to interactive atomic editing and calculation; do not let them become
a campaign across other surfaces.

**First owning slice: retained-axis authored deltas.** `Runtime::Observe` admits
journal/history without calculation and returns native affected scope;
`Runtime::Present` refuses inputs requiring calculation. The retained calculated
scene has its old revision, and `GridProfilePanel.svelte` disables picking,
station/create/retype controls and selection usage against that old scene.
Emit bounded native authored-geometry deltas and freshness evidence, then admit
them through the existing native worker/canvas contract. Keep authored revision,
computed revision and affected-section freshness distinct. Show new types,
placements, descriptions, deletion and stringing immediately; preserve the held
axis/camera and independent bands. Engineering curves/evidence remain attached
to their computed inputs until an explicit calculation replaces them. Never
retag old engineering as current, loosen revision fences, synthesize geometry in
TypeScript, or hide a refusal behind a full package/scene fallback. Prove repeated
UI and CLI edits without Refresh, Save, loss of picking or broken zoom/lasso.

**Next owning slice: retain unchanged terrain inside affected alignments.**
`ds-grid-engine/src/profile_atlas/case_projection.rs::prepare_after` retains
unaffected raw bands, but calls `profile_atlas.rs::raw_profile_surface_scoped`
for each affected band. That calls `projection.rs::profile_geometry`, which
restations corridor points and rebuilds ground even when a retype changes no
terrain or route. Separate retained route/terrain inputs from authored placements;
use native changed-table/dependency evidence to invalidate them. Retype,
description, deletion and stringing must not rebuild untouched ground. Terrain
point insertion/removal/movement must update its local surface/triangulation and
relevant spans, with declared downstream station/axis effects and parent/T-off
boundaries. Prove fresh-projection parity and exact reuse/work counts for one
changed alignment beside an independent alignment. Do not infer safety from
unchanged whole-model revision alone or duplicate old terrain in dense JSON.

**Next owning slice: localized explicit calculation and cheap queries.**
Accumulate native invalidation across many edits and undo/redo/branching. Only
explicit section calculation, Refresh, Analyze or reconciliation may solve.
Recompute affected tension sections, their spans and structure usage including
other incident forces at shared supports; stop there during interaction. Reuse
unchanged demands, ground, case preparation, markers and layout. Keep calculation
asynchronous and revision-fenced, allow further interaction while it runs, and
reject/suppress stale results without losing edits. Query native retained typed
or Arrow results directly for usage, failures, negative loading, violating
sections, qualification and blockers; expose bounded CLI/MCP projections and
counts with current-input/freshness evidence. Carry only necessary result/delta
bytes into Profile. The Server/native owner must not depend on Svelte, browser
state or Tauri; preserve explicit project/principal/model/session context.

**Measure and prove each slice.** Read `AGENTS.md`, PRINCIPLES.md, STABILIZATION.md
and BUG-EVIDENCE.md first; use the `ds` skill and live command discovery. Read the
owning native Profile contract and existing tests before changing its DTOs.
Expose bounded native phase timings, work/reuse counts and transfer sizes through
the CLI before attributing cost. Current live retypes take about 1.5–1.6 seconds
without calculation; native history observation is about 1 second and commit/
session-state refresh about 0.4–0.5 seconds. The owning receipts are under
`ds-cli/out/stabilization/profile-controls/intentional-*.json`; they use a debug
native Desktop and release browser WASM, not a release-performance acceptance.
Trace duplicated apply/replay, revision hashing, validation and state projection
before introducing parallelism. Target authored local edits and explicitly
requested local calculation below 0.5 seconds while preserving exact engineering
results. Test fresh-versus-incremental results, repeated A→B→A→B, undo/redo tails,
deleted entities, attachment/topology changes, independent/T-off alignments,
stale heads/results, interleaved contexts, cancellation and invalid RAM drafts.
Keep Save explicit and independent from calculation. Prove CLI/native refusal
paths before UI bindings; retain exact usage qualification, never solver approval.

Code directly on canonical `run`, no worktrees, no official goal tool. Place a
focused read-only review request in BUG-EVIDENCE.md before each source slice and
re-read it and STABILIZATION.md between slices. Preserve Claude's template edits
and processes; do not repair its unrelated test failures. Run two Cargo workers,
check memory/processes before heavy jobs, use disk scratch and avoid overlapping
large builds. Build/activate only local artifacts; no remote CI/Cloud Build.
Commit and push each coherent owning slice before the next. Delete fulfilled
backlog/evidence, keep durable proof in owning tests/contracts, and finish with
an honest measured result and the exact remaining boundary. Current worktree
registry is empty; release pins remain last.

- **Avoid redundant native Profile demand work**: `ds-profile-runtime::refresh` advances demands through new journal entries beyond the target cursor before restoring that cursor, so a coalesced edit/undo conservatively drops reuse. Advance only admitted forward receipts while retaining exact history/refusal fences; prove fresh-scene parity and computed/reused work for a future redo tail. `ds-grid-engine::profile_atlas_scenes` also screens the same model twice for the two print scales; share native screening evidence without changing scale-specific marker projection. These are independent optimization slices, not release-latency proof.

- **Remove dormant Profile package projection**: `ds-profile-runtime::Request::Project` still accepts a full package and can return the new head's history after restoring the old held session on Analyze projection refusal. Desktop transport and `profile-native.ts` now use only initialize/refresh; `ds-cli-map/src/profile.rs` drives that Desktop transport. Trace remaining callers, migrate package-era runtime tests/smoke examples to readonly native seed initialization, then delete the public package request/ingest branch and its dormant Analyze fallback. Preserve transactional seed/replay refusal and native offline smoke coverage.

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group A, to be fixed together with the Profile editing and performance items.

## 15. Project delivery code

- **Model 2 signed native criterion cases** (`732fcd12`, `e343d5ac`): complete the native path for distinct Method-2 positive and negative case bindings through 16.81 export/Restore. Merging the two criterion sets must not lose direction. Source geometry, endpoint capacity, native strength and site acceptance are tracked in [SEEDING.md](SEEDING.md#15-project-deliveries).

- **Clearance report case identity**: the Gisagara M3 report prints `vertical at — (null °C)` although its vertical case is 75 °C. Fix the producer/report projection from the exact selected case; never patch the label to a per-model constant.

## 16. Map data, datasets and screen styling

- `33003988` (minor): Wetlands dataset download fails with HTTP 422 after 12 attempts, then succeeds unprompted, so a one-shot install reports a false permanent failure.

- `b98643ff` (major): desktop data rwanda install without --resource times out at the Desktop frontend (504), so the documented default bulk path cannot complete the catalogue.

- `dbe4d17e` (major): Ground data download never settles: DEM/wetlands/powerlines bundles expand but are never indexed; status complete stays false.

- `011ae5a6` (blocker): Project data does not render on the map: a ready map data upload has no layer class, no styles and no tiles (today's upload and the April one alike); rendering project data must need no server.

- `ad44f3a0` (blocker): Desktop refuses a bare .shp dropped on the map and then freezes; the only working path is zip -> ds data convert -> GeoJSON -> local layer, split in two.

- `cf92af6c` (major): No way to measure staked-out LV line length by district or sector: survey aggregates count only, entries carry no admin fields, and geometry selection has no pagination.

- `46dec14e` (major): ds caps that force manual manoeuvring for one 10k-point layer: map draw max 10000 features, data inspect --rows max 5000 with no paging, prepared local layers invisible to the running app.

- Vector runner (ds-network `crates/ds-network/src/vector/runner.rs`): a feature whose operation yields no geometry is dropped without a reason. Proven 2026-10-04: `ds data vector buffer` over the Gisagara M3 conductors reported processed 121, produced 115, `skipped: {}`. The kernel cause is fixed (`535a50a`); the runner must still name every dropped feature in `skipped`.

- Never-landed (owner 2026-10-02, land adapted to run): Rust-owned selection: ds-network landed (one `RegionPredicate`, selector region clause; merge `78ed4a5`). The ds-web half (`slice/rust-owned-selection` `e6ee69e4`: `ds map design` selects through the WASM kernel) lands after a vendored WASM rebuild and its gate.

## 17. Project Work and Assets

- `e9b5858d`, `40b53f7f` (blocker): paired Desktop assets ingest fails with a rejected upload request (browser refusal or 422), so a document cannot be attached to a work task.

- `e01cf456` (major): Creating a free-standing PM task with two attachments takes eight commands across two domains and still fails; task attachment needs a working CLI route.

- Never-landed (owner 2026-10-02, land adapted to run): PM links to exact DS Grid model versions, validated at write (ds-brain 5e8707b/3cb94c8, kernel d051b7a/675fdda, ds-cli 58f870d/3e3db28, ds-web cc3c670), adapted to run's catalog versions and approval instead of the retired open/freeze markers.

- **Board and navigation failures**: fix the Windows production Board API error and local Web go/return navigation from exact authenticated route/request evidence. Preserve explicit project context and persistent attachment state; an unauthenticated 401 does not prove signed-in behaviour.

- Add a geometry-presence filter to shared task filtering and make project overview aggregate the canonical PM tasks, records and notes, including items without geometry.

- **Shared PM/Assets lifecycle and interaction**: implement consistent offline-first state and active/testing/archive scope across PM, Assets, overview and governance. Provide the missing dense governance/Installations interaction through native hit/snap or typed CLI proposals; keep engineering and graph decisions in Rust/server contracts. Project theme values are operational parameters in SEEDING.md.

- **Assets initial-index latency**: measure and fix the remaining slow initial local index acquisition without unbounded prefetch, duplicate authority reads or refetch on tab switching. Preserve the proved bounded snapshot/preview lifecycle.

## 18. Global DS Grid library


- Global DS Grid example forks still use server-derived legacy publication claims without the native manifest ID/revision. The current-schema publication fence refuses that source before writing a head; repair the owning fork producer through explicit current-package migration and exact native claims. No compatibility/default reader or unverified metadata fallback.

## 19. Country neutrality and duplicated rules

- **No single-country assumptions** ([PRINCIPLES.md §7](PRINCIPLES.md#7-no-single-country-assumptions); country scout 2026-10-02, ~223 findings). The worst are fallbacks that turn a Chad input into a Rwanda one. Queued Sol slices, started as capacity frees:
  - **CRS from the project, never Rwanda TM by default**: ds-io no-`.prj` and PLS import/writeback defaults; ds-brain country->CRS (Web Mercator for non-Rwanda) and the `looksProjectedCRS` refusal of real UTM `.prj`; reporter X/Y, print rectangles and collisions; WASM origin (30,-2)/32736; ds-web edit-projection policy -> kernel. Print graticule (`TmRwanda` default), MV booklet "Republic of Rwanda"/"EDCL" defaults and the fixed Rwanda MV context bbox go to the print slices.
  - **Country identity**: a required ISO-3166 `country_code` on the project; missing country never becomes "Rwanda"; one alias table; the reporter receipt digest per country; no `network_template` fallback to `master`.
  - **Solar generic** (after `solar-int`), reading the existing **Solar templates** (ds-brain `POST /api/v1/solar`, `internal/models/solar.go`: `_root`, `_config`, numbered sheets; kernel `solar_snapshot.rs`) as the only source, with no new settings invented: household `kind`/reference category instead of `Pauvre/Moyenne/Riche`; template currency and exchange rate (no XAF/600 constants); required `report_language`; `report_profile` for benchmarks, term sheet and standards; BOQ item roles.
  - **Report i18n** (after `print-lv`): project `report_locale` and a message catalogue for the reporter (sheets, headers, InfoTable, legends, the voltage-drop HTML report); one header-alias function built from the template.
  - **Vocabulary leftovers**: ds-web static EDCL `S100...S255` list merged into every project (can blank `struct_type` for ADERM values); `"ABC Terminal Assembly"` injected on tapping poles; `existing` accepting English aliases only; `rule_edcl` defaults in ds-brain `config_service.go` and the kernel `design_config`.
  - **Voltage classes**: the 30 kV-only MV clearance class and fuse columns keyed 15/30 kV (Codex main for DS Grid `structure_rules`/`clearance_criteria`).
  - **Guards**: rename invariance and parameter perturbation cover BOTH template systems (Network Template and Solar templates); the Chad conformance fixture `td-aderm-conformance` (from `arjgpydw_aderm_loc7`, `beinamar_02`, ds-solar `aderm_*` cities) beside the Rwanda peer; rename invariance (slice `template-invariance`); token-leak checks (a Chad run has no `Rwanda|EDCL|REG|NIX|RWF|Kigali`, a Rwanda run no `XAF|ADERM|OHADA|Tchad`); a per-repo literal ratchet. Chad DEM source curation is in [SEEDING.md](SEEDING.md#16-map-data-datasets-and-screen-styling); use its declared Copernicus GLO-30 source rather than reopening the recorded choice.

- **Duplicated rules queued behind running slices** (scout 2026-10-02; slices `dedup-numbers`, `dead-code`, `contract-docs` take the rest):
  - Transformer sizing twice: reporter `exports/transformer_sizing.rs` vs ds-network `network/sizing.rs` (category fallback is alphabetical; `minimum_tr_size` read three ways). ds-network exposes its size selection, the reporter renders it. After `tchad-rules`.
  - Report file stem, extension table and artifact classifier: kernel `report_formats.rs`, `project_control.rs`, `compute_artifact_inventory.rs`, ds-cli `design/preview.rs`, reporter `exports/mod.rs` (`Run_Ruhuha.a0.pdf` vs `run_ruhuha.a0.pdf`). One kernel rule. With or after `print-lv`.
  - Style seed geometry in five places (kernel vs four Go sites) and ds-cli cartography bounds (refuses stroke 1.5 that the kernel accepts). After `print-styles`.
  - Resumable upload transport twice: ds-cli `ds-cli-auth/src/upload.rs` (skips the digest re-proof) vs `ds-sync-runtime/src/native_transfer.rs`. Move ds-cli onto ds-sync-runtime.
  - Archive folder grouping twice: kernel refuses multi-valued group-by, ds-brain files it under `a+b`. The request carries the kernel's groups and digest; Go verifies.
  - Smaller copies: transformer-row predicate (kernel vs ~8 Go lists), NIX/ITRF transverse Mercator x3 (reporter vs ds-geo), survey aggregate bounds and ~30 `LANE` arg copies, ds-cli helpers (`truncate` x7, sha256-hex validation x17, `safe_relative` x3, `now_ms` x4).

## 21. Windows release code

- **First-release PLS-CADD control integration and freeze** (owner, 2026-10-07): inventory all existing PowerShell/Bash controls and consumers; preserve unique edits and integrate directly on canonical `run` in their owning repositories. Expose Restore, attach/reopen, qualification, AutoSag, reports, backup and delivery through declared `ds` CLI/MCP commands; package every required driver/resource and the characterized dialog catalogue. Prove installed discovery/paths, CLI/MCP parity and representative native workflows before retiring interim drivers. Preserve models and open sessions; stop at unknown dialogs and never call a backup roundtrip engineering acceptance. Finish the existing control surface for this release without expanding native scope; local builds only. Operator classification and engineering decisions remain in OPEN-X.md.

- **Windows Server public release integration**: complete any still-missing `headless-zip` Windows platform/`windows-server` target, canonical Server ZIP naming, receipt/engine registration and independent Desktop/Windows Server/Linux Server carry-forward. Existing local build/install/start scripts and named-pipe implementation do not by themselves prove public release-plane support. Package and real host acceptance belong in [SEEDING.md](SEEDING.md#21-windows-native-release).

- **Retire the interim PLS drivers**: verify the preserved `integrate/pls` deletion (`561f27e`) and integrate its remaining source removal on canonical `run` only after the Windows product toolkit is installed. Preserve any unique consumer/driver edits; do not assume the old worktree path still exists.

- **Solar design runs refused** (ADERM 20 Localités, every city at Network Costs): ds-web sends `concurrency: 0` ("automatic"); ds-solar native refuses anything outside 1..=32 since the Tauri extraction (`ebfb8be`). Slice `solar-ui` also removes Solar TypeScript that re-decides kernel results.

- **Windows source integration**: when its owning Codex is idle, inspect the preserved Windows ds-cli, kernel, brain, Web and simple_notes edits and integrate unique changes into canonical `run` with their owning gates. Reconcile the parked `simple_notes:salvage/release-config-port` work. The earlier file/count inventory must be refreshed; preserve occupied checkouts and the separate PLS-CADD process.

- `7519c28b` (major): Windows headless PDF report promotion fails with Access is denied.

## 22. Operations product code

- **Server monitor projection**: provide accurate per-agent CPU/memory/task facts and a sortable Processes table. The publisher’s polling frequency and watchdog timer recovery are operational configuration in [SEEDING.md](SEEDING.md#22-operations).

- **Feedback and SRE closure are not linked**: SRE request events are immutable and the current incident feed has no close action. Define the actionable auto-report lifecycle so a proven, closed feedback issue no longer reappears as open SRE work, while retaining request-event evidence.

## 23. Kernel controls, JSON UI and TypeScript retirement

Rules: [PRINCIPLES.md §5](PRINCIPLES.md#5-kernel-cli-and-ui-owner-2026-10-07) — migrate touched controls one vertical at a time; complicated TypeScript goes to the CLI and its UI becomes JSON; result renderers and their downloads stay.

- **First-release native controls and JSON UI**: move demonstrated UI-only or duplicated control decisions into the owning Rust kernel/native slice, expose and prove them through CLI/MCP, then bind Web/Desktop to that same authority. Supply a JSON input editor and native read-only preview/result where polished controls are not ready; apply must remain explicit, project/principal/revision fenced and faithful to authored choices. Render the owner’s validation, refusals and results without TypeScript recomputation or hidden defaults. Prove headless behavior and UI/native request/result parity, then delete replaced TypeScript control logic, direct gateway bypasses, obsolete adapters/tests/imports and dead code in that slice. Keep only useful presentation/transport/interaction code; preserve unrelated work. Durable contracts and tests live with their owner.

- **Style family landed?** (ds-web `docs/layer-domain-record.md`): finished 2026-09-11 on `fable/style-document` (kernel `01e4a94`..`88cb15c`, ds-web `1263b601`..`605009ee`), recorded as not merged. Check ancestry on `run`; land or retire.

- **Sidebar gateway bypasses** (ds-web `docs/layer-domain-claude-handoff.md`): Tiles/Files (`api/userData.ts`, `api/projectData.ts`), global tiles, survey pipeline/reports, basemap, Design section, PM context and drawer chrome still call API Gateway from TypeScript. One complete vertical each: kernel decides, native/WASM parity, negative controls, retired TS deleted. Then the local sketch-layer lifecycle (`docs/prompts/claude-local-sketch-layer-lifecycle.md`) and authenticated browser-to-Server transport (loopback/SSH only today).

- **One Rust communication authority** (`docs/prompts/rust-communications-refactor.md`): every request to API Gateway goes through one shared Rust client (identity/lane, auth, retries, cancellation, resumable transfer, durable scheduling) and the TypeScript copies go. The transfer protocol landed 2026-09-10; live acceptance and the remaining TS callers are unverified.

- **Project and instance isolation** (`docs/prompts/claude-project-instance-isolation.md`; contract `project-and-instance-isolation-contract.md`): after the Server execution context: live Desktop instance discovery and CLI/MCP routing that refuses stale or ambiguous targets; generation-fenced UI session state; multi-principal authority and fair capacity across Solar, printing and a third operation on one Server.

## 24. Owner feature requests (CLI-first, after stabilization)

- **DS Workflows — not now, not a priority** (owner direction 2026-10-07): declarative `ds workflow` documents over the Rust vector/table engines become the preferred data and geographic processing interface; Data Factory becomes one way to build a workflow (temporary or saved) and its TypeScript transforms are deleted; agents compose workflows instead of patching data with geopandas, pandas or Polars. Contract and owner questions: [docs/contracts/ds-workflows.md](../docs/contracts/ds-workflows.md) (§14). Phases, each landed and proven before the next, only once the owner admits the work:
  0. Inventory every Data Factory, upload-wizard and attribute-table transformation and map it to a spec §5.2 op; record gaps as spec §5.3.
  1. Kernel: `ds.workflow/v1` types, canonical digest, DAG validation, op registry, static schema propagation; tests over spec §13 documents and negative controls, building with `ds-network` absent.
  2. Kernel: Arrow frames with admission; `data.vector.*` and `geo.*` ops on `ds-geo-lite`, byte-identical to the standalone commands; dedupe and layer append moved from TypeScript to Rust.
  3. ds-cli: `ds workflow ops|op|new|step|validate|preview|plan|run` and one MCP chapter; spec §13 examples 1 and 3 and the negative controls through installed `ds`.
  4. ds-network: `table.*` ops over `ds-geo::table` and `ds-cleaning`, streaming via `ds-columnar`, memory class and `workflow_budget_exceeded`; spec §13 example 2 headless to `design.stage`.
  5. Saved immutable revisions, receipts, plan/source-changed guards.
  6. ds-web: Data Factory emits steps into a temporary draft; delete `data-factory-layer-merge.ts`, `data-factory-geometry-merge.ts`, the transformation parts of `attributeTable/table-tools.ts` and `data-factory/columnar-request.ts`; spec §13 example 4 byte-identical.
  7. Design intake as a built-in workflow template; 8. a `ds-workflows` skill proven by a blind agent.

- Per-project style overrides: project over global, separate live/print surfaces, reset to inheritance.

- Multi-village (leaf) Working Area and pipeline filters.

- Governed per-entity DS Grid tag editing, validating `tag_<id>` against definitions.

## 25. CLI reliability reporting and proactive feedback (priority)

Priority ([PRINCIPLES.md §3–4](PRINCIPLES.md#4-report-as-you-go-owner-2026-10-07)). Contract: [docs/contracts/ds-cli-reliability-and-feedback.md](../docs/contracts/ds-cli-reliability-and-feedback.md). Owner questions (opt-out, signed-out hosts, automatic-filing scope, read access) are in [OPEN-X.md block 25](OPEN-X.md#25-cli-reliability-and-feedback); slices below proceed on the spec's defaults. Land each slice on `run` before the next.

- **Teach the product-use duty (ds-cli, text only):** teach reporting outside coding sessions in the `ds` skill and MCP guidance; coding sessions must solve findings or use the owning backlogs, never submit feedback. Rewrite the `feedback.submit` description so product-use friction, confusion, slowness and ideas are filed as well as gaps. **Proof:** discovery and MCP tests pass; a blind product-use trial files seeded friction, while a coding trial uses only the owning backlogs (spec §7.7).

- **Store accepts `ds` events (ds-sre, ds-brain):** amend the request-event store contract with `source = "cli_command"` and `cli_rollup` and the fields in spec §3.2; ds-brain stamps `service = "ds"` from the source instead of the hard-coded `ds-web` (`internal/services/sre_event_store.go` `RecordClientRequestEvent`) and `ds sre events --source` filters them. **Proof:** brain unit tests plus the full emulator suite; a posted `cli_command` row reads back through `ds sre events --source cli_command`.

- **Reporter in `ds-client-core` (ds-command-kernel):** result classification (`failed` / `crashed` / `refused`), sanitizer, one event per invocation over `domains.sre.client_request_event`, bounded on-disk outbox, 30 s dedupe and per-minute cap, daily success rollup, no recursion, `DS_SRE_REPORTING=off` for tests. **Proof:** spec §7.1–7.5 as kernel tests (transport failure through outbox and flush, refusal not an SLI failure, privacy sentinels, byte-identical output on/off and online/offline).

- **Wire CLI and MCP (ds-cli):** one event per CLI process and per MCP tool call; `ds doctor` shows reporting state, outbox size and last flush; error envelopes carry `next.feedback`. **Proof:** installed `ds` against an unreachable gateway, then online, shows the event with the same `request_id`.

- **Invocation journal and `ds feedback draft --last` (ds-command-kernel, ds-cli):** the last 50 invocations under the §3.3 privacy rules; a draft that `ds feedback submit --yes` accepts after adding only expectation and acceptance. **Proof:** spec §7.6.

- **Automatic bug on crash:** a `crashed` invocation files one `bug` report as `ds (automatic)` with the SRE event id; repeats raise occurrences. **Proof:** spec §7.3 with a test-only panic hook.

- **SRE overview row (ds-sre, ds-brain):** `ds` client fault rate, top failing and most-refused commands. **Proof:** `ds sre overview` shows the row after the slices above emit events.
