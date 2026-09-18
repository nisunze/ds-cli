# Report generation and publication activity

The pure `report::activity` projection owns generation/publication counts,
per-transformer regeneration reasons and the no-progress assessment. Native
CLI and browser WASM consume this same owner. Hosts supply IO facts for one
explicit project; no saved active project can redirect the request. Foreign
project rows in the native store or receipt window refuse the entire projection.
This read-only command grants neither project access nor export authority.

Generation progresses queued → preparing → generating → generated or failed.
A valid durable local export increments generated immediately. Cloud publication
is separate and advances only from the native sync store for the exact
`client_publish_id` returned by that generation. Historical published files
never increment the current run's published count. Failures retain the name,
timestamps and error. Host callbacks record facts, not count or policy decisions.

The outbox summary describes supplied native sync-store publications, not every
retained batch directory. It includes pending batches/bytes, oldest pending time,
conflicts/refusals and latest issue. Upload/publish attempts are counted only in
the supplied receipt window, explicitly not lifetime totals. The UI must surface
inventory-read failures rather than show an empty healthy queue.

Current native export regenerates from current held input. Legacy cloud success
or file presence cannot claim native report reuse. Each row reports
revision_conflict, missing_outputs (including requested PDF IDs), or
local_regeneration. Required outputs come from the existing Rust format plan for
captured project settings and selection; the host must not infer paper IDs from
filenames. Hosted cloud freshness is unchanged.

A configurable 30–900 second threshold (default 120) detects absence of generation
phase progress while work remains. Sync traffic cannot reset this clock. The
result identifies waiting active work, or the next queued transformer if no worker
has reported a phase. This is an observable no-progress signal, not a diagnosis
that a process died. Completed generation waiting for sync is not a renderer stall.

Snapshots are bounded to 500 rows, 5,000 artifacts, 5,000 receipts and 32 requested
outputs. Project IDs, transformer identities, publication IDs and error lengths
are validated. The CLI's existing bounded report-plan action adapts this owner;
the UI only displays the projection and executes transport reads.
