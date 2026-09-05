# B — remaining work and next slices

This is the working plan accompanying [A.md](A.md), not a completion claim.
It records the owner's requests and the evidence already collected; it is not
a new all-repository audit. Uninvestigated items remain uninvestigated. Project
management and Architecture internals stay outside scope. Moving their existing
Governance navigation entries does not authorize redesigning those domains.

## Direction

- Put product operations in typed, discoverable CLI commands backed by the
  command kernel and the relevant Rust engine. UI and CLI use the same inputs,
  decisions, permissions, durable states and receipts. A map is a projection,
  never a prerequisite for an operation that can run headlessly.
- TypeScript may render, collect events and adapt host IO. Remove superseded
  TypeScript domain implementations and fallbacks; report actual deletion
  counts separately from generated bindings, tests and formatting.
- The offline switch is an isolation/stress-testing control, available in dev
  and production with the existing governance. It is not itself a license.
  Start from local data, perform useful work without contact, then reconcile
  in the background when online. Do not turn routine navigation into a renewal
  or upload workflow.
- Desktop installation policy, web-account access, project authorization and
  compute placement are distinct. The existing signed desktop control work
  must not become a browser license requirement or an excuse for blocking
  offline testing. Conditional upgrades must preserve data and recovery.

## Saved checkpoint and evidence limits

Pushed to `run`: kernel `b6b668f`, backend `29bb025`, web `e33c2fcc`.
These contain desktop policy evaluation, installation inventory/history,
Governance controls, retained signature verification, desktop/web separation
and quieter intentional-offline notifications. They do **not** establish
complete blackout, complete CLI control parity or production deployment.

Checkpoint verification: 48 focused frontend tests, kernel workspace tests,
backend edge/handler/router tests, 22 native authority tests, Svelte checks and
native host compilation passed. Firestore verification used a local emulator.
No Windows blackout test or cloud deployment was performed.

The Linux `ds-web/crates/ds-blackout` experiment is local and uncommitted. Its
socket-denial tests passed, but it is not wired into application startup and
does not prove isolation of the running product. Existing sockets, external IPC
brokers, child processes, WebViews and other operating systems need evidence.

## Prioritized work

Sizes indicate scope, not time estimates: **S** = a bounded adapter/UI change;
**M** = a command/engine slice with integration tests; **L** = multiple hosts,
persistence or cross-repository integration. Split L items before starting.

| Priority / size | Remaining outcome | Evidence and next step | Command / authority owner |
| --- | --- | --- | --- |
| P0 / M | Reliable dev CLI inspection | The bundled dev CLI reports missing native client profile catalog for several commands; paired/local commands and native commands have different availability. Discover the exact live contract, repair the supported dev build where needed, and verify the active project without borrowing another lane's identity. | `ds-cli`, existing dev launcher and native client profile owners |
| P0 / M | Explain and fix the two unconnected customers | Owner reports higher-wattage customers excluded in `beinamar_02`, project `arjgpydw_aderm_loc7`. The current warning says only `max_service_length=37m`; that does not prove the reason. Capture exact cached inputs/config, customer IDs, category/wattage, candidate connections and actual rejection reasons. Do not raise the length limit or change demand to make the warning disappear. | `ds-network` connection engine; bounded CLI inspection and processing |
| P0 / M | Correct, understandable pole-material warnings | Warning names `9m 150daN`, `9m 300daN`, `9m 650daN`. The warning producer reads `lv_poles_struct_type`; the exact consumed project catalog has not been inspected for this run. Compare selected Tchad rules, normalized catalog keys, materials, authored values and config freshness. Repair proven data/contract mismatch; present actionable user wording without guessed materials. | Existing engineering-standards/catalog authority in `ds-network`; project config IO |
| P0 / M | Attribute-table viewport filtering and sorting | Both are reported broken. Viewport filtering intersects row-index sets in `AttributeTable.svelte`; the failure has not been reproduced. Check stable feature identity across sorting, filtering, paging, vector-tile fragments and map movement. Sorting must cover the dataset, not just rendered rows. | Existing Data Factory/vector/table Rust query owner; UI only projects query results |
| P0 / M | Valid MV profile scenes | Reported failure: `scene.conductor_spans[123].junction_hint` is not exactly one hint for the physical tap-off attachment set. The web validator was located; the failing engine scene has not been captured. Compare engine grouping with consumer grouping and preserve one engine-owned physical-junction identity. Do not disable validation or invent a second geometry algorithm in TypeScript. | `ds-network` profile/grid engine; existing CLI scene projections and web consumer |
| P0 / M, externally blocked | Seed the requested MV design | Target project: `arjgpydw_aderm_loc7`. Sources on `magese@ds-server`: `~/data-solutions/artifacts/ader-projet-7-gis-complete` and `ader-projet-7-gis-seed`. SSH returned `Permission denied (publickey,password)` before listing either directory. Exact model filename, digest and tag contents remain unknown. After access is available, inspect the actual model, preserve granular tags, validate it, then import/associate it through the declared project/MV command. No seed has been performed. | Existing DS Grid exchange/import and project model authority; `ds-cli` |
| P1 / L | Complete offline startup and no-contact operation | API/fetch guards and paused outboxes exist; they are not a complete network boundary. Prepare exact bundled assets, boot without Vite/network dependence, isolate every shipping host, and stop background request producers rather than merely silencing errors. Never exempt Google, gateway, token refresh or synchronization from blackout. | Native transport/process boundary, browser host, shared connectivity command |
| P1 / L | Explicit offline project/data preparation | List prepared projects immediately; support first activation from a completed local pack. Let users select projects and data categories, with a defined minimum viable pack, manifests/digests, freshness and missing-resource diagnostics. Persist project network settings and required resources; no invented global engineering defaults. | Project preparation commands, local stores and shared reconciliation |
| P1 / L | Visible, durable tile caches | Requested default budgets: at least 2 GB vector tiles and 3 GB basemaps. Define bytes/budget precisely, use bounded disk storage and eviction, show usage/coverage/freshness, and clear cache by category. Separate evictable cache from retained project/survey data. Verify restart, low disk, partial downloads and no offline cache-miss requests. | Native cache IO plus shared cache inspect/prepare/clear commands; UI adapter |
| P1 / M–L | Installation management from CLI and UI | Add multi-device selection and policy settings in a popup. Generalize typed list/detail/change commands; add per-target revision fences, partial-success receipts and durable retries where needed. Conditional minimum-version/upgrade controls need explicit requested/delivered/enforced states. Preserve a working update/recovery route when work is blocked. | Command kernel and `ds-cli`; authenticated `ds-brain` governance enforcement |
| P1 / L | Secure shared-device attribution and control | Current authenticated heartbeat can change the registry's latest principal when the caller knows its UUID; device-held transfer proof is not implemented. This can interfere with outstanding work's registry binding. Separate observation from grant ownership, prove device possession where required, preserve original work-grant identity, and test account switching and revocation without affecting unrelated browser accounts. | Backend installation/work authority and native identity verifier |
| P1 / L | Generalized durable reconciliation | Design's local lifecycle is a foundation, not stack-wide closure. Complete exact-content authenticated edge/cloud transport, legacy queue migration, leases/claims/retries/cancellation/auth-refusal states, local queue inspection and acknowledgement-gated compaction. Offline UI/CLI must never pretend to upload. | Kernel lifecycle, native/browser storage hosts, authenticated transport owners |
| P1 / L | Offline Design completion and reporting | Finish project-settings local edits, version retrieval/comparison, single/batch/combined/compounded reports and physical printing from available resources. Use existing Data Factory/vector mathematics; comparison layers are temporary, named maps remain deferred. Preserve transformer history, source location changes and pending recovery evidence. | Kernel/workspace, `ds-network`, reporter and printing IO owners |
| P2 / L | Prove useful parallelism | Status-page batches were reported serial/cloud-bound; Solar and other workloads do not yet have proof of effective processor use. Profile scheduling, IO waits, worker limits, cancellation and memory. Benchmark identical semantics on desktop and quota-limited Cloud Run containers; 100% CPU is not itself the acceptance criterion. | Rust scheduling/engine owners and shared batch commands |

