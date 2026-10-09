# Desktop native tiling and reusable project snapshots

Owner direction: 2026-10-09. Status: implementation plan, not a claim of shipped
behavior. Implement serially on canonical `run`. This plan governs the desktop
migration; the cloud API and special-job implementation is assigned to an expert.

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

Windows and Linux Desktop build project and personal tiles locally with native
Rust. Provide the same platform-independent engine on macOS; prove its supported
build before advertising availability. Linux Server consumes the same owner.
Web displays and downloads existing tiles/data but has no generate/regenerate
control and does not automatically request cloud tiling. Remove ordinary
project/personal cloud tiling dispatch. An expert owns explicitly governed large
dataset jobs, their resource selection, cloud worker and API deployment.

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
| Cloud expert | `ds-brain/internal/services/tile_orchestrator.go`, source APIs, upload/publication grants; `ds-vector-tiler/src/{main,tiler}.rs` and special jobs |

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
whole authorized output scope. A reader who sees only a subset can tile that
subset privately, but cannot replace a project-wide archive. Include all
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
| Personal/project upload parsed and validated locally | Retain original bytes, create columnar projection and local tiles for its actual visibility scope |
| Filtered/partial/unsaved data | Permit explicitly scoped local tiles; never schedule replacement of the complete shared output |
| Hosted Web load/report | Read existing artifacts/status; do not dispatch a build |

Use a key per project/owner, output kind, scope, revision and engine/options.
Coalesce concurrent triggers and reuse existing jobs. A fresh current artifact
is a no-op. "Last computer to load" means an eligible reader can claim stale
work; it is not last-writer-wins. Allow local generation independently of a
shared-publication lease, but only a lease holder uploads the shared candidate.

Separate report success from tile success. Reports finish normally while a
background tile job can be queued, running, ready locally, waiting for network,
published, superseded, cancelled or failed. Retain bounded actionable diagnostics
and retry via existing job controls. No hidden remote fallback when the PC lacks
resources; the user can reduce budgets, retry on another PC or ask the expert.

Desktop gets a manual plan/generate/cancel/status surface backed by the same
kernel and native CLI/MCP contract; JSON input/result is acceptable. Report/load
triggers use only the existing admitted source scope and background policy, never
a new implicit access grant. Local computation is read-derived; shared publication
requires the delegated server authority described below.

## 6. Expert handoff: source and publication APIs

This is a required protocol, not permission for the desktop implementer to rewrite
the cloud stack. Agree versioned Rust wire types/fixtures first, deliver the client
adapter and a test server against them, and give the expert these exact obligations:

| Semantic operation | Required result/effect |
| --- | --- |
| Snapshot plan/acquire | Recheck user/project access; attest selection coverage; issue pinned snapshot manifest and bounded authenticated download handles for the source matrix in section 1 |
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
proof of safe content or faithful derivation. The expert must implement a
verification scheme against the admitted canonical snapshot, covering geometry,
IDs and allowed attributes; counts, client-reported engine identity and sampled
checks alone cannot confer shared-publication authority. Use deterministic
independent verification/recomputation where required and measure its cost.
An unverified candidate remains local/staged, never the shared catalogue head.
A newer source head or competing committed generation
refuses stale publication and preserves the last good archive. On ambiguous
network completion, read back the same request/generation before retrying.

Backend metadata/credential/lease storage is not the project data acquisition
policy. This plan does not migrate authentication or all control metadata out
of Firestore. Minimize project data reads by reusing exact snapshots.

The expert removes automatic cloud tiling triggers and closes ordinary project
build admission on the backend, not just its UI button. Existing archive reads
remain usable. Keep only an explicit privileged special-dataset job admission,
with dataset revision, cost/resource plan, memory/CPU limits, identity, destination,
status, cancellation and publication receipt. Cloud workers should consume the
same Rust engine; no duplicated tile algorithm. Expert-owned API/deployment work
does not block independent native code, but real shared publication cannot be
called complete against a fixture or an undeployed endpoint.

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
   budgets and typed refusals. Coordinate expert protocol fixtures. Prove source
   policy: survey cannot select Firestore; design requires no BigQuery. Prove
   filtered/paged/permission-limited/moving-head and zero-row cases.
2. **Columnar snapshot store.** Implement immutable manifests, batch readers,
   digests, private local catalogue, recovery and bounded retention. Prove
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
6. **Real API integration.** Expert deploys the agreed source/publication protocol;
   prove an authorized Windows reader obtains a complete snapshot, builds, uploads,
   commits and reads the exact shared generation. Prove stale revision, lease loss,
   restricted reader, revoked user, offline queue and ambiguous completion. Keep
   only operational installation/deployment/acceptance in SEEDING once code lands.
