# Open work requiring owner input

> **Moved 2026-10-07:** this backlog now lives in `ds-command-kernel/backlog/`. The old `ds-work/backlog/` no longer exists: edit these files here, commit them to ds-command-kernel `run`, and stage only `backlog/`. ds-work holds working sessions, not coding lists.

Only an unresolved owner decision, authority or supervised step belongs here. Each entry names the input needed and the boundary it controls. Agents continue independent code in [OPEN.md](OPEN.md) and parameter/seeding work in [SEEDING.md](SEEDING.md); they do not wait on this list to fix unrelated defects.

Check existing contracts, source evidence and recorded owner choices first. Routine design choices, test execution, difficult code and host availability do not require owner guidance. If evidence settles a question, return its implementation to OPEN.md or its operational action to SEEDING.md. The owner does not hand-test code: agents prove it through `ds` or owning tests. Delete a resolved decision; retain its durable authority in the owning contract or governed project data, with no historical fixes archive.

<!-- toc -->

| Area | Remaining entries |
| --- | ---: |
| [12. Engineering acceptance and material authority](#12-engineering-acceptance-and-material-authority) | 2 |
| [13. Between-angle policy](#13-between-angle-policy) | 1 |
| [14. Profile display and supervised editing](#14-profile-display-and-supervised-editing) | 0 |
| [15. Project scope and engineering choices](#15-project-scope-and-engineering-choices) | 3 |
| [18. Supervised native qualification](#18-supervised-native-qualification) | 1 |
| [22. Account and data-owner authority](#22-account-and-data-owner-authority) | 2 |
| [25. CLI reliability and feedback](#25-cli-reliability-and-feedback) | 1 |
| [Solar sync migration: owner inputs](#solar-sync-migration-owner-inputs) | 4 |
| [Parked: outside stabilization](#parked-outside-stabilization) | 2 |
| [Temporary: owner-supervised Codex handover](#temporary-owner-supervised-codex-handover-2026-10-07) | 7 |
<!-- /toc -->

## Temporary: owner-supervised Codex handover (2026-10-07)

From Claude to Codex. The owner supervises this session live: Codex
implements, the owner watches each step and gives the decisions it names.
Delete this section when the session ends; move anything left to its owning
list.

**State:** the MV template preview is proven headlessly.

- **Commits:** kernel `55014fc`/`c34fadb`, ds-cli `7a490bad`, ds-web
  `dabd6e34d`.
- **Command:** `ds report plan-profile --preview-layout <layout.json>
  --project <id> --scene <scene.json> --plan <plan.json> --model-crs <crs>`.
  It renders one sample sheet from in-memory projections; nothing is saved.
- **Proof:** sheet 1 of 91 of the Gisagara M1 seed, under the Rwanda Template
  layout `mv-a3-plan-profile-template`, now classified `mv_model /
  plan_profile`.
- **Headless setup:** a debug `ds` reaches the cloud with
  `DS_NATIVE_CLIENT_PROFILE_BUNDLE` set to the output of ds-web
  `scripts/run/server-profile.mjs`. `DS_REPORT_BIN` points at the dev
  `ds-report`.

1. **Relaunch and prove in the window.** Relaunch DS GridDesign Local (Dev).
   Its shell predates the new `ds_invoke` door, and the current one runs from
   Codex's supervisor. Then open Rwanda Template → Printing setup → MV Sheets
   → Plan and profile, tick an open DS Grid model, and click Preview. **Owner
   watches:** one plan and profile sheet appears, never the transformer list.
2. **Print the ticked models.** The PDF button is not wired for MV templates.
   Run the existing `report plan-profile` render once per ticked model, and
   show each model's pages as they arrive, never waiting for all of them.
   **Owner decides:** one PDF per model, or one combined booklet.
3. **Sync-store migration** (STABILIZATION §15.1). **Owner decides:** what a
   legacy `artifacts` row with a NULL account becomes, discarded or attributed
   to the current account. Codex fixes `ds-sync-store` and proves it on a copy
   of a v1 store.
4. **Select the Template's MV setup** (after step 3). Run `report project
   mv-setup set` for `mv-a3-plan-profile-template` at its current revision.
   **Owner gives:** `external_version` and the fixed `issue_date`.
5. **MV global-default fallback** (STABILIZATION §15.2). **Owner decides:**
   retire the global fallback, so every project selects its own MV setup, or
   read the default from a template project.
6. **Finish the Rwanda Template seed** (SEEDING §3; data only, no code).
   - Replace the logos and title-block text on the remaining LV layouts with
     the generated set in `outputs/rwanda-template-seed-20261007/logos/`.
   - Use fictitious companies and account numbers, and gibberish-Latin
     people's names.
   - **Owner gives:** whether signed-in read-only access needs a new grant,
     since no such control exists.
7. **Printing slices queued:** STABILIZATION §13 (Front matter purpose,
   subject per purpose), §14 (preview map context) and §15.3 (hairline
   dividers break the editor canvas). **Owner picks** the order.

Backups of the reclassified layouts are in
`~/.cache/claude-build-tmp/rwt/reclass/`. `printing-settings-offline.test.ts`
fails on a mock without `canonicalName`; that failure is not from the preview
change.

## 12. Engineering acceptance and material authority

- **Engineering acceptance tolerances**: the load/usage, sag/tension and clearance tasks refer to a tolerance without stating it. **Input needed:** identify the approved engineering acceptance basis and tolerances where existing project/native contracts do not already supply them. **Boundary:** declaring engineering equivalence or acceptance, not repairing reproduced computations. Agents implement exact case/source binding and report measured differences under [OPEN.md block 12](OPEN.md#12-ds-grid-engineering-codex-main); native comparison and data qualification remain in [SEEDING.md block 12](SEEDING.md#12-ds-grid-engineering-codex-main). Never widen tolerances to close a defect.

- **Material and capacity source conflicts** (`3d1c735c`, `df1f53ae`): nominal modulus disagrees with curves/GTP, and Method-2 artifacts remain unverified. **Input needed:** obtain the responsible engineering authority’s source precedence and qualified final-condition material/capacity basis when exact source tracing cannot resolve the conflict. **Boundary:** selecting a physical law or certifying capacity. Canonical creep-field transport, provenance, truthful unknown verdicts and reproducible computation fixes stay in [OPEN.md block 12](OPEN.md#12-ds-grid-engineering-codex-main); acquiring and seeding authoritative evidence stays in SEEDING.md.

## 13. Between-angle policy

- **Uncharacterized .012 policy** (`5d0fd02b`): Method-2 steel poles are unplaceable between table knots. **Input needed:** confirm the authoritative between-angle rule if native characterization does not establish it; a coding agent cannot invent interpolation or capacity. **Boundary:** enabling placement using that engineering interpretation. Source tracing, policy discovery, exact-knot tests and conservative unknown-policy refusals continue in [OPEN.md block 13](OPEN.md#13-spotting-codex-main).

## 14. Profile display and supervised editing

Moved 2026-10-07 to [STABILIZATION.md](STABILIZATION.md) group A (owner decisions kept with their Input needed and Boundary).

## 15. Project scope and engineering choices

- **ADERM new authoring scope** (`c89fea1b`): extend `beinamar_02` to reach the Health Post and Grand Marche within 37 m and size it at 315 kVA. **Input needed:** decide whether the required new LV pole/span authoring capability is admitted during the no-new-features period. **Boundary:** starting that new capability. Once admitted, put its CLI-first implementation in OPEN.md and its approved project delivery in SEEDING.md.

- **Nyamagabe Model 2 angle-duty choice**: EDCL drawing guidance discourages steel single poles at 30–60° angles. **Input needed:** the responsible engineering decision between steel single poles and wooden H-poles, with the approved `nyamagabe-mv` duty policy for `j-w-60d-S255.012` and `j-w-S255.012`. **Boundary:** changing the model’s pole family/policy. Existing strength checks and missing transformer-kVA corrections continue in [SEEDING.md block 15](SEEDING.md#15-project-deliveries).

- **Gisagara M3 publication go-ahead**: the retained rev-55 delivery explicitly requires the owner’s go before publication. **Input needed:** publication authorization for the exact reviewed candidate after usage, strict export and residue checks are complete. **Boundary:** publishing that candidate. Agents finish the concrete reviewable delivery and evidence in [SEEDING.md block 15](SEEDING.md#15-project-deliveries) before requesting this decision.

## 18. Supervised native qualification

- **Unknown PLS-CADD Error Log**: the Windows handoff preserved an unclassified Error Log, reported in process 23884, with screenshot/text under `q11-gisagara-m1-v31\unknown-error-log\`. **Input needed:** operator classification of the actual preserved dialog and the authorized next native action. **Boundary:** resuming that blocked PLS-CADD session; never dismiss an unknown dialog blindly. The complete candidate evidence, trust state and remaining native gates stay in [SEEDING.md block 18](SEEDING.md#18-global-ds-grid-library). Managed library lifecycle and export-admission coding continues independently in [OPEN.md block 18](OPEN.md#18-global-ds-grid-library).

## 22. Account and data-owner authority

- **Unused Firebase CLI token**: revoke the unused token in the Google account. **Input needed:** the account owner’s revocation or explicit account-authorized execution. **Boundary:** changing account credentials; do not assume project access grants Google-account authority.

- **Rwanda BigQuery geometry repairs**: repair the globe-covering reference geometries in the three identified datasets; backed-up rows require the data owner’s write authority. **Input needed:** that owner’s authorization for the exact reviewed row changes. **Boundary:** writing those authoritative datasets. Prepare and validate the concrete repair/readback before seeking approval; do not infer write authority from read access.

## 25. CLI reliability and feedback

- **Reporting and automatic-filing policy** (spec [ds-cli-reliability-and-feedback.md](../docs/contracts/ds-cli-reliability-and-feedback.md) §9): **Input needed:** whether operators may turn SRE reporting off (spec default: on, `DS_SRE_REPORTING=off` for tests only); whether signed-out hosts report as `anonymous` (default) or hold events until sign-in; whether automatic `bug` filing stays crash-only (default) or also covers repeated identical failures; whether a user may read their own CLI events (default: platform admins only). **Boundary:** changing those defaults. All [OPEN.md block 25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) slices proceed on the defaults.

## Solar sync migration: owner inputs

- **Solar publication capability. Input needed:** retain no capability, reuse
  `design.report`, or define a role-seeded `solar.publish`. **Boundary:** the
  Solar row in ds-brain's engine authority table. Implement on today's explicit
  value (none); independent admission and native migration proceed.
- **Retained Desktop IndexedDB Solar queue at cutover. Input needed:** choose
  one release of drain-only legacy queue with visible counts (recommended),
  visible per-city discard and re-run, or native-directory re-seal. Re-seal
  conflicts with the 2026-09-20 never-adopt-Solar ruling and needs an explicit
  superseding ruling. **Boundary:** Phase 5 deletion of the legacy queue only;
  native implementation and proof proceed without this answer. Never silently
  drop or adopt pending publications.
- **Co-located Desktop/Server store (out of scope). Input needed:** whether to
  share one machine store and under what host/installation fence. **Boundary:**
  unifying Desktop app-local-data and Server lane state paths; this migration
  retains their distinct stores.
- **Hosted web Solar (out of scope). Input needed:** authorization and release
  conditions for opening the Cloud Run publication lane. **Boundary:** changing
  the closed routing descriptor; parity work does not open hosted Solar.

## Parked: outside stabilization

- **Email/MIME provenance** (`68b6c668`): provide a full-conversation ingest boundary with retained provenance. **Input needed:** decide when this explicitly parked future feature enters the active scope. **Boundary:** starting implementation outside the current stabilization campaign. Once activated, implementation and proof belong in OPEN.md.

- **Failed design-upload quarantine** (ds-web `docs/TODOs.md` §1): retain failed signed-in project uploads (`lv_transformer`, `lv_batch`, `mv_network`) through a ds-brain intake in a quarantined `gs://` prefix, with admin-gated download and a visible notice; never sketch/local layers or successful uploads. **Input needed:** activate this parked feature and specify retention TTL and per-project quota. **Boundary:** adopting that retention policy and starting the feature. Once decided, implement and prove its native intake, access fences and expiry in OPEN.md.
