# Native report artifact authority

Desktop, CLI and Linux Server use this crate for the same durable publication
store. It was extracted from the Desktop reporter; the Desktop copy and its
private filesystem/publication modules have been removed.

The host supplies its protected root and verified engine output handles. This
crate owns held-directory confinement, receipt validation, streamed digest
verification, atomic batch visibility, recovery, read authority and digest-gated
discard. Domain format and publication admission decisions call
`ds-command-kernel`. It does not acquire a user credential or launch an engine.

`promote_local_artifacts(...).commit(...)` is the durable visibility boundary.
An interrupted promotion is invisible and recoverable; a committed batch stays
available across restarts. `open_batch_output` reopens only a receipt-declared
output. A copied file path is not publication evidence.

`ds-web/crates/ds-sync-runtime::reports::ReportProducer` is the shared projection
of those committed batches into Sync Center. Desktop and Server supply their
root and the common native storage-session transfer adapter. The shared sync
store remains the sole owner of grants, heads, leases and transfer receipts.

The native crate has its own workspace so its filesystem dependencies never
enter the WASM dependency graph. Its moved tests exercise receipt corruption,
identity and path confinement, partial discard, recovery and process-safe locks
without Tauri. Preview reads retain their memory bound; streamed recovery and
publication use the separate artifact size bound.
