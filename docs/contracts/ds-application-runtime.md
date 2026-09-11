# The DS application runtime — campaign record

**Status:** campaign record, 2026-09-11. The governing direction is
[`ds-headless-application-runtime.md`](ds-headless-application-runtime.md)
(normative owner direction, §1–§10); this file does not restate it. It keeps
what that contract asks the campaign to keep explicit: which existing piece
owns what, the runtime slices, and the per-platform acceptance state
(implemented / landed / deployed / operationally proven).

## 1. What already exists, and what each piece is under the contract

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

## 2. Runtime slices (each lands by the six steps of the contract's §8)

| # | Slice | Durable state / decision | Owner after |
|---|---|---|---|
| R1 | One authoritative queue + artifact store per execution context, behind the Rust boundary | desktop/server: a kernel-defined schema in a native store (SQLite via `ds-design-workspace` or a sibling crate); web: IndexedDB rows the kernel shapes; mobile: checkpointed captures | kernel (schema, transitions) + host (bytes) |
| R2 | Desktop background host | `ds sync serve` (or the shell resident) owning delivery when the window closes, with a lease so one worker drains; installation/startup/shutdown/update integration through the launcher family | ds-cli host + packaging |
| R3 | Server headless host | supervised `ds sync serve` with an explicit identity (device credential), bounded concurrency, graceful shutdown, restart recovery | ds-cli host |
| R4 | Verified handoff of pending work | the legacy per-user outbox → the new store: identities, replay keys, bytes, acknowledgements, unresolved failures preserved; competing owners prevented | kernel (handoff plan) + host |
| R5 | Enforceable architectural checks | tests that reject frontend backend imports, route construction, direct backend requests, alternate domain mutations, including indirect wrappers | ds-web contract tests |
| R6 | Web host | the browser loop over the same plan with fetch transport and IndexedDB, offline-capable, opportunistic background (workers) | ds-web runtime adapter |
| R7 | Mobile host contract | the six host calls + checkpoint/resume against the portable kernel; scheduled tasks; secure storage | future app; not yet verified |
| R8 | Fencing and lifecycle | account/project/deployment fences; cancellation, logout, account switch, explicit background shutdown; leases; idempotent effects | kernel + hosts |

## 3. Platform status (implemented / landed / deployed / operationally proven) — kept current

| Platform | Decision | Transport | Durable store | Background execution | Status |
|---|---|---|---|---|---|
| Linux/Windows desktop | kernel (`sync::plan`) | native (`gateway_transfer.rs`) | legacy IndexedDB (to move, R1/R4) | shell process only (R2 open) | implemented, tested in-process; not operationally proven |
| CLI | kernel | native (`ds-cli-auth`, weak-network harness green) | none yet (R1) | `ds sync run` pending (S8) | plan headless: implemented + tested |
| Server | kernel | native | R1 | R3 | not yet verified |
| Web | kernel (WASM) | fetch host | IndexedDB (TypeScript-shaped) | opportunistic (R6) | not yet verified |
| Android/iOS | kernel (portable since the severance) | — | — | scheduled tasks (R7) | not yet verified |
| Cloud producers | — | publish via ds-brain with attribution | ds-brain record | n/a | landed server-side (admission), not deployed |
