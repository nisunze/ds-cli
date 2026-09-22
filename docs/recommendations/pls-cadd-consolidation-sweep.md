# PLS-CADD consolidation sweep — ds-cli slice

Status: **index only, not reviewed** (2026-09-22, `run` 7c92301). Candidates
found from `fn`-name collisions, module headers and Cargo edges. Confirm each
item before moving it. Main index, sweep order and the ds-network items:
[ds-network/docs/diagnostics/pls-cadd-consolidation-sweep.md](../../../ds-network/docs/diagnostics/pls-cadd-consolidation-sweep.md).

PLS surface here: `ds-cli-pls` (native-file tasks over `ds-grid-tasks` + `ds-io`),
`ds-cli-dsgrid` (model ⇄ PLS source link, publish native, import),
`ds-cli-dsgrid-exchange` (convert / plan / sync / inspect), `ds-cli-library`
(global catalog, seed, resolve native).

### C1 — Filesystem/argument helpers copied per crate (sweep slice S1)
- `ensure_workspace_closed`: `ds-cli-dsgrid-exchange/src/sync.rs` and
  `ds-cli-pls/src/backup_create.rs`.
- `write_new`: `ds-cli-dsgrid/src/apply.rs`, `ds-cli-dsgrid-exchange/src/sync.rs`,
  `ds-cli-library/src/lib.rs`, `ds-cli-pls/src/backup_create.rs`.
- `sha256`: `ds-cli-dsgrid/src/apply.rs`, `apply_batch.rs`, `ds-cli-library/src/lib.rs`.
- Path validators in `ds-cli-pls/src/lib.rs` (`source_path`, `workspace_path`,
  `source_directory`, `output_path`, `file_digest`, `bounded_limit`) — check the
  other two PLS crates for their own versions.
- `parse_target` / `parse_mode` / `parse_container` in
  `ds-cli-dsgrid-exchange/src/request.rs` duplicate `ds-network/crates/ds-grid-cli`
  (ds-network item N1).
- Target: one helper module in `ds-cli-contract` (or the lowest shared crate).

### C2 — PLS workspace concepts spread over three crates (slice S6)
- Linking a model to a live PLS workspace: `ds-cli-dsgrid/src/model/pls_source.rs`
  (`read_workspace`) vs `ds-grid-tasks/src/reference_closure.rs` (`read_workspace`).
- Workspace handling also in `ds-cli-dsgrid/src/model/workspace.rs`,
  `publish_native.rs`, `import_external.rs`, and `ds-cli-dsgrid-exchange/src/sync.rs`.
- Candidate: native-workspace concepts live in one CLI crate (likely `ds-cli-pls`),
  the others call it.

### C3 — Engine logic restated in the CLI (slice S6)
- `ds-cli-dsgrid/src/feature_codes/mod.rs`: `voltage_class`, `load_standard` —
  also in `ds-grid-engine/feature_code_standard.rs` / `structure_rules.rs`.
- `ds-cli-dsgrid/src/package.rs`: `prior_schema_members` — also in
  `ds-grid-exchange/src/package.rs`.
- `ds-cli-dsgrid/src/report/structures.rs`: `material` — also in exchange
  `pls_pole_geometry.rs` / `staking_derivation.rs`.
- Rule to apply: the CLI adapts arguments and envelopes; the engine decides.

### C4 — Global catalog implemented twice (slice S8, ruling needed)
- `ds-cli-library/src/global_catalog.rs` (1,022): "The governed global DS Grid
  catalog, through the native user."
- `ds-command-kernel/crates/ds-client-core/src/grid_catalog.rs` (440): "The
  governed global DS Grid library and example catalog, headlessly."
- Kernel-first ordering suggests the kernel owns it and the CLI adapts.

### C5 — Docs copied from ds-network (slice S3)
- `docs/contracts/dsgrid-authority/00, 02, 04` are byte-identical copies of
  ds-network's; `01-server-required.md` has **diverged** from ds-network.
- `docs/contracts/program/04-structure-rules-spotting.md` differs from
  `dsgrid-authority/04-…` in this same repo.
- `skills/ds-pls-cadd-terrain-roundtrip/references/native-failure-modes.md`
  overlaps `ds-network/docs/contracts/pls-cadd-native-failure-modes.md`.
- `docs/reference/pls.md`, `dsgrid.md`, `dsgrid-exchange.md` overlap the PLS
  skills; keep whichever the discovery gates read, link the rest.
- Move: keep links to ds-network, delete the copies (mind the discovery-gate
  tests that read prose).

### C6 — Package repack restated per command (found 2026-09-22)
`ds-cli-dsgrid/src/apply.rs`, `apply_batch.rs`, `mutation.rs` and
`profile/labels.rs` each rebuild `PackOptions` from the package manifest; the
first three reset `library_pins` to empty while ds-network's own repacks keep
them. One `GridPackage::repack` in ds-grid-exchange (ds-network N9), called
from here; decide the pins rule once.

### Blast radius when changing C1–C4
`crates/ds/tests/domain_smoke.rs` (13,361 lines), `bridge_parity.rs`,
`mcp.rs`, `contract.rs`, `ds-cli-mcp/src/surface.rs`, `ds-cli-contract/src/spec.rs`.