## Basemap constraints

Google's standard Map Tiles API policy restricts unauthorized caching and
offline use; its HTTP cache directives are not permission to build an arbitrary
3 GB offline archive. Check the actual provider agreement before adding a
Google download pack. Support governed/owned or explicitly offline-permitted
basemap sources and preserve attribution. Do not advertise Google offline
coverage without applicable permission and working tests. See the
[Google Map Tiles policy](https://developers.google.com/maps/documentation/tile/policies).

## Small slice delivered with this plan

Governance navigation uses the existing application drawer in this order:
System, Project, Desktop installations, Users, Architecture. Architecture keeps
that drawer populated and places its own chapters in a horizontal bar above
the canvas. The duplicate Map shortcut is removed from the page; the normal
application navigation remains.
This is presentation work, not a new domain command or a CLI migration.

The Windows release's browser-WASM cache builder also omitted Solar provenance
when compiling the command kernel. It now verifies the pinned sibling Solar
checkout, includes its SHA in the cache identity, and passes that SHA plus the
active kernel lockfile to the build (converting the path for native Windows
Cargo). The missing-SHA panic was reproduced locally; the corrected contracts
release check and Linux/simulated-Windows cache regression pass. A native
Windows installer build still requires operator verification.

The web release's Grid exchange validation also exposed an obsolete expectation
for reconstructed STRUCT/PRT headers and a portable-projection rewrite of an
embedded PRT path. Production now verifies the actual emitted library member
against its retained digest before preserving those exact bytes through path
projection; it does not substitute an older asset for an edited export. The
regression checks exact retained library bytes, and all 34 conversion-planner
tests pass locally. This does not claim a new native PLS-CADD acceptance run.

## Acceptance for every implementation slice

1. Discover the dev CLI contract; add the missing typed command before relying
   on a UI-only action. UI and CLI must consume the same domain implementation.
2. Capture a reproducible fixture or exact artifact/config digests. For customer
   and profile defects, prove the reproduction fails before the fix and passes
   after it, retaining unrelated electrical and geometry constraints.
3. For offline claims, reboot offline and run through completion with network
   observation. Include images, tiles, workers, native children, authentication
   and cancellation. Local request-guard unit tests alone are insufficient.
4. Reconnect and prove background synchronization, conflict handling, restart
   recovery and no loss of Survey data, drafts, artifacts or deletion evidence.
5. Report implementation/deletion counts, meaningful tests, platform coverage,
   remaining limitations and exact commits. Keep implemented, locally tested,
   deployed and operator-validated as separate states.

Start with dev CLI readiness and the three reproducible product surfaces
(customers/materials, attribute table, profile scene). Seeding depends on server
access. Treat caching, isolation and fleet control as separate bounded slices;
do not reopen every part of A.md in one mixed checkpoint.
