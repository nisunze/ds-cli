# Headless application runtime for web, desktop and server

**Status:** normative owner direction, 2026-09-11. This contract defines the
destination and acceptance criteria; it does not claim that migration, release,
or platform validation is complete.

**Parent:** [The application kernel](ds-application-kernel.md). This document
refines that contract's runtime, transport, persistence and lifecycle boundary.
It is an implementation mandate, not a new wire protocol or command registry.

## 1. Mandate

Build a shared, headless Rust application runtime with **Linux server as the
primary execution target**. Web/PWA is a first-class online presentation client;
Linux and Windows desktop use the same Rust application authority through their
native hosts. TypeScript/Svelte renders state and forwards typed intentions.
Optimize each deployment without duplicating application logic.

**Product boundary, clarified by the owner on 2026-09-11:** this product has
no native Android or iOS deployment requirement, now or as a planned future
phase. Web/PWA remains first-class; Flutter remains the separate mobile
application. Keep Survey, ds-cli and MCP. Do not add native-mobile adapters,
background schedulers, packaging, acceptance gates or speculative abstractions
to this product. Preserve existing Flutter code. This supersedes the earlier
all-platform requirement and the subsequent wording that merely deferred it.

**Server-first clarification:** web/PWA does not require a separate offline
application replica or browser-owned synchronization engine. Its authoritative
work runs on the authenticated Rust server. This supersedes the earlier
prepared-offline-browser requirement. Preserve existing pending data through a
verified handoff; changing direction does not authorize dropping it.

On desktop, closing the application window must not stop enabled transformer
processing and publication. Server operations run without a browser, webview,
DOM or paired desktop. Closing a browser tab does not cancel accepted server
jobs. The guarantee is **durable progress and safe resumption in the owning
native host**, not continuous execution in a browser.

Keep Survey simple. Do not recreate the removed SQLite-based native Survey
workspace. The Server core's read-only survey copy
([survey-hold.md](survey-hold.md)) is not that workspace: plain files, no
edits, no outbox. Web image/thumbnail preparation can use local Rust/WASM during
capture; Flutter retains its existing field workflow and backend thumbnail
support. This direction does not migrate Flutter into this product.

## 2. One application authority

**Solar rule of the land (owner, 2026-09-16):** every Solar capability lives
in the native Server core. Input authoring and preparation, city and portfolio
calculation, result selection, dashboard and chart composition, document
authoring, delivery and publication must be usable through CLI/MCP with no
Svelte, browser WASM, Tauri or paired desktop. Svelte may render server
projections and forward typed intentions. Tauri may provide window and native
file-dialog integration; it must not own Solar behavior or execution. A new
Solar capability is incomplete until its headless server outcome is proven.
This supersedes the earlier Solar adaptation restriction on editing TypeScript.
Preserve retained inputs and pending work while removing competing owners.

- Every application command and decision passes through the Rust
  application/kernel boundary, regardless of which client initiates it.
- Rust owns application validation, backend route declarations and resolution,
  request construction, response interpretation, synchronization, retry
  decisions, scheduling policy, reconciliation and completion evidence.
- TypeScript/Svelte and other presentation clients render projected state and
  forward typed user intentions. They must not call backend routes directly,
  implement alternate domain rules, run independent synchronization loops or
  independently mutate authoritative application state.
- Pure kernel decisions remain separate from runtime effects and platform
  adapters. Necessary browser or native bindings execute explicit operations;
  they do not decide application behavior.
- Server-side authentication, authorization and admission remain authoritative.
  A locally validated command or Rust implementation never proves permission
  to access or publish shared data.

Presentation-only state such as focus, animation and component layout stays
with the presentation layer. Canonical selection, edits, workflow state and
delivery status belong to the Rust application.

Backend route knowledge belongs to the Rust client, not frontend call sites or
a frontend-maintained route catalogue. Server-hosted web clients call the
kernel host rather than domain backend routes. Necessary browser bindings
perform explicit Rust-directed platform effects; they do not choose backend
routes, build domain payloads or interpret domain responses. This is an
ownership boundary, not a claim that routes or bytes are secret.

