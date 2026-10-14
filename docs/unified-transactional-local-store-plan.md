# Unified transactional local storage and recoverable design conflicts

Status: implementation plan, source assessment on 2026-10-14. Author: Codex.
This document records the owner's new direction and an incremental execution
sequence. It is not proof of implementation, installed behavior or migration.
This assessment changes documentation only: no product code, live data,
transformer history, dependency installation, build or deployment.

## 1. Decision and scope

Proceed toward SQLite on native Desktop/Server and SQLite-WASM with OPFS in
the browser. Put domain state, schema, admitted transitions, freshness and
conflict decisions in the existing shared Rust owners. Keep TypeScript as
presentation and typed browser IO. SQLite transactions alone do not prevent
lost edits: expected revisions and acknowledgement identities must be checked
inside the same transaction as the writes they govern.

The first useful vertical is one LV transformer: durable edit, explicit Save of
an exact submitted revision, prompt queued publication, accepted acknowledgement or retained
conflict, restart and deliberate resolution. Use the existing GCP authority;
Cloudflare Durable Objects are not a new dependency or a migration requirement.
Solar, Reporter, PM and other consumers adopt the foundation in later bounded
slices; their computations and domain-specific authorization do not move here.

Owner direction for conflicts: preserve competing authored revisions under the
same transformer, show a recognizable conflict prefix, and let an operator or
agent resolve them deliberately. Do not use whole-project last-write-wins,
create duplicate transformers, or automatically increment governance vN.
Normal edits and conflict resolution can advance ordinary revisions while
leaving the explicit governance version unchanged.

This is storage/synchronization planning, not a new UI campaign, a CRDT engine,
whole-kernel extraction, automatic engineering interpretation, or a prerequisite
for the active Reporter/Solar/Network extraction releases. Honor the independent
[Network modularization plan](../backlog/DS-NETWORK-MODULARIZATION.md).

## 2. Source assessment and existing foundation

Canonical ds-server `run` checkouts inspected:

| Owner | Assessed HEAD |
| --- | --- |
| ds-command-kernel | `a811264fbe5f8bcbe77dc358ff60790b4039096b` |
| ds-web | `272c89a89ed7c155f287e7cb09735220d839297f` |
| ds-brain | `1552ed651622660dc4a1800161e1f8016b323183` |

Kernel and Web contain concurrent uncommitted work by other authors. The cited
line numbers are working-copy observations; they must be rechecked before an
implementation slice. Inspected Web changes were development inspection hooks;
selected kernel storage diffs were formatting/module-order changes. None proves
this migration. Brain was clean. Tests listed below were inspected, not executed.

