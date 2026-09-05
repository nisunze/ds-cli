# B — unfinished work and acceptance requirements

This is the single remaining-work register. It replaces A.md. Items below need
implementation, integration, deployment or verification as stated; a partially
implemented foundation is not a request to rebuild it. Completed checkpoints
and obsolete pause instructions are omitted. Git history and the owning
contracts retain implementation history.

This consolidation uses existing observations only; no new product-code audit
was performed. Uninvestigated risks remain hypotheses, not confirmed defects.
Project management and Architecture internals remain outside scope. Their
completed navigation changes do not authorize a domain redesign. Use the
source-backed DS GridDesign Dev CLI and local development launcher for future
implementation. This document alone does not launch work or deployment.

## Required direction for unfinished slices

- Expose product operations through typed, discoverable CLI commands backed by
  the command kernel and owning Rust engines. UI and CLI use the same named
  commands, versioned parameters, permissions, state transitions and receipts.
  A map supplies explicit identities or geometry; it is never hidden process
  input or project authority. Scope is project, with map state as presentation.
- Remove superseded TypeScript decisions, transformations, stores, scheduling
  and computational fallbacks once the Rust owner takes over. Keep only host
  adapters for rendering, events, files/storage, transport, workers and platform
  effects. An unavailable/incompatible Rust or WASM owner returns a typed
  refusal; it must not silently run the old implementation.
- Keep domain algorithms and report production separate from IO and command
  admission. Preserve permanent browser/cloud, desktop/edge and supported local
  WASM/native execution. Placement changes are explicit, policy-controlled and
  visible; they cannot silently change inputs, algorithms or credentials.
- Treat the offline switch as an isolation control in dev and production,
  governed by the existing app-level all-governance permission. It is not a
  license. Start with local data and reconcile in the background when online.
  Desktop licensing, browser access, project authorization and compute policy
  remain separate authorities. Language and Theme remain profile preferences.

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
| P2 / L | Prove useful parallelism | Status-page batches were reported serial/cloud-bound; Sustained desktop and quota-limited cloud utilization remain unverified; a prior local Solar sample reached roughly 15 core equivalents on 16 logical CPUs, which is not a controlled cross-host benchmark. Profile scheduling, IO waits, worker limits, cancellation and memory. Benchmark identical semantics on desktop and quota-limited Cloud Run containers; 100% CPU is not itself the acceptance criterion. | Rust scheduling/engine owners and shared batch commands |

## Remaining command and Rust migration closure

- Complete UI edit lifecycle, project/design settings transformations, source
  and selection semantics, persistence and remaining scheduling convergence
  behind the kernel. Source precedence and some batch/version commands already
  have Rust owners; verify their consumers before replacing anything.
- Keep domain parameters (identities, immutable revisions, source manifests,
  settings and operation) separate from execution context (host capabilities,
  placement, resource budget, progress, cancellation and policy/grant revision).
  Results must expose typed outcomes, input fingerprints, implementation
  identity, artifacts, revision changes and pending reconciliation state.
- An operation ID pins one immutable request. Changed inputs/settings require
  new work identity, not a retry. Do not expose arbitrary URLs, SQL, shell
  arguments, generic engine opcodes or caller-authored transport bodies as the
  product command interface.
- Record production TypeScript before/after counts, deletions, additions and
  net change against an exact baseline and file set. Identify removed behavior
  and callers; justify each remaining adapter. Count bindings, tests, docs,
  formatting and renames separately. Prove both UI and CLI reach the Rust owner
  and the old implementation is unreachable; no full removal inventory exists.

## Remaining reconciliation and storage closure

The local Design lifecycle and version archive are foundations; exact-content
transport and stack-wide convergence remain open. Follow
[the reconciliation contract](docs/reconciliation.md) rather than building a
second queue. Required consumers are Design, Solar, Survey, settings,
layers/styles, portfolios, reports and attachments.

- Complete authenticated exact-content admission and acknowledgement. The
  server version-create endpoint snapshots server head; it cannot acknowledge
  retained offline bytes. Do not replay that endpoint later against a changed
  head or label `pending_transport` as delivered.
- Migrate untouched legacy intents and domain outboxes; connect standalone
  native persistence with browser/paired storage semantics. The paired CLI's
  access to application storage does not prove standalone convergence.
- Settle the shared versioned envelope against actual consumers: operation,
  project/actor and object identity, server base revision, input fingerprint,
  dependencies, policy revision and typed immutable payload reference. Keep
  domain validation, authorization and conflict rules with their owners; a
  shared protocol is not a generic write endpoint or shared raw database.
