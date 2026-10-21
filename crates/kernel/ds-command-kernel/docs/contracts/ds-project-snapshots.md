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
`apply_delta` measures its entire successful call, including page canonicalization,
pin acquisition and either replay check; an exact replay still reports zero
imported/reused bytes and chunks. `finish_delta` additionally includes its pending
page read and revalidation in its whole-call successful receipt timing.

Every Store connection enables and query-verifies SQLite `PERSIST_WAL` before
schema access. WAL/SHM remain private after the last connection closes, avoiding
last-reader unlink/recreate races without serializing readers. A 4 MiB journal
size setting is a post-checkpoint retention target, not a hard byte bound while
transactions or readers hold WAL; completed checkpoints permit truncation.
Optional-sidecar privacy guards also
handle legacy overlapping handles: strict no-follow absence may pass, extant
replacements undergo full privacy verification again with bounded retries, and
observed symbolic links or exhausted verification refuse. Permanent catalogue,
lock and pin privacy guards retain their strict checks. Linux tests prove
sidecar inode persistence and concurrent connection lifecycles; they do not
establish Windows execution.

`pin` captures an immutable manifest behind a shared process lock. Streaming
`visit_layer` validates retained bytes, then applies the existing exact area
predicate locally; it cannot check remote freshness or acquire source data.
`retain` preserves the current head, explicit keep-set and every pinned manifest;
only unreferenced chunks and abandoned owned staging files are removed under the
catalogue lock. A refresh can promote a new head while an old reader remains pinned.

Proof commands: `cargo test --locked --manifest-path crates/ds-layer-store/Cargo.toml
--lib` (including real Parquet admission/masks/recovery/retention and
privacy refusal); the Windows helper additionally passes an MSVC target metadata
check. The latter is not Windows runtime, locking, filesystem durability or
packaging acceptance. Acquisition adapters, spatial
index/Polars projection, predicate pushdown and deployed callers remain distinct
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
removed ID is an addition. Replaying an identical completed delta while its
target remains head is a no-op, including after base retention. A stale base,
malformed page, missing tombstone or
corrupt input leaves the current head/cursor untouched. Opaque revision ordering
and source attestation remain the authenticated source protocol's responsibility;
local structural/byte checks never substitute for source authorization.

Owning native tests execute deletion, restoration and movement from actual
Parquet artifacts, compare retained results with full logical source features,
assert unchanged chunk reuse, and prove malformed batches cannot advance a head.

`stage_delta_page` journals ordered metadata in the private SQLite catalogue.
`pending_delta` reports acquisition metadata and its next page; `pending_delta_pages`
returns the exact checked page prefix after reopen. An exact duplicate page is
idempotent. Conflicting identities, duplicate payloads and out-of-order pages
refuse without changing the prefix or head. The terminal page makes metadata
ready, not downloaded or verified. `finish_delta` passes caller-owned chunk paths
through full admission and tombstone verification again. Failure or cancellation
preserves both journal and head. `discard_delta` removes only pending metadata.
Neither API stores download state or owns the caller's files. Transport and
persistent chunk-download resumption remain separate integration obligations.

Admission commits all per-page digest receipts, the new head and pending metadata
removal in one SQLite transaction. Complete-page replay checks every page digest,
page count, batch/base/target and authority identity. It checks both before pinning
the base and again under the exclusive catalogue lock; simultaneous identical
callers yield one promotion and one no-op. An existing target without the exact
recorded batch refuses; replay after head advances refuses rather than rolling
back. `finish_delta` requires pending pages; after successful completion retry
with `apply_delta` and the complete original pages. Staging a completed batch
refuses and never repopulates its pending journal.

Limits are 8 pending batches, 256 pages per batch, 1 MiB serialized per page,
16 MiB per batch and 32 MiB across all pending metadata. Prefix change operations
also have a 100,000-operation bound. The completed receipt history keeps the most
recent 64 batches and at most 256 page digests each. Replay recovery is bounded:
after receipt expiry an old batch has no permanent idempotence guarantee and
cannot mint a receipt merely because its target exists. Retention protects every
pending base even after an independent admission changes head; explicit discard
releases that protection. Delta admission uses catalogue-before-pin ordering, and finalization releases
its read lock before admission's exclusive lock. Local view capture accepts an
already held immutable pin and takes a catalogue read lock; retention's exclusive
catalogue lock only tries pin locks nonblockingly, so it cannot wait on that pin
while capture waits on the catalogue. SQLite uses the existing private catalogue and FULL synchronous WAL policy.


