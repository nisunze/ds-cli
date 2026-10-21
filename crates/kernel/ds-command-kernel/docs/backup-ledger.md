# Recovery ledger command

`backup_ledger::evaluate` and WASM `backupLedgerCommand` accept the same JSON
commands: `project { rows }` and `download { rows, selected_ids }`.

Projection is metadata-only. It returns stable event identities, stored archive
references, and the latest recorded activity before deletion. Native `.dsgrid`
references use `backup_layer_uri`; legacy `.firestore.json` references use
`backup_json_uri`. Missing references are unavailable, never loading. Selection
and rendering do not fetch or inspect an archive. An explicit download plan
accepts 1–100 known downloadable event IDs and admits four concurrent transfers.
The plan returns `delivery: file` for one distinct event and `delivery: zip`
for multiple events. A single native package or legacy JSON downloads exactly
as stored, without an extra ZIP wrapper.
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

The offline CLI `assets.backup.plan` exposes this owner through bounded local
metadata requests. A project selector cannot redirect the request. Deletion
reason and administrative evidence remain distinct from the deletion actor and
source activities. Missing Rwanda district evidence is a required-location
finding, never a UI choice; old unknown evidence is not fabricated.

Explicit `dsgrid.backup.preview` uses `ds-grid-exchange::backup_preview` (also
bound as grid WASM `backupPreview`). It verifies the native package and exact
project/transformer origin, then reuses the Rust transformer decoder for its
archived layers. Legacy JSON has unverified origin, identified as such. Input
and decoded geometry are bounded to 64 MiB, 128 layers and 100000 features.
Its retained metadata is historical evidence, not current project state. Web
previews occupy a read-only RAM layer group fenced to the signed-in principal;
visibility, focus and removal are view state. They never seed, edit, restore,
publish or change the active project. Loading, principal change and leaving a
pending request cancel and fence the result.
