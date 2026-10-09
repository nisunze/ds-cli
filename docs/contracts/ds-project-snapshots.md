# Reusable project snapshots

The wire authority is `src/project_snapshot.rs`, schema
`ds.project-snapshot/v1`. JSON schemas derive from those Rust types. Snapshot
acquisition, disk storage and publication consume the same captured contract;
paths, timestamps and UI active-project selection confer no authority.

Each manifest binds principal, deployment lane, audience, exact project/personal
owner, visibility, source kind, source epoch/revision vector/change cursor,
coverage and vocabulary revision. Complete acquisition requires a terminal
server receipt with matching expected/observed layer and page coverage and
continuous deletion history. Zero layers or rows are valid only with that
receipt. A missing layer is not an empty layer. The local mutable work-area mask
is separate from immutable acquisition coverage.

Survey accepts only the governed BigQuery mirror and identifies its mirror
watermark and lag explicitly. Saved design accepts Firestore/DS Grid authority
and exact saved model digests/transformer/tag revisions, without a BigQuery
transaction gate. Unsaved design cannot become a published source snapshot.
Geographic uploads retain original asset identity/generation/digest separately
from their derived columnar chunks.

Layer schemas declare CRS, geometry kind and stable ID/revision columns. Each
immutable chunk binds SHA-256, bytes, rows and an opaque local catalogue handle.
Layer counts must match their chunk manifests. The logical full-data digest is
distinct from physical chunk layout, so local compaction cannot change source
identity. Structural validation does not verify file bytes or authenticate a
server receipt; storage verifies bytes and the central publication authority
independently verifies derivation and authorization before shared promotion.

Filtered and permission-limited snapshots remain usable local/scoped inputs;
they cannot replace a complete project output. A complete mirror-behind snapshot
is not current. Personal coverage never confers project replacement authority.
PCs contribute compute, never own or serve shared tiles. All map consumers read
the verified central publication; an offline/local candidate is not delivery.

Delta pages bind exact base manifest, captured context, source revision, target,
batch identity and page order. A page either continues by one or carries the
terminal manifest. Epoch/schema/scope changes require declared recovery rather
than silent cursor reuse. Chunks and tombstones have bounded validated identities.
The store must verify all pages and bytes before atomically advancing the cursor,
reuse unchanged chunks, reject stale heads and make duplicate batches idempotent.
An unchanged revision is a freshness no-op, not a delta or a tile rebuild.

Acquisition metrics distinguish observed zero from unavailable billing evidence.
Scanned/billed BigQuery bytes and Firestore document reads are nullable; missing
observations never become claimed savings. Cold/warm/delta/report/area acceptance
must record requests, transfer, source work, cloud invocations and latency.

Owning tests: `tests/project_snapshot.rs` proves source policy, zero data,
filtered/restricted scope, pagination, moving heads, mirror lag, context fences,
upload identity, chunk counts, closed schemas and cursor-bound delta refusal.

Native GeoParquet consumers use the canonical `ds-geo-lite::wkb::geometry_from_wkb`
codec with an explicit node budget. It preserves polygon rings and multipart
geometry, admits either WKB byte order, and refuses truncation, trailing bytes,
non-finite coordinates, mismatched multipart types, extended dimensions, excessive
nesting and declared counts exceeding the remaining input or budget. It does not
assert CRS or geometric validity; those remain source admission and exact-mask
responsibilities. The compatibility envelope reader is not an admission validator.

Manifest byte identity uses SHA-256 over `ds.project-snapshot-manifest/v1` plus
one NUL byte and compact UTF-8 JSON of the entire manifest with
`manifest_sha256` set to the empty string. Object keys are recursively sorted;
array order is preserved. Observation time and opaque artifact handles are bound
as well as source/context fields. `computed_sha256` and `verify_sha256` own this
algorithm. Verification is distinct from structural admission and never makes a
locally created receipt an authenticated publication authority.

## Native immutable storage

