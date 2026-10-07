# Open work requiring owner input

> **Moved 2026-10-07:** this backlog now lives in `ds-command-kernel/backlog/`. The old `ds-work/backlog/` no longer exists: edit these files here, commit them to ds-command-kernel `run`, and stage only `backlog/`. ds-work holds working sessions, not coding lists.

Only an unresolved owner decision, authority or supervised step belongs here. Each entry names the input needed and the boundary it controls. Agents continue independent code in [OPEN.md](OPEN.md) and parameter/seeding work in [SEEDING.md](SEEDING.md); they do not wait on this list to fix unrelated defects.

Check existing contracts, source evidence and recorded owner choices first. Routine design choices, test execution, difficult code and host availability do not require owner guidance. If evidence settles a question, return its implementation to OPEN.md or its operational action to SEEDING.md. The owner does not hand-test code: agents prove it through `ds` or owning tests. Delete a resolved decision; retain its durable authority in the owning contract or governed project data, with no historical fixes archive.

<!-- toc -->

| Area | Remaining entries |
| --- | ---: |
| [12. Engineering acceptance and material authority](#12-engineering-acceptance-and-material-authority) | 2 |
| [13. Between-angle policy](#13-between-angle-policy) | 1 |
| [14. Profile display and supervised editing](#14-profile-display-and-supervised-editing) | 2 |
| [15. Project scope and engineering choices](#15-project-scope-and-engineering-choices) | 3 |
| [18. Supervised native qualification](#18-supervised-native-qualification) | 1 |
| [22. Account and data-owner authority](#22-account-and-data-owner-authority) | 2 |
| [25. CLI reliability and feedback](#25-cli-reliability-and-feedback) | 1 |
| [Parked: outside stabilization](#parked-outside-stabilization) | 2 |
<!-- /toc -->

## 12. Engineering acceptance and material authority

- **Engineering acceptance tolerances**: the load/usage, sag/tension and clearance tasks refer to a tolerance without stating it. **Input needed:** identify the approved engineering acceptance basis and tolerances where existing project/native contracts do not already supply them. **Boundary:** declaring engineering equivalence or acceptance, not repairing reproduced computations. Agents implement exact case/source binding and report measured differences under [OPEN.md block 12](OPEN.md#12-ds-grid-engineering-codex-main); native comparison and data qualification remain in [SEEDING.md block 12](SEEDING.md#12-ds-grid-engineering-codex-main). Never widen tolerances to close a defect.

- **Material and capacity source conflicts** (`3d1c735c`, `df1f53ae`): nominal modulus disagrees with curves/GTP, and Method-2 artifacts remain unverified. **Input needed:** obtain the responsible engineering authority’s source precedence and qualified final-condition material/capacity basis when exact source tracing cannot resolve the conflict. **Boundary:** selecting a physical law or certifying capacity. Canonical creep-field transport, provenance, truthful unknown verdicts and reproducible computation fixes stay in [OPEN.md block 12](OPEN.md#12-ds-grid-engineering-codex-main); acquiring and seeding authoritative evidence stays in SEEDING.md.

## 13. Between-angle policy

- **Uncharacterized .012 policy** (`5d0fd02b`): Method-2 steel poles are unplaceable between table knots. **Input needed:** confirm the authoritative between-angle rule if native characterization does not establish it; a coding agent cannot invent interpolation or capacity. **Boundary:** enabling placement using that engineering interpretation. Source tracing, policy discovery, exact-knot tests and conservative unknown-policy refusals continue in [OPEN.md block 13](OPEN.md#13-spotting-codex-main).

## 14. Profile display and supervised editing

- **Usage-% colour bands** (owner request, 2026-10-02): no bands are defined for per-case Profile labels. **Input needed:** choose thresholds and colours, and confirm their meaning for limited-scope screening versus qualified capacity. **Boundary:** seeding semantic colour bands. Native case transport, labels, uplift/clearance findings and UI binding continue in [OPEN.md block 14](OPEN.md#14-plan-and-profile-authoring); do not invent safety bands or hide an unknown verdict.

- **Human/CLI edit demonstration and chosen changes**: the requested full edit cycle includes a person demonstrating edits, deciding which scoped proposals to accept and reviewing modification notes. **Input needed:** that demonstration and the intended edits on an identified model/revision. **Boundary:** applying the person’s choices and marking notes reviewed. Agents first finish and prove pinned draft checkpoints, proposal/decision handling, export gates and HTML note generation in [OPEN.md block 14](OPEN.md#14-plan-and-profile-authoring); ordinary code testing is their responsibility.

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

## Parked: outside stabilization

- **Email/MIME provenance** (`68b6c668`): provide a full-conversation ingest boundary with retained provenance. **Input needed:** decide when this explicitly parked future feature enters the active scope. **Boundary:** starting implementation outside the current stabilization campaign. Once activated, implementation and proof belong in OPEN.md.

- **Failed design-upload quarantine** (ds-web `docs/TODOs.md` §1): retain failed signed-in project uploads (`lv_transformer`, `lv_batch`, `mv_network`) through a ds-brain intake in a quarantined `gs://` prefix, with admin-gated download and a visible notice; never sketch/local layers or successful uploads. **Input needed:** activate this parked feature and specify retention TTL and per-project quota. **Boundary:** adopting that retention policy and starting the feature. Once decided, implement and prove its native intake, access fences and expiry in OPEN.md.
