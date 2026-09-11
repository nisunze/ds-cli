# The DS application runtime — web, desktop and server

**Status:** apex ruling for the sync gate campaign and everything after it. Owner
directive, 2026-09-11, recorded verbatim in §1. It sits above
`ds-application-kernel.md` (Rust owns the product), `ds-transport-authority.md`
(one transport), `ds-sync-engine.md` (one decision) and ds-brain's
`edge-admission-and-publish.md` (authority is handed, once). Where they differ
from §1, §1 wins, subject to the later owner scope amendment in §0.

## 0. Current scope — later owner amendment, 2026-09-11

The owner has deferred Android and iOS to keep the current migration nimble.
**Web, Linux, Windows and server are the current first-class targets.** Android
and iOS are future extensions, not current implementation or acceptance gates.
Do not build mobile adapters, schedulers, packaging or speculative abstractions
now. Choose storage and runtime boundaries for the current targets; extend them
when a concrete mobile project is authorized. Existing mobile code is not
ordered removed.

This amendment supersedes the mobile obligations in the verbatim historical
directive below. R7 is deferred and must not block completion of the current
campaign. The detailed runtime requirements are maintained in
[the headless runtime contract](ds-headless-application-runtime.md).

## 1. The original directive (verbatim; scope amended by §0)

> Steer the campaign toward a shared, headless Rust application runtime, with web, Android, iOS, Linux, Windows, and server as first-class targets. TypeScript/Svelte becomes a thin presentation client. Optimize execution for each platform without duplicating application logic.
> Start from the existing kernel, ds-client-core, queues, transfer machinery, storage owners, CLI/MCP interfaces, and packaging. Extend existing contracts where necessary. Do not introduce competing command registries, parallel clients, or another synchronization implementation.
>
> **Mandatory ownership boundary**
> - Every application command and decision passes through the Rust application/kernel boundary.
> - Rust owns validation, backend routes, request construction, response interpretation, synchronization, retry decisions, reconciliation, and completion evidence. Server-side authorization remains authoritative.
> - TypeScript/Svelte displays projected state and forwards typed user intentions. It must never bypass Rust through backend calls, alternate business rules, or independent mutations of authoritative application state.
> - Keep pure kernel decisions separate from runtime effects and platform adapters. Necessary browser or native bindings execute explicit operations; they do not decide application behavior.
> - Keep backend route knowledge inside the Rust application client. Frontend code must not construct backend requests or interpret backend protocols.
>
> **Platform execution** — share application behavior while respecting different execution environments:
> - Web: Rust/WASM runs locally, using browser-native persistence and workers where supported. Web remains fully supported without a native companion. Local decisions and prepared offline work must not require server round trips. Browser background execution is opportunistic; closing the browser cannot be assumed to leave synchronization running.
> - Android/iOS: integrate with supported background scheduling, secure storage, and application lifecycle. Checkpoint work so suspension or termination preserves committed local captures. Resume when execution becomes available. Do not promise continuous background execution.
> - Linux/Windows: a native background host owns synchronization independently of the UI. Closing the application window must not stop delivery when background operation is enabled. Integrate startup, shutdown, installation, and updates with the existing application lifecycle.
> - Server: a supervised headless host runs the same application operations without a browser, webview, DOM, or paired desktop. Support durable storage, explicit identities, bounded concurrency, graceful shutdown, and restart recovery.
> - CLI/MCP: invoke the same application operations without requiring frontend participation.
> Do not impose a desktop daemon architecture on web or mobile. A browser worker, mobile scheduled task, desktop process, and server worker have different lifetimes; each must execute the same durable state machine correctly.
>
> **Durability and ownership**
> Establish one authoritative persistent queue and artifact store for each execution context.
> Browser persistence remains appropriate for the web. It cannot remain the sole owner of desktop work expected to continue after the webview closes. Use storage appropriate to each host behind the shared Rust application boundary.
> Migrate existing pending work through a supported, verified handoff. Preserve identities, replay keys, original artifact bytes, acknowledgements, and unresolved failures. Prevent competing queue owners and silent loss.
> Recover after UI closure, process crashes, connectivity loss, suspension, and machine restart. Resume supported transfers without restarting completed work or falsely reporting success.
> Fence work by account, project, and deployment. Define cancellation, logout, account switching, and explicit background shutdown. Multiple workers or application instances must not execute the same work unsafely; use explicit ownership, leases where appropriate, and idempotent effects.
> Server execution must use an explicitly supported identity model. Never substitute ambient credentials or silently broaden authority.
>
> **Performance**
> Optimize startup, WASM size, memory, battery, bandwidth, and storage access for each platform.
> Use bounded batches, incremental state projections, appropriate concurrency, and efficient byte transfer. Avoid copying whole datasets or large artifacts across the UI boundary. Avoid unnecessary polling and background wakeups.
> The universal guarantee is durable progress and safe resumption whenever execution and connectivity are available. Continuous background execution is a platform-specific capability.
>
> **Execution and integration**
> Coordinate the existing campaign lanes around this destination and preserve completed work.
> Land coherent vertical slices. For each slice: 1. Identify the application decision, durable state, and current owner. 2. Move ownership into the appropriate Rust component. 3. Connect the relevant platform adapters and headless entrypoints. 4. Remove the competing TypeScript implementation and bypass paths. 5. Prove behavior, recovery, and compatibility. 6. Land the slice before expanding scope.
> Keep UI communication typed and bounded. Do not expose arbitrary HTTP, filesystem, or code-execution access through a local bridge.
> Add enforceable architectural checks that reject frontend backend imports, route construction, direct backend requests, and alternative domain mutations. Check indirect wrappers as well as obvious calls. Keep necessary platform bindings narrowly defined.
>
> **Acceptance** — demonstrate, with platform-specific evidence:
> - Desktop work continues after the UI closes.
> - Web performs prepared local work offline without a native companion.
> - Mobile suspension and process termination preserve committed local work and permit safe resumption.
> - Interrupted delivery resumes without losing work or duplicating committed effects.
> - Reopened or reconnected UI reflects the runtime's authoritative state.
> - Equivalent operations run headlessly on a server without frontend dependencies.
> - Restart, concurrent-worker, account-isolation, cancellation, and blocked-work behavior are verified.
> - Every deferred or refused item has a truthful reason and an actionable recovery path.
> - No migrated frontend workflow can bypass the Rust application boundary.
> Desktop success does not certify mobile or web. Mark each platform as implemented, tested, or not yet verified.
> Build locally; never trigger remote CI or Cloud Build. Distinguish implemented, landed, deployed, and operationally proven states. Do not declare the transition complete while competing frontend authority remains.