7. **Web retirement and dependency deletion.** Remove Web tiling buttons, handlers,
   auto-dispatch, bypasses, dead imports and tests. Desktop controls remain native.
   Coordinate backend refusal of ordinary cloud build dispatch with the expert.
   Remove Tippecanoe/external conversion tooling, workstation installers, package
   checks, obsolete options and cloud-routing fallbacks after tracing consumers.
   Update placement tests, schemas, docs, skills, release pins and discovery.
8. **Installed acceptance.** Package one matching release; verify `ds --version`,
   `ds doctor`, installed commands, skills receipts, reporter source identity and
   actual Windows/Linux archive rendering. Build artifacts locally. Deployment
   and installs follow their existing authorized release procedures separately.

Do not stop at engine unit tests or a Linux-only prepared-input command. The
acceptance boundary is the whole Windows acquisition -> automatic native build ->
verified shared publication -> Web/Desktop consumption path, plus private local
uploads and reusable Parquet analytics. If expert integration is outstanding,
name its exact protocol/deployment boundary and retain local deliverables; never
claim the migration complete. No new owner decision is needed for routine code.

## 9. Acceptance matrix and deletion evidence

| Case | Required evidence |
| --- | --- |
| Windows reader, complete survey | BigQuery snapshot receipt; zero direct Firestore survey reads; native archive and shared readback at the admitted revision |
| Windows report, all transformers | Exact saved Firestore/DS Grid design revision; complete output coverage; report succeeds; background tiles published or visibly pending |
| Partial/filtered/restricted snapshot | Private scoped output works; shared project replacement refused; current catalogue unchanged |
| Local personal/project upload | Original bytes retained, GeoParquet query works, local map displays native tiles; no cloud build |
| Fresh, empty, stale, corrupted | Correct distinct result; fresh no-op; valid empty output replaces stale data through admitted commit; corruption rebuilt |
| Two PCs / source edit during job | One publication lease; obsolete candidate cannot win; old good generation remains available |
| Offline/cancel/crash/logout | No partial promotion or credential crossover; retry/recovery and retained report/snapshot behavior proven |
| Geometry and zooms | Points, lines, holes/multipart polygons, boundaries, large features, attribute types, layer names and IDs independently decoded; real map evidence at low/high zoom |
| Memory/disk | Recorded budgets and measurements for small project, dense project and larger streaming fixture; deterministic refusal instead of OOM |
| Hosted Web | No build controls or build requests on load/report/upload; map/status/download of existing outputs still work |
| Packaging | Real Windows native execution with external tiling tools absent; one installed CLI/MCP/reporter contract and matching skills |

Final source searches must account for every remaining Tippecanoe, external
PMTiles command and ordinary cloud-dispatch reference. Delete replaced callers,
adapters, installers and dormant fallbacks, not unrelated expert APIs or PMTiles
readers. Migrate `docs/contracts/ds-native-tiling.md` to the implemented truth and
update the owning contracts in other touched repositories. Delete fulfilled
OPEN entries; retain proof in owning tests/contracts and project receipts, never
a completed-work backlog. Retire this temporary plan after its obligations are
absorbed by those contracts and no implementation work remains.

## 10. Prompt for the ds-server implementation agent

> Read `ds-command-kernel/docs/desktop-native-tiling-migration-plan.md` and the
> repository instructions. Implement the native desktop track completely on
> canonical `run`, one tested/committed/pushed vertical at a time, preserving
> other agents' work. Treat the source matrix and 2026-10-09 owner ruling as
> authoritative: survey through governed BigQuery snapshots; design through
> current saved Firestore/DS Grid authority; no design-BigQuery transaction gate.
> Replace Tippecanoe and external conversion with pinned VersaTiles Rust libraries
> behind the shared ds-tile-runtime owner; Martin and a bespoke engine are not
> alternatives to evaluate. Qualify Windows, explicit zoom/point policy and
> oversized-tile refusal before switching callers. Keep DS contracts authoritative.
> Expose it through ds-network-reporter and the DS CLI/MCP, support actual Windows
> native generation, add complete-snapshot report/survey triggers, reusable
> Parquet/GeoParquet and Polars workflows, and remove Web generation controls and
> replaced code. Coordinate the exact source/lease/publication protocol with the
> cloud expert; do not implement or deploy their cloud work without assignment.
> Finish all independent native work and prove Windows through owning tests and
> real native artifacts, not owner hand-testing. Complete shared-publication
> integration when the expert protocol is available; report any exact remaining
> integration boundary candidly. Do not stop after a plan, mocked publication,
> Linux-only success or parallel legacy implementation. Follow the acceptance
> matrix, package matching discovery/skills/pins, and delete fulfilled backlogs.
> Do not resume or publish the separately paused stable release without the
> owner's instruction.