The parent contract's statement that hosts perform effects does **not** require
TypeScript to own the API client, credential lifecycle, persistence
orchestration or synchronization service. Prefer Rust hosts and adapters; keep
JavaScript bindings only where browser/platform integration requires them.

## 3. Reuse the existing architecture

Build on the existing kernel, `ds-client-core`, durable queues, transfer
machinery, storage owners, CLI/MCP interfaces and packaging. Extend their
existing contracts when necessary. Do not introduce competing command
registries, parallel clients, duplicate queue engines or a second set of
application rules.

CLI help, capability discovery and MCP remain projections of their existing
declarations. Headless support must become available through those established
interfaces, without inventing another discovery system.

This contract defines responsibilities, not a requirement to put every
dependency or operating-system integration into the pure kernel crate.

## Delivery cadence and per-request project context

Prove the requested outcome through the installed CLI/MCP before refactoring.
If a supported command or authored configuration satisfies it, deliver through
that route. A demonstrated gap is repaired in its existing Rust owner, exposed
and proven headlessly through CLI/MCP, then bound by the UI. Remove replaced
TypeScript domain behavior only in the touched slice; no server operation in
that slice may require TypeScript or an open browser. Keep this migration
incremental. Run one bounded blind smaller-model workflow trial after the
implementation is ready, using live discovery without source access.

A native server or desktop background core has no mutable global active
project. Every operation captures an explicit project together with principal,
lane and deployment audience at entry, authorizes it, and retains that context
through execution, retries, publication and cancellation. A project identifier
alone grants no access. Durable jobs and caches remain stateful; request
routing must not depend on another client's selection.

Native terminal observation is an explicit, bounded read of an admitted job,
never a second scheduler. `ds-compute-runtime` registers process-local keyed
listeners before the host reads durable state and notifies them only after
completed/failed commits return. The host's cancellation path similarly
notifies after its durable cancelled commit, including queued cancellation.
Notifications carry no authority or result: the host rechecks the captured
credential and durable principal/lane/deployment/project visibility before
returning. Observers use a separate bounded budget, release ordinary request
capacity while asleep, and perform no timer-driven SQL or status polling.
Timeout, disconnect and shutdown detach observation without cancelling work;
restart reattachment reads the retained durable job without requiring a replay.

The web retains its active-project presentation and injects that context into
requests. CLI project operations carry their project explicitly and never
resolve it from saved selection or active UI state. Older compatibility
commands are migrated as their boundaries are touched. Project-scoped queues,
locks, idempotency identities, artifact lookups and
cancellation isolate concurrent projects. Restart recovery preserves the
original context. Migrate these boundaries as they are touched, proving that
interleaved requests, selection changes and interruptions cannot cross projects.
Do not claim all older commands already meet this contract.

## 4. Platform execution

| Target | Required execution model | Lifecycle obligation |
| --- | --- | --- |
| Web/PWA | Online presentation client of the authenticated Rust server. Local WASM serves explicit browser computation, such as image preparation, using the shared owners. | Accepted jobs belong to the server and continue after tab closure. Reconnection projects retained status; it does not start a competing browser queue. |
| Linux | A native background host independent of the application window, integrated with installation, startup, shutdown and updates. | Closing the UI leaves enabled background work running. Explicit shutdown, logout, restart and update behavior must be defined and recoverable. |
| Windows | A native background host independent of the application window, integrated with installation, startup, shutdown and updates. | The same durability and background-operation guarantees as Linux, using appropriate Windows lifecycle integration. |
| Linux server (primary) | A supervised headless host with durable storage, explicit identity, bounded concurrency, graceful shutdown and restart recovery. | Work survives process replacement and restart when its storage is retained. Multiple workers have explicit ownership and safe idempotent effects. |

Share domain behavior while keeping execution ownership explicit. Browser
workers perform bounded local effects; native desktop/server hosts execute the
durable state machine independently of presentation lifetimes.