- Commit local changes, immutable payload references and outbox intents
  atomically. Fence work to project, principal, credential audience and lane.
  Ambient ADC/service accounts, borrowed credentials or project labels do not
  establish remote authority. Local hashes cannot fabricate server revisions.
- Make delivery idempotent under duplicate, delayed and out-of-order attempts.
  Changed input under a repeated operation conflicts. Preserve dependency order
  and bounded per-item partial outcomes; do not claim exactly-once networking.
- Verify immutable artifacts by size/digest. Distinguish upload completion from
  governed publication. Acknowledgements must identify the exact operation,
  input revision and artifact set before clearing an intent. Missing report
  media stays explicit.
- Preserve conflicts and local work. Geometry/topology and engineering settings
  do not default to last-writer-wins. Domain owners specify automatic merges
  and human-resolution cases.
- Close durable tombstone, retry, worker lease/claim, interruption, cancellation,
  credential-expiry and authorization-refusal states. Queue inspection must
  remain local, without attempts to upload while intentionally offline.
- Gate compaction and garbage collection on retained acknowledgement/history
  and references. Closing a UI or a transient upload error cannot prune pending
  inputs or artifacts. Synchronization alone never evicts Survey/version data.
- Finish category-level storage inventory and explicit cleanup with pending
  intent/reference protection. Show project data and actual available disk
  where measurable; browser usage/quota is not physical free disk. Existing
  cleanup covers downloaded cloud version copies only.

## Remaining desktop governance and recovery closure

Inventory, signed desktop policy and some enforcement already exist. Finish
shared CLI/UI control and verify end-to-end enforcement rather than declaring
all licensing absent or complete.

- Complete governed installation metadata: stable device/installation identity,
  product/build, OS, organization/project association, entitlements, install and
  last-seen times, install path/site, and current/past authenticated users with
  dated associations. Distinguish reported geographic location from install
  path/site; retain provenance and freshness and leave unavailable data unknown.
- Define bounded feature/session/placement/failure observations, authorized
  access, retention and audit. Last seen is not live knowledge of offline
  devices, and observed user history grants no account or project rights.
- Complete multi-device selection and settings popup, backed by the same typed
  list/detail/change CLI commands. Support assignment/revocation, scoped user
  blocks, installation disabling, minimum versions/forced upgrades and
  product/operation/placement controls. Fence per-target revisions and retain
  partial-success/retry receipts, actor, reason, policy revision and target.
  Distinguish requested, delivered and enforced action.
- Preserve the initially-allowed installation policy. Verify seven-day signed
  offline validity and daily handshake when used online, explicit clock rollback,
  expiry and revocation behavior. Intentional offline mode makes no renewal or
  expiry contact; a new remote block is learned at the next online handshake.
  Offline mode/grants confer no remote API or project authority.
- Keep an installation block/expiry separate from that user's browser access
  and other devices; web account blocks require separate governed action.
  Web administrators need no desktop license to manage installations. Define
  user blocking on a shared device without blocking unrelated users.
- Prove device possession/transfer where required. Separate heartbeat observation
  from grant ownership; knowing an installation UUID must not rebind outstanding
  work authority. Preserve original grant identity across account switching.
- Finish recovery-only read/export/sync behavior under expiry, revocation and
  required upgrades. Block new governed work as policy requires while retaining
  drafts, queued evidence and an available update/recovery route. A policy
  change must never erase data or falsely acknowledge pending work.
- Gate final transformer deletion on authenticated Recovery Center acknowledgement
  of the exact recoverable revision and record, including offline-created
  transformers. An offline delete atomically retains revision, required inputs,
  artifacts and pending intent; it may hide the active item with visible pending
  recovery. Publish creation/history/recovery in dependency order. Transport
  success alone cannot authorize purge. Preserve evidence across restart,
  retries, upgrades, expiry and blocking, then apply retention policy.
- Deploy/verify the existing recovery activity capture for new deletion rows:
  previous activity and deletion have separate actor/time evidence. Historical
  rows without evidence remain “Not recorded”; do not infer prior action from
  updated timestamps or automatically download backups to invent it. See
  [the recovery command contract](docs/backup-ledger.md). The previous session
  did not verify backend deployment or backfill.

Remote control cannot provide instant revocation to a disconnected device, or
recover storage externally erased before any synchronization. Verify bounded
offline authorization and application-controlled deletion against those limits.

## Remaining offline startup, settings and delivery closure

- Prove cold restart with no network contact using prepared/bundled routing and
  assets. Central fetch/API guards and paused outboxes are insufficient: cover
  WebViews, tiles, workers, native providers, child processes, external brokers
  and already-open sockets. Include gateway, ds-brain and token refresh, with
  no blackout exceptions. Prevent repeated background attempts and notification
  floods; reserve useful hints for deliberate user actions.
