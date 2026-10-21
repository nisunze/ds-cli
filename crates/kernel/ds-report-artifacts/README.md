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

This crate is BYTES, not the queue (owner, 2026-09-20). The sync store is the
queue: `ds-sync-runtime::reports::seal` writes a batch here and records its
row in the store in the same acknowledged step, `ReportProducer` moves and
frees bytes the store's rows name, and `sync_store::queue_status` reads the
queue from the rows. Nothing here decides what is queued; the batch receipts
under `publication-batches/` bind outputs to their integrity receipts and are
what a discard verifies against. The file lock serialises writers of this
directory and is not a liveness signal.

The native crate has its own workspace so its filesystem dependencies never
enter the WASM dependency graph. Its moved tests exercise receipt corruption,
identity and path confinement, partial discard, recovery and process-safe locks
without Tauri. Preview reads retain their memory bound; streamed recovery and
publication use the separate artifact size bound.

A local printing review retains the actual saved input receipt and render recipe
in `local_print_recipe`. Its batch uses `LOCAL_ONLY_SCHEMA` while retaining the
real renderer release identity. It remains readable through the same artifact
locator, but is excluded from main output inventory, publication candidates and
recovery adoption. The explicit review inventory reads only these receipts; it
never replaces the ordinary `export-<transformer>/default` publication head.