CLI and MCP must reach equivalent application operations without frontend
participation. Where a desktop background host owns a local queue, headless
clients communicate with that owner instead of opening a competing executor.
Equivalent commands do not require different devices to share one physical
database; cross-device reconciliation still follows the governed contracts.

## 5. Durable state and safe handoff

Establish one authoritative persistent queue and artifact store for each
execution context. Use storage appropriate to the current web, desktop and
server requirements behind the shared Rust application boundary. Do not add a
store or storage abstraction solely for a hypothetical mobile port. The contract
requires durable behavior; it does not mandate one database technology on every
target.

Browser persistence may retain presentation caches and unfinished local inputs.
It is not the authoritative transformer processing/publication queue. IndexedDB
cannot remain the sole owner of native work expected to continue after a
webview closes. Do not make a service scrape browser storage; use a supported
handoff for existing pending work.

Migrate existing pending work through a supported, verified handoff:

1. Identify the current owner, queue entries, artifact dependencies and replay
   identities.
2. Persist the destination data and verify its completeness and integrity.
3. Transfer execution ownership without allowing competing writers or drains.
4. Retire source ownership only after the handoff is durably acknowledged and
   recoverable after interruption.

Preserve canonical identities, replay keys, original artifact bytes,
acknowledgements and unresolved failures. A migration must not silently drop
work, mint replacement identities to evade conflicts, or treat copied metadata
as proof that artifact bytes arrived.

Recover after UI closure, crashes, connectivity loss, suspension and machine
restart. Resume supported transfers without repeating completed effects or
falsely reporting success. Define what a local commit guarantees; the UI must
not report a durable capture before the owning store confirms that commit.

Durable native Network LV and prepared Solar jobs capture their addon at accepted
admission. After context, input, idempotency and capacity checks, the owning store
holds one write transaction while the native lane resolver captures the absolute
executable, SHA-256, owner identity and positive host worker share. The same row
retains that pin, exact input and full execution context atomically. Duplicate
and refused admissions start no addon. Retry and restart validate the retained
program and identity; they do not resolve the active lane pointer or executable
override again. Solar's existing owner worker limit also bounds its admitted
share. A reduced host or owner share refuses execution instead of changing the
captured width. Completed idempotent reads need no addon or freshness probe.

Historical or misbound addon pins are excluded from claims and capacity census
without deleting their rows or blocking valid jobs. Their status and input remain
readable and pending work remains cancellable. Recovery names missing-pin jobs
and directs the owner to export the stored input and submit a new key with the
explicit project and a compatible available addon; it never invents a pin.
Prepared tiles keep their existing execution contract.

Distinguish local retention, attempted transfer, server acknowledgement and
downstream visibility. Preserve the evidence required by each existing
operation. Every deferred or refused item needs a truthful reason and a
supported recovery action, including explicit absence of an automatic remedy.

## 6. Identity, concurrency and lifecycle

- Fence work by principal, project, deployment lane and credential audience.
  Preserve these fences through retries, restarts and migration.
- Define cancellation, logout, account switching and explicit background-host
  shutdown. Closing a window must not be confused with any of these actions.
- Prevent duplicate execution by multiple windows, application instances,
  background hosts or server workers. Use explicit ownership, leases where
  appropriate, and idempotent effects. Recover abandoned ownership safely.
- Keep credential acquisition, refresh, storage and revocation behind the
  application/platform boundary. Presentation clients do not become token or
  request-header managers.
- Server execution uses an explicitly supported identity model. Do not
  substitute ambient credentials or silently broaden authority.
  Reuse native `ds auth login` or approved device linking over SSH under the
  exact Linux user that runs the service. A running host must not silently
  change credential providers after revocation or account changes.
- Account device management must show linked devices, login/last-seen times
  and revocation. Google and password access link to the same canonical UID
  through verified account linking; matching email text alone is insufficient.
- Transfer tooling, including an rclone integration, remains behind Rust.
  Bucket addresses are destinations, not authorization. Preserve scoped
  grants, object identity, publication preconditions and DS receipts regardless
  of which tool transfers the bytes.