## Persisted local work-area views

`set_work_area(name, area, expected_revision)` stores a named local Polygon or
MultiPolygon admitted by `reference_cache::validate_area` (including its accepted
Feature wrapper); `None` persists an explicitly cleared predicate. Clearing is
distinct from `remove_work_area`, which removes the saved predicate with revision
CAS and never removes data. An absent mask cannot implicitly become an all-data
query: create or clear it explicitly before capture. No mask operation changes
head, source revision, source/chunk bytes, acquisition evidence or tile inputs,
and none performs download, refresh or a remote freshness check.

The private SQLite catalogue admits at most 128 masks, names up to 256 UTF-8
bytes (nonempty, trimmed, without controls), each named-mask body at most 256 KiB
and 4 MiB across stored bodies. Input traversal caps depth at 32 and JSON nodes
at 25,000 before recursive serialization/geometry validation. Per-mask byte
counting and total catalogue admission precede incoming owned JSON copies and
hashing. Serialized bodies are canonical compact JSON of `[name,area]` with all
object keys sorted recursively; `content_sha256` hashes
`ds.snapshot-work-area-content/v1` plus NUL and those exact bytes.

The revision separately binds namespace, canonical content and a durable
increasing namespace generation. The clock persists independently of mask rows;
removal and every changed update advance it in the same FULL-sync transaction
as the mutation. Integrity checksum, missing/negative/regressed clock and signed
SQLite integer overflow checks refuse without repairing/resetting state. Mask
revision hashes `ds.snapshot-work-area-revision/v1` plus NUL and the canonical
JSON tuple `[namespace_sha256,content_sha256,generation]`. `expected_revision=None`
is create-only for an absent name. Existing names require their exact current
revision **before** any identical-content no-op. Stale edits, even same-payload
retries, return `SnapshotStaleView`; callers reread before recovering. A→B→A and
removal/recreation retain equal content digests but never reuse revisions. A
current-revision duplicate leaves the generation and body unchanged.

`capture_view(name,pin)` refuses pins from another physical store or any other
principal, lane, audience, project, visibility or source namespace. The returned
`LocalView` owns the immutable mask and source pin. Changes to a saved mask or
head, moves/deletions in later deltas, and retention cannot alter its query.
It performs no mutable head lookup at read time. Its `visit_layer` delegates to
`Pin::visit_layer` and the existing exact intersects predicate; a cleared mask
selects all **retained** rows. Successful reads return mask content/revision/
generation, predicate, pinned manifest and source revision, context, read source,
selection, acquisition filter, unchanged acquisition/coverage evidence, layer,
observed row count and local elapsed milliseconds. Filtered source scope and
mirror-behind evidence stay explicit, including when the observed row count is
zero. No local predicate establishes completeness or proves an uncovered area
empty.

Persistence is not spatial pushdown: each query currently hashes/scans retained
layer chunks and decodes Parquet rows. It validates the captured area at query
admission, then reuses the geometry owner's immutable `PreparedArea` for every row.
The one-shot `selects_area` delegates to the same owner; holes, boundary touching,
centroid/intersects selection and malformed-feature behavior do not fork. A
later mutation of the input JSON cannot change a prepared predicate. Projection,
spatial/Polars pushdown, missing-coverage acquisition and deployed caller integration remain
separate obligations. Owning real-Parquet tests prove reopen/shrink/expand/
clear/revisit, unchanged source bytes/head, two-writer CAS, generation ABA and
recreation fences, old captured views across source moves/deletions, partial
coverage evidence, namespace refusals, malformed/byte/count/depth budgets and
clock corruption/overflow refusal. Linux tests do not establish native Windows
execution, account ACL behavior or cross-platform replacement durability.