- Decide integration or removal of the uncommitted Linux `ds-blackout`
  experiment. Its isolated socket-denial tests do not prove product isolation;
  Windows and other shipping hosts still need enforcement and leakage evidence.
- Complete explicit project/data preparation and first activation from a local
  pack. Pin minimum viable resources, manifests, digests, coverage and freshness;
  expose missing data without guessing. Use project network settings, not
  invented global network defaults. Navigation should immediately present
  prepared local data and fill gaps through bounded background reconciliation.
- Complete offline network-setting sheet edits with a durable local transaction
  and authenticated exact-content reconciliation. Cached reads are not offline
  publication; never reuse a server reporter receipt with mutated settings.
  Verify assembly dirty state and Fast/Standard output selection through the
  actual saved project configuration, without hardcoded export substitutes.
- Prove a transformer can be created/imported, edited, processed and reported
  using explicit local inputs with no map or desktop dependency for headless
  commands. Pin project config, libraries, templates, fonts, images and reference
  data required by the operation. Missing references or unsupported configured
  output formats must produce visible blockers, not apparent completion.
- Close single, batch, combined and compounded reporting, media retrieval and
  physical printing on supported hosts. Native PDF creation is not printer
  spooling. Preserve report export policy, multi-format/page selections, retained
  artifact bytes and queued reconciliation; partial delivery is not complete.
- Complete transparent caches with at least 2 GB vector-tile and 3 GB basemap
  budgets as requested: explicit units/budgets, bounded disk use, eviction,
  coverage/freshness and per-category clear operations. Separate evictable cache
  from retained project/Survey work. Test restart, partial downloads, low disk
  and offline misses without network attempts.