- Keep local UI/runtime communication typed, bounded and restricted to its
  intended caller. Do not expose arbitrary HTTP, filesystem or code-execution
  access through a bridge.
- Integrate background startup, shutdown and updates with existing packaging.
  Persist work before termination and handle incompatible state or versions
  explicitly; an upgrade must not strand or erase a queue silently.

## 7. Performance and resource use

Optimize startup, WASM size, memory, battery, bandwidth and storage access for
each platform. Share the application rules while choosing appropriate adapter
implementations and execution strategies.

Independent work runs concurrently by default, bounded by CPU, memory, storage
and network capacity. This includes nested engine work, not just the outer job
queue. Use bounded batches, incremental view-state projections and efficient
byte transfer. Avoid copying entire datasets or large artifacts
across the UI boundary. Avoid unnecessary polling and background wakeups.

Measure representative workloads on the relevant target. Record the workload,
resource constraints and observed result; a fast native build does not prove
acceptable web startup, desktop resource use or server memory consumption.

### Prompt queue sync and cheap input freshness

Owner clarification, 2026-10-13: collaborative mutation queues sync promptly
through their existing owner when connectivity and admission permit. Preserve
durable writes, acknowledgements, exact replay and pending local work. Queue
progress does not force repeated downloads of unchanged analysis inputs.

Retain exact version-pinned inputs and producing data across sessions. An online
consumer needing current data obtains a bounded, cheap, authorized source-basis
stamp and uses the shared kernel freshness comparison. The stamp includes the
complete principal, lane, audience, project, resource and dependency-revision
fences. Equal complete stamps reuse captured bytes; changed stamps acquire the
affected data through its existing owner. Concurrent consumers join that work.
Unknown or unavailable freshness remains explicitly unknown while captured data
stays available under its existing permission/revocation policy. Freshness
comparison itself performs no IO and does not grant authorization.

Local analysis, report iterations and presentation changes reuse their captured
versions. Acquire freshness only for an intentional online need or a consumed
invalidation; unused data incurs no probe. Preserve explicit Refresh and offline
operation, with no polling, eager rereads, competing cache or queue. Verify warm,
concurrent, offline and changed-source request/body counts when each owning slice
is touched. Transformer analysis and reporting illustrate this general contract;
they do not authorize an unrelated implementation audit or migration.

## 8. Implementation and landing method

Coordinate the existing campaign lanes around this destination and preserve
completed work. Follow the parent contract's authority-first migration method.
Land coherent vertical slices rather than accumulating a cross-stack rewrite.

For each slice:

1. Identify the application decision, durable state and current owner.
2. Move ownership into the appropriate Rust component.
3. Connect the relevant platform adapters and headless entrypoints.
4. Remove the competing TypeScript implementation and bypass paths.
5. Prove behavior, recovery and compatibility with the existing contracts.
6. Land the slice and report unverified platform work before expanding scope.

Preserve behavior where correct; document deliberate corrections with their
evidence. Do not retain a competing frontend authority merely to minimize a
diff. Do not claim a slice is fully portable while its required platform
adapter or durable store remains missing.

Add enforceable architectural checks that reject frontend backend imports,
route construction, direct backend requests and alternative domain mutations.
Check indirect wrappers as well as obvious calls. Keep necessary platform
bindings explicitly bounded, and prove forbidden bypasses fail the checks.

Build locally. Never launch remote CI or Cloud Build. Follow each repository's
required verification and deployment agreements; this contract does not grant
additional deployment or identity authority.

## 9. Acceptance evidence

The following are required outcomes, not claims of current completion:

- Desktop work continues after the UI closes when background operation is
  enabled; reopening shows the same authoritative state.
- Web submits typed intentions to the Rust server, closes, and reconnects to
  the same retained jobs and results without running a browser sync engine.
- Interrupted delivery resumes without losing work or duplicating committed
  effects, including interruptions during transfer and acknowledgement.