## 2. What already exists, and what each piece is under this directive

| Piece | Today | Role under §1 |
|---|---|---|
| `ds-command-kernel` (pure, `cdylib`+`rlib`, severed from the engine 2026-09-11) | decisions: `sync::plan`, `compute_artifact_engine`, `transfer`, `response_policy`, `upload_queue`, `reconciliation`, the design/printing/settings modules | **the application/kernel boundary**: every decision; no effects |
| `ds-client-core` (`ds-web/crates/ds-client-core`) | gateway operation registry (route/method/action/authority), request construction, response decode, profile digest | **the Rust application client**: all backend route knowledge; the browser reaches it only through the generated manifest and the WASM |
| native transport hosts: `src-tauri/src/gateway_transfer.rs`, `ds-cli-auth/src/upload.rs`; browser host `runtime/commands/gateway-transfer.ts` over fetch | drive `transfer` | **platform adapters** for bytes — execute, never decide |
| `src-tauri/src/sync_loop.rs` (landed 2026-09-11) | run-on-trigger loop over `sync::plan` in the shell process | the **desktop UI-attached host**; the background host (§3 R2) shares its durable store and takes a lease |
| `ds-cli` (`ds …`, `ds mcp serve`) | native commands, `desktop.*` bridge commands, MCP shim | **CLI/MCP and the server/background host** (`ds sync run`, then a supervised `ds sync serve`) |
| `crates/ds-design-workspace` (kernel repo, detached native host crate, rusqlite) | offline rooms/versions store | candidate **desktop/server durable queue + artifact store** owner behind the Rust boundary |
| IndexedDB through `stores/db.ts` | the browser's persistence, TypeScript-shaped today | **web persistence**, to be driven through the WASM boundary (the kernel decides the row, the host writes bytes) |
| ds-brain `work/open` · `work/publish` · `compute_artifacts` (admission from the release registry, landed 2026-09-11) | grants and re-authorization | **server-side authorization, authoritative**; hands authority, decides nothing else |

## 3. Runtime slices (added to the sync gate campaign; each lands by the six steps of §1)

| # | Slice | Durable state / decision | Owner after |
|---|---|---|---|
| R1 | One authoritative queue + artifact store per execution context, behind the Rust boundary | desktop/server: a kernel-defined schema in a native store (SQLite via `ds-design-workspace` or a sibling crate); web: IndexedDB rows the kernel shapes | kernel (schema, transitions) + host (bytes) |
| R2 | Desktop background host | `ds sync serve` (or the shell resident) owning delivery when the window closes, with a lease so one worker drains; installation/startup/shutdown/update integration through the launcher family | ds-cli host + packaging |
| R3 | Server headless host | supervised `ds sync serve` with an explicit identity (device credential), bounded concurrency, graceful shutdown, restart recovery | ds-cli host |
| R4 | Verified handoff of pending work | the legacy per-user outbox → the new store: identities, replay keys, bytes, acknowledgements, unresolved failures preserved; competing owners prevented | kernel (handoff plan) + host |
| R5 | Enforceable architectural checks | tests that reject frontend backend imports, route construction, direct backend requests, alternate domain mutations, including indirect wrappers | ds-web contract tests |
| R6 | Web host | the browser loop over the same plan with fetch transport and IndexedDB, offline-capable, opportunistic background (workers) | ds-web runtime adapter |
| R7 | Deferred mobile extension | No implementation or acceptance work in the current campaign; define from concrete requirements when authorized | future scope |
| R8 | Fencing and lifecycle | account/project/deployment fences; cancellation, logout, account switch, explicit background shutdown; leases; idempotent effects | kernel + hosts |

## 4. Platform status (implemented / tested / not yet verified) — kept current

| Platform | Decision | Transport | Durable store | Background execution | Status |
|---|---|---|---|---|---|
| Linux/Windows desktop | kernel (`sync::plan`) | native (`gateway_transfer.rs`) | legacy IndexedDB (to move, R1/R4) | shell process only (R2 open) | implemented, tested in-process; not operationally proven |
| CLI | kernel | native (`ds-cli-auth`, weak-network harness green) | none yet (R1) | `ds sync run` pending (S8) | plan headless: implemented + tested |
| Server | kernel | native | R1 | R3 | not yet verified |
| Web | kernel (WASM) | fetch host | IndexedDB (TypeScript-shaped) | opportunistic (R6) | not yet verified |
| Android/iOS | deferred | — | — | — | outside current scope; does not block acceptance |
| Cloud producers | — | publish via ds-brain with attribution | ds-brain record | n/a | landed server-side (admission), not deployed |
