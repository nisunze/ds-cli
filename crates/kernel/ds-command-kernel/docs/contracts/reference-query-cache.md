# Reference query cache v2

Brain owns source discovery and checks project membership plus the published
catalog access policy on every cloud read. Resource IDs are opaque SHA-256 and
versions identify the actual native BigQuery table metadata. GCS Parquet
objects are not runtime query providers.

`reference_cache` validates immutable full-resource response pages for native
storage. Every page binds resource, version, offset, exact UTF-8 feature JSON
SHA-256, and its optional query area. Area-selected pages cannot be stored as
full-resource pages. Completion requires a contiguous sequence from offset zero
through an explicit terminal page. Missing, corrupt and different-version pages
never prove readiness.

The contract admits 1,000 features / 16 MiB per page, 10,000 pages and 1 GiB per
installed resource. Hosts own atomic persistence, cancellation, progress,
principal-bound catalog metadata and installed resource selection. Resource
readiness alone does not prove project offline readiness.

Queries accept a WGS84 GeoJSON Polygon or MultiPolygon. Cloud queries use
BigQuery `ST_INTERSECTS`. A completed native install builds a SQLite RTree from
the validated pages; local queries use the RTree for candidates and the shared
geometry kernel for exact intersection. Both return explicit truncation.

This is a versioned local materialization of an authorized cloud resource, not
a second cloud catalog. Existing village DSAB semantic identities are not
replaced by a similarly named catalog resource. Report consumers retain
resource/version provenance separately from calculated engineering data.
