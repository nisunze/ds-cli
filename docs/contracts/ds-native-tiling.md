# Shared native tiling

## Governing migration direction, 2026-10-09

The owner has superseded the Windows/cloud placement and external-tool target
below: Windows and Linux Desktop must tile locally through a shared Rust engine
hosted by `ds-network-reporter`; Tippecanoe and external PMTiles conversion are
to be removed from that path. Hosted Web consumes existing outputs and loses
ordinary tiling controls/dispatch. Survey acquisition uses governed BigQuery
snapshots; saved design uses current Firestore/DS Grid authority without a
BigQuery transaction gate. Reusable Parquet/GeoParquet snapshots feed local
reports, tiles and Polars analytics. Retire ALL current cloud tile generation
and obsolete custom BigQuery tiling/explorer paths. Future expert CLI-driven
container/custom-job APIs are outside this migration. See the executable sequence,
minimal central coordination protocol and full
acceptance in [the migration plan](../desktop-native-tiling-migration-plan.md).

The sections below describe the implementation baseline, not acceptance of the
new direction. Replace them with the proven contract as each slice lands.

### Project compute contribution and central consumption

Owner ruling, 2026-10-09: a PC is a compute contributor to a project, not a
personal project asset or owner of its results. Hardware ownership does not change;
project outputs and their visibility remain under project authority. Contribution
grants no additional permissions. Credentials, staging and queues remain
principal-fenced. Sync Center must explain this role and distinguish computation,
pending delivery, verification and centrally committed publication; local completion
or upload alone is not synchronized delivery. A producer may be recorded for bounded
diagnostics, never as a personal results library or special read path.

PCs generate candidates and upload for central verification/publication; they never
serve tiles through localhost, LAN or generated local files. Web/Desktop maps read
central project generations, available when the contributing PC is offline.
Automatic work claims/coalesces project compute before expensive generation.
Reducing cloud cost and unnecessary reads is required acceptance: measure source
requests/bytes, compute and latency; unchanged refresh transfers no source rows,
and cached work-area changes make no network calls. Verification must not recreate
the removed cloud compute/read cost through unconditional retiling/full rereads.

### Selected engine: VersaTiles Rust libraries

Owner decision, 2026-10-09: embed pinned VersaTiles vector-processing and PMTiles
container libraries behind `ds-tile-runtime`, hosted by `ds-network-reporter`.
Martin and a bespoke tile engine are not parallel options. DS retains source,
authorization, completeness, freshness, budget, job and publication authority.
Expose typed DS requests, not arbitrary upstream pipelines or network sources.
Qualify actual Windows/Linux builds, geometry/attribute preservation, explicit
zoom/point policy, bounded memory and deterministic output. Silent oversized-tile
dropping is forbidden. Qualification gaps require a named fix/refusal; they do
not authorize an engine switch or a Tippecanoe fallback. Details and proof are
in section 4 of the migration plan; this selection does not claim it is installed.

### Differential source refresh and local work areas

Owner requirement, 2026-10-09: revision/coverage metadata drives cheap freshness;
survey/design refresh acquires cursor-bound changes and deletions, reusing
unchanged immutable Parquet chunks. Unchanged refresh performs no source-row
download or tile rebuild. Work areas filter retained disk snapshots locally;
changing a mask inside cached coverage performs no data request and does not
change the underlying complete snapshot or project tile freshness. Missing
coverage alone is acquired. Cursor gaps, source/schema/access changes require
explicit bounded recovery, never silent full downloads or false completeness.
The protocol, atomicity and measurable acceptance are in sections 3.1/3.2 of
the migration plan; this is required migration work, not a shipped capability.

## Implementation baseline

Tiling belongs to the Rust server product. Web adapters call a container;
Linux Server and Desktop execute the same native owner. Source acquisition and
verified publication can use online services. Browser code does not compile
vector tiles or own the native job lifecycle.

`src/tiling_placement.rs` owns the platform decision. Hosted web and Windows
Desktop select `online_container`; Linux Desktop and Linux Server select
`local_native`. macOS continues through the online container until a supported
local toolchain exists. Every route keeps source acquisition and verified
publication as authenticated online service effects. The local cache is
content-addressed; the container cache remains service-managed.

## Owners

- `crates/ds-tile-runtime`: prepared input validation, immutable source
  snapshots, Tippecanoe/PMTiles execution, cancellation, archive verification
  and local artifact cache. No network or credentials.
- `crates/ds-compute-runtime::tiles`: durable source bytes, explicit admitted
  principal/project/lane, retry and cancellation through the existing job store.
- `ds-cli-server`: protected HTTP/CLI submission and result presentation.
- `ds-network-reporter::ds-geospatial-io`: online geospatial acquisition.
  Native tiling remains in the owners above; `ds-vector-tiler` is retired and
  is neither a runtime dependency nor a fallback.

## Toolchain and cache

`crates/ds-tile-runtime/toolchain.json` pins the Linux x86_64 Tippecanoe source
revision/version and PMTiles version/digest. Native execution first uses the
administrator-configured absolute `DS_TILING_TOOLS_DIR`, then the host PATH.
It validates the supported versions and never downloads a tool at runtime.
Tippecanoe and PMTiles are independent system tools: one shared installation
can serve DS and other applications. `ds workstation` may guide an interactive,
digest-verified installation; Server and Desktop packages do not own or bundle
the tools.

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
