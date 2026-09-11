# Headless application runtime for web, desktop and server

**Status:** normative owner direction, 2026-09-11. This contract defines the
destination and acceptance criteria; it does not claim that migration, release,
or platform validation is complete.

**Parent:** [The application kernel](ds-application-kernel.md). This document
refines that contract's runtime, transport, persistence and lifecycle boundary.
It is an implementation mandate, not a new wire protocol or command registry.

## 1. Mandate

Build a shared, headless Rust application runtime with **web, Linux, Windows
and server as the current first-class targets**. TypeScript/Svelte is a thin
presentation client. Optimize execution for each platform without duplicating
application logic.

**Scope narrowed by the owner, 2026-09-11:** Android and iOS are future
extensions, outside this campaign's implementation and acceptance scope. Do not
build mobile adapters, background schedulers, packaging or speculative
abstractions now. Extend the application for those targets when concrete work
is authorized. This supersedes the earlier all-platform requirement; it does
not direct removal of existing mobile application code.

On desktop, closing the application window must not stop synchronization when
background operation is enabled. Server operations must run without a browser,
webview, DOM or paired desktop. Web must remain fully functional within browser
capabilities, including prepared offline work, without a native companion.

The universal guarantee is **durable progress and safe resumption whenever
execution and connectivity are available**. Continuous background execution is
a platform-specific capability, not a guarantee that a browser will keep a
process alive.

## 2. One application authority

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
a frontend-maintained route catalogue. A necessary browser binding may execute
an opaque Rust-constructed request using browser APIs; it must not select a
backend route, construct its domain payload or interpret its response. This is
an ownership boundary, not a claim that client-side routes or bytes are secret.

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

## 4. Platform execution

| Target | Required execution model | Lifecycle obligation |
| --- | --- | --- |
| Web | Local Rust/WASM with browser-native persistence and workers where supported. Local decisions and prepared offline work require neither a server round trip nor a native companion. | Browser background execution is opportunistic. Preserve work across suspension and browser closure, then resume when execution becomes available. A worker alone does not guarantee continued execution after closure. |
| Linux | A native background host independent of the application window, integrated with installation, startup, shutdown and updates. | Closing the UI leaves enabled background work running. Explicit shutdown, logout, restart and update behavior must be defined and recoverable. |
| Windows | A native background host independent of the application window, integrated with installation, startup, shutdown and updates. | The same durability and background-operation guarantees as Linux, using appropriate Windows lifecycle integration. |
| Server | A supervised headless host with durable storage, explicit identity, bounded concurrency, graceful shutdown and restart recovery. | Work survives process replacement and restart when its storage is retained. Multiple workers have explicit ownership and safe idempotent effects. |

Share behavior, not assumptions about execution. A browser worker, desktop
background process and server worker have different
lifetimes; each executes the same durable state machine correctly.

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

Browser persistence remains appropriate for the web. Browser IndexedDB cannot
remain the sole owner of desktop work expected to continue after its webview
closes. Do not make a native service scrape or reverse-engineer browser storage.

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

Use bounded batches, incremental view-state projections, suitable concurrency
and efficient byte transfer. Avoid copying entire datasets or large artifacts
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
- Web performs prepared local work offline without a native companion and
  resumes retained work after browser restart.
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
Deferred Android/iOS work is not an acceptance gap for this campaign.
Distinguish **implemented, landed, deployed and operationally proven** states.
Desktop success does not certify web or server behavior. Do not declare
the transition complete while competing frontend authority remains.

## 10. Campaign instruction

Use this document as the standing steer for the Sync Center and headless
application migration. Start from the current activity and existing owners,
identify the next coherent slice, implement and validate it, then land it.
Preserve other lanes' work and keep the platform acceptance gaps explicit.
Do not create another architecture or protocol document to restate this mandate.
