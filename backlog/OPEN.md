# Open code changes

> **Moved 2026-10-07:** this backlog now lives in `ds-command-kernel/backlog/`. The old `ds-work/backlog/` no longer exists: edit these files here, commit them to ds-command-kernel `run`, and stage only `backlog/`. ds-work holds working sessions, not coding lists.

Unfinished code that an agent can implement and prove without further owner input. The backlog is declarative: it lists only what is known not to work, and an entry is removed the moment it is handled ([PRINCIPLES.md §0](PRINCIPLES.md#0-the-backlog-is-declarative-owner-2026-10-07)). The standing owner rulings — the backlog split, how to work an entry, priorities, reporting, kernel/CLI/UI, engineering and country neutrality — are in [PRINCIPLES.md](PRINCIPLES.md); the start order is [ORDER.md](ORDER.md). Owner decisions are in [OPEN-X.md](OPEN-X.md); template building and seeding that need no code change, plus deployment and acceptance, in [SEEDING.md](SEEDING.md).

<!-- toc -->

| Area | Remaining entries |
| --- | ---: |
| [1. Source integration and release code](#1-source-integration-and-release-code) | 4 |
| [2. Green `run` and release gates](#2-green-run-and-release-gates) | 8 |
| [3. Template creation code](#3-template-creation-code) | 1 |
| [4. Print template and style code](#4-print-template-and-style-code) | 0 |
| [5. Print renderer and booklet code](#5-print-renderer-and-booklet-code) | 2 |
| [7. Report compute, export and combined output](#7-report-compute-export-and-combined-output) | 14 |
| [8. Sync, publication and edge outbox](#8-sync-publication-and-edge-outbox) | 6 |
| [9. LV processing, sizing and headless intake](#9-lv-processing-sizing-and-headless-intake) | 8 |
| [10. Voltage drop](#10-voltage-drop) | 3 |
| [11. PLS-CADD exchange round trip](#11-pls-cadd-exchange-round-trip) | 12 |
| [12. DS Grid engineering (Codex main)](#12-ds-grid-engineering-codex-main) | 11 |
| [13. Spotting (Codex main)](#13-spotting-codex-main) | 8 |
| [14. Plan and Profile authoring](#14-plan-and-profile-authoring) | 1 |
| [15. Project delivery code](#15-project-delivery-code) | 2 |
| [16. Map data, datasets and screen styling](#16-map-data-datasets-and-screen-styling) | 9 |
| [17. Project Work and Assets](#17-project-work-and-assets) | 8 |
| [18. Global DS Grid library](#18-global-ds-grid-library) | 2 |
| [19. Country neutrality and duplicated rules](#19-country-neutrality-and-duplicated-rules) | 2 |
| [21. Windows release code](#21-windows-release-code) | 5 |
| [22. Operations product code](#22-operations-product-code) | 4 |
| [23. Kernel controls, JSON UI and TypeScript retirement](#23-kernel-controls-json-ui-and-typescript-retirement) | 5 |
| [24. Owner feature requests (CLI-first, after stabilization)](#24-owner-feature-requests-cli-first-after-stabilization) | 5 |
| [25. CLI reliability reporting and proactive feedback (priority)](#25-cli-reliability-reporting-and-proactive-feedback-priority) | 3 |
<!-- /toc -->

## 1. Source integration and release code

- **Independent Desktop/Server component releases — deferred architectural priority** (owner 2026-10-10): targeted updates do not yet replace the coupled Desktop/Server release. The owner reports three-hour builds becoming eight-hour cycles after deployment failures. The required result is cloud-like release quickness: update only the changed Kernel, Network, Network Reporter or Solar component without rebuilding or reinstalling unchanged hosts, frontends or engines. Cloud deployment and native publication remain separate choices. Stabilization through year-end and production blockers take precedence under [PRINCIPLES §3](PRINCIPLES.md#3-priorities-owner-2026-10-07). Resume this campaign only when the owner admits it with sufficient credits; do not turn backlog consolidation into extraction work.

  **Coupling to remove, recheck at campaign start:** [Desktop Cargo dependencies](../../ds-web/src-tauri/Cargo.toml) link Kernel, Network/Profile and Solar; the [Server crate](../../ds-cli/crates/ds-cli-server/Cargo.toml) also links Kernel and Solar. Reporter has a bounded executable boundary in [network_reporter.rs](../../ds-web/src-tauri/src/network_reporter.rs), but this alone does not establish independent installation. [component_manager.rs](../../ds-web/src-tauri/src/component_manager.rs) and [desktop-components.json](../../ds-web/desktop-components.json) bind components to the compiled app release. The Windows/Linux source capture and [server-bundle.mjs](../../ds-web/scripts/desktop/server-bundle.mjs) currently admit a whole source vector. Replace unnecessary whole-stack admission with each artifact's real dependency closure; do not weaken exact-source admission or reinterpret old manifests silently.

  **Moving execution sequence:** finish and prove one useful owning vertical before starting the next; installed rollout belongs in [SEEDING §2](SEEDING.md#2-release-acceptance).

  1. **Reporter first:** inventory actual release dependencies, introduce versioned component admission, and publish/update `ds-report` independently on Windows Desktop and Linux Server. Prove an unchanged host and unrelated component digests, compatibility refusal and rollback. Keep the existing typed executable boundary; changing only an installer wrapper does not remove coupling.
  2. **Solar second:** extract linked-host execution into an independently versioned native runtime on Desktop and Server, preserving shared native/cloud computation and CLI/MCP semantics. Preserve copied seed inputs, authored maps and exact artifact parity where promised.
  3. **Kernel/Network later:** inventory actual interactive calls and state ownership before separating runtimes. Use closed typed IPC/executable contracts, with an owned persistent runtime where edit latency needs it. Do not expose an arbitrary shell or Rust compiler ABI, duplicate policy, or copy the monolith into four processes. Pure shared crates may remain component build dependencies, recorded in that component's exact closure. A real protocol change may require a coordinated compatible set; unrelated changes must not.
  4. **Updater/supervisor hardening:** admit verified immutable artifacts by component/version, OS/architecture, lane, protocol capabilities, dependency compatibility and digest/provenance through the existing publication authority. Do not use an unverified mutable `latest` URL. Stage and verify the complete selected unit before atomic activation; retain an immediately usable previous version. Interrupted acquisition or power loss must leave a usable current unit. Never replace an executable underneath active work: jobs retain their captured engine version and project/principal/revision context through cancellation and publication. Restart only the compatible component; explain and defer a required host restart before applying it. Preserve projects, unsaved edits, credentials and publication queues.

  **Cross-cutting admission:** update checks are explicit, navigation-driven or push-driven, never polling. Bound workers and reap them on completion, cancellation, failure and explicit shutdown, without orphan processes. The single tray notification owner is host lifecycle infrastructure, not one receiver per component; preserve concurrent workspace account/project isolation and peer delivery. Reports never wait for tiling. Preserve CLI/MCP ownership and native/cloud semantics; prove matching UI/native results. Component extraction must retain [immutable release input capture](../../ds-web/docs/deployment-contract.md#immutable-release-inputs), separate acquisition/execution/activation deadlines, verified expected-package installation and truthful publication success when optional activation fails. These are existing release constraints, not a request to reimplement completed release fixes.

  **Effort assessment, not dates or commitments:** one experienced engineer, focused slices, working local build/native acceptance hosts. Unattended builds and unavailable Windows access can extend calendar time. Complete separation is high difficulty; Reporter first is moderate. The challenge is stable typed contracts, state ownership, cancellation and compatibility, not rewriting algorithms or merely renaming repositories/adding Cargo features.

  | Owning vertical | Estimated focused engineering effort | Main uncertainty |
  | --- | --- | --- |
  | Dependency inventory, versioned admission and first Reporter-only Windows/Linux update | 3–5 days | Unchanged-host compatibility and rollback |
  | Solar targeted runtime/update on Desktop and Server | 4–7 days | Removing linked execution while retaining native/cloud parity |
  | Kernel and Network/Profile separation | 10–20 days | Interactive state, policy, large inputs, differential edits and CLI/MCP parity |
  | Updater/supervisor hardening and native failure/upgrade acceptance | 4–7 days | Power loss, Windows executable locks, active jobs and mixed versions |

  First useful targeted release: approximately **3–5 engineering days**; full four-component Windows/Linux Desktop/Server capability: approximately **4–7 focused engineering weeks**. These are non-additive planning estimates, not measured delivery dates. Re-estimate after the first independent update and interactive inventory. Some owning work can overlap while respecting host memory and other sessions. Do not wait for all four components before shipping a proven first reduction, reopen the whole backlog, or relax notification/deployment safeguards.

- **In progress:** `wasm-size` (take back the network WASM growth).

- **Security `c9df4098`: signed reads only.** ds-apis-tf `infra/default_bucket.storage.rules` still has `allow get: if !isServerOwnedName(...)` (legacy public read), so `report_lv`/`lv_drafting` project objects stay readable by URL. Put them behind signed reads only and prove the ds-web/Desktop/field/kernel consumers use `sign_url`; make kernel `artifact_identity` accept `https://storage.cloud.google.com/<bucket>/<object>` as ds-brain `artifact_reference.go` already does. Deploy, token revocation and the anonymous-GET refusal are in [SEEDING.md](SEEDING.md).

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
  `src/profile_display.rs`, a diagnostic at `src/sync_store.rs:176`, and `cloned_ref_to_slice_refs` in report preview and
  printing tests. These paths are unchanged by Solar sync. Leave the Profile
  findings to its owner and the remaining findings to their owning gate slices;
  proof requires the strict command to pass without lint suppression.
- **CLI release gates have unrelated baseline failures.** The unchanged
  `domain_smoke` assertions omit
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

- **ds-cli contract suites red (wave 3, 2026-10-09)**: observed red, some possibly from then-uncommitted work: `context_budget::command_help_is_bounded` (`ds doctor --help` 1426 > 1380 bytes; `ds report layout list --help` 3083 > 3020); `contract` (`design.download.fetch` summary 73 > 70 chars); `refusal_coverage` (two `Failure::invalid/conflict(code, …)` sites in `ds-cli-auth/src/device.rs`, and ds-web `src/lib/desktop/cli-sync-center.ts:126` and `cli-assets-args.ts:29` mint refusal codes that are not literals); `lens_core_boundary` (`ds-cli-map/src/profile_calculation.rs` uses a paired availability without `Requires::Window`; window commands 46/45 against a ceiling of 44); `clippy -p ds-cli-design -D warnings` (`lv/voltage_drop.rs:186` `type_complexity`). Re-run each on `run` and fix those still red in their owning files. **Proof:** the full `ds` test suites and that clippy pass.

- **ds-web test gates red on `run`**: `gateway-bypass-gate` (`src/lib/api/client.ts` has 6 remote call sites against a reviewed budget of 5); `native-tools-page.test.ts` (2 failures since `a8d3e98f`: `projectUploadsRoute` now returns `/projects/<p>/data?view=import…`); `src/lib/solar/snapshot-kernel.test.ts` byte parity (kernel `cba2603` changed the governed Solar snapshot fingerprint and key order); `projects-api.test.ts` "composes all-project listing from deterministic lifecycle buckets", even run alone; `svelte-check` errors in `src/tests/setup.ts` (`CanvasFreeGraph`, `canvasGraphAutoArrange`, `CanvasFreeCardGeometry` missing from the vendored `ds_network_wasm`). Fix each in its owning slice (reduce the bypass, update tests to the current contract, refresh the vendored WASM); never raise the budget to pass.

- **ds-network WASM export inventory stale**: `docs/contracts/wasm-public-surface.json` records 251 exports while `ds-network-wasm` carries 256 `#[wasm_bindgen]` items, so `documented_wasm_export_inventory_count_matches_source` fails. Its generator `scripts/generate-wasm-surface-inventory.py` is Python (owner: no Python); replace it with Rust or shell, regenerate, then delete the `lv_voltage_drop` export, which has no JS/TS caller in ds-web.

- **Kernel vendored-drift and literal-ratchet tests red**: `tests/geo_lite_drift` `vendored_bytes_match_live_ds_network` fails on vendored `canvas/free_layout.rs` drifting from ds-network `ds-canvas`; the template literal ratchet counts 12 against baseline 10 because `crates/ds-client-core/src/sre.rs:572` and `:613` (`ef40648`) add the token `"service"`. Resync the vendor copy and remove or name the literals; never raise the baseline.

- **ds-network differential receipts**: `network::differential::tests::recorded_transformers_manual_and_auto_process_outputs_are_byte_identical` was red because the `renumber_lines` process setting adds `"renumber_lines":true,` to `settings_used` (lv-differential-wingwa, auto=true). With that setting on `run`, regenerate the recorded receipts if the test is still red and prove the suite.

- **Stabilization after the 7 October test sweep** (owner, 2026-10-07): the full list is in [STABILIZATION.md](STABILIZATION.md). It may touch any other backlog surface it intersects.

- **National ID and phone in external reports: not urgent, no code now** (owner, 2026-10-07): DS does handle national IDs. Reporter `a69402e` hides `nid`, `national_id` and phone from every external report projection, but that restriction came from the datasolutions.rw product context and was not meant for everything. Kernel `3e23208` overstated it as a general "personal identity never prints" rule, and reporter `ac7e740` aligned the XLSX and as-built schedule tests with `a69402e`, so the former XLSX test failure is green. Implement no identity removal or restoration until the owner scopes it; correct the `3e23208` wording when this is taken up.

## 3. Template creation code

- **Template creation fixtures and UI binding**: complete representative transformer-outlier fixtures and any missing selectable-copy UI binding over the declared CLI contract. Keep explicit template identity and copy choices in the owner; Tchad layout contents and project parameters belong in [SEEDING.md](SEEDING.md#3-template-projects).

## 4. Print template and style code

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group E (printing templates, previews and booklets).

## 5. Print renderer and booklet code

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group E (printing templates, previews and booklets).

- **Dead retired renderer in the reporter**: ds-network-reporter `src/print_iterations.rs` still holds a `cfg(test)`-only retired renderer (`render_transformer_svg_legacy`, `compose_svg` and helpers, about 2,000 lines) reachable only from its own tests. Delete it with its tests in a dead-code slice; keep live renderer coverage.

- **Long diagonal leaders on vertical structure labels** (cosmetic): on supports whose top sits near the profile envelope top, vertical labels get leaders up to 27 mm long (real Nyamagabe scene), running from the support top down to the label base. They cross no furniture. Shorten them without moving labels into furniture.

## 7. Report compute, export and combined output

- **Local collision detection is not wired into Project Control** (owner report 2026-10-10): Desktop's `detectCollisions` still sends `/report` `detect_collisions` to the cloud, where non-developers hit remote compute admission. Implement local detection through the existing native reporter owner and CLI/MCP, reuse unchanged authorized transformer rooms, preserve explicit project/revision context, then bind the Desktop action and refresh persisted results. Prove ordinary Desktop operation without developer authority or cloud compute, genuine zero versus unknown results, unchanged cache reuse, and project capability refusals. Do not unblock remote compute or imply local detection already exists.

- **Product gaps left by the ds-work script removal** (old drivers: `git show f3809be:skills/_scripts/<name>.py`): (1) `ds design download fetch` fetches only `plan.filtered_urls` (reports, prints, archives); the retired `fetch-download-plan.py` also fetched each in-scope row's source uploads (`rows[].source_entries`/`source_files`: draft/sketch/process). Fetch those through the same signer into the manifest, and accept the `storage.cloud.google.com` locator form, which the retired script rewrote and which now refuses the whole plan; (2) find a room's district from its artefacts when `design tag enrich-preview` returns `not_located` (was `find-districts-from-artefacts.py`).

- **Combined output parity**: Desktop and headless combined outputs must agree on MV content and explicit project context (`a9b76d47`, `155d4782`, `70f507fc`, `72155a40`, `655b5a0d`).

- **Northern Hub true combined output is unavailable** (`42f3abd1`): the available combined transformer task emits workbook/geospatial data but no A0/A3 PDFs, while `report project combined` is a compounded ZIP. Expose and prove true combined A0/A3 PDFs and workbook without presenting a compounded archive as combined.

- `74bdd848` (blocker): Headless export computed 113 valid reports, but none reached cloud publication; the project remained stale and Combined Report empty.

- `f6597311` (major): map design batch report: CLI advertises --file-level/--combine-per-group the Desktop rejects, and it needs an unprepared report_archive grouping.

- `da000524` (minor): Export delivery needs a JSON manifest with hash verification and destination receipts.

- `69f60e82` (major): Combined report waits on slow serial publication of all transformer reports.

- `1849f4a3` (major): Group successful report warnings so engineering blockers stay actionable.

- `c9df4098` (major): listed report and drafting artefacts remain readable by URL with no credential (legacy public read in ds-apis-tf `infra/default_bucket.storage.rules`); the code is the §1 signed-reads slice.

- `7e3067c9` (major): the governed compounded report for `czgmdwth_gisagara` (all 94 active participants) refused `report_no_individual_artifacts` with `missing_individual_artifact_count` 94 and `reporter_request_failed`. Make the service reporter fan-out produce or reuse fresh individual XLSX/GIS artifacts with exact source pins, name genuine row failures, and expose the nested non-secret reporter cause (transport, capacity or refused input). The deployed replay follows in SEEDING.md.

- `ae4fe689`: reporter combined quantities let a `fill_in` name prefix override explicit `new_transformer` authority. The explicit field must win; prove with a `fill_in_*` transformer whose `new_transformer` says otherwise.

- `9d0f28da` (major): installed ds-canary 0.1.6 refuses `report project settings --project czgmdwth_gisagara --lane stable` and `report layout get` with `auth_response_unreadable`, although all 31 config sheets read through `design config sheets`; the closed-contract break sits in the report-settings read (printing catalogue or receipt decode). Reproduce on a native-profile build of current source, fix the decode, and prove both reads plus a typed refusal for a genuinely unreadable response.

- Never-landed (owner 2026-10-02, land adapted to run): Generic archive-group readers: kernel, ds-cli (890bac3) and ds-web (54a65730) read the `group_count`/`groups` ds-brain writes, not `district_count` only.

## 8. Sync, publication and edge outbox

- **Cross-process native publication wake without polling** (owner 2026-10-10): Desktop sync runs on auth/startup, intentional navigation/refresh, reconnect, visible return and native/local producer events. Deliver seals/recovery transitions from independent CLI/MCP/native producers into that existing wake path using an owned IPC/filesystem event; schedule retained retry work only at the kernel's deadline. Preserve account/install/project fences, coalescing and headless publication. Do not restore idle thirty-second queue scans or use report-activity timers to discover work. Prove an external sealed result wakes the primary and duplicate wake events do not duplicate publication.

- `fd5bce6a` (blocker): Gisagara A0 contour reports cannot replace shared project publications.

- `d50cc39d` (blocker): Edge compute without a visible, self-healing publication: 593 sealed report artifacts (379 MB) stuck in the Desktop pending-upload outbox behind a dead-process lock since 2026-09-18; nobody was told.

- `6ffc5d52` (blocker): when report admission fails before queueing, Sync Center loses the failure. It must stay visible after restart with producer, revision and retry guidance, retain its files and receipts, and recover without duplicate publication (ds-web/kernel sync lifecycle). ds-web `a78f5ab44` only removed the "all delivered" wording.

- Expose cloud-conflict review as bounded JSON for both DS Grid and LV, with exact local/base/cloud identity, before/after facts, complete change counts, explicit omitted details and useful resolution choices. Existing Grid CLI comparison lists changed IDs but lacks field values; the Grid menu lists revision/digest pins without a content review. Preserve working copies and current head/digest overwrite guards. Advice must distinguish two-way differences from a merge proved against an actual base; provide the same native review facts through CLI/MCP and the JSON view.

- **Bounded cloud-head reads**: the affected `fill_in_gishubi` cloud-head read produced no output for more than ten minutes. Provide a bounded result or named stalled-acquisition refusal while preserving retained local artifacts. Reproduction receipts: `ds-command-kernel/_discardable/artifact-inventory-refresh/root-installed-publications-stalled.json`; publication recovery is tracked in [SEEDING.md](SEEDING.md#8-sync-publication-and-edge-outbox).

## 9. LV processing, sizing and headless intake

- `4070556f` (major): a kept approved network skips the process-time protection (and span-length) audit: ds-network `keep_approved_network` (`network/mod.rs`) returns the supplied network without `sizing::audit_transformer_protection` or `quick_build::audit_lv_span_lengths`, so a frozen transformer whose saved DB cable, breaker or fuse disagrees with its size ships no `transformer_protection_mismatch` warning (the report-time Engineering Review still flags it). **Proof:** a `native_lv_preservation_contract` case where a kept job carries the same mismatch warning as an unkept control.

- `00ad9848`, `47136650` (major): saved LV designs whose spans or nodes lack processor node ids are refused for voltage drop ("saved span has no from_node_id", "saved topology has no LV spans to prove feature ids"). Add an analysis-only topology derivation (or a topology patch preview plus a save door) on a copy, with exact source-digest proof and a typed ambiguity refusal. Saved designs that carry node ids already get non-destructive VD through `keep_network_as_drawn`.

- `007c1624` (major): Legacy (unfenced) transformer rooms cannot be read headlessly: lv project-export refuses, features select scans 0, location path resolves nothing, no headless migrate.

- `6ed42257` (blocker): `design.lv.project-save` requires the request source layers to equal the exported saved-head digest (ds-cli `crates/ds-cli-design/src/lv/project_save.rs` `source_layers_for_fence` refuses with `fast_lv_save_input_invalid`; kernel `crates/ds-client-core/src/transformer_save.rs` digest check), so a client-supplied design that differs from the head is refused. Add a governed supplied-layers save fenced on the exported head version/digest, carrying the kept-as-drawn rows, with a dry-run diff against the head.

- `09de401e` (minor): after `design migrate apply --overwrite`, every overwritten head carries one more feature property key than its source (agasharu 61 vs 60, agatare 38 vs 37, akamana 38 vs 37; the fresh copy upgrade_agacyamu 65 vs 65). Find what survives from the replaced head in ds-brain design-migrate. **Proof:** pinned preview `properties_before` is equal on source and destination.

- `7f5ac170` (major): No migration from the deprecated struct_type 'Existing' marker to existing_status across a project's transformers.

- `83f3712b` (major): DBF-truncated field names: (a) dropped empty export columns are not reported; (b) ds-web duplicates the truncation mapping in TypeScript (`src/lib/design/columns.ts` `suggestDesignColumnHeader`, used by `upload-inspect.ts`): expose ds-network `restore_dbf_truncated_columns` through WASM and delete the TS and its tests; (c) the headless `ds design intake upload --mode lv-process` path does not run the native restore (fold into `14fd08e8`).

- `14fd08e8` (major): Headless LV intake cannot upload to Northern Hub.

## 10. Voltage drop

- **Analysis/Reporter input binding** (reporter, ds-cli): (1) reporter `src/exports/voltage_drop.rs` `admit_engine_result` must admit `jobs[0].analysis` with `jobs[0].layers` through `admit_analysis(transformer, analysis, layers)` and record `analysis.inputs_digest` instead of `summary.inputs_digest`, so a standalone result and a saved analysis bind one digest; (2) ds-cli `docs/reference/design.md`: document the additive per-job `analysis` member in the `ds.lv-voltage-drop.result/v1` example and prose; (3) direct-artifact discovery only after a reproduced failing case. Release acceptance is in [SEEDING.md](SEEDING.md#10-voltage-drop).

- **Report title block names the project by its id** (`e538e632`): ds-brain `network_reporter_input_receipt.go:407` labels `printing_context.project` with `project_data.project_name` ("gisagara") instead of the display name. Use the display name and prove it in the receipt test. The governed logo source is an owner decision.

- **Voltage-drop export contract text still describes the old A4 behaviour** (ds-cli, residue of reporter `026e441`; feedback `159282a4`, `e538e632`): the `report export` purpose (`crates/ds-cli-report/src/export.rs:51`) still says it prints reserved/incomplete/refused status on A4 and that governed identity supplies logos, and `docs/reference/report.md:126-129` still promises A4 status pages and logos on every A4 title block. Restate both: A4 prints only calculated analysis, without logos; non-calculated analysis is refused with `voltage_drop_pdf_not_calculated`; the combined A3 overview covers reserved and refused transformers. Regenerate ds-web `src/lib/tools/native-catalog.generated.json`. Optionally drop stale status-document wording in `crates/ds-cli-style/src/a4_seed.rs` and the stale test comment at reporter `src/exports/voltage_drop_pdf.rs:738`. **Proof:** the discovery and doc-contract tests pass and the regenerated catalogue matches the new text.

## 11. PLS-CADD exchange round trip

- **PLS engineering fidelity** (`4ed6b202`, `ef2478d4`, `34ad2bea`, `0a653e07`, `9817e17e`): the native round trip must retain connected T-offs, use section ruling spans for clearance, reject Method-2 uplift without a negative weight branch, and preserve valid cable and criteria records. Prove each against its filed PLS acceptance before issuing strength results.

- **PLS restore acceptance** (`1086e058`, `6f346aa3`): handle the catalogued Save changes exit prompt, then compare every restored file with the backup; a dialog count alone must not report success.

- **Import validation misses reversed routes** (`41176f22`): the model lacks typed source or tap roles. Govern the source relation so validation flags reversed alignments without false positives.

- **Old `.dsgrid` revisions**: refused with a named code once the old readers go; `ds grid model upgrade` republishes every published head.

- **8. PLS-CADD comparison harness and exchange fidelity**: a repeatable harness (DS Grid vs PLS-CADD on the pinned revisions, through the installed `ds pls desktop` verbs) whose outputs are committed tests. Keep the exchange suite green (612 ds-grid-exchange, 432 ds-io, 488 ds-network at `f1a8e02`). Exchange items: `7a836b7a`/`9f2316b3`/`6405185d` Repair Wizard; `e258b287`; `f27c69f1` Model 2 merges 76 of 709 sections; `732fcd12` two criterion sets; `3724e4ce` ACSR member; `91331096` Method-2 signed wind cases; `9817e17e` export re-authors CRI/cable files; `0a653e07` ACSR cable extra line; `de849b9b` 1 m alignment gap; `8f312f23` immutable source provenance; `b3bdc857`, `b1a2b25c`, `2b8cd4ae`, `ec73ef89`, `90cf2311`.

- **Restore failure reproduction** (`7a836b7a`): repair the affected DS-authored backup only from its exact source and native Restore evidence. Include the known clean Gisagara round-trip as a positive control; relative `FILENAME=.\` headers alone are not an established Repair Wizard trigger.

- **Restore path handling** (`9f2316b3`): verify the filed project `.str`/`.cri` rebasing failure on the affected source before changing writers. Keep clean native positive controls and source bytes; do not preserve the disproved blanket claim that every relative member header causes repair.

- `90cf2311` (minor): a bounded PLS workspace export of only the members an edit touches (e.g. the Model 2 cables folder for a creep check) is not possible without first earthing every existing structure. The refusal already names every unearthed structure and its remedy.

- `7ce604c5` (major): the duplicate-weather-label refusal names labels and ids but says to rename all but one state without saying which to keep. In a workspace export with a preserved source CRI, renaming the carried legacy state (e.g. High wind 50 C / 980.8 Pa) still fails with `carried weather row "High wind" does not match any model weather state` (`pls_cadd_library.rs:2467`). The refusal must say which state matches the carried source CRI row (keep its label) and which to rename, or the carried-row check must explain the mismatch.

- `3a302e54` (major): PLS export can write a blank final weather case, so PLS-CADD stops on open (`wc_name_to_index couldnt find weather case`). The final-after-load weather cases are still unaddressed; write them from the model or refuse by name. Windows restore/readback acceptance is in SEEDING.md §21.

- **Imported PLS structure orientation** (needs characterization): a structure's `orientation_rad` is its travel bearing (e.g. 5.0776 rad at Nyamagabe `str-423815a32fed4309-420`), but the imported PLS STRUCT puts its transverse offset in `local_x`; `placed_local_xy` rotates local x by orientation, so imported transverse phase offsets would be placed along the line, and `resolve_directional_strain` reads PLS strain azimuths (1.571/4.712) as transverse and refuses. Compare ds-grid-exchange `pls_cadd_workspace.rs` attachment import with ds-grid-engine `projection.rs` `placed_local_xy` and `structure_placement.rs` `resolve_directional_strain`; prove placement against a PLS reference.

- **`linked_models` suite red**: ds-grid-exchange `tests/linked_models.rs` fails 6 of 9 on `run` with "stored checkpoint differs from its graph baseline" (composite verify). Find the regression and prove the suite.

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

- **One transformer acquisition boundary for every consumer** (owner 2026-10-10, current stabilization slice): pinning, map/edit opening, printing, individual/combined reports, collision analysis, tiling and native CLI/Server consumers must ask one acquisition owner for the complete transformer room. Consumers never fetch transformers independently. Shared native freshness checks existing head/revision/class inventory; reuse unchanged local rooms without polling or per-consumer probes, acquire only missing/stale inputs, preserve dirty rooms, and coalesce simultaneous consumers/download jobs under explicit principal/project/transformer context. Recheck freshness and local revision at publication so a download started before an edit/save cannot overwrite it. A preview is derived from the complete canonical room, never promoted into editable truth. Save readback and conflict comparison are explicit cloud-snapshot intents through that same boundary, preserving their fresh authoritative reads.

  **Derived outputs are part of this boundary** (owner's follow-up 2026-10-10): the decision to print/recalculate or reuse an existing artifact belongs to the same shared freshness authority. Reuse authorized project artifacts produced by another user or machine when the exact relevant inputs and output identity match; producer identity alone must not force recomputation. Require per-output producing provenance, complete dependency identity (source revisions/content, analysis, project configuration/seeds/reference data, styles, layout, extent/context layers, media and output options), compatible engine/build and verified bytes. A matching transformer name, date, report success flag, filename or the existing server input-base fingerprint alone is insufficient. Unchanged independent outputs remain reusable when another output or transformer changes. Historical downloads stay available without claiming current freshness; explicit regenerate/Refresh remains an intentional new run. Reports never wait for tiling. See [report-inventory.md](../docs/contracts/report-inventory.md#freshness-and-automatic-reuse).

  **Remaining execution plan:** (1) move remaining native headless features/data coverage/collision readers onto `ds-project-data::room_hold::acquire`, sharing the native freshness rule and preserving exact saved-content digests; (2) trace tiling and report derivative workers to complete admitted snapshots and remove their remaining independent reads; (3) bind print/report reuse decisions to complete per-output dependency provenance through this owner, exposed first through native CLI/MCP, then consumed by hosts. Prove reuse of another authorized user's output without source download or rendering, exact byte verification, changed source/analysis/style/layout/extent/media/options/engine refusals, missing historical provenance, revoked access, concurrent edits/publication and unaffected-output reuse. Record reads, transferred bytes, renders and latency for cold acquisition, warm reuse and one changed dependency. Native project exports hold rooms but headless features/data extent and computed-result readers still call cloud context directly. `report::inventory` retains per-output origins but is a read-only projector, not reuse authority; `report_export::input_base_fingerprint` v2 covers transformer/version, sheets and country/reference, not the full requested print closure. Preserve retained A4 previews' richer exact binding and intentional Refresh contract. Audit remaining metadata consumers, including shared-style reads, native printing/context and derivative workers, against the last-observed project metadata owner; classify intentional navigation/Refresh and authority reads separately from consumer-triggered probes. Bind future style/status push invalidations to that owner without polling. Contract: [transformer-acquisition.md](../docs/contracts/transformer-acquisition.md); installed map acceptance belongs to SEEDING §16. Do not claim all consumers migrated before each remaining audited path is proven.

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
  remove standalone/custom tile generation and obsolete BigQuery tiling/explorer, and
  measure read/cost/latency savings. Future expert jobs are outside scope.
  Maintain the moving remaining plan here; no new owner input is needed for
  autonomous code. Whole-path acceptance is in the plan's matrix.

  Active owning vertical: **4 — durable report derivatives**. Reports must never
  call or await tiling. Connect the closed reporter derivative worker and its
  verified handoff to durable scheduling. Persist a minimal derivative descriptor
  through the existing report seal/commit and independently reconcile durable jobs
  over retained artifacts: Desktop staging and CLI temporary results are deleted;
  cloud object names are reusable, so pin/retain the exact upload generation or
  immutable output. Do not mistake a ready handoff for a scheduled/completed job.
  complete survey acquisition must schedule the same owner locally. No standalone
  vector-tiling API or custom dataset/explorer replacement is admitted. Next
  independent vertical: retire/archive the old tiler repository and deployed
  services after the coordinated replacement cutover. Brain/gateway adapter and
  standalone/custom dispatch removal is source-proven; deployment/retirement
  remains in SEEDING §22. Preserve original
  upload bytes and exact source generations, mirror leases and central reads.
  The optional HTTP/TLS dependency closure is outside this critical integration
  path; no local generator request may select a remote source.
  Remaining **2 — store integration** around `ds-layer-store::project_snapshot::Store`: spatial indexes/Polars projection and filter
  pushdown, recovery estimates and bounded retention policy.
  `Pin::visit_layer` still hashes/scans full layer chunks; prepared predicates do
  not establish spatial/columnar pushdown. Later CLI/report/map consumers
  must share the captured persisted views rather than interpreting the mask as
  source completeness.
  Prove actual Windows ACL/locking/crash recovery; the helper MSVC compile check
  is not Windows execution. Extend the existing chunk/head/pin implementation
  described in `docs/contracts/ds-project-snapshots.md`, do not replace it or
  infer analytical pushdown from its initial streaming local-mask door.
  Source adapters must consume the shared snapshot contract through the real
  authenticated source/delta protocol in slice 6; current production acquisition
  has not migrated. Use installed `/usr/bin/ds-canary` for development acceptance;
  the separately paused stable release remains outside scope, except the
  explicitly authorized 2026-10-10 named sprint in SEEDING §22. Remaining **3 — engine
  qualification/integration** extends `ds-network/crates/ds-native-tiles/CONTRACT.md`:
  buffered clipping and representative concave/boundary/hole/sliver fidelity,
  bounded Parquet ingestion and larger-project streaming/RSS,
  actual Windows execution/link/privacy, third-party notices and cross-platform
  byte parity before switching production callers. Do not replace the existing
  checked writer/readback or reintroduce external tools. Container/core
  dependencies still include TLS with defaults disabled; do not claim a
  credential-free dependency graph itself establishes an offline-only closure.
  **4** durable native jobs plus actual Windows execution.
  Keep CLI/Desktop generation behind the reporter process boundary and remove
  replaced MBTiles/SQLite code from `ds-tile-runtime`. Preserve the existing
  reporter/Store SQLite and kernel time pins; use the pinned container feature
  boundary and reproducible actual-consumer resolution gate in
  `ds-network/crates/ds-native-tiles/CONTRACT.md`, rather than upgrading unrelated
  stores or restoring MBTiles dependencies.
  **5** existing CLI/MCP and Desktop
  controls/automatic triggers with report independence; **6** real governed
  source/delta/lease/upload/independent verification/CAS publication APIs and
  central readback. Reuse existing survey read authority, but do not treat
  `SurveyEntriesChangesService` as snapshot continuity: it returns geometry and
  selected columns, validates all scoped rows on each page, and the mirror sync
  currently DELETEs tombstones. Implement maintained epoch/change/deletion
  evidence and full typed source properties before adapting that route; a
  timestamp-fenced page is not a retained change ledger. **7** standalone cloud generation, obsolete custom BigQuery explorer/
  tiling and external-tool/fallback deletion with consumer tracing; **8** matching
  development packaging, installed discovery and Windows/Linux real-map matrix.
  Every slice is proved, committed and pushed on canonical `run` before the next.
  Acceptance retains before/after cold/warm/delta/report/area request counts,
  source work/billed bytes where observable, transfer, cloud invocations, latency
  and peak memory/disk. Assert zero standalone/custom cloud tiling, unchanged source downloads,
  mask-induced remote calls and redundant cross-PC compute; never claim live
  publication or Windows proof from fixtures/Linux alone. Sync Center describes
  project compute contributors and central publication, never PC tile serving.
  Retire/archive ds-vector-tiler and remove its deployment/gateway bindings only
  after needed non-tiling consumers move. Complete survey downloads and combined
  preparation must reuse acquired data for background native derivatives, cloud
  and local, with distinct tile outcomes and centrally verified publication.
  Ordinary users retain cloud simple reports; cloud combined/compounded report
  generation is developer-only. Native/local generation keeps existing project
  permissions, with repeatable local combined reports for every authorized user
  and no developer requirement or cloud-generation quota. Future custom short-lived containers and governed dataset jobs
  remain outside scope. The stable release remains paused.


- `33003988` (minor): in-flight and transient dataset-download refusals reach the CLI as `desktop_refused`/failed with `retryable: false`. Give them a typed retryable code with a stated wait (a `cliRefusal` in ds-web reference-data TS, the refusal declared in ds-cli-desktop `data.rs`, regenerated ds-web `native-catalog.generated.json`), and stop truncating the nested cause (ds-cli-desktop `bridge.rs` `bounded()` keeps only the first line, at most 160 chars).

- `b98643ff` (major): a bulk `data.rwanda.install` stops at the first failed dataset and returns no installed list: `seedReferenceCatalog` in ds-web `src/lib/reference-data/resources.ts` and the loop in `src/lib/reference-data/commands.ts` throw at once. Continue past a failure and name which datasets landed and which failed, moving that orchestration (planning, install loop, refusal mapping) from TypeScript into the kernel.

- `dbe4d17e` (major): Ground data never settles: (a) datasets with no published bundle must be reported as unpublished, not as incomplete downloads; (b) the Data panel re-runs acquisition on every visit and must settle to complete=true once everything publishable is held; (c) once the named cause is reproduced (SEEDING.md §16), fix why expanded DEM/wetlands/powerlines bundles are never indexed.

- `011ae5a6` (blocker): a project-data tile output is listed by neither `ds tile list` nor `ds tile status` (this needs a new row type in the closed client decoder), and the upload receipt says ready before that output exists.

- `cf92af6c` (major): No way to measure staked-out LV line length by district or sector: survey aggregates count only, entries carry no admin fields, and geometry selection has no pagination.

- `46dec14e` (major): one 10k-point layer still needs manual manoeuvring: (1) the map draw cap of 10,000 features (`MAX_LAYER_FEATURES` in ds-cli-map and ds-web `cli-map.ts`, held equal by bridge parity) should become the same byte bound as `map local register` (64 MiB), or `data convert` should emit GeoJSON directly, so one 10,748-point shapefile becomes one visible layer in two `ds` calls with no scripting; (2) the running app does not load prepared local layers live, and `map view` / `map local list` do not say whether a prepared layer is loaded; (3) the ds-cli-data `lib.rs` `--source` help still says "zipped Shapefile"; (4) regenerate ds-web `src/lib/tools/native-catalog.generated.json`, whose `data.inspect` entry lacks the `offset` argument.

- **Vector skip parity:** the ds-network runner now names engine-empty drops (`8e56edcb`: `engine_empty`, `no_station`, `no_buffer`, `skipped_features`). The kernel portable `vector_ops` (`src/vector_ops*.rs`, WASM `vectorProcess`) still counts such features as processed and does not name them; ds-cli `domain_smoke` `vector_cli_file_and_json_packets_match_the_kernel_and_frozen_native_baseline` is red on `run` (packet 4: processed 0 vs frozen 2; `skipped` lacks `no_station`). Make `vector_ops` name the same drops, refreeze `ds-command-kernel/tests/fixtures/vector_processing_packets.json`, and turn kernel `tests/vector_processing.rs` and that ds-cli test green.

## 17. Project Work and Assets

- **Account-wide push and one background receiver — required before the deployment sprint** (owner 2026-10-10): no timer-driven data reads or repeated bounded Gateway long polls. Navigation, explicit refresh, canonical mutations/producer commits and actual push invalidations drive reads. Project conversations remain the same authorized project groups in every workspace; task assignment/message notifications from project X must reach the addressed user while viewing Y without silently switching navigation. Browser uses its existing account/FID/epoch-fenced push registration and canonical inbox; prove dedupe, logout/revocation and dirty-workspace click admission. Remaining native code: replace Gateway-buffered account SSE with an admitted true-push transport, one receiver per account/lane with peer invalidation (a lock that starves other visible windows is forbidden), and lifecycle-independent credential renewal. Owner clarification after the power cut: always-on means Desktop in the tray, not a warm cloud Brain instance. Only one notification owner may remain resident while supporting multiple visible workspaces; complete receiver ownership/handoff and peer fanout across the native host's residency boundary. Preserve multiple Desktop instance isolation. Do not claim close-to-tray establishes singleton push, or restore polling when push is unavailable. Native Desktop and CLI/MCP discovery/controls must expose truthful delivery state; no new project/chat authority or notification content store. Real installed Windows/Linux close→receive assignment from another project→restore/click/account-change proof is in SEEDING §22. Do not deploy until this mandatory gate is fulfilled.

- `e01cf456` (major): one command must create an inbox task with two files attached and return their ids (`pm task create --attach <file>`, repeatable), with `--detach`, and a scheduled task must be able to return to unscheduled. `ds pm task attach` attaches only one file per call. The feedback parks these as additions under SRE mode; confirm the owner's go in OPEN-X.md before coding.

- **Board and navigation failures**: fix the Windows production Board API error and local Web go/return navigation from exact authenticated route/request evidence. Preserve explicit project context and persistent attachment state; an unauthenticated 401 does not prove signed-in behaviour.

- **Project overview aggregation**: make the project overview (the existing `/projects/[id]` page; drawer entry `overview` / `mom_nav_project_overview`) aggregate the canonical PM tasks, records and notes, including items without geometry: a kernel/CLI read first, which the page then renders.

- **Assets list stale after ingest**: on canary 0.1.6 (`it_rwanda`), a plain `ds assets list --folder X` right after `ds assets ingest` leaves the new row out until `--refresh`; the client's own write does not invalidate its index. Reproduce on current source and invalidate the index on a successful ingest.

- **Stale PM doc comment**: kernel `src/project_management/reads.rs` still cites the deleted `filterProjectWorkTableRows` (`views.ts:174-188`) as the Table's search box; the Table now calls this kernel rule directly. Correct the comment.

- **Shared PM/Assets lifecycle and interaction**: implement consistent offline-first state and active/testing/archive scope across PM, Assets, overview and governance. Provide the missing dense governance/Installations interaction through native hit/snap or typed CLI proposals; keep engineering and graph decisions in Rust/server contracts. Project theme values are operational parameters in SEEDING.md.

- **Assets initial-index latency**: measure and fix the remaining slow initial local index acquisition without unbounded prefetch, duplicate authority reads or refetch on tab switching. Preserve the proved bounded snapshot/preview lifecycle.

## 18. Global DS Grid library

- `9e9f6527` (minor): a library read of an existing global release returns no member assets inventory (replayed 2026-10-09), so the explorer has no member inventory or preview. Migrate the legacy release assets inventory and provide an authority-owned bounded member preview through `ds`.

- Global DS Grid example forks still use server-derived legacy publication claims without the native manifest ID/revision. The current-schema publication fence refuses that source before writing a head; repair the owning fork producer through explicit current-package migration and exact native claims. No compatibility/default reader or unverified metadata fallback.

## 19. Country neutrality and duplicated rules

- **No single-country assumptions** ([PRINCIPLES.md §7](PRINCIPLES.md#7-no-single-country-assumptions); country scout 2026-10-02, ~223 findings). The worst are fallbacks that turn a Chad input into a Rwanda one. Queued Sol slices, started as capacity frees:
  - **CRS from the project, never Rwanda TM by default**: ds-io no-`.prj` and PLS import/writeback defaults; ds-brain country->CRS (Web Mercator for non-Rwanda; switching to auto UTM needs an inventory of stored project CRS first); reporter X/Y, print rectangles and collisions; WASM origin (30,-2)/32736; ds-web edit-projection policy -> kernel. Print graticule (`TmRwanda` default) and the fixed Rwanda MV context bbox go to the print slices; reporter `print_graticule::render_project` hard-codes `GraticuleCrs::TmRwanda` as the metric class and ignores the kernel `Graticule.crs`, leaving the `Wgs84` arms of `auto_interval`/`label` and `ANGULAR_STEPS_S` test-only: rename the class to a metric one or delete it there.
  - **Country identity**: a required ISO-3166 `country_code` on the project; missing country never becomes "Rwanda"; one alias table; the reporter receipt digest per country; project creation (ds-brain `project_template_service.go` / `models.DefaultNetworkTemplate`) still defaults `network_template` to `"master"`.
  - **Solar generic** (after `solar-int`), reading the existing **Solar templates** (ds-brain `POST /api/v1/solar`, `internal/models/solar.go`: `_root`, `_config`, numbered sheets; kernel `solar_snapshot.rs`) as the only source, with no new settings invented: household `kind`/reference category instead of `Pauvre/Moyenne/Riche`; template currency and exchange rate (no XAF/600 constants); required `report_language`; `report_profile` for benchmarks, term sheet and standards; BOQ item roles.
  - **Report i18n** (after `print-lv`): project `report_locale` and a message catalogue for the reporter (sheets, headers, InfoTable, legends, the voltage-drop HTML report); one header-alias function built from the template.
  - **Vocabulary leftovers**: `"ABC Terminal Assembly"` injected on tapping poles; `existing` accepting English aliases only; legacy unnamed `lv_poles_rules` rows are still named `rule_edcl` (ds-brain `defaultRuleSetName`, kernel `DEFAULT_RULE_SET`, ds-network).
  - **Voltage classes**: the 30 kV-only MV clearance class and fuse columns keyed 15/30 kV (Codex main for DS Grid `structure_rules`/`clearance_criteria`).
  - **Guards**: rename invariance and parameter perturbation cover BOTH template systems (Network Template and Solar templates); the Chad conformance fixture `td-aderm-conformance` (from `arjgpydw_aderm_loc7`, `beinamar_02`, ds-solar `aderm_*` cities) beside the Rwanda peer; rename invariance (slice `template-invariance`); a run-level token-leak check (a Chad run has no `Rwanda|EDCL|REG|NIX|RWF|Kigali`, a Rwanda run no `XAF|ADERM|OHADA|Tchad`); a per-repo literal ratchet. Chad DEM source curation is in [SEEDING.md](SEEDING.md#16-map-data-datasets-and-screen-styling); use its declared Copernicus GLO-30 source rather than reopening the recorded choice.

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

- **Source-attributed unnecessary API calls** (ds-web, kernel client-core, ds-brain; owner 2026-10-10): remaining navigation/remount, messaging, project catalogue, report/status and credential-refresh call paths lack a complete caller/action attribution and no-poll acceptance matrix. Use the existing [Brain frequency baseline](../../ds-brain/docs/operations/cloud-request-frequency.md), without repeating its measurement or inferring caller identity from route counts. Reproduce the remaining redundant reads, repair them in their owning vertical, and prove bounded request counts for repeated navigation, explicit refresh, mutations, push, account change, hidden/closed windows and multiple workspaces. Keep authorized reads/signing on the tiny ordinary Brain, including `/report`; heavy generation belongs to its compute owner. Preserve membership/revocation, expiry and durable writes; do not reduce cost by dropping notifications or suppressing necessary credential renewal. True native push and single resident ownership are the separate mandatory OPEN §17 gate. Live cost attribution and post-cutover comparison are SEEDING §22.

- **Ordinary Brain 128 MiB, staged bulk containment** (ds-brain, ds-apis-tf; owner 2026-10-10): follow `ds-brain/docs/authorization-only-128mib-migration-plan.md` and `docs/contracts/runtime-bulk-placement.md`. Remaining execution: (1) exhaustively classify ordinary routes/actions, startup and background work, starting with report/proxy buffers, form catalogue/purge, Project Work graphs, single/batch mutation decoding, project-form activation and shared media/asset/sync constructors; (2) profile each retained surface and move unsafe work behind existing bulk endpoints, removing only safely separable initialization; (3) prove full-runtime mixed bursts, slow clients, cancellation and soak, then select proven 128 MiB limits/concurrency. Inherit the recorded source-probe baseline and bounded-collector results; do not repeat known OOM levels or treat isolated source tests as server qualification. Preserve Simple Notes' single gateway, NDJSON terminal summary, durable creates and update/delete/restore contracts through owning tests. Brain stays server-side for authority, signing and bounded commits; bulk may remain permanently. BQ-to-GCS/signed-object redesign and bulk retirement are deferred. The owner subsequently authorized monitored 128 MiB rollout before completing full qualification, using OOM-to-SRE/email to hunt residual routes. Do not claim cloud savings from sizing or placement alone. A clean maintenance cutover with brief downtime is acceptable; no production fallback or owner hand-testing. Simple Notes application changes are deferred today; its current `/mobile` bundle is part of the explicitly authorized coordinated production/Canary sprint in SEEDING §22. Keep remaining full-runtime qualification open after deployment.

- **Busy-container admission and client recovery** (ds-brain, kernel client-core, ds-cli-auth, simple_notes; owner 2026-10-10): qualify bounded bulk admission before handler/body decoding, returning typed busy/`Retry-After` and proving zero effects on rejection; preserve stream cancellation/backpressure and do not exhaust slots with unrelated long-lived streams. Native `TransportResponse` currently drops headers and `WeakNetwork` uses fixed blocking sleeps; wire the existing kernel `response_policy.rs` Retry-After/jitter authority rather than introducing another policy. Mobile request/outbox waits ignore server hints; preserve durable retryLater and stream checkpoint semantics. Never expand ambiguous write retries: current optional entry replay receipts are sequential check/execute/record and mobile outbox lacks an atomic idempotency guarantee. Brain's existing bounded retry contract is in `docs/contracts/reliability-contract.md`; mutation safety requires its owning guarantee, not merely a 503.

- **Feedback and SRE closure are not linked** (kernel, ds-cli): kernel `ds-client-core` `sre.rs` `read_ds_client`/`project_ds_client` and the ds-cli-sre overview must read settled automatic reports and present a command+code as addressed when its newest event is older than the close of its newest settled report, keeping the counts as evidence (contract §5.3a step 4).

## 23. Kernel controls, JSON UI and TypeScript retirement

Rules: [PRINCIPLES.md §5](PRINCIPLES.md#5-kernel-cli-and-ui-owner-2026-10-07) — migrate touched controls one vertical at a time; complicated TypeScript goes to the CLI and its UI becomes JSON; result renderers and their downloads stay.

- **First-release native controls and JSON UI**: move demonstrated UI-only or duplicated control decisions into the owning Rust kernel/native slice, expose and prove them through CLI/MCP, then bind Web/Desktop to that same authority. Supply a JSON input editor and native read-only preview/result where polished controls are not ready; apply must remain explicit, project/principal/revision fenced and faithful to authored choices. Render the owner’s validation, refusals and results without TypeScript recomputation or hidden defaults. Prove headless behavior and UI/native request/result parity, then delete replaced TypeScript control logic, direct gateway bypasses, obsolete adapters/tests/imports and dead code in that slice. Keep only useful presentation/transport/interaction code; preserve unrelated work. Durable contracts and tests live with their owner.

- **Sidebar gateway bypasses** (ds-web `docs/layer-domain-claude-handoff.md`): Tiles/Files (`api/userData.ts`, `api/projectData.ts`), global tiles, survey pipeline/reports, basemap, Design section, PM context and drawer chrome still call API Gateway from TypeScript. One complete vertical each: kernel decides, native/WASM parity, negative controls, retired TS deleted. Then the local sketch-layer lifecycle (`docs/prompts/claude-local-sketch-layer-lifecycle.md`) and authenticated browser-to-Server transport (loopback/SSH only today).

- **One Rust communication authority** (`docs/prompts/rust-communications-refactor.md`): every request to API Gateway goes through one shared Rust client (identity/lane, auth, retries, cancellation, resumable transfer, durable scheduling) and the TypeScript copies go. The transfer protocol landed 2026-09-10; live acceptance and the remaining TS callers are unverified.

- **Project expiry countdown is shown nowhere**: ds-web `ProjectExpiryChip.svelte` has been unmounted since `92c874ca` (2026-08-21), while ds-web `CLAUDE.md` says the header chip carries the expiry countdown. Mount it again over the existing expiry read (or correct the doc if the owner retired it), with a render test.

- **Project and instance isolation** (`docs/prompts/claude-project-instance-isolation.md`; contract `project-and-instance-isolation-contract.md`): after the Server execution context: live Desktop instance discovery and CLI/MCP routing that refuses stale or ambiguous targets; generation-fenced UI session state; multi-principal authority and fair capacity across Solar, printing and a third operation on one Server.

## 24. Owner feature requests (CLI-first, after stabilization)

- **DS Workflows — reusable analytics and reconciliation, deferred during stabilization** (owner direction 2026-10-07; generalized 2026-10-10): declarative `ds workflow` documents over the Rust vector/table engines become the preferred data and geographic processing interface; Data Factory becomes one way to build a workflow (temporary or saved) and its TypeScript transforms are deleted; agents compose workflows instead of patching data with geopandas, pandas or Polars. Contract and owner questions: [docs/contracts/ds-workflows.md](../docs/contracts/ds-workflows.md) (§14). Phases, each landed and proven before the next, only once the owner admits the work:
  0. Inventory every Data Factory, upload-wizard and attribute-table transformation and map it to a spec §5.2 op; record demonstrated missing operations in this entry and define their owning contracts before implementation.
  1. Kernel: `ds.workflow/v1` types, canonical digest, DAG validation, op registry, static schema propagation; tests over spec §13 documents and negative controls, building with `ds-network` absent.
  2. Kernel: Arrow frames with admission; `data.vector.*` and `geo.*` ops on `ds-geo-lite`, byte-identical to the standalone commands; dedupe and layer append moved from TypeScript to Rust.
  3. ds-cli: `ds workflow ops|op|new|step|validate|preview|plan|run` and one MCP chapter; spec §13 examples 1 and 3 and the negative controls through installed `ds`.
  4. ds-network: `table.*` ops over `ds-geo::table` and `ds-cleaning`, streaming via `ds-columnar`, memory class and `workflow_budget_exceeded`; spec §13 example 2 headless to `design.stage`.
  5. Saved immutable revisions, receipts, plan/source-changed guards.
  6. ds-web: Data Factory emits steps into a temporary draft; delete `data-factory-layer-merge.ts`, `data-factory-geometry-merge.ts`, the transformation parts of `attributeTable/table-tools.ts` and `data-factory/columnar-request.ts`; spec §13 example 4 byte-identical.
  7. Design intake as a built-in workflow template; 8. a `ds-workflows` skill proven by a blind agent.

  **Workflow delivery specimens:** [Gisagara reconciliation scripts and design-impact notes](workflows-enhancements/gisagara-lv-reconciliation/README.md) capture read-in-place temporary sources, ephemeral compositions promoted only when reuse has value, mixed GIS/table cleaning, guarded patches, report-only plan-kVA projection and bidirectional geometry review. Replace these bridges with the existing typed workflow contract; include GPKG file sinks, grouped local-map hydration and explicit source/target fences. Original source files remain in place; the copied scripts are evidence, not product entry points. No separate implementation campaign is admitted by preserving them.

  **General reconciliation gap, not a pole-restore feature** (owner clarification 2026-10-10; existing feedback `959dd4d6-c75c-44da-b54a-7689cf59d7e4`): existing property selectors/staged edits, revision comparisons and GPKG upload/processing do not provide a composable, read-only multi-source analytics → match → selected-field diff → patch preview → governed apply → saved-readback workflow. The reported delivery needed temporary Python/SQLite, Shapely, SciPy and pyproj to read GeoPackages, match records, orchestrate DS commands and verify exported readbacks. Replace that operator-side bridge with reusable native sources, typed operations and bounded sinks; do not introduce a special April/pole restoration algorithm or require external scripting/GIS packages. Internal compiled library dependencies are an owning implementation choice, not an operator prerequisite. The shared semantics are in [DS Workflows §5.3](../docs/contracts/ds-workflows.md#53-reusable-reconciliation-and-guarded-batch-edits).

  Remaining composition: digest-pinned local GeoPackage and existing DS snapshot/cache readers with explicit layers/schema/CRS; deterministic key/identity or explicitly chosen geometry matching with cardinality checks and unmatched/ambiguous ledgers; selected-field differences and allowlisted patches preserving target identity, geometry, counts and unrelated properties; separate explicitly scoped field assignments; revision-fenced staging/apply with per-object atomicity, resumable batch receipts and verified persisted readback. Reuse the existing native edit/save authorities, and link affected supplied-layers save work to [OPEN §9](#9-lv-processing-sizing-and-headless-intake); do not bypass it or rebuild a second design store. Read-only analytics must also work without a design sink. Reuse unchanged authorized local inputs, acquire only missing/stale revisions, and write only genuinely changed targets; no polling, forced full-project downloads or recomputation of untouched analyses.

  **Proof when admitted:** run the same native composition for at least two distinct datasets/field mappings, including the reported historical pole-type restore and an independent table/key-based correction. For the pole case, match only the same declared position, leave unmatched positions untouched, never balance type totals or move/add/remove poles, and keep the separately approved feature scope independent of the type-restore scope. Declare CRS/precision explicitly; the report's 0.001 m normalization is bridge evidence, not a default proximity tolerance or permission to choose nearest neighbours. Prove duplicate/ambiguous matches refuse, changed source/head invalidates the plan, dirty local edits survive, retry does not double-apply, and saved readback has identical target geometry/counts and unchanged non-allowlisted properties. Any required reference-field normalization must be a separately visible authorized change, not hidden inside Save. Report exact totals with bounded samples/omissions, per-step timings, request/read counts, fetched bytes, cache hits and peak memory for cold, warm unchanged and one-changed-target runs. Sample preview is not full-population proof. Keep this feedback open until the installed native CLI/MCP workflow satisfies its acceptance; no new feedback or implementation campaign is started by recording it.

- Per-project style overrides: project over global, separate live/print surfaces, reset to inheritance.

- Multi-village (leaf) Working Area and pipeline filters.

- Governed per-entity DS Grid tag editing, validating `tag_<id>` against definitions.

## 25. CLI reliability reporting and proactive feedback (priority)

Priority ([PRINCIPLES.md §3–4](PRINCIPLES.md#4-report-as-you-go-owner-2026-10-07)). Contract: [docs/contracts/ds-cli-reliability-and-feedback.md](../docs/contracts/ds-cli-reliability-and-feedback.md). Owner questions (opt-out, signed-out hosts, automatic-filing scope, read access) are in [OPEN-X.md block 25](OPEN-X.md#25-cli-reliability-and-feedback); slices below proceed on the spec's defaults. Land each slice on `run` before the next.

- **Web refusal coverage cannot trace existing relays** (ds-cli owning test, ds-web adapters): `every_application_refusal_code_is_documented` fails on the existing dynamic `cliRefusal` calls in `src/lib/desktop/cli-sync-center.ts:126` and `cli-assets-args.ts:29`. Trace the closed Sync Center recovery vocabulary and Assets helper callers into the declared command refusals, retaining detection of unknown/dynamic codes. Do not exempt these adapters or claim the aggregate gate passes. The governed repair/notes/workflow Rust vocabulary is checked separately; this is a pre-existing Web coverage failure, not proof of an API/runtime failure.

- **Wire CLI and MCP (ds-cli):** (1) ds-cli `docs/reference/feedback.md:57-58` still says `error.feedback` carries the draft and `sre_event`; the code puts `ds feedback draft --last` as the last `error.next` and carries no event id. Fix the text. (2) Nothing sets `DS_CLI_HOST_KIND`, so every event reports `host_kind=headless`; Desktop and Server must set it (spec §3.2). (3) Optional: bound `reporter::file_automatic` by `SEND_BUDGET`. The installed readback is in [SEEDING.md block 25](SEEDING.md#25-cli-reliability-and-feedback).

- **`ds feedback draft --last` completeness (kernel, ds-cli):** (a) the draft `component` must be the owning repository from the command registry (spec §4): kernel `reporter.rs` `draft_one` still emits `ds-cli/{domain}`, and ds-cli-contract `Command` has no owner field. (b) A CLI draft without `DS_CLI_AGENT` still lists `needs: agent` while `feedback.submit` requires `--agent`; the MCP adapter (ds-cli-mcp `tools.rs` `run_bounded`) never passes `clientInfo.name` to the child as `DS_CLI_AGENT`; the JSON draft's `context` is an object, not the repeated `key=value` input submit takes (only the human render maps it). (c) The `ds` skill (`ds-cli/skills/ds/SKILL.md`, "Report as you go") does not name `ds feedback draft --last`. Signed-in installed acceptance (spec §7.6) is in SEEDING.md block 25.

- **`ds capabilities --search` does not find `feedback.submit` (ds-cli, text only):** `ds capabilities --search 'report a bug'`, `'report a problem'` and `'file a bug'` return no `feedback.submit` (its search list is empty); only `feedback`, `friction` and `idea` find it. **Proof:** all three phrases return `feedback.submit`; the discovery tests pass.
