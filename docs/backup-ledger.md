# Recovery ledger command

`backup_ledger::evaluate` and WASM `backupLedgerCommand` accept the same JSON
commands: `project { rows }` and `download { rows, selected_ids }`.

Projection is metadata-only. It returns stable event identities, stored archive
references, and the latest recorded activity before deletion. Native `.dsgrid`
references use `backup_layer_uri`; legacy `.firestore.json` references use
`backup_json_uri`. Missing references are unavailable, never loading. Selection
and rendering do not fetch or inspect an archive. An explicit download plan
accepts 1–100 known downloadable event IDs and admits four concurrent transfers.
The authenticated gateway remains the authority for bucket/access checks.

The archive writer records `source_activity_json` alongside the deletion event.
Each activity retains its own action, actor, UTC time, integer milliseconds and
optional status. The writer captures existing source stamps; it does not infer
which operation produced a generic metadata update. The kernel ignores report
invalidation (`stale`) and chooses the latest recorded timestamp, preferring an
explicit operation over a generic document update at the same timestamp.
Missing actors remain missing; the deletion actor never fills that gap.
This is the latest retained source stamp, not a reconstruction of every action.

Old ledger rows can lack activity evidence. Show “Not recorded”; do not download
old archives, query current transformer state or backfill guessed actors during
navigation. The reader uses an optional JSON projection so older ledger schemas
remain readable before the new optional column exists. New evidence requires
the updated backend writer to be running when a deletion occurs.

The browser stores bounded ledger metadata by gateway, principal and window.
Navigation uses that cache (including a known empty result). Cold windows and
explicit refreshes may fetch metadata; archive bytes remain click-only. Metadata
requests have a 30-second deadline, archive transfers 120 seconds, and leaving
the surface cancels requests. Failed/partial refreshes retain the previous cache.

Validation: Rust tests cover native/legacy references, explicit selection,
missing-reference refusals and independent forensic actors. Mounted Svelte tests
use the real WASM command to prove selectable `.dsgrid` rows, no idle downloads
or false spinners, mixed ZIP downloads and unmount cancellation. Backend tests
exercise schema extension and activity capture without cloud access.

The command is available to native Rust callers; a new public `ds backups`
CLI chapter is not part of this UI repair. It must reuse these commands when added.
