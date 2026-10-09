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