- Equivalent operations run on a headless server without frontend dependencies.
- Restart, concurrent-worker, account-isolation, cancellation, logout and
  blocked-work behavior are verified, including negative controls.
- Existing pending queues and artifacts survive migration with their
  identities, bytes and evidence intact.
- CLI/MCP and presentation clients reach the same application authority.
- No migrated frontend workflow can bypass the Rust application boundary.
- Resource measurements demonstrate suitability for each supported target.

Record current-target results separately: implemented, tested or not yet verified.
Native Android/iOS ports are outside this product and are not acceptance gaps.
Distinguish **implemented, landed, deployed and operationally proven** states.
Desktop success does not certify web or server behavior. Do not declare
the transition complete while competing frontend authority remains.

## 10. Campaign instruction

Use this document as the standing steer for the Sync Center and headless
application migration. Start from the current activity and existing owners,
identify the next coherent slice, implement and validate it, then land it.
Preserve other lanes' work and keep the platform acceptance gaps explicit.
Do not create another architecture or protocol document to restate this mandate.

## Native extraction order and installed-state isolation

Complete the native dependencies needed for Linux Server before the broad
frontend migration. Move existing implementations into shared native owners;
Desktop and Server must call those owners. Creating a Server copy while keeping
an independent Tauri implementation is not an extraction.

Report publication storage belongs to `crates/ds-report-artifacts` in this
repository. Native Sync Center orchestration, report inventory, verified cache
reads and sealed storage-session transfers belong to this repository's
`crates/ds-sync-runtime`. The pure kernel retains the plan, record-answer verdict
and transfer state machine. Tauri supplies shell integration, cancellation and
progress projection. Computation artifacts have the same synchronization rules
regardless of engine. Prefer downloading and verifying a valid shared artifact,
including another user's result, over recomputing it. Retain governed input data,
authored parameters and provenance so recomputation remains available when needed.
Artifact reclaim never deletes those inputs or reduces the obligation to sync
computed artifacts.

