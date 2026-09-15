# Shared native tiling

Tiling belongs to the Rust server product. Web adapters call a container;
Linux Server and Desktop execute the same native owner. Source acquisition and
verified publication can use online services. Browser code does not compile
vector tiles or own the native job lifecycle.

## Owners

- `crates/ds-tile-runtime`: prepared input validation, immutable source
  snapshots, Tippecanoe/PMTiles execution, cancellation, archive verification
  and local artifact cache. No network or credentials.
- `crates/ds-compute-runtime::tiles`: durable source bytes, explicit admitted
  principal/project/lane, retry and cancellation through the existing job store.
- `ds-cli-server`: protected HTTP/CLI submission and result presentation.
- `ds-vector-tiler`: online source and publication adapters. It consumes an
  exact file-digested snapshot of `ds-tile-runtime`; it no longer owns a second
  archive algorithm. `scripts/sync_tile_runtime.py --check` proves the snapshot.

## Toolchain and cache

`crates/ds-tile-runtime/toolchain.json` pins the Linux x86_64 Tippecanoe source
revision/version and PMTiles version/digest. Product resources contain
`tiling-tools/tippecanoe` and `tiling-tools/pmtiles`. Native execution first uses the administrator-configured absolute
`DS_TILING_TOOLS_DIR`, then the host PATH, then the convenience bundle. It
validates the supported versions and never downloads a tool at runtime.
Tippecanoe and PMTiles are independent system tools: one shared installation
can serve DS and other applications. Build systems may compile and bundle a
reproducible copy for convenience; the server does not own their installation.

The cache hashes the captured authority/project, sorted layer names and exact
source bytes, tiling options, tool binaries, toolchain and runtime source.
Placement paths do not enter archive metadata. A cache hit must pass digest,
length and PMTiles structural verification. Corruption triggers a rebuild;
failed computation preserves the last good result. Force bypasses reuse but
never an active key lock. Only truly empty input returns an empty result.

Builds snapshot inputs before hashing. Interprocess key locks prevent duplicate
work, and an artifact keeps its key lock while the caller consumes it. Archive
storage is trimmed toward 2 GiB, skipping artifacts currently held by a caller;
a single active artifact can exceed that budget. Stable lock files remain so
waiters never lock a replaced inode. Successful artifacts are retained across
process restarts. Cancelled/failed temporary work receives no success receipt.

## Headless contract

`ds server tile submit --input <file> --key <key>` submits a
`ds.tiles.prepared/v1` envelope containing `project`, `layers` (GeoJSON sequence
text), `options` and optional `force`. The HTTP equivalent is
`POST /v1/tile-processing/:key`. An optional named project checks the envelope;
the server never reads an active UI project. Inputs are bounded to 64 MiB at
this door. Status, cancellation, retained input and result use existing job
routes. Result state is `ready_local` or `empty_local`, never published.

Automatic project `tile.generate` needs authorized source snapshot acquisition
and verified publication around this compute owner before Desktop routing is
advertised. Packaging alone is not evidence that the automatic UI flow has
migrated. Existing web/container routing remains until that end-to-end boundary
is proven. The new prepared-input API is usable headlessly in the meantime.
