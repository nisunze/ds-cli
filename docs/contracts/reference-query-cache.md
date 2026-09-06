# Reference query cache v1

Brain owns source discovery and checks project membership plus the published
catalog access policy on every cloud read. Resource IDs are opaque SHA-256;
versions identify the actual BigQuery table metadata or GCS generation/CRC32C.
A dated GeoParquet snapshot and a live table remain different resources.

`reference_cache` validates immutable full-resource response pages for native
storage. Every page binds resource, version, offset and exact UTF-8 feature JSON
SHA-256. Bounded queries cannot be stored as full-resource pages. Completion
requires a contiguous sequence from offset zero through an explicit terminal
page. Missing, corrupt and different-version pages never prove readiness.

The contract admits 1,000 features / 16 MiB per page, 10,000 pages and 1 GiB per
installed resource. Hosts own atomic persistence, cancellation, progress,
principal-bound catalog metadata and installed resource selection. Resource
readiness alone does not prove project offline readiness.

Local spatial queries return WGS84 envelope candidates with explicit truncation.
Cloud queries use the same envelope candidate semantics. Consumers requiring an
exact geometry intersection must apply their own geometry predicate.

This is a disposable query-response cache, not a second canonical vector format.
GeoParquet and Arrow IPC remain governed by ds-network's columnar contract and
ds-columnar. Existing village DSAB semantic identities are not replaced by a
similarly named catalog snapshot. Report consumers must retain resource/version
provenance separately from calculated engineering data.