Google offline caching requires applicable provider permission. Verify the
actual agreement before adding download packs; do not advertise coverage from
ordinary cache headers. Use governed/owned or explicitly offline-permitted
sources, retain attribution and consult the
[Google Map Tiles policy](https://developers.google.com/maps/documentation/tile/policies).

## Remaining version retrieval, comparison and location consistency

Build on existing `design_versions` native/WASM capture, room digests, local
catalog, retrieval and Data Factory (`ds-geo`) comparisons. Local versions use
`local-<digest>` identities, not invented server `vN` labels. Extend and verify
these foundations; do not introduce parallel geometry mathematics.

- Complete cross-host storage and exact-content transport above. Preserve
  independent layer/metadata/config rooms and explicit missing settings,
  identity, CRS or source/result evidence. Comparisons/results may occupy their
  own room; expose before/after findings through UI and CLI and temporary layers.
  Named maps remain explicitly deferred.
- Retain stable transformer identity, immutable revision IDs, replacement/import
  lineage, location/CRS, declared design area, input fingerprints and processing
  revision. A name is a label; the viewport is not a declared design boundary.
- Detect relocation, same-name replacement, departure from the retained boundary
  and input/result-location mismatches. Use normalized coordinates, explicit
  units and versioned tolerance policy. Record before/after location, displacement,
  boundary relationship, affected revisions/results and the triggering rule.
  Missing identity/CRS/boundary evidence yields unknown, never a clean finding.
- Expose consistent Design status/comparison results headlessly. Retain detection
  and resolution history; acknowledgement/replacement reasons cannot erase
  original evidence or make old computation current.
- Retrieve exact authored geometry, settings, pinned inputs, provenance and
  result references offline when available. Report missing local dependencies.
  Retrieval cannot move current head. Restoration creates a new revision with
  recorded lineage and expected-head fencing; do not rewrite history or reuse
  results from a different input/location fingerprint.
- Prove a transformer designed inside a rectangle then replaced far away under
  the same name retains both versions and exposes discontinuity/stale results
  through offline retrieval and reconciliation. Also test small tolerated
  corrections, acknowledged relocation and insufficient spatial evidence.

## Remaining Solar IO and performance closure

- Keep `ds-solar-weather` at the IO boundary: reusable versioned weather input
  pinned by dataset identity, digest, spatial/temporal coverage and preparation
  version. Repeated city iterations reuse matching local input; acquisition or
  refresh is explicit. Missing/incompatible data must not trigger silent
  download, substitution or regeneration inside computation/report production.
- Close seeding-to-draft/result/artifact offline IO and background publication,
  including report/web images and portfolio IO. Verify manifests, media and
  report input completeness without changing computation/report accuracy scope.
- Preserve the existing kernel portfolio batch command and removed TypeScript
  coordinator. Remaining limitations include paired effect-host dependence
  rather than a standalone daemon, unavailable hosted active cancellation and
  hosted custom source/draft overrides, and dev publication admission blocked
  by a missing exact release manifest in the recorded run. Verify/fix each
  supported path explicitly, without TypeScript emulation or relaxed provenance.
- Benchmark sustained parallel city/portfolio/Design jobs on desktop and
  quota-limited Cloud Run with pinned inputs, per-stage timings and CPU/memory
  budgets. A prior 6.4-second seven-city local sample and near-15-core peak are
  observations, not proof of sustained utilization or cloud equivalence.
  GCP execution, memory pressure and concurrent-request behavior were unverified
  because billing was unavailable; no cloud capability is assumed here.
- Use a shared CPU/memory budget rather than nested pools claiming all cores.
  Verify actual progress, cancellation acknowledgement, interruption/restart,
  deterministic outcomes and resource limits. Rust compilation alone does not
  prove performance, and input/result bounds are not an OS memory cap.

## Remaining Tchad investigation

The original report of Rwandan assembly names in computed output remains
unreproduced. Inspected local `beinamar_01`, `_02` and `_04` histograms had Tchad
codes; the inspected cache selected `rule_tchd` and contained its referenced
assemblies. That does not prove cloud seed freshness or earlier consumed bytes.

- Identify the originally affected output and exact run configuration, catalog
  bytes, locks, authored values and computed recommendations. Do not present a
  sheet-save, preset change or reseed as a demonstrated computation fix.
- Preserve the existing Rust rule-family correction, selected-table refusal and
  authored/surveyed/installed component authority. See the owning
  `ds-network/docs/contracts/lv-poles-rules-family-contract.md`.
- Define suspension/earthing policy as explicit standard-level data; do not
  guess Tchad equivalents. Verify cloud deployment and legacy Standard routes.
  The earlier full release gate also reported exchange Clippy failures; those
  were not revalidated by the subsequent conversion test fix.

## Uninvestigated risks requiring bounded verification

The earlier resource limit prevented investigation of these hypotheses; this
document consolidation did not investigate them either:

- Reachable duplicate TypeScript settings, selection, edits or state decisions
  despite equivalent Rust paths; mismatched browser/native/cloud defaults,
  refusals, cancellation and result revisions.
- Domain outboxes with incompatible identity, dependency, retry, acknowledgement,
  conflict or cleanup assumptions; Solar/Survey reuse cannot be assumed.
- Incompletely pinned source manifests, forms, libraries, configuration and media.
- Main-thread work, full-snapshot copies, nested concurrency, cache lifetime or
  oversized intermediate results limiting speed and memory.
- Executable-hash fencing for native resumption being costly on debug binaries
  or too coarse for upgrade-compatible recovery.
- Signed media references, printing settings or unavailable local assets blocking
  real offline delivery despite passing basic PDF tests.
- Multi-device geometry edits, actor changes, grant expiry, tombstones and policy
  revocation leaving unresolved recovery/reconciliation cases.

## Outstanding release verification

- Rerun the Windows installer locally with the pushed Solar provenance fix;
  Linux and simulated Windows build-cache tests are not native Windows proof.
- Rerun the web dev release gate with the pushed exact-library export correction.
  Passing 34 planner and 146 library tests does not prove the entire release gate,
  deployment or a fresh native PLS-CADD reopen.
- Verify intended installations/backend deployments before relying on locally
  tested control/recovery changes. No cloud deployment or Windows blackout run
  was established by the preceding checkpoints.

## Acceptance for closing any item

1. Select one bounded slice and inspect the live dev CLI contract. Add the
   missing typed command before relying on a UI-only operation. UI and CLI must
   call the same domain implementation with the same parameters.
2. Capture reproducible fixtures or exact artifact/configuration digests. Prove
   relevant failures before and after the fix, retaining unrelated engineering,
   identity, authority and geometry constraints.
3. For offline claims, reboot offline and complete the operation under network
   observation, including media, tiles, workers, authentication, native children
   and cancellation. Request-guard unit tests alone do not establish blackout.
4. Reconnect and prove background sync, conflicts, interrupted work and recovery
   without losing Survey data, drafts, artifacts, versions or deletion evidence.
5. Publish exact production removal/addition/net counts, meaningful test results,
   platform coverage and remaining limitations. Distinguish implemented, tested,
   pushed, deployed and operator-validated; remove an item only at its declared
   acceptance boundary or restate its genuinely remaining part.

Start with dev CLI readiness and the customer/material, attribute-table and
profile failures. Seeding depends on server access. Keep isolation, caching,
fleet governance and reconciliation in separate coherent implementation slices.
Assisted drafting from roads/buildings/contours/imagery remains future context:
versioned evidence, reversible proposals and human validation must remain
possible, but autonomous routing inference and DSGrid physics accuracy are not
part of this backlog's current implementation scope.
