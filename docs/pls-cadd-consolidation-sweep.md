# PLS-CADD consolidation sweep — kernel slice

Status: **index only, not reviewed** (2026-09-22, `run` 0d3714e). Main index
and sweep order:
[ds-network/docs/diagnostics/pls-cadd-consolidation-sweep.md](../../ds-network/docs/diagnostics/pls-cadd-consolidation-sweep.md).

The kernel carries little PLS code: `src/local_models.rs` (`PlsSourceLink`),
`src/design_attachments.rs` (test fixture label only),
`crates/ds-client-core/src/grid_catalog.rs`.

### K1 — Copied contract doc (slice S3)
`docs/contracts/dsgrid-authority/01-server-required.md` is byte-identical to
ds-cli's copy, and both differ from the ds-network authority. Keep one text in
ds-network and link to it; settle which version is current first.

### K2 — Global catalog implemented twice (slice S8, ruling needed)
`crates/ds-client-core/src/grid_catalog.rs` (440) and
`ds-cli/crates/ds-cli-library/src/global_catalog.rs` (1,022) both describe
themselves as the governed global DS Grid catalog. Kernel-first ordering
suggests this crate owns it and ds-cli becomes a thin adapter.

### K3 — PLS version semantics restated (low)
`PlsSourceLink.pls_version` is derived from the DON `SOURCE='PLS-CADD Version …'`
line. The parser that owns that line is in ds-network
(`ds-io/src/pls_cadd_native/version.rs`, `ds-grid-exchange/src/pls_cadd_version_contract.rs`).
Check that the kernel only stores the string and never parses it itself.