| Finding | Source evidence | Consequence |
| --- | --- | --- |
| Shared pure freshness is already `Fresh / Stale / Unknown`. | Kernel `src/freshness.rs:1-21`; [runtime contract](contracts/ds-headless-application-runtime.md#prompt-queue-sync-and-cheap-input-freshness). | Reuse it; no second timer, TTL decision or host-specific isFresh. |
| Native sync has one durable SQLite queue, leases, fenced rows and immutable receipts. | `crates/ds-sync-store/src/lib.rs:573-623`; `src/sync_store.rs:46-151`; [store contract](contracts/ds-sync-store.md). | Reuse the transactional spine and existing publication pump. |
| Native producer bytes and LocalProduced already seal atomically. | `crates/ds-sync-store/src/publications.rs:25-79`. | Preserve producer provenance and exact replay rather than create a parallel outbox. |
| Native Design commits immutable revision, head, operation receipt and outbox in an IMMEDIATE transaction. | `crates/ds-design-workspace/src/lib.rs:345-412`. | Good authored-state foundation, but stale expected head returns before candidate insertion. |
| Browser and Desktop transformer rooms still use raw IndexedDB, not Dexie. | Web `src/lib/stores/db.ts:23-26,85-89,211-247`; database `ds_web_store` v26. | Desktop SQLite does not yet own these editable rooms. |
| Room/header writes are atomic but ordinary edits have no local expected-revision check. | Web `db.ts:1526-1629`; `design/loader.ts:245-272`. | Serialized writes can still replace a different tab's authored work. |
| A save ACK can mark the current room clean without matching the submitted local revision, digest, account or operation. | Web `db.ts:1438-1472`; `components/design/StatusTable.svelte:2814-2823`. | A late ACK can hide an edit made during the save. |
| Differential save decisions remain TypeScript; pending IDs are header flags rather than immutable queued payloads. | Web `design/differential-save.ts:102-131,318-496`; StatusTable `2753-2823`. | Move these decisions into the shared owner, then delete the host decider. |
| Cloud save uses a Firestore transaction, but missing base_version disables CAS. | Brain `internal/bulk/services/transformer_service.go:266-287,1191-1258`; model `data_models.go:193-198`. | Require an explicit expected head for replacements; storage substitution alone does not fix this loophole. |
| Stale cloud save returns 409 without retaining the submitted candidate. | Brain service `1252-1253,1330-1344`; handler `data_interop.go:1728-1744`. | Introduce a committed conflict-retained outcome that leaves the canonical head unchanged. |
| Cloud drafts share one mutable slot per normalized email; save deletes that slot without matching session/digest. | Brain `transformer_draft_service.go:15-23,95-150`; save service `1267-1274,1325-1329`. | Same-user sessions need independently retained candidates and exact draft acknowledgement. |
| Batch operation_id is correlation, not a durable receipt; batch items commit independently. | Brain `data_models.go:257-259`; service `1466-1469,1544-1562`. | Add per-command idempotency; keep truthful per-item partial success. |
| Presence/leases and content use different revision semantics. | Brain `transformer_governance.go:214-266,329-359,501-519`. | Presence cannot replace CAS or spuriously become an engineering revision. |
| Stronger receipt/CAS paths already exist for repair and version promotion. | Brain `transformer_repair.go:497-561`; `transformer_version_promotion.go:88-147,199-231`. | Extend the existing mutation owner rather than add another privileged save route. |
| Stable catalog version IDs and immutable revision IDs already exist; display numbering is separate. | Brain `transformer_version_catalog.go:23-64,543-599`; [version authority](../../ds-brain/docs/contracts/design-version-authority.md). | Reuse this catalog for intentional versions; do not create catalog entries merely to preserve conflicts. |
| OPFS is already used for Arrow upload-chunk spill, not for SQLite databases. | Web `design/upload-blobs.ts:1-13,33-43,86-94`. | Keep its byte lifecycle distinct from the database VFS. |
| Some native room/catalog stores are JSON+file locks, while dataset/snapshot stores use separate SQLite. | Kernel `ds-layer-store/src/scoped.rs:138-201`, `local_models.rs:327-360`, `project_dataset_cache.rs:131-244`. | Consolidate transaction semantics incrementally; do not rename filesystem locks as portable SQLite proof. |

Further relevant observations:

- Kernel Design, Layer Store and Client Core are detached native crates. Their
  filesystem, rusqlite, credential and Network/Rayon dependency closures cannot
  simply be included in the browser build. Extract the portable policy/SQL
  boundary without bringing those native effects into root WASM.
- `ds-layer-store/src/local_models.rs:327-360` replaces package bytes before
  catalog metadata; its documented crash window can mismatch bytes and digest.
  `project_snapshot.rs:83-172,331-365,504-557` provides a stronger immutable
  chunk/manifest and verified orphan-recovery foundation.
- Dataset open performs schema DDL, including under shared read locks
  (`project_dataset_cache.rs:131-244,716-762`). Migration must be an exclusive,
  versioned operation; read-only operations cannot create or alter schema.
- Dataset tickets and local view CAS currently depend on external file locks
  around some pretransaction reads. Move their required reads/checks/writes
  inside the same transaction or owned writer protocol before porting to OPFS.
- Dataset aggregate freshness may imply fresh when no current source was
  available (`project_dataset_cache.rs:1200-1238`). Preserve the shared Unknown
  result rather than equating no observed stale coverage with proven freshness.
- The inspected transformer owners have bounded metadata/digest reads but no
  durable transformer delta cursor/tombstone feed. Existing push is an
  invalidation hint, not ordered delivery or an acknowledgement.

## 3. One owner, explicit host effects

Retain the [application kernel](contracts/ds-application-kernel.md),
[transport authority](contracts/ds-transport-authority.md),
[sync engine](contracts/ds-sync-engine.md) and
[transformer acquisition](contracts/transformer-acquisition.md) ownership.

| Shared Rust owner | Native host | Browser host |
| --- | --- | --- |
| Row shapes, migrations, queries, command validation, state transitions, expected-revision/ACK checks, conflict receipts, freshness and paging. | SQLite connection/transaction execution, private filesystem, protected credentials, native scheduling and IPC. | SQLite-WASM worker, admitted OPFS VFS, browser capabilities and message transport. |
| Existing acquisition, retry, grants and publication policy. | Execute the declared transport effects. | Execute the same admitted effects; render returned facts. |

Extract a small SQL/transaction executor boundary from the existing Store and
Design owners. Share SQL definitions, schema and transition code; do not create
an ORM framework or duplicate decision code in JavaScript. The actual database
read set, admission checks and write set execute under one transaction/owned
writer boundary. A plan produced from unlocked stale rows is not sufficient.
Native and browser SQLite bindings may differ; that difference is an IO adapter.

"Unified" means one admitted semantic/schema family and one transaction program,
not one physical database shared over the network or an immediate mega-file for
every engine. Inventory existing databases, assign each logical record one
owner, and use one database wherever a local commit must be atomic across
authored revision, receipt and outgoing intent. A transaction across unrelated
SQLite files is not assumed. Preserve existing derived-payload ownership.

## 4. Identity, local records and projections

Separate these concepts in typed requests and receipts:

1. Canonical object identity: existing project/kind/transformer identity. Never
   derive it from a conflict label or silently rename legacy references.
2. Local authored revision: immutable snapshot/write-set identity plus local
   monotonic generation; includes the exact base used for the edit.
3. Remote engineering head: authoritative generation and content revision/digest.
   Today metadata.version/base_version denote this counter, not governance vN.
4. Operation identity: principal-fenced stable command ID and intent digest.
5. Explicit governance version: existing catalog identity/display vN and its
   pinned revision. Creating one remains a deliberate separate command.
6. Presence lease: holder/session/fencing generation/expiry; coordination only.

Every persistent scope includes principal, deployment lane/origin, credential
audience/binding, project and object kind/identity; install/session identity
applies to local queues and writers. Do not authorize from a path or display name.
Keep OS/browser credential protection outside general SQLite/OPFS content tables.

Required logical records, mapped into existing owning tables rather than a
second queue:

| Record | Minimum facts |
| --- | --- |
| Authored revision | Object/scope, immutable revision/digest, payload manifest, actual parent/base, provenance and creation operation. |
| Working head | Local revision/generation and writer incarnation, last acknowledged remote generation/revision/digest, pending submitted operation, authority capture. |
| Command/receipt | Exact actor/intent digest, submitted revision, expected base, accepted/unchanged/conflict-retained outcome and observed head. |
| Conflict candidate | Candidate ID, same canonical object ID, immutable local revision, exact base and observed remote head, unresolved/resolved disposition. |
| Existing outgoing intent | Immutable submitted revision, stable replay identity and dependencies, admitted transition/lease state. |
| Source observation | Complete source-basis stamp, completeness/cursor, freshness knowledge, invalidation generation and originating read intent. |

Dirty is a projection of working head/generation versus acknowledged submitted revision/generation;
it is not an independent boolean that any late callback may reset. Preserve a
monotonic local generation across reload and A-to-B-to-A account transitions.

Display a conflict as, for example, `Conflict - Rubaya - <short candidate id>`.
The prefix is view metadata only. It must not alter transformer_name, object ID,
network geometry, transformer nature, report membership or quantities. Normal
inventories select one canonical effective head. Conflict review lists the
candidate revisions separately, with source/session attribution; no duplicate
transformer point, BOQ row, tile source or load-flow participant is introduced.

## 5. Transaction and acknowledgement contract

### Local edit

Validate the explicit object, immutable expected local base and admitted typed
edit. Prepare bounded payload bytes/digests before taking the writer transaction.
Inside the transaction, read current local head, compare the expectation and
record the authored revision and local operation receipt together.
Only return durable success and emit the view invalidation after commit.
This local edit/autosave does not submit a canonical cloud save. Existing
explicitly enabled draft-sharing intent has separate authority and effect;
it cannot promote the draft to the engineering head.

If another local writer moved the head, preserve the admitted authored result
as a conflict candidate derived from its actual immutable base. Leave the head
unchanged. Exact replay returns that same candidate/receipt. Do not silently
rebase geometry or rerun the edit against a different base. Missing/invalid base
or unauthorized scope is a refusal, not a fabricated candidate or success.

### Explicit Save/submission

Save captures an exact immutable local revision, local generation and writer
incarnation. Its local transaction verifies those facts, records the submitted
command and seals its outgoing head-publication intent atomically. It does not
increment governance vN. Subsequent editing creates another local generation;
it cannot mutate the submitted bytes. Only this explicit Save, an admitted Share
of a candidate, or an explicitly cloud-applying resolution authorizes that
operation's remote delivery. Prompt draining never converts ordinary local
autosave into publication.

Do not hold SQLite transactions across HTTP, user review, calculation or large
file acquisition. For external content-addressed blobs, write/verify/durably
seal immutable bytes before committing references. A failed SQL commit leaves
an unreferenced blob for governed cleanup, not an acknowledged edit. Referenced,
pending, pinned and unresolved candidate bytes are never evicted as cache.

### Cloud admission and lost acknowledgements

One head-mutation admission contract returns `accepted`, `unchanged`,
`conflict_retained` or a typed refusal. Brain keeps authentication, capability,
membership, lifecycle, source integrity and atomic Firestore effects.

- All replacements require an exact expected content head. Creation uses an
  explicit expected-absent intent. Remove the missing-base compatibility bypass
  after admitted consumers/migration proof; do not preserve it indefinitely.
- Check an actor/intent-bound durable operation receipt first. An exact retry
  after a lost response returns the original committed outcome even if the head
  has since advanced. Reusing the ID with different bytes/actor/scope refuses.
- Matching base and changed content: retain the exact submitted revision and receipt, advance the
  canonical head/generation, and invalidate dependent outputs atomically.
- Matching base and equal canonical content: record an `unchanged` durable
  command acknowledgement without advancing the head/generation. Check the
  mandatory base before the digest no-op; matching bytes with a stale base are
  not evidence of this command's acceptance. Exact prior receipt replay still
  precedes a new CAS evaluation.
- Stale base: retain incoming immutable candidate + conflict metadata + receipt,
  leave canonical head and governance catalog unchanged. A committed
  conflict-retained outcome must not return an error from the Firestore
  transaction callback, which would roll back preservation. Its transport
  status and command receipt must be unambiguous to all consumers.
- Digest preflight is observation, never command acknowledgement. Align its
  stale/equal classifications with admission; neither matching bytes nor push
  proves this operation committed.
- Delete a cloud draft only if session/candidate/digest matches the submitted
  draft. A save from one session cannot clear a newer draft from another session
  of the same user.

Keep current bounded compressed-frame storage for the initial slice. If a
candidate exceeds Firestore limits, retain it locally and refuse remote
preservation truthfully until the existing admitted immutable-object upload
contract covers it. A later GCS payload is prepared content; the Firestore
head/receipt transaction is publication. Do not introduce an unbounded 128 MiB
gateway payload path or arbitrary object-store writes.

### Local ACK application

In one local transaction, match principal/scope, operation ID, submitted local
revision, local generation, writer incarnation, submitted digest and base expectation. Append immutable commit evidence
and settle only that operation. Clear dirty only when the current working head
still equals the submitted local revision and generation under that incarnation.
Content A-to-B-to-A can recreate the same digest, so digest equality alone must
never let an old ACK clean a later operation. If newer local edits exist, preserve
them and their intents; reconcile their actual causal base using the admitted
rule, or retain an explicit conflict. Never rewrite their captured expected base
just to force the next upload. A duplicate, late or unrelated ACK cannot clear
another operation, head or account's work.

Apply the same rule to cancellation, retirement and acquisition publication:
completion of old work cannot replace a new local head or revoke its pending
intent. Existing append-only receipts and lease/fence tests are the foundation.

## 6. Conflict review and deliberate resolution

Conflicts are ordinary recoverable revision states. Persist candidates locally
first. Unsaved local conflicts remain local; synchronize candidate references
only after an explicit Save/Share or other declared remote-submission intent,
when connectivity and admission permit. Show whether a candidate is local-only
or remotely retained.
Permission/lifecycle refusal must preserve local bytes without misrepresenting
them as an admitted shared design.

Bound revision/candidate payloads, counts and paged review at admission. Coalesce
updates within the same unsaved candidate lineage instead of cloning a new
transformer or cloud candidate on every drag. Unresolved, pinned and pending
authored bytes are not TTL cache entries; storage pressure returns an explicit
recoverable refusal. Retention/archival is deliberate and dependency-checked.

Expose CLI/MCP first through the existing Design controls: bounded list/show,
exact base/local/current comparison, and preview/apply resolution. Proposed
operation names are design intent, not claims of currently installed commands.
Keep JSON review complete in counts with explicit omitted details and paging.

Resolution choices:

- Keep canonical: resolve the conflict disposition, retain its authored evidence,
  and select the existing head. Discarding bytes is a separate explicit action.
- Accept candidate: preview impact, then apply against the exact reviewed current
  head. Preserve the displaced revision and the candidate's actual ancestry.
- Apply reviewed edits: produce an admitted successor from an actual common base
  and explicit selected edits. Never describe two-way differences as a proven
  three-way merge or resolve engineering geometry by timestamp.
- Leave unresolved: keep both revisions and their provenance available.

Apply reads conflict state and current head in the same writer transaction,
checks the review digest/fence and records resolution + successor/head + receipt
atomically, including the outgoing intent only when that resolution command
explicitly applies to shared state. Local-only resolution still needs an explicit
Save to publish. If either changed since preview, retain work and
require fresh review; do not silently overwrite. Replaying apply returns its
exact original result. One candidate may have multiple later review attempts,
but cannot be resolved twice by racing operators.

No resolution implicitly creates, renumbers, deletes or promotes governance vN.
Existing immutable catalog pins, PM links, submissions and accepted versions
continue to reference their original bytes. Playback remains read-only; an
explicit governance capture or catalog promotion uses its existing authority.

## 7. Browser SQLite and cross-host qualification

Use a pinned community SQLite-WASM build and declared OPFS VFS, served locally
with the application release. No runtime CDN dependency, invented database
engine or silent IndexedDB/memory fallback. Declare minimum SQLite SQL features,
extensions, schema compatibility and host capabilities in the release manifest.
Native and browser bindings need not be identical; query/transition fixtures do.

The official SQLite 3.54.0 WebAssembly and JavaScript release archive is
711.72 KiB, and the current canonical `sqlite3.wasm` artifact is 679,587 bytes.
Budget approximately 0.8-1.5 MiB of packaged application growth for the engine,
worker/VFS scripts and the first typed DS adapter, then measure the actual
compressed transfer, compile latency, instantiated heap, page cache and query
peaks in the supported browser and installed WebView. OPFS database bytes are
separate retained product data, not application bundle size. Pin the exact
SQLite release and build receipt; do not substitute a CDN at runtime.

Introducing SQLite-WASM creates a physical WASM artifact even when Kernel owns
the transaction policy. The browser has no fixed engine count. Keep the
permanent baseline small: Kernel and Interactions for ordinary UI, plus SQLite
when durable local storage opens. Network and any later heavy engines load only
for their admitted intent. Do not force unrelated engines into one binary to
reduce an artifact count. Ordinary storage and UI activity still causes zero
Network, Solar and Reporter loads.

The browser database has one admitted worker writer/broker per fenced database.
Tabs and windows submit commands to it instead of opening competing writers.
Use an explicit browser capability/lease ownership protocol with generation
fencing and recovery; a per-module singleton or Svelte event is insufficient.
Choose and qualify the VFS against that topology before rollout. SQLite's
SAH-pool VFS requires single-owner coordination; standard OPFS alternatives have
different concurrency/isolation requirements. Do not start both as fallback
paths. Native WAL/FULL durability is not proof that a browser VFS supports WAL
or the same crash/power-loss guarantees. Prove the selected journal and lifecycle
on each supported browser, including quota/error propagation and restart.

Requests return bounded typed projections, cursors and exact revision facts;
large complete data remains in the owning worker/store. Preserve map, table,
report preview and download validation surfaces. Browser JS adapters supply
file/worker/transport effects only, with no duplicated dirty/freshness/retry/
conflict policy. SQLite cannot turn a browser into an OS credential store or
guarantee continued computation after the browser is closed.

Official references, checked 2026-10-14:

- [SQLite WASM](https://sqlite.org/wasm/doc/trunk/index.md).
- [SQLite persistence/VFS/concurrency](https://sqlite.org/wasm/doc/trunk/persistence.md).
- [OPFS](https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system).
- [Firestore transactions](https://firebase.google.com/docs/firestore/manage-data/transactions).

## 8. Freshness, queue delivery and collaboration economics

Retain the existing shared Fresh/Stale/Unknown rule and complete source fences.
Prompt publication of explicitly submitted, locally committed operations is
independent of fetching inputs. Local transformer autosave is not a publication
trigger. Offline analysis/report iterations consume their captured revisions
without incidental gateway calls or credential/freshness probes.

Intentional navigation/current-data demand or consumed invalidation performs one
bounded authorized metadata/source-basis read for the needed scope. Equal complete
stamps reuse retained bytes; unknown remains unknown; changed stamps acquire only
required changed/missing units through the existing acquisition owner. Coalesce
concurrent consumers and compare captured local generation again at acquisition
commit. Push retains invalidation and never downloads unused data.

Add a bounded transformer change/tombstone journal only through the accepted
head transaction, with cursor/scope/completeness and reset-required semantics.
It supports later changed-record acquisition; it is not a new always-on poll or
whole-project snapshot fan-out. Receipt-carried heads avoid a redundant post-save
read. Source revision/digest comparison excludes presence-only updates so a lease
heartbeat cannot make every engineering input appear changed.

One connected publication pump uses the existing queue/lease/retry authority.
Independent native producers wake it through the declared IPC/event path.
Browser restart/reconnect invokes the same planner. Unsupported transport or
expired authority is explicit; retained authorized input remains readable under
the existing revocation policy. Do not manufacture offline permission from a
cached project string.

Measure calls, Firestore reads, transferred bytes, database writes, peak memory
and latency for cold, warm, concurrent, offline, unchanged-online and one-changed
object cases. Do not claim lower cost from SQLite adoption alone.

## 9. Migration, deletion and contract reconciliation

Inventory every authoritative IndexedDB/file owner before deleting storage:
main `ds_web_store` v26, `ds-grid-catalog` v5, sketches, Assets index, report
previews, messaging push state, capture intents, map/project state, Solar
documents/results and native room/catalog files. Classify authored data,
immutable outputs, disposable caches, credentials and browser-managed handles.
Browser FileSystem handles are structured-clone capabilities; they cannot be
converted into SQLite rows by JSON serialization. Define explicit rebinding or
platform capability retention separately from domain state.

For each admitted slice, use a bounded one-shot export/import with manifest,
schema, principal/source scope, counts, blob digests and preserved operation IDs.
Read unknown legacy ownership as unknown: quarantine/recover explicitly rather
than assigning another user's dirty data to the current account. Keep original
bytes until import commit and exact readback/restart proof succeed. Do not let
existing old-version cache eviction delete the only copy during migration.

Switch the actual consumer once, then delete that slice's IDB/file writer,
schema mirror, independent queue and TypeScript decision path. Keep no runtime
dual-write, read-through legacy fallback, permissive missing-base route or
permanent migration service. Temporary import tooling is retired after admitted
platform/data completion; unresolved recovery remains explicit. Browser-origin
changes require an explicit old-origin export; OPFS does not read another origin.

Reconcile contracts at implementation time, with corresponding consumer proof:

- `ds-sync-store.md` currently names IndexedDB for Web; replace that host table
  only after actual SQLite-WASM parity and rollout, preserving the shared rows.
- `transformer-acquisition.md` names an IndexedDB commit boundary; preserve its
  authority, dirty-room and late-response checks in the new transaction owner.
- Web [survey-native-offline.md](../../ds-web/docs/contracts/survey-native-offline.md)
  explicitly retired a previous separate native capture workspace. The new
  common store must not resurrect its old CLI operations or a second survey
  architecture. Survey migration is a later bounded slice with mirror semantics.
- `lv-transformer-versioning.md:122-127` still describes retired
  process_transformer forwarding. Source tests retire that action; do not use
  the obsolete writer in the new plan. Correct that stale contract separately.
- Existing catalog rename/display numbering and promotion source are richer
  than the installed CLI assessed earlier. Source presence does not close an
  installed-control feedback without matching release acceptance.
- Browser Solar/Reporter compute is a separate modularization decision. Storage
  unification alone does not remove their admitted cloud routes or change report
  permissions, printing placement or engine algorithm ownership.

## 10. Sequencing, code reduction and engine placement

The owner-approved sequence is conservative: complete the current extraction
boundaries without also rewriting their algorithms or storage engines. Correct
synchronization and acknowledgement semantics are the first horizontal priority.
After those contracts and their real consumers are stable, simplify one feature
or stabilization slice at a time. Do not run a broad Polars-to-DuckDB, IDB-to-
SQLite, TypeScript-to-Rust or GIS-library conversion campaign in parallel with
extraction.

The intended order is:

1. Finish the active Network, Reporter, Solar, Kernel and Interactions
   extractions with parity and actual-consumer proof.
2. Fix submitted-revision, expected-base, late-ACK, durable conflict and
   recoverable restart behavior, initially over the existing stores where that
   gives the smallest truthful vertical.
3. Move each admitted transactional owner to the shared SQLite schema and
   transaction program, switch its actual consumer and delete its old writer.
4. Stabilize the resulting contracts and measure source calls, writes, memory,
   build closure and installed behavior.
5. During later feature or stabilization work, replace one tabular, IPC or GIS
   implementation only where the replacement deletes more product machinery
   than it adds and preserves exact behavior.
6. Remove a dependency only after its final justified consumer and compatibility
   adapter are gone from that crate and release closure.

This sequence deliberately separates ownership movement from algorithm
replacement. A moved implementation is compared with its existing oracle. A
later engine substitution is compared with the stabilized moved implementation.
That keeps failures attributable and allows small slices to finish without a
repository-wide rewrite.

### Technology placement

Use the smallest established engine that matches the workload. A lower
application line count is useful when policy and tests remain explicit and the
removed code becomes a maintained dependency rather than a hidden second
implementation.

| Workload | Intended placement |
| --- | --- |
| Authored revisions, working heads, operation receipts, outgoing intents, conflict candidates, transactional local state and synchronization | SQLite through the shared Rust transaction owner, with native and browser host bindings. |
| Large joins, grouping, windowing, pivoting, portfolio aggregation, BOQ rollups and bounded report projections | Consider DuckDB later, behind the owning Network, Reporter or Solar application contract. |
| Arrow IPC schemas and cross-component columnar bytes | Direct Arrow schema/array/IPC libraries and explicit compatibility fixtures; do not require a DataFrame engine merely as an IPC codec. |
| Governed engineering decisions, revisions, refusals and deterministic numeric algorithms | Typed Rust in the existing domain owner. SQL does not become a policy or engineering-authority language. |
| Mutable map features, row edits and UI working state | The admitted SQLite/Interactions owner or a simple typed collection, selected by measured access patterns; an analytical engine is not the default editor. |
| Vector/raster format translation, CRS operations and broad GIS interoperability | Consider a captured GDAL boundary or DuckDB Spatial only in a later owning slice with native packaging, licensing, driver and exact-version proof. |
| TypeScript | Presentation, browser capabilities, worker/transport effects and typed IO only; no duplicate synchronization, freshness, conflict or engineering decisions. |

Rust can use GDAL through maintained bindings, and DuckDB Spatial bundles a
GDAL copy for supported geospatial formats. Either choice externalizes mature
format/CRS machinery but introduces a native binary and driver closure that must
be captured and qualified on Linux and Windows. It does not replace domain
geometry policy, UI hit testing, engineering calculations or release proof.
Do not start macOS packaging or add GDAL to browser startup as part of this
storage plan.

### Optional heavy compute placement and lifetime

The existing browser transformer path is the precedent: a deliberate LV
processing request loads Network and invokes its owning
`process_lv_transformers_chunked` WASM operation. It is local browser compute,
not a second Brain engineering implementation. The current worker's
`solar-snapshot` and `report-command` branches call Kernel projections; their
presence does not prove that the full Solar or Reporter compute engines already
run in the browser.

Keep full Solar and Reporter computation native in their standalone executables
for Desktop and Server by default. This suits their heavier memory, artifact and
packaging workloads and the expected affinity of power users for Desktop. A
cloud-native browser should not download either engine during startup, map use,
synchronization or ordinary result viewing. It can consume already retained or
published results without loading the producing engine.

A later browser Solar or Reporter WASM build is justified only if one compiled
Rust owner can serve browser and native consumers and the slice deletes a
meaningful duplicate calculation/orchestration implementation from ds-brain or
another host. Brain continues to own authentication, authorization, cloud
admission, durable receipts, project publication and artifact access. It does
not need to retain a second calculation merely because a browser is a possible
host. Conversely, adding WASM while keeping both compute paths indefinitely is
not simplification.

Every optional heavy engine uses an explicit lifecycle:

1. `absent`: no module request, compilation or engine heap;
2. `loading`: initiated by processing intent or a narrowly defined deliberate
   imminence signal, with exact engine/build identity;
3. `active`: bounded captured inputs, cancellation and progress;
4. `warm`: retain the initialized engine for nearby repeated work when measured
   memory permits;
5. `terminated`: release the worker and linear memory after an idle bound,
   memory pressure, logout/scope retirement, explicit disable or fatal trap.

Committed inputs, receipts and results live in SQLite/OPFS or the owning
immutable artifact store, never only in a warm engine heap. Recreating a worker
must reproduce the same result from captured inputs. The UI may expose a clear
Desktop-preferred route or an explicit browser-engine download for heavy work;
it must not silently fall back to an unversioned cloud calculator.

Before admitting a browser heavy engine, measure its compressed and installed
bytes, cold compilation, peak linear memory, transfer copies, warm reuse,
termination recovery and low-memory refusal. Prove module-request counts with a
positive detector: ordinary startup/open/edit/sync requests SQLite as designed
and requests no Network, Solar or Reporter engine; each deliberate processing
entry point requests only its named engine. Full application startup proof is
separate from an isolated adapter test.

This creates two valid product placements rather than an automatic browser
port: Desktop/native remains the primary heavy-processing environment; browser
WASM is admitted where it removes more Brain/host code and operational surface
than its bundle, worker and compatibility costs add. Transformer Network
processing already demonstrates the single local compiled-owner direction.

### Independently published browser engines

WASM engines may be released without republishing the whole Web application.
The app ships a stable typed loader and resolves an admitted engine through an
immutable release manifest. Browsers fetch the named worker, JavaScript glue,
WASM module and optional data/extension artifacts only when the engine's load
policy admits them. This is an established deployment pattern: DuckDB-WASM
selects versioned CDN or self-hosted module/worker bundles; ONNX Runtime Web
accepts independent absolute WASM asset paths; Pyodide loads a versioned runtime
and packages from a configurable index URL.

Each DS browser-engine manifest records at least:

- component, channel, semantic release and exact source/build closure;
- immutable URLs, byte sizes and SHA-256 digests for glue, worker, WASM and
  extensions/data files;
- loader/protocol/schema compatibility ranges and required browser features;
- thread/isolation requirements, memory/admission class and lifecycle policy;
- release authority, publication time, rollback predecessor and cache policy.

Use immutable digest/version URLs. A small authorized channel pointer may select
the release for new jobs, but never overwrite an artifact behind an existing
URL. A running job keeps its captured engine identity. An older installed Web
loader may adopt a newer engine without a Web deployment only when the manifest
falls inside its declared compatibility range and its glue/protocol pair passes
admission. An ABI, worker protocol, CSP, UI contract or required-host-capability
change requires a compatible Web/Desktop release. Mixed glue/WASM versions and
unknown manifest schemas refuse visibly and leave the prior engine usable.

Serve DS releases from the controlled artifact origin or same-origin facade,
with `application/wasm`, CORS, CSP and worker headers qualified for the chosen
mode. Threaded modules additionally require the declared cross-origin-isolation
headers. Browser caching is keyed by immutable identity; cache presence is not
release authorization. Offline Desktop packages or deliberately installs the
same verified artifacts. Do not depend on a public CDN, a mutable `latest` URL
or runtime compilation from source.

Independent publication changes the release cadence, not ownership. Kernel,
Network, Solar, Reporter and storage retain separate typed contracts. A new
engine version cannot add authority, bypass cloud admission or reinterpret old
SQLite records. Module-count telemetry remains useful for detecting accidental
loads and duplicates, but there is no fixed maximum. Acceptance proves the
declared baseline, zero incidental heavy-engine requests, one request per cold
intent, warm reuse, termination and digest-pinned rollback.

Deployment references, checked 2026-10-14:

- [WebAssembly streaming instantiation and MIME type](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/instantiateStreaming_static).
- [DuckDB-WASM CDN, Vite and self-hosted bundles](https://duckdb.org/docs/preview/clients/wasm/instantiation).
- [ONNX Runtime Web external WASM asset paths](https://onnxruntime.ai/docs/tutorials/web/deploy.html).
- [Pyodide configurable versioned runtime URL](https://pyodide.org/en/stable/usage/index.html).

### Polars and DuckDB policy

Do not declare Polars categorically defective, and do not preserve it by
default. Its value depends on whether the owning code performs a real columnar
pipeline. Freeze speculative new Polars use while the extraction and
synchronization work is active. Existing consumers remain until their own
bounded replacement proves simpler behavior and deletes the old path.

The 2026-10-14 canonical inventory found twelve production Rust files mentioning
Polars or polars-arrow, totalling about 9,070 physical file lines. That is not
9,070 replaceable lines:

- the 5,487-line Grid IPC module primarily uses polars-arrow arrays and IPC while
  retaining substantial DS schema, compatibility and validation behavior;
- the 698-line ds-geo LayerTable is a mutable feature table with row scans and
  rebuilds, not a substantial analytical query pipeline;
- the 677-line cleaning IPC module combines serialization with DS conversion and
  cleaning rules that remain regardless of engine;
- Solar's approximately 114 lines of direct integration primarily support an
  optional debug table/Parquet rendering.

Consequently, replacing present imports mechanically would add risk without
deleting most domain code. Direct Arrow is the likely later simplification for
IPC-only users. SQLite or a typed collection is the likely direction for mutable
feature editing. The cleaning path needs a measured slice. The small Solar debug
writer should not pull a second engine into an otherwise engine-free runtime.

DuckDB becomes a candidate when an owner contains repeated procedural code for
file scans, casts, joins, filters, grouping, ordered windows, pivots or
analytical exports. One named parameterized SQL projection plus a bounded typed
adapter can replace DataFrame expression plumbing and intermediate conversions.
Keep SQL versioned with its input/output schema, explicit null and cast behavior,
deterministic ordering, limits and refusal tests. Return typed projections or
Arrow batches through the owning application API; do not expose generic SQL or
argv execution to consumers.

A DuckDB experiment must be one real consumer with an existing oracle. Compare
exact rows, nulls, ordering, numeric values, historical inputs and refusals.
Measure product lines deleted, adapter/test lines added, binary and WASM size,
compile time, peak memory, latency and offline packaging. Retain DuckDB only when
the net boundary is simpler. Removing DataFrame calls while retaining
polars-arrow elsewhere is not removal of the Polars dependency closure.

DuckDB is an embedded analytical engine optimized for bulk operations. It is not
the synchronization database and does not replace SQLite's transaction role.
Its standard browser distribution brings another worker, WASM engine and
optional extensions. Do not add DuckDB-WASM to the permanent browser baseline.
A future browser analytical need may publish it as an independent intent-loaded
engine when the owning boundary, code deletion, bundle and load proof justify
it. Ordinary opening, editing, movement, selection and synchronization must not
load an analytical or engineering engine.

Any DuckDB or Spatial extensions used in a release are pinned, captured and
served from the installed release. Runtime extension downloads, CDNs and hidden
network installation are prohibited. Keep analytical database files distinct
from the authoritative SQLite store unless an explicit import/projection
contract proves otherwise.

Official references, checked 2026-10-14:

- [DuckDB workload and embedding](https://duckdb.org/why_duckdb).
- [DuckDB Rust client and Arrow interchange](https://duckdb.org/docs/current/clients/rust/overview).
- [DuckDB Parquet projection and filter pushdown](https://duckdb.org/docs/current/data/parquet/overview).
- [DuckDB-WASM deployment components](https://duckdb.org/docs/lts/clients/wasm/deploying_duckdb_wasm).
- [DuckDB Spatial and bundled GDAL boundary](https://duckdb.org/docs/lts/core_extensions/spatial/gdal).

### Code-size and modularity criterion

Do not use repository line count alone as an architectural acceptance gate.
DS contains governed engineering, offline, revision, packaging and platform
obligations that a general GIS platform may leave to plugins. Solar, reporting
and Network capabilities remain available; they become narrower stable owners
rather than being discarded to resemble another product.

Count both product code and dependency closure. Stable crates and captured
third-party engines should prevent hosts from reimplementing their behavior.
A successful simplification deletes the displaced consumer code, fallback,
schema mirror, build edge and tests that only protected the duplicate, while
retaining contract and parity tests at the new boundary. Moving Rust behavior
into TypeScript, SQL strings or generated adapters without deleting the former
owner is not a reduction.

Prefer opportunistic simplification during an owning feature or stabilization
session. Each slice must leave the application releasable, preserve historical
reads and installed-platform behavior, and avoid a temporary copy becoming a
permanent parallel implementation.

## 11. Ordered implementation slices and proof gates

Each slice is independently reviewable, implemented on canonical run and landed
after its focused proof. Re-read moving extraction owner paths before starting;
do not block its release cadence on this entire campaign. CLI/MCP precede UI.

| Slice | Scope and deletions | Required acceptance |
| --- | --- | --- |
| 1. Exact submitted command and ACK | Kernel typed submitted-revision/receipt/ACK decision; Brain mandatory expected base, actor+intent idempotency and matched draft cleanup; bind existing Web storage as a temporary executor of the same rule. Retire permissive base omission and component-owned clean decisions. | Edit during save remains dirty; same-user two sessions retain both drafts; lost ACK replay is exact; missing-base, changed-intent and foreign-actor refusals; no new vN. |
| 2. Native authored conflict retention | Extend existing Design transactional revision/head/operation owner with durable conflict candidates; implement bounded CLI/MCP review/resolution. Bind an explicit Save's immutable revision/receipt/intent to the existing queue database in one transaction; move the relevant slice-4 storage work here if needed. Migrate the old Design outbox, never bridge two independently durable queues with a later write. | Two writers on one base: one head, other retained candidate; restart/retry preserves both; reviewed stale resolve refuses without loss; resolve once; no canonical identity duplication; local autosave publishes nothing; crash cannot separate submission from queue intent. |
| 3. Cloud retained-candidate admission | In existing bulk authority, commit incoming revision/candidate and command receipt on stale base; align preflight and mutation outcomes. Route save/repair/promotion/lifecycle/migration replacements through common head-transition semantics with distinct permissions. | Real Firestore transaction tests for retained 409-equivalent outcome, exact replay and newer draft survival; mixed batch truth; retired/denied/lease refusals retain local work; explicit vN untouched. |
| 4. Portable SQLite transaction program | Extract shared schema/query/transition execution from existing native stores behind narrow bindings. Move schema creation out of reads and all CAS read sets inside write transactions. Preserve existing durable producer closure. | Native existing queue and transformer lifecycle pass; read-only absent store creates nothing; unsupported schema unchanged; rollback covers head/revision/receipt/intent together; late receipts cannot settle newer work. |
| 5. Browser SQLite-WASM/OPFS vertical | Add one pinned worker/VFS writer topology and migrate transformer rooms/headers/revision/conflict/intent for Web and Desktop's browser copy. Delete their IDB owner and TypeScript differential-save/clean decisions after proof. | Actual browser/native corpus equivalence; two tabs/windows, crash/restart, worker takeover and storage-full refusal; unchanged geometry/types/blank statuses; real save conflict and ACK paths; no memory/IDB fallback. |
| 6. Acquisition and changed-record reads | Adapt existing complete-room/source-observation owners and atomic acquisition publication; add bounded cursor/tombstones if required. Remove independent transformer fetchers and duplicated metadata decisions. | Eight concurrent consumers coalesce; warm/offline/unused invalidation has zero source reads; changed/deleted source reconciles; Unknown remains Unknown; an in-flight download cannot replace a newer edit. |
| 7. Other owners, one vertical at a time | Migrate dataset/snapshot/local-view catalogs, Grid storage, PM, survey intents, Assets and other relevant stores through the proven contract. Remove each replaced writer/fallback; retain existing algorithm, permission and mirror owners. | Per-owner transactional/import proof, account/lane/audience separation, read economics and real native/browser consumer acceptance; no revived retired workspace or duplicate queue. |
| 8. Installed release and retirement | Publish matching schema/engine/host manifests, qualify Windows/Linux/browser, complete controlled data import and remove temporary tooling. | Installed build/platform identities and migration receipts, cold/warm restart and conflict recovery; explicit minimum writer protocol; superseded clients cannot bypass revision admission; no orphaned authored data. |

Slices 2 and 4 may share one focused storage implementation if that reduces
duplication; dependent admission/receipt decisions remain one coordinated lane.
Independent read-only assessment/testing may run in parallel; shared source files
and schema changes do not. Queue delivery is prompt, and source acquisition stays
lazy, throughout the transition.

## 12. Test matrix and measured completion

Existing tests to extend (not run during this documentation assessment):

- Kernel `crates/ds-design-workspace/tests/lifecycle.rs`: immutable histories,
  exact retry and concurrent writers.
- Kernel `crates/ds-sync-store/tests/store.rs`: migrations, unsupported schema,
  leases, read-only absence, late ACK races and payload rollback.
- Kernel Layer Store dataset/snapshot tests: tickets, overlapping acquisition,
  changed authority, immutable captures, ABA masks, delta replay and orphan recovery.
- Web `src/tests/design-transformer-headers.test.ts`,
  `local-transformer-draft.test.ts`, `transformer-acquisition.test.ts`,
  `differential-save.test.ts`, `design-conflict-parity.test.ts` and
  `design-presence-parity.test.ts`. A mocked markClean is not real-store race proof.
- Brain `internal/bulk/services/transformer_service_test.go`,
  `transformer_draft_service_test.go`, `transformer_analysis_emulator_test.go`,
  `transformer_retirement_emulator_test.go`, and services' catalog/promotion/lock tests.
  A skipped Firestore emulator test is not acceptance.

New shared cases must exercise:

1. A explicitly Saves/submits L1; A edits L2 while offline/in flight; ACK L1 advances only confirmed
   facts and cannot clear L2 or its pending intent.
2. A and B edit base R0; A commits R1; B produces a durable conflict revision Rb;
   canonical R1 remains, Rb survives reload and is separately inspectable.
3. Same user, two tabs/sessions; neither overwrites the other's candidate or
   deletes its draft. A stale primary/writer lease cannot commit after takeover;
   A-to-B-to-A content and old ACK cannot clear the newer generation.
4. Preview Rb against R1; head changes to R2; apply preserves Rb and refuses that
   old review. A new deliberate resolution writes one ordinary successor revision.
5. Lost response and exact retry; altered actor/payload/operation collision;
   unchanged content with stale base; partial batch failure; denial/retirement.
6. Candidate prefix does not create a second transformer, alter equipment fields,
   inflate counts, include both networks in reports or change governance vN.
7. Transaction abort/storage full/worker death at every local write boundary;
   no successful submission receipt without durable revision+intent; no visible half-import.
8. Native/browser SQL and transition corpus parity with the selected SQLite/VFS;
   bound database messages and reuse binary payloads; record memory and latency.
9. Full scope isolation and A-to-B-to-A callbacks; absent/expired authority;
   read-only schema refusal; offline retained inputs with zero incidental IO.
10. Content blob survives migration/export/import, digest and byte readback agree,
    unresolved/pinned/pending data survives cleanup, and obsolete writers are gone.
11. Browser request instrumentation proves the SQLite bootstrap separately from
    optional engines: ordinary open/edit/sync causes zero Network/Solar/Reporter
    loads; named processing produces one request, warm reuse produces no second
    request, termination releases the worker and restart reproduces the result.

Completion is the real consumer and installed release passing these gates, not
a native helper test, a build, a schema-only port or the existence of this plan.
Preserve geometry, pole types, blank statuses, source attribution and engineering
quantities throughout. Existing review/printing/download surfaces remain usable.

## 13. First implementation handoff

Start with slice 1 and the actual submitted-revision/late-ACK race. It is a small
data-integrity vertical that is needed even if browser persistence remains IDB
temporarily. Use the existing actor-bound repair/promotion receipts and pure
conflict/presence policy as evidence; do not weaken cloud admission for parity.
Then retain the stale authored candidate in the existing native Design transaction
and bind one real CLI consumer before starting the OPFS replacement.

Unfinished implementation remains in [OPEN section 8](../backlog/OPEN.md#8-sync-publication-and-edge-outbox),
with existing acquisition obligations in section 16. Update owning contracts
alongside implementation and remove each fulfilled backlog obligation only after
its proof. Do not turn this plan into a session progress ledger.
