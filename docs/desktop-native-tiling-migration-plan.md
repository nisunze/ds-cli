# Desktop native tiling and reusable project snapshots

Owner direction: 2026-10-09. Status: implementation plan, not a claim of shipped
behavior. Implement serially on canonical `run`. This plan migrates project
tiling to a pool of user PCs and removes all current cloud tile generation.
Future CLI-driven large-container/custom-job APIs are a separate expert project,
not an implementation track, dependency or exception in this migration.

## 1. Decisions and scope

The source and placement decisions below supersede conflicting tiling placement,
Tippecanoe and design-BigQuery requirements in older documents. The standing
ruling is recorded in [backlog/PRINCIPLES.md](../backlog/PRINCIPLES.md#9-desktop-tiling-and-data-sources-owner-2026-10-09).

| Data | Read source for this migration | Local reusable representation |
| --- | --- | --- |
| Survey | Existing governed BigQuery survey read/mirror, with an explicit sync watermark | Complete Parquet/GeoParquet snapshot |
| Saved project design | Existing authoritative Firestore design data and saved DS Grid versions, through the authenticated project API | Complete snapshot of those exact versions |
| Personal/project geographic uploads | Authorized original asset bytes and their immutable generations | Parquet/GeoParquet projection retaining original asset identity |
| Future cross-project design analytics | BigQuery mirror of complete saved design snapshots | Same versioned schema, queryable across projects |

Design is NOT required to enter BigQuery before reporting or tiling. Do not
introduce a design transaction/sync gate through BigQuery. The earlier broad
"nothing reads Firestore" statement is corrected for current design authority;
it does not authorize moving survey reads back to Firestore. Native clients use
their protected DS user context and fixed API contracts, never Firestore SDKs,
ADC, service accounts or direct database credentials.

Windows and Linux Desktop compute project tiles, including admitted personal
uploads in their project/visibility scope, locally with native
Rust. Provide the same platform-independent engine on macOS; prove its supported
build before advertising availability. Linux Server consumes the same owner.
Web displays and downloads existing tiles/data but has no generate/regenerate
control and does not request cloud tiling. Remove ALL existing cloud tile
generation, including custom BigQuery table/dataset tiling and its explorer
surfaces. No dormant cloud fallback or retained special-dataset build admission.

PCs are compute contributors, never tile servers. A job produces a private
candidate, uploads it through the governed project publication protocol, and
the shared catalogue points to the centrally stored verified archive. Web and
Desktop consume that archive through the existing central storage/distribution
path. Do not add localhost tile HTTP endpoints, VersaTiles serving, a PC-backed
map source, LAN/peer serving or direct generated-file map consumption. A PC may
go offline after upload without affecting other users. Offline candidates remain
pending artifacts; consumers retain the last centrally published generation.

`ds-network-reporter` hosts the desktop typed tiling process. The shared algorithm
remains a reusable native crate, not a second reporter implementation. Remove
Tippecanoe and the external PMTiles conversion command from the migrated path.
Do not merely wrap those executables in Rust.

**Selected implementation: VersaTiles Rust libraries**, embedded behind
`ds-tile-runtime` and consumed by `ds-network-reporter`. This is an owner decision,
not an open Martin-versus-VersaTiles evaluation. Do not introduce Martin, a
separate tile server, DuckDB or a new bespoke tile engine for this migration.
DS owns its typed contracts, source acquisition, policy, budgets, jobs, cache and
publication; VersaTiles supplies tile processing and PMTiles container machinery.
Do not expose arbitrary VersaTiles pipeline expressions or network sources to
callers. Construct the admitted local pipeline from validated DS request types.

Parquet is reusable canonical snapshot data; PMTiles is a lossy display
derivative. Future BigQuery mirrors must ingest the full snapshot, never decode
PMTiles to recover original data. Keep Polars as the first analytics engine;
DuckDB is not a dependency or prerequisite of this migration.

### 1.1. Economic and user-experience acceptance

Cloud cost must be low enough for the app to remain bearable to operate. Cutting
unnecessary reads is a first-release requirement, not optional performance
polish. The same changes must reduce repeated network waits, source acquisition
latency, time to useful data and UI stalls. Moving CPU to PCs while leaving
repeated full database scans/downloads is not success.

Measure a baseline before replacement and compare the same admitted revision and
workload afterward. Record source requests, scanned/billed BigQuery bytes where
available, Firestore design read counts where available, transferred bytes,
cloud compute invocations, local peak memory/disk, acquisition/report latency
and work-area query latency. Distinguish observed metrics from unavailable billing
evidence; do not invent currency savings. Repeated warm refresh, report opening
and area changes are mandatory scenarios, not just the first cold acquisition.

Required outcomes: zero cloud tile-generation jobs; zero source-data downloads
for unchanged warm refresh; zero remote data/freshness calls caused solely by a
cached-area mask change; changed-only acquisition for normal refresh; shared
leases/coalescing so multiple PCs do not trigger redundant expensive work; and
central artifact reads that never depend on a user's PC serving traffic. Reuse
source manifests and cached validation evidence during publication; do not turn
verification into unconditional full source downloads or another cloud tiling
job. Central storage, authentication and bounded verification still have costs;
report those honestly. Preserve correct data and authorization while reducing
work, rather than hiding staleness or dropping features to improve a benchmark.

### 1.2. Explicit removal and future-work boundary

Delete current custom BigQuery table/dataset tile-generation, arbitrary dataset
explorer and related obsolete UI/CLI/MCP/API orchestration, adapters, tests,
permissions and documentation after tracing consumers. Do not carry them forward
behind hidden flags. Re-thought expert-only CLI workflows, bigger rented
containers, custom jobs and new cloud-compute endpoints are OUT OF SCOPE. They
are not required to close this migration and must not be implemented here.

This deletion does not remove governed survey BigQuery reads, the future full
design analytics export schema, local Polars workflows, existing published
reference-tile downloads, or unrelated BigQuery-backed application features.
Keep central source/freshness/upload/publication APIs needed for PC contribution;
they coordinate and distribute data, not generate tiles. Keep unrelated current
ds-vector-tiler building/elevation/exchange endpoints unless their own consumers
are deliberately migrated in a separately assigned slice. Demoting tiling is
not authority to indiscriminately delete that whole service.

## 2. Starting points to inspect, not rewrite blindly

At planning time the kernel checkout is `81f4b0f6a0dd9a130dfd9326031be54fc03f9b64`.
Re-read current `run` and live `ds` descriptors before coding; other agents work
on these repositories. Preserve their bytes and stage only your own changes.

| Owner | Existing paths / responsibility |
| --- | --- |
| Kernel | `crates/ds-tile-runtime/src/{lib,archive}.rs`: immutable inputs, cache, cancellation and external tool invocation; `toolchain.json`: external binaries |
| Kernel | `src/tiling_placement.rs`: currently Windows/Web -> online container; Linux -> native |
| Kernel | `crates/ds-compute-runtime/src/tiles.rs`: durable prepared jobs, currently 64 MiB JSON input and Linux Server routing |
| Kernel | `crates/ds-client-core`: closed project data, report and tile transports; add snapshot/publication contracts here, not arbitrary HTTP |
| Reporter | `src/bin/ds-report.rs`, `src/tasks.rs`, `src/combined_tasks.rs`: named process commands, schemas and report artifacts |
| CLI | `ds tile` declarations/dispatch, `ds data`, `ds report`, MCP projection and `skills/ds-tiling` in `ds-cli`; discover exact modules with `rg` |
| Desktop/Web | `src/lib/compute/runtime-control.ts`, `src/lib/desktop/cli-tile.ts`, `src/lib/api/pipeline.ts`, `src/lib/components/layers/PipelineTiles.svelte`, upload dialogs and report/load callers in `ds-web` |
| Central coordination / cloud retirement | `ds-brain/internal/services/tile_orchestrator.go`, source/freshness and upload/publication grants; `ds-vector-tiler/src/{main,tiler}.rs`, gateway bindings and worker deployment callers |

Reuse the existing durable job store, native credential boundary, private cache
and archive verifier. The current `tile.generate` descriptor dispatches a cloud
write; it must change, including examples, effects, refusals and MCP projection.
The current compute adapter identifies every tiling host as Server; do not use
that to accidentally keep Windows Desktop refused. Distinguish a desktop worker
from the Linux-only HTTP Server product. Do not extend unrelated Windows server
hosting merely to run a local tile job.

## 3. One snapshot contract

Declare `ds.project-snapshot/v1` once in owning Rust types and derive JSON
schemas, CLI/MCP descriptors and UI request/result views. Minimum fields:

- Captured principal/audience, deployment lane, exact project or personal owner,
  source kind, source revision vector, schema version and snapshot digest.
- Selection scope: `project_complete`, `principal_complete` or `filtered`;
  exact form/transformer/model/layer coverage and an explicit filter expression.
- For each layer: canonical source-layer name, geometry type/CRS, schema digest,
  stable object IDs, row count, byte count, file SHA-256 and local artifact handle.
- Acquisition receipt: all pages completed, expected/observed coverage, source
  watermark, deleted-record policy and whether that watermark is authoritative
  or a BigQuery mirror watermark. Capture creation time separately from revision.
- For saved design: saved model/version digests, transformer revision vector,
  project tags and categorical vocabulary revision. Unsaved editing is not a
  published project snapshot.

An empty filter string is not proof of completeness. The API must attest the
whole authorized output scope. A reader who sees only a subset can compute a
scoped candidate, but cannot replace a project-wide archive or serve it from a
PC. Only its admitted centrally published scope may become a map source. Include all
relevant enabled survey forms, all report transformers, MV/DS Grid layers and
tag projections required by the output. A complete LV report alone does not
prove complete design coverage when its snapshot lacks MV layers.

Acquire pages against a pinned revision/cursor contract. A source change during
acquisition requires a consistent snapshot or a typed retry; never combine
different source heads and label the result complete. BigQuery sync/coverage
is proved by its governing ledger/receipt, not an invented per-entry `isSynced`
field. If its mirror trails the authoritative source, identify that revision
and do not claim it is current. Do not poll Firestore to make survey complete.

Write columnar batches directly; avoid full-project FeatureCollection JSON in
RAM. Use existing DS Arrow/geometry codecs, WKB GeoParquet metadata with explicit
CRS, stable IDs, nullable typed properties and schema evolution rules. Preserve
original asset bytes separately. Retain deletion/tombstone information where
incremental mirrors need it; exclude deleted objects explicitly from live layers.
Validate count/schema/digest before atomically promoting a staged snapshot.

The durable local catalogue is private and fenced by account, lane, audience,
project/owner and visibility scope. It maps immutable snapshot IDs to immutable
files; paths never act as authority. Logout/project switch stops or detaches the
captured job safely and prevents another account from opening its data. Implement
Windows ACL/private-root protection, atomic rename and locking, not Unix-only
chmod assumptions. Bound disk use, protect artifacts in use, retain last good
snapshots and remove abandoned staging after recovery. Local builds may reuse
an admitted cached snapshot offline; online publication requires fresh authority.

### 3.1. Cheap freshness and differential acquisition

Owner requirement, 2026-10-09: survey and design refresh must reuse durable local
data and acquire only changes. A warm load must not download a complete project
again just because the user reopened it, refreshed a report or moved a work area.

Extend the snapshot contract with a server-issued source epoch, revision vector,
change cursor, coverage revision and immutable chunk manifest. Chunk identities
bind bytes, not timestamps or local paths. The central source owner provides a cheap conditional
freshness operation over maintained source/mirror metadata; it must not count or
scan all Firestore documents or BigQuery rows for every PC's freshness check.
Coalesce checks across views/jobs and honor bounded revalidation/backoff policy.
Show locally cached, checking, current-at-revision, changed, mirror-behind and
offline-unverified states distinctly. Wall-clock age alone never proves freshness.

For an admitted previous snapshot, request the delta from its exact cursor to a
pinned target revision. The response carries changed/new chunks or object batches,
stable IDs, object revisions, deletion tombstones, changed schema/tag/coverage
metadata, next cursor and a terminal completeness receipt. Survey deltas follow
the governed BigQuery mirror's change/deletion ledger; design deltas follow saved
Firestore/DS Grid revision manifests. Neither client polls the database directly.
Server ordering must cover equal timestamps, delayed mirror writes, deletions,
restores and geometry movement. A timestamp-only `updated_at > last_time` query
is not an adequate change protocol. Avoid one remote read per unchanged object.

Apply deltas transactionally into a new immutable manifest that reuses unchanged
Parquet chunks. Persist the new cursor only after all pages/chunks and tombstones
are durable and verified. Retry the same batch idempotently; out-of-order/stale
responses cannot roll back the head. Readers/report/tile jobs pin one manifest
while refresh prepares the next. Compact bounded delta fragments locally without
redownloading or rewriting all unchanged data for every edit. Maintain ID and
spatial indexes and before/after geometry bounds as changes land.

An expired cursor, changed source epoch, incompatible schema, missing deletion
history or changed access scope returns a typed resnapshot requirement with its
reason and estimated cost. Reuse verified unchanged chunks where still admitted;
acquire only missing authorized coverage. A complete reacquisition is a visible,
bounded recovery path, never the ordinary silent refresh. Revoked/restricted
access invalidates the affected local catalogue/view and shared eligibility;
cached data cannot retain authority after a known revocation.

Delta-derived snapshots retain complete status only when their base coverage was
complete and the server attests continuous change coverage through the target.
Validate the logical full-data digest independently of chunk/compaction layout.
Tile/report freshness binds that effective data digest plus its actual schema,
tags/vocabulary and processing dependencies. An unchanged refresh causes no data
transfer, tile rebuild or report recomputation. Changed partitions may invalidate
only their dependent native work; do not promise partial PMTiles mutation unless
the immutable archive writer proves it. Rebuilding an archive locally must still
reuse the existing full local data instead of downloading it again.

### 3.2. Work area is a local view over retained coverage

Store acquired data independently of the active work-area mask. The work area,
form/transformer selection and attribute filters are typed local query predicates
over the disk snapshot and its spatial/row-group indexes. Use Polars projection
and predicate pushdown plus the existing native exact spatial predicate; a bbox
index narrows candidates but does not replace exact geometry selection.

Changing, shrinking, clearing or revisiting a work area already covered on disk
performs no source-data request/download and does not mutate/delete the underlying
snapshot. Refresh checks are separate from mask changes, so dragging a work area
cannot issue repeated remote freshness or data queries. Map, table, report and
analytics consumers share the pinned source snapshot but may have different views.
Return the source revision, view predicate and coverage in each result.

If local coverage is partial, compute the exact missing coverage first and fetch
only admitted missing partitions/objects against a consistent revision. Merge
and deduplicate by stable ID/revision; do not re-fetch the overlap. A moved feature
must enter/leave masked views correctly using the change feed, including objects
whose old position was inside but new position is outside the area. Keep coverage
receipts separate from the current mask; an uncovered region is not an empty one.
Do not infer project completeness from the union of arbitrary area queries
without a server coverage attestation. Do not eagerly download the whole project
merely to display one uncached area; explicit whole-project tiling acquisition is
a separate admitted plan with visible cost.

A work-area-filtered view over a complete current disk snapshot may reuse that
underlying complete snapshot for automatic project tiling. The job uses its
unmasked complete source, never the visible subset; a view-only mask change does
not stale project tiles or replace them with cropped data. Preserve existing
unsaved authored edits as a separate revision; incoming remote deltas must not
overwrite them. Route conflicts through the existing revision/reconciliation
owner rather than last-write-wins.

## 4. Rust tile engine and reporter process

Evolve `ds-tile-runtime` into the pure native tile-generation owner. Replace
`TippecanoeOptions` with versioned engine options; translate any retained legacy
input deliberately and delete the translation when its named callers migrate.
Pin one exact VersaTiles release/revision and its required Rust crates in the
owning Cargo manifests/locks; audit licensing and feature/dependency closure.
Use the minimal vector/container features, excluding unnecessary HTTP serving,
cloud clients, raster/GDAL and alternative analytics dependencies. Confirm the
selected pin contains the required APIs; upstream `main` documentation is not
proof of a released dependency's behavior. A PMTiles writer alone does not
replace a vector tiler.

Qualify the selected libraries on actual Windows and Linux before switching
production callers. Explicitly override automatic zoom selection and point
reduction to the DS policy; never allow oversized tiles to be silently dropped
by a streaming writer. Convert such omissions into a typed failure, or implement
the missing bounded behavior in the owning adapter/upstream contribution. Keep
stable source IDs as declared properties when MVT's numeric feature ID cannot
represent them; prove uniqueness and lossless lookup. Validate multipart/holes,
clipping, geometry/attribute preservation, memory, cancellation and deterministic
PMTiles using section 9. If qualification finds a gap, report the exact gap and
finish independent work; do not silently switch engines, reinstate Tippecanoe
or claim qualification passed. A change of selected engine needs an owner decision.

Reference documentation (implementation must verify against its pinned source):
[VersaTiles pipeline](https://github.com/versatiles-org/versatiles-rs/blob/main/versatiles_pipeline/README.md),
[Rust integration](https://docs.rs/versatiles/latest/versatiles/).

Required algorithm contract:

1. Read validated snapshot record batches and transform declared CRS to the
   tile coordinate system. Refuse unknown CRS; handle world edges/antimeridian.
2. Deterministically assign features to intersecting tiles, clip with buffer,
   simplify by zoom under declared tolerances and encode standards-compliant MVT.
   Preserve polygon holes, multipart geometry, line continuity, IDs, categories,
   tags, layer names and supported property types. Bound pathological geometry.
3. Preserve approved project zoom/detail policy. Design objects must not silently
   disappear through density dropping. Any aggregation, simplification or refusal
   is deterministic, declared and counted; do not hide loss to meet tile limits.
4. Write PMTiles v3 directly: ordered tile IDs/directories, declared compression,
   metadata/vector-layer schema, bounds/zoom range and checked offsets/counts.
   Verify every generated archive before promotion. Empty valid input produces
   an explicit empty result; malformed geometry is not silently called empty.
5. Bound worker concurrency, memory and scratch storage. Spill intermediate tile
   partitions to disk rather than materialize the whole pyramid. Make budgets
   configurable in the kernel, report estimates/observations and refuse with a
   remedy when insufficient. Check cancellation during reads, geometry work,
   encoding and compression. Resume/retry must never expose a partial archive.

Cache keys bind source bytes/revision, visibility scope, relevant tags/vocabulary,
options and engine identity. A visual-only pen change need not rebuild geometry;
an encoded attribute/layer/zoom policy change must invalidate affected outputs.
File paths, wall-clock times and machine identity must not affect deterministic
archive bytes. Verify digest and structure on a cache hit; rebuild corruption.

Add a named `ds-report` tiling task accepting a typed request file and producing
a new result file, using the established process boundary. It accepts prepared
local snapshot handles, options and captured context; it has no credentials and
does not fetch or publish. Include it in `task-schemas` and engine build identity.
Return `ready_local`, `empty_local`, `cancelled` or a typed failure with artifact
digests, coverage, layer/feature/tile diagnostics and bounded performance metrics.
Keep the durable job scheduler in the native runtime so CLI, Desktop and Server
share lifecycle semantics instead of making three reporters.

## 5. Automatic scheduling and desktop controls

The kernel owns eligibility and freshness. UI emits typed events after successful
snapshot acquisition/report preparation; it must not decide completeness itself.

| Trigger | Action |
| --- | --- |
| Report preparation loaded all transformers with no filter | Reuse its complete saved design snapshot; acquire only missing output layers through the admitted snapshot API; check freshness |
| Survey load finished with no filter and complete coverage | Reuse the acquired survey snapshot; check freshness |
| Explicit desktop/CLI tile request | Plan required complete acquisition, reuse matching local data, then build under admitted budgets |
| Personal/project upload parsed and validated locally | Retain original bytes and columnar projection; compute a candidate for governed central publication in its project/visibility scope, never PC serving |
| Filtered/partial/unsaved data | Permit explicitly scoped candidate computation; the subset cannot replace complete shared output or become a PC-backed map source. An independently attested complete unmasked disk snapshot may still supply a project job |
| Work-area mask changes inside disk coverage | Query locally, issue no data/freshness request solely for the mask change, and retain complete project tile freshness |
| Hosted Web load/report | Read existing artifacts/status; do not dispatch a build |

Use a key per project/owner, output kind, scope, revision and engine/options.
Coalesce concurrent triggers and reuse existing jobs. A fresh current artifact
is a no-op. "Last computer to load" means an eligible reader can claim stale
work; it is not last-writer-wins. Automatic triggers claim project-scoped compute work before expensive generation,
coalescing the same source revision across PCs with bounded renewable leases.
Lease loss cancels unnecessary work; expiry admits another eligible contributor.
Explicit offline/diagnostic generation may stage a candidate without a lease,
but cannot publish or serve it; only the current admitted lease holder uploads
and commits a shared candidate.

Separate report success from tile success. Reports finish normally while a
background tile job can be queued, running, ready locally, waiting for network,
published, superseded, cancelled or failed. Retain bounded actionable diagnostics
and retry via existing job controls. No hidden remote fallback when the PC lacks
resources; the user can reduce budgets or retry on another eligible PC. Future
expert cloud jobs are outside this migration, not an available fallback.

Desktop gets a manual plan/generate/cancel/status surface backed by the same
kernel and native CLI/MCP contract; JSON input/result is acceptable. Report/load
triggers use only the existing admitted source scope and background policy, never
a new implicit access grant. Local computation is read-derived; shared publication
requires the delegated server authority described below.

## 6. Central coordination protocol and retirement boundary

This is required central coordination, not a new cloud tile-compute API. Reuse
existing authority/storage contracts and implement the minimal missing source,
delta, lease and publication adapters in their owning repositories. Agree
versioned wire types/fixtures with the backend owner first and implement/prove
both ends, or name the exact separately assigned integration boundary. Mock
servers are development evidence only; future expert compute APIs cannot become
a prerequisite. Deployment follows existing authorization separately.

| Semantic operation | Required result/effect |
| --- | --- |
| Snapshot plan/acquire | Recheck user/project access; attest selection coverage; issue pinned snapshot manifest and bounded authenticated download handles for the source matrix in section 1 |
| Conditional snapshot freshness | Compare admitted epoch/revision/coverage token against maintained metadata; unchanged returns no row/chunk payload and performs no full source scan |
| Snapshot delta / missing coverage | Issue continuous cursor-bound upserts, tombstones and changed chunks to a pinned target; return completeness or a typed recovery requirement; reuse authorized unchanged chunks |
| Tile freshness | Return current published source vector/digest, engine/options identity, visibility scope and catalogue generation; distinguish stale, absent, empty and inaccessible |
| Claim/renew/release | Atomically fence one publication lease to principal/project/output/snapshot/expiry; losing a lease never changes the current archive |
| Candidate upload admission | Issue exact scoped staging destination and expected digests/lengths, not arbitrary bucket/path write privileges |
| Verify/commit | Verify candidate bytes/archive/coverage, current source revision and lease; atomically advance catalogue pointer with compare-and-swap; return committed generation and readback handle |
| Failure/recovery | Idempotent request IDs; expired candidate cleanup; old generation retention; typed revoked, stale, incomplete, busy, invalid and transient results |

Read permission must suffice to obtain a narrowly scoped derived-tile contribution
grant, as directed by the owner; it does not grant editing source data, arbitrary
catalogue writes, access to hidden rows or cross-project publication. The server
checks complete coverage against its own source receipt, not client row counts.
Validate MVT layer/attribute bounds and hostile archives; a client digest is not
proof of safe content or faithful derivation. The publication owner must implement a
verification scheme against the admitted canonical snapshot, covering geometry,
IDs and allowed attributes; counts, client-reported engine identity and sampled
checks alone cannot confer shared-publication authority. Use independent,
source-bound validation and retained source receipts; measure its cost. Do not
regenerate tiles on cloud workers as the default verification mechanism: that
would reintroduce the retired compute path. If validation is insufficient,
refuse shared promotion with its exact reason rather than accept untrusted bytes.
An unverified candidate remains local/staged, never the shared catalogue head.
A newer source head or competing committed generation
refuses stale publication and preserves the last good archive. On ambiguous
network completion, read back the same request/generation before retrying.

Backend metadata/credential/lease storage is not the project data acquisition
policy. This plan does not migrate authentication or all control metadata out
of Firestore. Minimize project data reads by reusing exact snapshots.

Remove every current cloud tile-generation trigger and backend build admission,
not just Web buttons. Inventory and retire worker routes `/vector_tile`,
`/tile_dataset`, `/tile_design/firestore`, `/tile_survey/firestore`, `/tile_upload`
and `/global_tiles/tile`, their callers, queue/retry dispatch and gateway bindings.
Trace custom BigQuery table/dataset explorer-to-tiling consumers across ds-web,
ds-cli, ds-brain, ds-vector-tiler and gateway/deployment configuration. Remove
retired worker resources only after distinguishing them from remaining non-tiling
endpoints. Keep central published-archive reads and contribution coordination.
Prove old requests cannot silently enqueue cloud builds. Future expert special
jobs and their new endpoints are not retained admission stubs. Real shared
publication cannot be called complete against a fixture or undeployed endpoint.

## 7. Parquet analytics and later BigQuery estate

Expose snapshot discovery, schema, bounded preview and local workflow execution
through `ds data` and MCP. Polars lazy scans should project/filter Parquet batches;
use the existing native geographic owners for spatial operations. Do not turn
Polars into a required codec dependency where Arrow/Parquet suffices.

Prove column projection, predicate filtering, joins/grouping and representative
geographic analyses on project and larger bounded fixtures. Measure peak memory,
disk spill, elapsed time and cancellation on a normal PC. Inspect streaming plans:
an operation falling back to an unbounded in-memory plan must be planned/refused
or bounded, not labelled streaming. Keep DuckDB out unless a later measured gap
and owner decision changes the engine choice; preserve the same files either way.

Prepare the full snapshot export schema for later large cross-project BigQuery
tables: tenant/owner ID, project ID, layer/object ID, source revision/snapshot,
geometry, typed properties, tag revision and deletion/current-generation state.
The eventual expert chooses partitioning/clustering from actual queries and
maintains idempotent generation ingestion and a current-generation projection.
Never delete every project to replace one project's mirror. Even if today's
projects do not intersect spatially, project IDs and authorization remain explicit;
geometry overlap cannot confer ownership. BigQuery is the analytical mirror,
not a transaction coordinator or prerequisite for desktop design work.

Creating/populating those giant tables and new elevation acquisition/contour
architecture are later expert work. Do not expand this tiling migration into a
terrain rewrite. Retain current read/render consumers until their own replacement
is integrated and proven.

## 8. Implementation order and per-slice proof

Implement one vertical, prove it, commit and push canonical `run`, then continue.
Fetch before/after pushes, preserve unrelated dirty edits and sync the Windows
and server checkouts safely. No feature branches, temporary coding worktrees,
remote CI builds or deployment under this plan alone.

1. **Contracts and source adapters.** Reproduce current `ds tile` routing and
   snapshot/report acquisition behavior. Define snapshot/completeness/freshness,
   budgets and typed refusals. Coordinate source/publication owner protocol fixtures. Prove source
   policy: survey cannot select Firestore; design requires no BigQuery. Prove
   filtered/paged/permission-limited/moving-head and zero-row cases.
2. **Columnar snapshot store.** Implement immutable manifests, batch readers,
   digests, private local catalogue, differential refresh, local spatial views,
   recovery and bounded retention. Implement sections 3.1/3.2 as first-release
   requirements, not a later optimization. Prove
   schema/geometry roundtrip, nulls, IDs, tags, deletions, interrupted download,
   wrong lane/account/project, corruption, Windows locks/ACLs and offline reuse.
3. **VersaTiles native generator.** Qualify the pinned Rust libraries and replace
   external generation/conversion inside the owning runtime. Prove geometry/MVT
   semantics and independently decoded PMTiles;
   deterministic shuffled input, cross-platform byte identity, cancellation,
   bounded dense data and invalid inputs. Retain baseline artifacts as test
   fixtures where lawful, not the obsolete executable as a shipped fallback.
4. **Reporter and durable native jobs.** Add the typed task, integrate cache and
   job lifecycle, package the matching reporter. Run a real Windows-native build
   and local generation without WSL, SSH, Tippecanoe, external PMTiles or cloud.
   Linux tests alone are not Windows proof. Keep existing report outputs passing.
5. **CLI/MCP and Desktop.** Update existing `tile.plan/generate/status` contracts
   rather than add a parallel command family. Add native job/cancellation and
   snapshot discovery controls where absent. Implement automatic report/load
   triggers and Desktop manual controls, using one kernel decision. Prove cold,
   fresh and stale caches, filtered input, concurrent loads and report independence.
6. **Real API integration.** Implement and prove the minimal agreed source, delta,
   compute-lease and publication protocol in its existing owning services;
   prove an authorized Windows reader obtains a complete snapshot, builds, uploads,
   commits and reads the exact shared generation. Prove stale revision, lease loss,
   restricted reader, revoked user, offline queue and ambiguous completion. Keep
   only operational installation/deployment/acceptance in SEEDING once code lands.
7. **Web retirement and dependency deletion.** Remove Web tiling buttons, handlers,
   auto-dispatch, bypasses, dead imports and tests. Desktop controls remain native.
   Remove ALL cloud tile-generation dispatch/admission, custom BigQuery
   table/dataset tiling and obsolete BigQuery explorer paths. Preserve governed
   survey reads, non-tiling workers and central published-archive consumers.
   Remove Tippecanoe/external conversion tooling, workstation installers, package
   checks, obsolete options and cloud-routing fallbacks after tracing consumers.
   Update placement tests, schemas, docs, skills, release pins and discovery.
8. **Installed acceptance.** Package one matching release; verify `ds --version`,
   `ds doctor`, installed commands, skills receipts, reporter source identity and
   actual Windows/Linux archive rendering. Build artifacts locally. Deployment
   and installs follow their existing authorized release procedures separately.

Do not stop at engine unit tests or a Linux-only prepared-input command. The
acceptance boundary is the whole Windows acquisition -> automatic native build ->
verified central publication -> Web/Desktop central consumption path, plus
project-scoped upload contributions and reusable Parquet analytics. No PC serves
tiles. If separately assigned coordination integration is outstanding,
name its exact protocol/deployment boundary and retain local deliverables; never
claim the migration complete. No new owner decision is needed for routine code.

### Project compute pool: ownership and Sync Center

Owner ruling, 2026-10-09: **a PC is a compute contributor to a project, not a
personal project asset or an owner of the results.** This describes its DS role,
not ownership of the physical hardware. Eligible authorized PCs contribute
interchangeable compute; publication, visibility and lifecycle belong to the
project. Contributing CPU grants no additional data rights. Credentials, local
staging and account-owned queues remain principal-fenced; they are not pooled.

Sync Center must explain this role in its visible introduction. Its native view
must identify the project, source revision, coverage and freshness, and distinguish
waiting for compute, claimed by another contributor, computing, pending upload,
verification/refusal and centrally committed publication. Local completion or
upload alone is not synchronized delivery. Producer identity may support bounded
diagnostics, never a personal results library, ownership or a special read path.
PCs never serve tiles; map clients consume the centrally published generation,
which remains available after the contributing PC goes offline.

### 8.1. Moving execution plan

At implementation start, create the live sequence in the coding agent's plan
tool and maintain the durable remaining obligations in OPEN.md section 16's
tiling entry. Seed it from the eight slices above. Name the active slice, next
independent slice and exact blocking boundary; refresh after each material
finding and each tested push. Remove fulfilled residuals instead of accumulating
checked boxes or a completed-work archive. Tests, owning contracts, commits and
bounded acceptance receipts retain proof. Operational deployment/install work
moves to SEEDING only once its code exists; actual owner decisions alone belong
in OPEN-X. Do not create a fourth backlog or park difficult code as an owner
question. Keep measured cost/read/latency criteria in the live acceptance plan.

## 9. Acceptance matrix and deletion evidence

| Case | Required evidence |
| --- | --- |
| Windows reader, complete survey | BigQuery snapshot receipt; zero direct Firestore survey reads; native archive and shared readback at the admitted revision |
| Windows report, all transformers | Exact saved Firestore/DS Grid design revision; complete output coverage; report succeeds; background tiles published or visibly pending |
| Partial/filtered/restricted snapshot | Scoped candidate works; unauthorized shared project replacement refused; no local serving; current catalogue unchanged |
| Personal/project upload contribution | Original bytes retained, GeoParquet query works, candidate centrally published at its admitted visibility and consumed through the central path; no PC tile server or cloud tile build |
| Fresh, empty, stale, corrupted | Correct distinct result; fresh no-op; valid empty output replaces stale data through admitted commit; corruption rebuilt |
| Two PCs / source edit during job | One publication lease; obsolete candidate cannot win; old good generation remains available |
| Offline/cancel/crash/logout | No partial promotion or credential crossover; retry/recovery and retained report/snapshot behavior proven |
| Geometry and zooms | Points, lines, holes/multipart polygons, boundaries, large features, attribute types, layer names and IDs independently decoded; real map evidence at low/high zoom |
| Memory/disk | Recorded budgets and measurements for small project, dense project and larger streaming fixture; deterministic refusal instead of OOM |
| Unchanged warm refresh | Metadata revalidation only; zero source-row/chunk download bytes, zero full-source scans, zero tile rebuilds and zero report recomputation |
| Incremental survey/design refresh | Insert/update/delete/restore and geometry-move fixtures match a fresh complete snapshot at the same revision; only changed/missing chunks transfer; record request counts, bytes and source work |
| Delta recovery and concurrency | Duplicate/equal-time/out-of-order pages, interrupted apply, source epoch reset, cursor expiry, schema/tag/access changes preserve atomicity and produce the declared recovery/refusal |
| Cached work-area changes | Shrink/expand/clear/revisit within retained coverage produces correct exact spatial results with zero data requests/downloads; full snapshot and project tile freshness unchanged |
| Missing area and moving features | Only missing admitted coverage transfers; overlap deduplicates; moved/deleted features leave/enter views correctly; partial coverage cannot masquerade as complete |
| Hosted Web | No build controls or build requests on load/report/upload; map/status/download of existing outputs still work |
| Cloud retirement and obsolete explorers | All six inventoried worker build routes and all traced custom BQ tiling/explorer callers retired; old API/CLI calls cannot dispatch compute; no fallback or new expert-job endpoints |
| PC compute pool, central consumption | Two PCs contribute without serving any tile URL; shut down the contributing PC and verify another Desktop/Web still consumes the committed central archive |
| App economics and latency | Before/after cold/warm/delta/report/area metrics record source work, transfer and latency; zero cloud tiling, zero unchanged-data download and zero mask-induced remote calls are asserted |
| Packaging | Real Windows native execution with external tiling tools absent; one installed CLI/MCP/reporter contract and matching skills |

Final source searches must account for every remaining Tippecanoe, external
PMTiles command, all cloud tile-dispatch and custom BQ tiling/explorer references.
Delete replaced callers,
adapters, installers and dormant fallbacks, not unrelated non-tiling APIs or PMTiles
readers. Migrate `docs/contracts/ds-native-tiling.md` to the implemented truth and
update the owning contracts in other touched repositories. Delete fulfilled
OPEN entries; retain proof in owning tests/contracts and project receipts, never
a completed-work backlog. Retire this temporary plan after its obligations are
absorbed by those contracts and no implementation work remains.

## 10. Prompt for the ds-server implementation agent

Implement `ds-command-kernel/docs/desktop-native-tiling-migration-plan.md` completely,
following its standing contracts and repository instructions on canonical `run`.
Maintain a moving execution plan and the remaining obligations in OPEN.md section
16; prove, commit and push one owning vertical before starting the next. Preserve
other agents' edits and synchronize Windows/server checkouts safely.

A PC contributes compute to a project; it is not a personal project asset or owner
of outputs. Never serve tiles from a PC, localhost, LAN or generated local file.
Generate candidates natively, upload and verify through central project authority,
and let Web/Desktop consume centrally published artifacts even when the producer
PC is offline. Make that role clear in Sync Center; keep credentials and queues
principal-fenced. Claim/coalesce automatic compute across PCs before heavy work.

Embed pinned VersaTiles Rust libraries in the shared ds-tile-runtime through
ds-network-reporter and typed DS CLI/MCP controls. Remove Tippecanoe/external
conversion. Prove actual Windows-native and Linux builds, fidelity, determinism,
bounded resources, cancellation and explicit oversized-tile refusal. Survey reads
use governed BigQuery snapshots; current design uses saved Firestore/DS Grid,
without a BigQuery transaction gate. Reuse Parquet/GeoParquet snapshots with Polars.
Implement cheap revision freshness, cursor/tombstone deltas and immutable chunk
reuse. Work areas mask retained disk data locally: cached-area changes make zero
network calls; unchanged refresh transfers zero source rows and triggers no build.
Only complete authorized snapshots may replace complete project tiles. Keep
report success independent of background tiling.

Cut cloud cost and unnecessary reads as acceptance requirements; measure request,
byte, compute and latency changes against the same workloads. Implement and prove
minimal existing source/lease/publication coordination, without unconditional cloud
retiling or full rereads as verification. Remove ALL current cloud tile generation,
Web tiling buttons/dispatch, obsolete custom BigQuery tiling and BigQuery explorer,
and traced replaced dependencies, permissions, tests and dormant fallbacks. Future
expert CLI-driven large-container/custom-job APIs are outside scope; do not retain
or create them as a fallback. Preserve non-tiling services and central consumers.

Follow the whole-path acceptance matrix, prove refusals and installed discovery,
and update owning contracts/skills/pins. Do not claim completion with mocks,
Linux-only proof or remaining cloud generation. Delete fulfilled backlog items;
keep only genuine unresolved owner input in OPEN-X and operational acceptance in
SEEDING. Do not resume or publish the separately paused stable release.