The current native executor consumes `sync::publication_verdict`; the legacy
Server Solar publisher and Desktop TypeScript queue still await migration to
that executor. Solar's producer and completion observer now live in the shared
`ds-sync-runtime::solar_producer`; the Server delegates to them. This is not
proof of Solar publication parity. The destination is one
`compute_artifacts` open/transfer/finalize sequence, not the retired report
work-grant protocol. [The sync engine](ds-sync-engine.md#6-where-each-publication-rule-lives)
records the rule inventory and the publication kinds missing from the common
Server path.

A source development launcher must use a separate native identity/context
store, installation identity, compute and sync database, artifact/cache root
and listening port from the installed Server, including installed Canary.
Sharing public deployment profiles is allowed. Copying production credentials
or opening production mutable state to make development start is not. Source
CLI commands must resolve the same development namespace as their development
Server. Production installation continues through the published download path.

Artifact-store tests move with their owner. Record actual shared-runtime,
Desktop adapter and Server consumer validation separately; a clean build is
not a live authenticated publication proof or remote GUI parity.

### Explicit retained PM reads

The existing `ds-layer-store` PM owner holds five fixed, bounded rooms per
protected identity/project binding: canonical Graph (8 MiB), last selected
note/version/claim (2 MiB), last selected task/optional timeline (8 MiB), last
exact note-query page (8 MiB) and last scoped note-context response (8 MiB), plus
bounded metadata. Another selection replaces its room. Each room has its own
capture generation and selector digest; it cannot displace a different room.

Bindings include protected UID, lane/audience, credential instance, explicit
project, body digest and shared local invalidation epoch. Receipts retain their
actual independent source revisions. A task's graph revision does not establish
its records-context revision; a note query/context retains its own exact context
revision and incomplete/migration-needed flags. No consumer synthesizes authority
or treats a captured version as proof of current remote freshness.

Normal reads currently acquire canonical source responses. Explicit `--offline`
uses only an exact retained selection, with zero session restoration, directory,
authorization or freshness acquisition. Capture-time permissions and unknown
remote freshness are disclosed. This source slice does not establish the cheap
online unchanged-body path required by the general freshness contract above.

Intentional overlapping captures serialize/coalesce through private locks.
PM write attempts, including correspondence mutations, mark existing holdings
potentially stale before effects; read/preview intents create no invalidation.
Known project access denial is durable before best-effort cleanup of all rooms.
The shared revocation floor prevents reacquiring one room from reviving denied
bytes left in another after failed cleanup. A capture cannot publish across an
invalidation or identity change. Named selected-note/task absence is distinct
from project denial. Existing kernel receipt validators run before persistence
and offline reuse, preserving partial totals, cursors and literal-only historical
notes. Model-reference reads capture graph plus one bounded records page under a single protected authority; the unused generic Context command is retired. Private-note bodies remain a distinct consumer.
No polling or timer-based freshness policy is introduced.

## Core cache corruption at startup

The canonical native Core queue/history database is a rebuildable cloud cache
when SQLite positively establishes structural corruption. After the existing
single-owner transport lock, connection owner and native installation identity
are verified, Core retains the damaged database and fixed SQLite sidecars with
digest evidence, then uses the ordinary store owner to create the same canonical
cache. It does not recover rows, choose another state directory, mint another
identity, reset credentials/components, or delete authored inputs/outputs.

Previously such structural damage refused `core.serve` until an operator acted.
The new startup behavior resumes with fresh queue/history metadata; existing
Desktop and CLI consumers keep their typed commands and require a Core artifact
containing this owner to receive it. Old metadata is retained diagnostically and
is not silently imported. Healthy constraint/migration/schema errors and
permission, authentication, principal, ownership or quarantine failures continue
to refuse. See [the exact store boundary](ds-sync-store.md#core-cloud-cache-structural-corruption).

## Deliberate native account cache transition

Ordinary Core startup continues to refuse another principal's connection and
cache. After an intentional account change, the Core-owned `core cache retire`
command requires a stopped owner, a protected current native credential and an
explicit preview digest plus canonical confirmation. The digest binds the
resolved canonical state path, lane, previous/current account fences and exact
connection/SQLite/sidecar bytes. It retains one old epoch with immutable receipts;
it never imports that account's rows or rewrites credentials, installation identity,
components or authored files. Existing protected native lock and filesystem
owners govern both Linux and Windows; unsupported platforms remain unavailable.

A pending retirement journal fences startup before connection/database creation.
Atomic evidence publication and native no-replace moves admit explicit same-plan
continuation after interruption; failures attempt exact old-epoch restoration.
This deliberate operation is separate from positive SQLite-corruption recovery.
Permission, lane, account, stale digest and active-owner failures remain refusals.
Source support is not installed platform acceptance. The live command and
[Core reference](../../../ds-cli/docs/reference/core.md#deliberate-account-cache-retirement)
own operator inputs, effects, refusal remedies and migration behavior.

## Native retained Profile sessions

The protected Core host admits `profile_session` through the existing execution
context owner. Each session is fenced to its captured native principal, lane,
deployment, install and explicit project; the package SHA, model and exact
history bind its retained managed Network process. Opening a package verifies
the existing exchange manifest/members/library pins and captures the existing
Profile seed; it never restores an embedded engineering engine to Desktop.
Opaque session handles are process-lifetime addresses, never authorization.
Closing, bounded idle retirement and Core shutdown reap the supervised worker.
An independently installed older Core without the closed command refuses with
`core_incompatible`; Desktop must request a qualified Core update. Unsupported
Profile operations remain explicit refusals. Package opening and native scene
proof do not alone establish editing, publication or print acceptance.

Core's retained Profile view composes only the existing managed Network
RuntimeScene and the existing `ds-profile-runtime` pure display projection.
`native_projection::prepare` retains DisplayState patch/style/options/label
refusals; `native_projection::project` accepts only a scene whose revision
exactly matches the retained native history. The helper opens no GridSession,
performs no engineering calculation, and creates no second live Runtime.
Desktop renders that owner's complete scene/display result through a closed
adapter; browser Grid availability cannot qualify a native shell release.