`ds-layer-store::project_snapshot::Store` owns retained chunks and its SQLite
catalogue; there is no network client in this boundary. Its namespace hashes the
entire captured context plus source kind. The catalogue rechecks that authority.
A store needs an absolute base with trusted directory ancestors. The new
`snapshot_privacy` helper fails closed on insecure owners/modes, symlinks,
reparse points, multiply linked files or unsupported privacy semantics. It does
not chmod workspace ancestors. Unix entries are 0700/0600; Windows directory
creation supplies a protected current-user-only DACL at creation. Files may
inherit that owner-only ACL. System, Administrators and the exactly resolved
TrustedInstaller identity are trusted only for ancestor substitution checks.

`admit` takes already acquired local files keyed by declared SHA-256 and an
expected head. It bounds chunks to 64 MiB, snapshots to 64 GiB/5 million rows,
and reads 128-row batches with a 100,000-node geometry limit per feature.
It checks byte hashes/counts, actual schema/layer/row coverage, stable ID and
object revision columns, geographic geometry admission and duplicate live IDs.
The logical digest derives from actual decoded Parquet rows. No cursor/head
advances until all validation and durable chunk writes finish; one FULL-sync
SQLite transaction advances the manifest and head together. Errors/cancellation
retain the old head. Verified orphan chunks from interruption can recover locally.

Logical row identity is SHA-256 over `ds.snapshot-feature/v1` plus NUL and the
compact recursively key-sorted JSON feature returned by `batch_to_features`.
All normalized scalar properties, including nulls, ID and complete geometry are
bound. The snapshot logical digest hashes `ds.snapshot-logical-rows/v1` plus
NUL, followed by rows sorted by layer and stable ID using binary UTF-8 order.
Each row contributes layer, stable ID, object revision and its lowercase feature
hash, each prefixed by its UTF-8 byte length as little-endian u64. Thus chunk
layout/compaction does not change logical identity. Duplicate IDs per layer refuse.

A repeated current manifest performs no import or chunk verification. A changed
manifest reuses indexed unchanged chunks without re-decoding/reimporting them;
it currently rehashes those local bytes for integrity. Admission reports imported
chunks/bytes, reused chunks, locally verified reuse bytes and elapsed milliseconds.
These are local measurements, not cloud read or billing evidence.

`pin` captures an immutable manifest behind a shared process lock. Streaming
`visit_layer` validates retained bytes, then applies the existing exact area
predicate locally; it cannot check remote freshness or acquire source data.
`retain` preserves the current head, explicit keep-set and every pinned manifest;
only unreferenced chunks and abandoned owned staging files are removed under the
catalogue lock. A refresh can promote a new head while an old reader remains pinned.

Proof commands: `cargo test --locked --manifest-path crates/ds-layer-store/Cargo.toml
--lib` (108 tests, including real Parquet admission/masks/recovery/retention and
privacy refusal); the Windows helper additionally passes an MSVC target metadata
check. The latter is not Windows runtime, locking, filesystem durability or
packaging acceptance. Acquisition adapters, ordered delta application, spatial
index/Polars projection, persisted masks and deployed callers remain distinct
obligations in OPEN section 16; full streaming local masks alone do not satisfy
that performance matrix.

## Ordered differential application

`assemble_delta` validates the complete ordered page set against an exact base
manifest: one batch/target, contiguous pages, one final terminal receipt, bounded
changes/tombstones and an unchanged layer schema/inventory. Retiring absent or
repeated chunks, repeating changed chunks, or a terminal manifest inconsistent
with accumulated replacements refuses. Both base and terminal manifest identities
must verify. Layer inventory/schema changes require explicit resnapshot recovery.

`Store::apply_delta` pins the base and applies the plan in the same atomic
admission transaction. Before advancing the head it checks every removed live
object ID has a final tombstone and no tombstoned ID remains in the target.
A stable-ID move is a replacement rather than deletion; restoring a previously
removed ID is an addition. Replaying an identical completed delta against a still
retained base is a no-op. A stale base, malformed page, missing tombstone or
corrupt input leaves the current head/cursor untouched. Opaque revision ordering
and source attestation remain the authenticated source protocol's responsibility;
local structural/byte checks never substitute for source authorization.

Owning native tests execute deletion, restoration and movement from actual
Parquet artifacts, compare retained results with full logical source features,
assert unchanged chunk reuse, and prove malformed batches cannot advance a head.
Persisted page-by-page acquisition/resume and replay receipts surviving base
retention remain acquisition integration obligations; this door accepts an
already acquired complete bounded page set.
