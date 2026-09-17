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
workspace. Web image/thumbnail preparation can use local Rust/WASM during
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
reads and sealed storage-session transfers belong to `ds-web/crates/ds-sync-runtime`.
The pure kernel retains the plan and transfer state machine. Tauri supplies
shell integration, cancellation and progress projection. Different backend
publication protocols share the durable store and project lease; a Solar row
must never be submitted through a report work-grant protocol.

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
