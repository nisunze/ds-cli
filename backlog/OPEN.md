# Open code changes

> **Moved 2026-10-07:** this backlog now lives in `ds-command-kernel/backlog/`. The old `ds-work/backlog/` no longer exists: edit these files here, commit them to ds-command-kernel `run`, and stage only `backlog/`. ds-work holds working sessions, not coding lists.

Unfinished code that an agent can implement and prove without further owner input. The backlog is declarative: it lists only what is known not to work, and an entry is removed the moment it is handled ([PRINCIPLES.md §0](PRINCIPLES.md#0-the-backlog-is-declarative-owner-2026-10-07)). The standing owner rulings — the backlog split, how to work an entry, priorities, reporting, kernel/CLI/UI, engineering and country neutrality — are in [PRINCIPLES.md](PRINCIPLES.md); the start order is [ORDER.md](ORDER.md). Owner decisions are in [OPEN-X.md](OPEN-X.md); template building and seeding that need no code change, plus deployment and acceptance, in [SEEDING.md](SEEDING.md).

<!-- toc -->

| Area | Remaining entries |
| --- | ---: |
| [1. Source integration and release code](#1-source-integration-and-release-code) | 3 |
| [2. Green `run` and release gates](#2-green-run-and-release-gates) | 3 |
| [3. Template creation code](#3-template-creation-code) | 2 |
| [4. Print template and style code](#4-print-template-and-style-code) | 0 |
| [5. Print renderer and booklet code](#5-print-renderer-and-booklet-code) | 0 |
| [7. Report compute, export and combined output](#7-report-compute-export-and-combined-output) | 11 |
| [8. Sync, publication and edge outbox](#8-sync-publication-and-edge-outbox) | 4 |
| [9. LV processing, sizing and headless intake](#9-lv-processing-sizing-and-headless-intake) | 13 |
| [10. Voltage drop](#10-voltage-drop) | 2 |
| [11. PLS-CADD exchange round trip](#11-pls-cadd-exchange-round-trip) | 9 |
| [12. DS Grid engineering (Codex main)](#12-ds-grid-engineering-codex-main) | 11 |
| [13. Spotting (Codex main)](#13-spotting-codex-main) | 8 |
| [14. Plan and Profile authoring](#14-plan-and-profile-authoring) | 1 |
| [15. Project delivery code](#15-project-delivery-code) | 2 |
| [16. Map data, datasets and screen styling](#16-map-data-datasets-and-screen-styling) | 7 |
| [17. Project Work and Assets](#17-project-work-and-assets) | 5 |
| [18. Global DS Grid library](#18-global-ds-grid-library) | 1 |
| [19. Country neutrality and duplicated rules](#19-country-neutrality-and-duplicated-rules) | 2 |
| [21. Windows release code](#21-windows-release-code) | 5 |
| [22. Operations product code](#22-operations-product-code) | 2 |
| [23. Kernel controls, JSON UI and TypeScript retirement](#23-kernel-controls-json-ui-and-typescript-retirement) | 4 |
| [24. Owner feature requests (CLI-first, after stabilization)](#24-owner-feature-requests-cli-first-after-stabilization) | 4 |
| [25. CLI reliability reporting and proactive feedback (priority)](#25-cli-reliability-reporting-and-proactive-feedback-priority) | 5 |
<!-- /toc -->

## 1. Source integration and release code

- **In progress:** `wasm-size` (take back the network WASM growth).

- **Security `c9df4098`**: yes, short-lived signed URLs via ds-brain replace Firebase download-token URLs (code tonight; deploy with ds-brain).

- **Move ds-network to Polars 0.55** (owner, 2026-10-07): `ds-network` pins `polars`, `polars-arrow` and `polars-io` at exactly 0.53.0 (workspace `Cargo.toml:74-76`), while ds-solar is already on 0.55.2. Rust 0.x minor versions break APIs. `polars-arrow` writes the `.dsgrid` Arrow IPC tables, so prove the emitted packages are byte-identical before and after: no package format change (schema 5 is final). Then run the ds-network, ds-cli and reporter suites and check the WASM size (`wasm-size`). Polars 2.0 is the Python release and does not apply.

## Solar sync: one native artifact path (Codex)

Contract: [ds-sync-engine.md §6](../docs/contracts/ds-sync-engine.md#6-where-each-publication-rule-lives).
Solar and Network Reporter computation artifacts require identical synchronization
logic; prefer verified shared artifacts over unnecessary recomputation. Governed
inputs, parameters and provenance must survive artifact reclaim so computations
can be repeated when needed. Owner inputs are in
[OPEN-X.md](OPEN-X.md#solar-sync-migration-owner-inputs). The queue cutover
is ruled in the sync contract: simple local clean and recompute.

- **Placement remains a TypeScript decision; adapter parity is unproven.** Capture
  current placement answers, replay them in the pure kernel and native/WASM
  consumers, and prove identical Solar/reporter bytes and head identity from
  native and locally built Cloud Run adapters. Hosted web Solar remains closed.
- **Strict runtime Clippy fails on an untouched report enum.**
  `cargo clippy --offline --manifest-path crates/ds-sync-runtime/Cargo.toml
  --all-targets --all-features -- -D warnings` reports `large_enum_variant` at
  `src/reports.rs:307` (`SealOutcome::Sealed`, 1368 bytes versus 25). The enum is
  identical to baseline `24a58f2`. Repair in its owning release-gate slice; do not
  sweep unrelated report code into the Solar migration. Proof: the same strict
  command passes without lint suppression. With that diagnostic excepted on the
  command line, the test target also reports `single_element_loop` at
  `crates/ds-sync-runtime/src/gateway.rs:352`; that path is likewise unchanged.
- **Strict kernel/store Clippy has untouched baseline diagnostics.**
  `cargo clippy --offline -p ds-command-kernel -p ds-sync-store --all-targets
  --all-features -- -D warnings` reports `redundant_closure` and `needless_borrow`
  in `src/profile_encoding.rs`, `type_complexity` in `src/style_legend.rs`,
  `too_many_arguments` in `src/style_multiscale.rs` and
  `src/printing/style_capture.rs`, `single_element_loop` in
  `src/profile_display.rs`, and `cloned_ref_to_slice_refs` in report preview and
  printing tests. These paths are unchanged by Solar sync. Leave the Profile
  findings to its owner and the remaining findings to their owning gate slices;
  proof requires the strict command to pass without lint suppression.
- **CLI release gates have unrelated baseline failures.** The Solar continuation
  vector reports `report layout render --help` at 2545 bytes against its 2480-byte
  allowance in `context_budget::command_help_is_bounded`. Its unchanged
  `domain_smoke` assertions expect 44 map window commands instead of 46, omit
  `map.profile.calculation` and `map.profile.results` from the coverage set, and
  expect one Profile calculation example instead of two. Repair these in their
  owning CLI/Profile slices and prove the full context-budget/domain-smoke suites;
  the four new native recovery commands must retain their derived budgets.
  `cargo test -p ds --test semantic_coverage` also fails because that unchanged
  inventory omits both Profile commands. Solar and native sync have a separate
  exact semantic inventory proof; add the two Profile entries in their owning
  slice and prove the full suite.

## 2. Green `run` and release gates

- **Workspace formatting gate**: `cargo fmt --all --check` from `ds-cli` fails on committed Profile runtime formatting (`calculated_transfer_tests.rs`, `lib.rs`, `publication.rs`), the Network engine module order, and CLI Solar code/tests (`portfolio_headless.rs`, `project.rs`, `domain_smoke.rs`, `semantic_coverage.rs`). Format only those owning files, preserving concurrent edits, then prove the same workspace check. Exact diagnostics: `ds-web/out/windows-readiness-20261009/cli-fmt.log`.

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

- **Product gaps left by the ds-work script removal** (old drivers: `git show f3809be:skills/_scripts/<name>.py`): (1) run a download plan: fetch the listed artefacts byte-exact with a sha256 manifest (`ds design download plan` only previews; was `fetch-download-plan.py`); (2) find a room's district from its artefacts when `design tag enrich-preview` returns `not_located` (was `find-districts-from-artefacts.py`); (3) MV/LV orphan join: the kernel join landed (ds-command-kernel `src/mv_lv_orphans.rs`, `fef9493` + `382a697`; schema `ds.mv-lv-orphans/v1`). Remaining: a `ds` CLI/MCP door over `mv_lv_orphans` (MV GeoJSON plus transformer points or `ds design status` JSON; `--tolerance-m`, `--junction-m`, `--exclude-prefix`, `--limit`, optional `--out` orphan GeoJSON; closed `Code` enum mapped to documented refusals; a `domain_smoke` assertion) (was `mv-lv-orphans.py`).

- **Combined reports can publish incomplete output** (`4151b6b7`): a `combined_inputs_not_current` refusal must leave no externally visible partial archive. Desktop and headless combined outputs must agree on MV content and explicit project context (`a9b76d47`, `155d4782`, `70f507fc`, `72155a40`, `655b5a0d`).

- **Northern Hub true combined output is unavailable** (`42f3abd1`): the available combined transformer task emits workbook/geospatial data but no A0/A3 PDFs, while `report project combined` is a compounded ZIP. Expose and prove true combined A0/A3 PDFs and workbook without presenting a compounded archive as combined.

- **Deleted LV transformer restore has no CLI** (`81ae43eb`): ds-brain serves `read_transformer_backup` (verified receipt plus a 15-minute generation-pinned download) and `restore_deleted_transformer` (exact generation+sha256 pin; refuses `transformer_exists`/`history_diverged`) on `/report` (ds-brain `6fc25cb`, `01785fc`, `5dd52c9d`; `docs/contracts/transformer-retirement.md`). The kernel client is `DeletedBackupRequest` in `ds-client-core` (`8c102d6`). Add the `ds design transformer` commands over it (explicit `--project`, backup read/download, restore-deleted with the read pins), MCP exposure and closed refusal mapping.

- `74bdd848` (blocker): Headless export computed 113 valid reports, but none reached cloud publication; the project remained stale and Combined Report empty.

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

- **Voltage-drop export contract text still describes the old A4 behaviour** (ds-cli, residue of reporter `026e441`; feedback `159282a4`, `e538e632`): the `report export` purpose (`crates/ds-cli-report/src/export.rs:51`) still says it prints reserved/incomplete/refused status on A4 and that governed identity supplies logos, and `docs/reference/report.md:126-129` still promises A4 status pages and logos on every A4 title block. Restate both: A4 prints only calculated analysis, without logos; non-calculated analysis is refused with `voltage_drop_pdf_not_calculated`; the combined A3 overview covers reserved and refused transformers. Regenerate ds-web `src/lib/tools/native-catalog.generated.json`. Optionally drop stale status-document wording in `crates/ds-cli-style/src/a4_seed.rs` and the stale test comment at reporter `src/exports/voltage_drop_pdf.rs:738`. **Proof:** the discovery and doc-contract tests pass and the regenerated catalogue matches the new text.

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

### Differential compute after an edit (the item to watch, owner 2026-10-08)

Read this whole entry before coding. It carries the owner's steers, the
reasons behind them, the code that exists, the numbers, and the traps found
on 2026-10-08, so the next implementer starts from there rather than from
zero. Rulings: [PRINCIPLES §6](PRINCIPLES.md#6-ds-grid-engineering).
Contracts: [Profile as a transactional database](../../ds-network/docs/contracts/profile-transactional-database.md)
(§0 is the leading rule) and
[alignment terrain partitions](../../ds-network/docs/contracts/alignment-terrain-partitions.md).

#### The owner's steers on 2026-10-08, in order

1. **One `.dsgrid` database, two ways to query it.** The plan map shows all
   stored terrain as GeoJSON context for line routing and corridor planning;
   the Profile is a separate query engine optimised for performance. Same
   data, two readers; neither re-projects the other.
2. **Server native first.** Tauri and TypeScript are afterthoughts; the
   behaviour lives in Rust and works headless.
3. **Never compute what has not changed.** Terrain points carry a feature code
   and, at most, alignment membership — no tagging system. Relationships are
   built once and advanced from what changed; "the cost is to compute what has
   not changed."
4. **Structure numbers** (Codex implemented, owner approved): a permanent ID
   never changes; numbers are dense 1..N by alignment order, then chainage; a
   shared support has one number; any reorder, reverse, insert, move, delete,
   restation or spotting renumbers atomically; no contradicting manual
   override; PLS import keeps the NUM roster and export refuses stale numbers;
   renumbering changes no geometry, topology or stringing. It must be cheap:
   renumber from the changed structure onward; reconciliation may wait for Save
   ("pending in RAM").
5. **Retype warns, never blocks.** A design-rule finding warns and lets the
   person confirm; the structure stays flagged.
6. **Updates are section-based and alignment-based** — the furthest
   station-dependent extent — never whole-model invalidation.
7. **One codebase, every target; the kernel is the only contact surface.**
   A person (Desktop/Web) and agents (CLI/MCP) co-design one live model. WASM
   and native run the same Rust with the same results and refusals; native may
   only be faster (Rayon). "If something that localizes compute is justified in
   WASM, the same is justified in native." No host decides recompute scope.
8. **Measure** the retype, the return of results and the reconstruction of
   the whole model; the owner confirms the numbers in the Desktop.
9. **Never undo work to make a problem go away.** A revert of the Desktop
   section-update wiring was refused: "that Profile should not be undone…
   solve any conflict semantically — I am not going to lose 8 hours."
10. **One unit, user and CLI alike.** "If I drag a pole, that is it — wires
    are invalidated, results invalidated, with no healing or recompute. That is
    a lifecycle you are forgetting to close."
11. **Observe the real thing.** Update the Desktop, move a pole, watch the
    model behave, then fix the lifecycle — pins come after.
12. **Land the action, not half of it.** "The problem is not timing — it is
    landing an action and reconciling the Profile cleanly, not landing half an
    action."
13. **Whole landing is right; the scope is wrong.** "This is the desired
    behaviour 100% — but that reconciliation should not reconcile on tension
    section, no terrain change, and not all alignments; the maximum of what can
    reconcile is the alignment where that structure belongs."
14. **The next action lags** "as if the model was starting from scratch."
15. **Do not fix it in the same breath.** "Be proud that the lifecycle thing is
    done; the differential compute we fought for since this morning is the
    thing to watch."
16. **The server/kernel is the core of Web, Desktop and CLI.** Use the local
    Linux Desktop dev build for real structure retypes and handle drags. Observe
    the affected sections healing and rebuilding; much of the code already
    exists and its wiring must be proved, not assumed (owner, 2026-10-08).
17. **Time predictable human/assistant co-design.** Keep per-phase timings and
    actual work counters with the parity proof. After spotting an alignment,
    moving or retyping a few structures must not repeat its unchanged work.
    Optimum spotting/typing on one alignment must retain unchanged terrain,
    other alignments and their typing; those repeated costs were unacceptable
    in earlier iterations (owner, 2026-10-08).

#### Why a transactional, differential query engine is the way

- **Scale makes whole-model work a freeze.** Nyamagabe MV Model 1 has 666
  structures, 1,568 tension sections, 2,564 spans, 33 alignments and about 44k
  terrain rows. Its full Profile scene is 56.6 MB and takes 1.1 s natively.
  The browser/Desktop worker is single-threaded WASM, two to four times slower.
  Every clone, hash, re-index, transport or repaint of the whole model per edit
  turns into seconds. Only work proportional to the change keeps a live session
  usable.
- **A design is a database, not a file.** An edit is a transaction with an
  exact write set; revision tokens, indexes and derived views advance from that
  write set instead of being rediscovered from a full snapshot. The Profile
  scene, its usage/clearance facts and its pick index are materialised views of
  the model; each edit maintains them incrementally through dependency indexes
  (support ↔ section ↔ structure ↔ alignment ↔ terrain partition). Save commits
  changed units; a full `.dsgrid` is an explicit export.
- **Two readers of one model.** The map reads everything as display context;
  the Profile reads each alignment's own support closure through retained
  partitions. A pole on one alignment cannot change another alignment's wires,
  ground or clearances, so it must not cost them.
- **Live co-design needs exact, shared answers.** A person and an agent edit
  the same head. Whatever the kernel decides an edit touches must be the same
  on every target and visible to both; a host that guesses scope diverges.
- **Parity is what makes localisation safe.** The differential result must
  equal a full calculation byte for byte (in the held axis). With that proof the
  narrow computation is trustworthy; without it, it is a guess. Engineering
  numbers never change because work was skipped.

#### Remaining differential compute work

The governing parity, receipt, shared-support and complete-update contracts
and owning tests live in `ds-network/docs/contracts/profile-transactional-database.md`
and `alignment-terrain-partitions.md`. Finish these independent costs on
canonical `run`, one proved/pushed slice at a time:

- **Per-row signing and transport after a local edit.** Use the retained
  native copy-on-write arrays and keep signatures per row so changed rows
  alone are hashed and compared. Preserve canonical byte order,
  signed zero, delta bounds and current publication/history refusal paths.
  `profile_calculated_delta.rs::CalculatedSceneBase`, the retained projection
  and `ds-grid-wasm/src/profile_calculated.rs` still traverse unchanged span
  rows and hash equal splice prefixes/suffixes. The authored view also emits
  all 2,564 temporary connections against the consumed calculated baseline;
  generate only the affected support/section incidence. The Nyamagabe
  lifecycle probe exposes a 647 ms native complete
  update, 773 ms worker install and 1,241,283-byte delta for one support; do not
  substitute another full baseline for reducing these costs.
- **Pick and geometry-preview scope.** `profile_pick_cache.register` still
  indexes the complete scene after a delta, and the drag preview still prepares
  raw bands/surface beyond the changed support. Advance the native pick index
  from the same affected bands/identities, retain unchanged picking and inspector
  evidence, and request only changed geometry before work. Prove no far band or
  unchanged ground projection during repeated drags.
- **Signed base through case selection.** Runtime `cached_cases.rs` and the
  worker's case installation discard the calculated base. Keep an exact native
  base across prepared-case selection on both sides, transport the native
  selection delta unchanged and prove that another edit uses it. Preserve
  read-only case preparation, election, axis and publication fences; the host
  must not invent a second scene/signature.
- **Undo/redo scope and next-edit continuity.** Preserve the native journal and
  history pins. Characterize retained reconciliation from actual navigation
  receipts rather than defaulting every alignment to a fresh build. Prove
  repeated drag/retype/undo/another edit against fresh held-axis calculation,
  including canonical order, shared supports, unavailable sections and named
  wider-axis transitions.
- **Duplicate observation/publication refusal.** A same-family retype in the
  older Desktop returned `profile_publication_failed` while a later view had
  healed the same head. Inspect begin/confirm/discard and asynchronous fallback
  races through native/transport tests and the matching Desktop. Do not relax
  publication, history or signed-zero checks to suppress the error. Original
  receipts are `ds-network/out/profile-differential/desktop-retype-*.json`.
  Attribute timing and actual work to the complete transaction, including the
  observation that first healed it: `cow-next-retype-apply.json` reports zero
  applied replay entries and retained full authored-work counters on the later
  CLI observation. Its successful 5.18 s completion does not identify the
  earlier solve's work. Keep per-phase costs and actual scope observable to
  human and agent callers without repeating the calculation.

Installed Desktop acceptance belongs to `SEEDING.md`; it remains part of this
user-requested delivery. Use two build workers, disk scratch and the warm
release cache `/home/magese/.cache/ds-network-probe-target`. No new worktree is
needed. Preserve the parked duty-retype source at
`ds-network/out/parked-numbering-retype-20261008-1119/`; it is separate from
this differential calculation correction.

### Profile-only continuation: atomic editing and intentional calculation

Owner scope, 2026-10-07: make Profile precise, continuously usable and fast,
driven by native CLI/MCP controls. Work only its owning engine/runtime, result
queries, CLI transports and Web/Desktop bindings. Defer unrelated stabilization,
printing/templates, NUM conversion, library curation and release pins. The
remaining Profile package branch and print-scale screening item below are
secondary to interactive atomic editing and calculation; do not let them become
a campaign across other surfaces.

**Required contract and manual Opus execution plan (owner, 2026-10-08).**
[Profile as a transactional database](../../ds-network/docs/contracts/profile-transactional-database.md)
owns the complete requirements: data at rest/in motion, native transactions,
relationship indexes, deliberate Ground scope, differential calculation/query/
projection and changed-unit Save. Read its current-source evidence before
coding. The following items are unfinished implementation obligations, in order;
they are not an invitation to replace the whole stack or add a SQL engine.

1. **Native transaction/write-set foundation.** Start with one existing local
   Profile edit through `ds-grid-engine::GridSession` and the retained runtime.
   Produce exact typed record/relationship changes, old/new memberships and
   dependency tokens at apply; retain independent authored, calculated and
   durable heads. Introduce partition/record sharing and touched-index updates
   in that owning path so subsequent Observe/replay does not reconstruct a
   full snapshot merely to rediscover the write set. Replace broad validation
   only where strict reference/uniqueness/topology evidence and retained indexes
   prove the same refusals. Preserve exact canonical roots; do not substitute
   a new incremental digest for an existing fingerprint. Native tests must
   prove idempotency, stale-head/foreign-context refusal, repeated A/B edits,
   deletion and undo/redo branch identities. Extend the same path in small
   commits to retype, stringing, placement and deletion.

2. **Typed alignment relations and native Ground controls.** Use the existing
   `AlignmentRow` parent graph, ground-basis references and all structure
   placements; add compatible principal/T-off qualification and explicit
   many-to-many terrain membership with native incidence indexes. Preserve
   absent legacy field semantics, source bytes and schema-5 lineage. Provide
   bounded inspect and history-pinned preview/apply through closed CLI/MCP
   contracts. Native spatial candidates must use admitted corridors/CRS/source
   evidence, preserve shared/ambiguous memberships and report unassigned data.
   Bootstrap is deliberate, never an automatic rewrite on open. Admit Ground
   scope explicitly; refuse route/terrain writes outside it. Prove independent
   routes, shared observations/supports, actual T-offs, bad parent graphs,
   ambiguous spatial candidates and stale previews before UI bindings.

3. **Profile reads terrain through retained partitions.** See
   [alignment terrain partitions](../../ds-network/docs/contracts/alignment-terrain-partitions.md).
   Owner ruling, 2026-10-08: one model, two readers. The plan map shows all
   stored terrain as GeoJSON context; the Profile is its own query engine and
   reads only each alignment's support closure. Remaining: the session builds
   the partition index once and `advance`s it from each transaction's write
   set; Profile ground, structure ground, clearance and display consumers
   read through it instead of scanning the whole table per call; structure,
   wire and section edits never clone, hash, validate or transport untouched
   terrain (today about 20 ms of each edit on a 44k-row model). Surface domain
   meshes per family, with point insert/delete/move touching only old/new
   membership windows and characterized triangulation neighbourhoods, follow;
   they never silently replace the selected ground basis. Provider
   acquisition commits and PLS/GIS imports still take no admission footprint.
   Prove fresh parity for effective ground, source/CRS/datum/gaps/feature
   clearances, support bases and station/axis consequences, and that edits do
   no work on unrelated alignments.

4. **Explicit local calculation and cached facts.** Consume the transaction
   ledger's accumulated affected dependencies across many edits, undo/redo and
   branching. Reuse unchanged case/demand/route/terrain preparation; solve only
   affected requested tension sections, spans and supports. Assemble all
   incident forces at shared supports with valid retained neighbor results;
   missing/stale incident inputs are named dependencies, never zero. Stop
   propagation at section ends; include actual adjacent T-off incidence without
   solving the family. Localize independent Analyze clearance and fact/index
   reconstruction where dependency evidence supports it; name remaining global
   dependencies honestly. Native usage/failure/negative-loading/violation/
   qualification/blocker pages must stay bounded and perform no calculation.
   Bind browser/peer calculation to a separate native asynchronous job owner,
   preserving exact history/context/read-set and publication fences. Prove fresh
   complete engineering parity, unchanged payload identity, cancellation,
   stale/foreign admission and picking/selection during the job.

5. **Projection from changed records, then bounded transport.** Generate native
   per-element authored and calculated display patches/tombstones from the
   same write/dependency sets. Retain terrain, static display and pick indexes
   until their actual inputs change. Sag/string/retype edits must carry their
   changed curves/supports/markers rather than the whole terrain-symbol array;
   membership bounds do not justify replacing the entire alignment. Reuse the
   native scene/delta producer and receiving worker, preserving original Rust
   JSON strings and exact signed-zero/base/history/result/axis/publication
   fences. Complete closed runtime/CLI/MCP result contracts for the missing
   display/analysis patches before Web binding. Prove full fresh projection and
   findings parity, repeated edits, missing/tampered/oversized/stale bases,
   rollback and native pick continuity. Global axis/band transitions must name
   their required reconciliation rather than silently calculate or fall back.

6. **Changed-unit Save transaction and recovery.** Trace native
   `checkpoint_dsgrid` → `dsgrid::emit` → package packing and the existing local
   checkpoint storage consumers. Replace ordinary full-package Save with a
   native expected-head persist plan over changed authored records/chunks and
   required relation/source references. Reuse exact unchanged objects, stage
   missing verified bytes, then atomically publish the complete manifest/head.
   Keep full portable `.dsgrid` materialization as an explicit export and
   preserve strict package verification and schema 5. Result retention remains
   separate. Prove byte/digest reuse, write-set completeness, reopen equality,
   idempotent retry, conflict, interruption before/after head publication,
   missing objects, invalid draft and edits made during Save. Only a durable
   receipt clears the captured dirty work; Save never calculates or makes old
   engineering current. No automatic Save/publication is introduced.

7. **Web/Desktop transport and removal of replaced TypeScript.** Bind the proved
   native controls/results using their declared DTOs, including a usable JSON
   input/preview/result surface where polish is pending. Keep native explicit
   project/principal/model/session context and refusal text. Remove only the
   replaced TypeScript decisions, broad adapters, dead imports and tests in
   each touched slice after tracing callers; retain interaction/presentation
   and result download/verification. Prove request/result/refusal parity and
   repeated retype, stringing, deletion, picking, selection and zoom at the
   retained axis without Refresh or Save. Do not touch Claude's template slice.

8. **Apply/replay cost closure and matching installation.** Use the existing
   native phase/work counters to reduce remaining whole-snapshot cloning,
   revision hashing, validation and projection, including cold seed/open work
   where it blocks the Profile path. Separate a characterized local transaction
   from explicitly larger reconciliation; never weaken engineering checks.
   Build matching native runtime, CLI/command/skill bundle and WASM locally and
   verify source/ABI identity before installed acceptance. Coordinate the
   authorized Desktop restart when these bindings are ready. Use a scratch
   representative large model for the existing SEEDING.md §14 acceptance:
   sub-0.5-second local edits and explicit local calculation with continuous
   interaction, durable recovery and exact engineering parity. Report remaining
   costs and boundaries if that target is still unmet. RAM retention and small
   native fixtures alone cannot qualify it.

**Evidence and working discipline.** Current installed retype acceptance is
unmet: debug-native Nyamagabe edits take 1,512–1,549 ms, mostly Observe/replay
and commit/session-state. Exact source-level scene deltas still accompany full
display/analysis; 10,001 byte-identical terrain glyphs account for 99.3% of the
3.62 MB display during sag edits. Do not spend another slice measuring that
unchanged payload; correct the transaction/dependency/projection boundary.
The contract records reproduction paths and the limits of each measurement.
Terrain/route changes, broad validation/hash/clone work, independent Analyze,
cold initialization and changed-unit persistence need separate native proof.

The owner starts Opus manually; Codex stops source coding for that interval and
returns to independently validate/refine the resulting slices. Opus must read
AGENTS.md, PRINCIPLES.md, STABILIZATION.md, BUG-EVIDENCE.md, OPEN-X.md and
SEEDING.md plus the contract. Use the `ds` skill and installed discovery before
claiming a live control gap; distinguish source tests from unavailable/unpaired
installed controls. Prove each owning slice headlessly, including refusals,
before its UI binding. Save remains an explicit action; an edit lands whole
(PRINCIPLES §6, 2026-10-08).

Work directly on canonical `run`, no worktrees or official goal tool. Re-read
focused review evidence between slices; delete fulfilled obligations and retain
proof in owning contracts/tests. Preserve Claude's template edits/processes.
Use two Cargo workers, check RAM and active processes before heavy jobs, disk
scratch and no overlapping large builds. Disk use up to 90% is authorized;
do not repeatedly prune reusable caches. No remote CI/Cloud Build. Commit and
push each coherent repository slice before proceeding; avoid a mixed unreviewable
checkpoint. Deferred unrelated stabilization stays deferred.

- **Share Profile print-scale screening**: `ds-grid-engine::profile_atlas_scenes` screens the same model twice for the two print scales; share native screening evidence without changing scale-specific marker projection. This secondary print optimization is not release-latency proof.

- **Remove dormant Profile package projection**: `ds-profile-runtime::Request::Project` still accepts a full package and can return the new head's history after restoring the old held session on Analyze projection refusal. Desktop transport and `profile-native.ts` now use only initialize/refresh; `ds-cli-map/src/profile.rs` drives that Desktop transport. Trace remaining callers, migrate package-era runtime tests/smoke examples to readonly native seed initialization, then delete the public package request/ingest branch and its dormant Analyze fallback. Preserve transactional seed/replay refusal and native offline smoke coverage.

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group A, to be fixed together with the Profile editing and performance items.

## 15. Project delivery code

- **Model 2 signed native criterion cases** (`732fcd12`, `e343d5ac`): complete the native path for distinct Method-2 positive and negative case bindings through 16.81 export/Restore. Merging the two criterion sets must not lose direction. Source geometry, endpoint capacity, native strength and site acceptance are tracked in [SEEDING.md](SEEDING.md#15-project-deliveries).

- **Clearance report case identity**: the Gisagara M3 report prints `vertical at — (null °C)` although its vertical case is 75 °C. Fix the producer/report projection from the exact selected case; never patch the label to a per-model constant.

## 16. Map data, datasets and screen styling

- **Desktop native tiling and reusable snapshots:** Windows routes ordinary
  tiling to cloud and the shared runtime invokes Tippecanoe/external conversion;
  complete report/survey snapshots do not provide the required automatic native
  build and governed publication path. Implement the [2026-10-09 plan](../docs/desktop-native-tiling-migration-plan.md):
  shared Rust generation through reporter/CLI/MCP, Parquet/GeoParquet and Polars,
  cheap revision freshness, differential reads and local work-area masks,
  real Windows proof, complete-snapshot triggers, Web-control/legacy deletion
  and revision-fenced central publication. Treat PCs as project compute contributors,
  never personal result owners or tile servers; explain this in Sync Center.
  Claim/coalesce project work, implement minimal source/lease/publication APIs,
  remove ALL cloud generation and obsolete custom BigQuery tiling/explorer, and
  measure read/cost/latency savings. Future expert jobs are outside scope.
  Maintain the moving remaining plan here; no new owner input is needed for
  autonomous code. Whole-path acceptance is in the plan's matrix.

  Active: **2 — immutable columnar store**, private account/lane/
  audience/project fencing, verified chunks, atomic cursor/delta application,
  exact local work-area masks, recovery, retention and Windows ACL/locking proof.
  Source adapters must consume the shared snapshot contract through the real
  authenticated source/delta protocol in slice 6; current production acquisition
  has not migrated. Use installed `/usr/bin/ds-canary` for development acceptance;
  never resume the paused stable release. Next independent slice: **3** pinned
  VersaTiles native generation and fidelity/
  determinism/budget/cancellation qualification; **4** reporter task and durable
  native jobs plus actual Windows execution; **5** existing CLI/MCP and Desktop
  controls/automatic triggers with report independence; **6** real governed
  source/delta/lease/upload/independent verification/CAS publication APIs and
  central readback; **7** ALL cloud generation, obsolete custom BigQuery explorer/
  tiling and external-tool/fallback deletion with consumer tracing; **8** matching
  development packaging, installed discovery and Windows/Linux real-map matrix.
  Every slice is proved, committed and pushed on canonical `run` before the next.
  Acceptance retains before/after cold/warm/delta/report/area request counts,
  source work/billed bytes where observable, transfer, cloud invocations, latency
  and peak memory/disk. Assert zero cloud tiling, unchanged source downloads,
  mask-induced remote calls and redundant cross-PC compute; never claim live
  publication or Windows proof from fixtures/Linux alone. Sync Center describes
  project compute contributors and central publication, never PC tile serving.
  Owner extension during implementation: archive the entire ds-vector-tiler
  service/repository after tracing and replacing/retiring its non-tiling consumers;
  remove its deployment/gateway bindings. Combined and simple report generation
  becomes PC-only for ordinary users; keep central artifact publication/download.
  Enforce remote report generation for verified developers only at the API,
  with ordinary compute menus read-only and labelled as migrating to PC.
  Remote tiling is removed for everyone, including developers.

- `33003988` (minor): a second install of the same dataset while the first is still downloading reopens the same `.part`; on Windows every attempt fails at once with "open existing partial download safely", the bridge reports 422, and the first download finishes on its own, so a one-shot install reports a false permanent failure. **Proof:** a concurrent second request for the same destination is told the download is in flight, and the one-shot wetlands install completes.

- `dbe4d17e` (major): Ground data download never settles: DEM/wetlands/powerlines bundles expand but are never indexed; status complete stays false.

- `011ae5a6` (blocker): Project data does not render on the map: a ready map data upload has no layer class, no styles and no tiles (today's upload and the April one alike); rendering project data must need no server.

- `cf92af6c` (major): No way to measure staked-out LV line length by district or sector: survey aggregates count only, entries carry no admin fields, and geometry selection has no pagination.

- `46dec14e` (major): ds caps that force manual manoeuvring for one 10k-point layer: map draw max 10000 features, data inspect --rows max 5000 with no paging, prepared local layers invisible to the running app.

- **Vector skip parity:** the ds-network runner now names engine-empty drops (`8e56edcb`: `engine_empty`, `no_station`, `no_buffer`, `skipped_features`). The kernel portable `vector_ops` (`src/vector_ops*.rs`, WASM `vectorProcess`) still counts such features as processed and does not name them; ds-cli `domain_smoke` `vector_cli_file_and_json_packets_match_the_kernel_and_frozen_native_baseline` is red on `run` (packet 4: processed 0 vs frozen 2; `skipped` lacks `no_station`). Make `vector_ops` name the same drops, refreeze `ds-command-kernel/tests/fixtures/vector_processing_packets.json`, and turn kernel `tests/vector_processing.rs` and that ds-cli test green.

## 17. Project Work and Assets

- `e01cf456` (major): Creating a free-standing PM task with two attachments takes eight commands across two domains and still fails; task attachment needs a working CLI route.

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

- **Windows source integration**: when its owning Codex is idle, inspect the preserved Windows ds-cli, kernel, brain, Web and simple_notes edits and integrate unique changes into canonical `run` with their owning gates. Reconcile the parked `simple_notes:salvage/release-config-port` work. The earlier file/count inventory must be refreshed; preserve occupied checkouts and the separate PLS-CADD process.

- `7519c28b` (major): Windows headless PDF report promotion fails with Access is denied.

## 22. Operations product code

- **Server monitor projection**: provide accurate per-agent CPU/memory/task facts and a sortable Processes table. The publisher’s polling frequency and watchdog timer recovery are operational configuration in [SEEDING.md](SEEDING.md#22-operations).

- **Feedback and SRE closure are not linked**: SRE request events are immutable and the current incident feed has no close action. Define the actionable auto-report lifecycle so a proven, closed feedback issue no longer reappears as open SRE work, while retaining request-event evidence.

## 23. Kernel controls, JSON UI and TypeScript retirement

Rules: [PRINCIPLES.md §5](PRINCIPLES.md#5-kernel-cli-and-ui-owner-2026-10-07) — migrate touched controls one vertical at a time; complicated TypeScript goes to the CLI and its UI becomes JSON; result renderers and their downloads stay.

- **First-release native controls and JSON UI**: move demonstrated UI-only or duplicated control decisions into the owning Rust kernel/native slice, expose and prove them through CLI/MCP, then bind Web/Desktop to that same authority. Supply a JSON input editor and native read-only preview/result where polished controls are not ready; apply must remain explicit, project/principal/revision fenced and faithful to authored choices. Render the owner’s validation, refusals and results without TypeScript recomputation or hidden defaults. Prove headless behavior and UI/native request/result parity, then delete replaced TypeScript control logic, direct gateway bypasses, obsolete adapters/tests/imports and dead code in that slice. Keep only useful presentation/transport/interaction code; preserve unrelated work. Durable contracts and tests live with their owner.

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

- **Wire CLI and MCP (ds-cli):** ds-cli implements `Transport::sre_client_event` and an owner-only, on-disk `ReporterStore` in the DS state directory (never `/tmp` or `/dev/shm`); calls `reporter::finish_invocation` once per CLI process and once per MCP tool call, with the MCP send off the request path; `ds doctor` shows reporting state, outbox size and last flush from `reporter::status`; every `failed`, `crashed` or repeated-`refused` error envelope carries `next.feedback` (the `ds feedback draft --last` command and the SRE event id; the field is ds-client-core-owned, spec §6). **Proof:** ds-cli tests show byte-identical stdout, stderr, envelope and exit code with reporting on or off, online or offline (spec §7.5). The installed readback is in [SEEDING.md block 25](SEEDING.md#25-cli-reliability-and-feedback).

- **Invocation journal and `ds feedback draft --last` (ds-cli):** (a) ds-cli provides `ds feedback draft --last [--n N] --output json`, reading the journal from the on-disk owner-only reporter store. (b) The draft's `component` is the owning repository from the command registry (spec §4), not the current `ds-cli/<domain>` placeholder. (c) The draft maps onto `feedback.submit`'s real inputs (`context` as repeated `key=value`; `agent` filled when known), so after a failure `ds feedback submit --yes` accepts it once the caller adds only expectation and acceptance; today a CLI invocation without `--agent` drafts with `needs: agent`. (d) Once the command exists, name `ds feedback draft --last` in MCP `FEEDBACK_GUIDANCE` and in the `ds` skill "Report as you go" section (spec §5.4). The journal is written only inside `record()`, so with `DS_SRE_REPORTING=off` `draft --last` has nothing to return; decide that with the [OPEN-X block 25](OPEN-X.md#25-cli-reliability-and-feedback) opt-out question. Touch ds-command-kernel only if the draft shape changes. **Proof:** spec §7.6.

- **`ds capabilities --search` does not find `feedback.submit` (ds-cli, text only):** `ds capabilities --search 'report a bug'`, `'report a problem'` and `'file a bug'` return no `feedback.submit` (its search list is empty); only `feedback`, `friction` and `idea` find it. **Proof:** all three phrases return `feedback.submit`; the discovery tests pass.

- **Automatic bug on crash:** a `crashed` invocation files one `bug` report as `ds (automatic)` with the SRE event id; repeats raise occurrences. **Proof:** spec §7.3 with a test-only panic hook.

- **SRE overview row, CLI rendering (ds-cli):** `ds sre overview` already returns `ds_client` in JSON (ds-client-core `ef40648`; ds-sre `REQUEST_EVENT_STORE_CONTRACT.md` §7.8; ds-brain `d8b8f04` service filter). Render it as a text line in `crates/ds-cli-sre/src/overview.rs` (fault rate, crashes, refusals, top failing, most refused; `available=false` shows the reason; a null fault ratio shows unknown), declare `ds_client` in the command output and `docs/reference/sre.md`, with render tests for available, unavailable and quiet windows.
