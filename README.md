# ds-command-kernel

Component-decoupling work and release acceptance use the
[Architecture queue](../ds-architecture/backlog/DECOUPLING.md). Other unfinished
work remains in the local backlogs; implementation contracts and tests stay here.

Shared deterministic command and state transitions used by native DS clients
and browser WASM. Svelte remains the presentation owner; hosts retain network,
credential storage, IndexedDB, OS, and rendering effects.

`project_context` owns routing for commands still attached to the UI runtime.
The CLI project takes precedence for map work; if absent, the UI supplies its
current project. A project difference requests a switch of the existing UI,
followed by identity and target verification. Background commands keep their
independent project and never enter this UI boundary. This is an incremental
step toward map-independent commands through the kernel, not a survey-loader
or application-wide migration.

The [offline Design workspace](docs/design-workspace.md) adds shared source
authority and version-fenced feature transitions. Its separate native
`ds-design-workspace` crate owns durable revisions, background jobs, report
inputs and pending publication. Network computation and rendering remain with
their existing owners. Browser source precedence has no TypeScript fallback.

The first deliberately small slice was Firebase login-response validation. It
allows `ds` and ds-web to bind UID, email, token subject, Firebase audience,
issuer, and expiry with one implementation. Password collection, HTTP calls,
refresh-token persistence, Google UI, and session side effects remain host
adapters.

The Design Activities slice (`design_activities`,
[contract](docs/contracts/design-activities.md)) compacts one project's raw
transformer status rows into a complete per-transformer activity ledger and
folds retained ledgers across projects — users, projects, a complete timeline,
the changes since each project's previous capture, anomalies and facts — as
the one authority behind `ds design activities read`, MCP and the Governance
page. Hosts capture, retain and sweep; the kernel decides nothing a row does
not carry. `design_activities_store` owns where a capture lives — the
snapshot envelope and its digest, the hashed store key, retention, the pair
the fold diffs, and the sequential sweep a host should run next; it schedules
nothing.

The second slice admits and projects sealed Solar portfolio results by calling
the canonical `ds-solar-contracts` read operation. Native `ds` and browser WASM
therefore share schema, digest, totals, membership, and representative-graph
validation while retaining their own file/session transport.

The Project Control slice owns transformer-room scope and freshness decisions,
dirty-room preservation, memory-bounded concurrent download schedules, report
reuse/refresh decisions, and the local combined/compounded report projection.
The browser and declarative `ds` commands enter that same semantic desktop
operation. Hosts execute the plan: credentials, HTTP, IndexedDB, filesystem
writes, browser downloads, progress rendering, and cancellation remain outside
the kernel.

The Solar network-seeding slice joins two deliberately separate fact
authorities: DS Grid contributes revision-pinned MV alignment lengths and
granular city/phase tags, while local reporter rooms contribute transformers,
customers, and LV lengths. Explicit city bindings resolve different source
spellings; no fuzzy name join is permitted. The LV tag projection remains its
exact exported text and is verified against its supplied SHA-256 before use.
The result is a deterministic,
digest-pinned plan of complete Solar city documents. MV structure counts are
outside this version and are rejected rather than silently consumed. Native
`ds-report`/`ds solar` and browser WASM call this one operation; hosts retain
file selection, IndexedDB, downloads, and background cloud publication.

Expansion is evidence-gated. A new slice belongs here only when it immediately
serves a native `ds` operation and removes duplicated TypeScript domain logic.
Parallel mutable implementations are not allowed.

Concurrency is also evidence-gated. Project Control bounds independent network
requests by both a resident-room cap and a request-concurrency cap; the host may
run those admitted batches concurrently without holding the whole project in
memory. Tokio is not embedded in the browser WASM contract, and Rayon would add
overhead to a metadata projection bounded at 2,000 rows. Native async or CPU
parallelism belongs only behind this same plan when measurement shows a real
bottleneck. Every parallel executor must retain deterministic receipts and
native/browser parity.

```bash
./build.sh
```

The layer catalogue slice projects canonical backend IDs and their runtime
companions for the native CLI and browser bridge. It preserves server order,
reports truncation, and does not invent host visibility or backend assembly.

Style authoring now shares appearance, categorical second dimensions, line types,
casing and fill recipes across native and WASM. The native guided planner reads
backend editor vocabulary and bounds, preserves the complete authored document,
and resolves its save target. It performs no HTTP or rendering. Captured legacy
TypeScript fixtures verify 47 authoring cases against the same Rust operation.

Solar report image discovery and presentation projection use the shared
`ds-solar-contracts::report_media` parser through `solarReportImages` and
`projectSolarReportImages`. Native and WASM callers share Markdown semantics;
the browser has no secondary parser.

### Offline report commands

`report::evaluate` / WASM `reportCommand` owns the closed
`ds.report.command/v1` decisions used by installed UI and CLI report operations:

- `plan`: exact explicit scope, or complete prepared project inventory plus
  local dirty rooms; missing rooms remain named. Processor count and a resident
  job limit determine concurrency. A targeted cache never proves full inventory.
- `archive`: applied `report_archive` group coverage and deterministic safe
  ordinal folders. Missing, duplicate, foreign or archived membership refuses.
- `publication`: verified release output is `pending`, `queued`, or `composed`;
  the explicit development engine is `local_only`. Unknown engine identities
  refuse. Native Tauri consumes this same Rust policy when sealing its receipt.

Hosts own prepared-input storage, native execution, byte delivery and outbox IO.
Neither export nor scope planning requests cloud admission, saves dirty rooms,
refreshes project inputs, or replaces a failed WASM decision with TypeScript.
Publication failure cannot invalidate a successfully committed local artifact.

`reportCommand` also exposes `artifact_output` and `artifact_inventory` for
report bridge receipts and local artifact links. They reuse `report_formats`
for output IDs, wire formats, MIME types and ordering, and `printing` for
paper/layout identity. GeoJSON's output ID is `geojson`; its wire format is
`geojsonzip`. Native export, recovered publication and local inventory reads
must preserve that distinction. Inventory reads admit stored archives without
requiring publication order; duplicate identities and invalid declarations
still refuse. The host retains receipt scope checks and opaque-locator IO.

Report format policy lives in `report_formats`: `reportCommand` exposes
`formats` (exact outputs for pinned sheets and report shape) and `format_options`
(available presets and formats). PDF preset selection is additive and multiple
presets may be selected. Native and reporter consumers compile the same policy;
the UI has no format-selection fallback. `PrintPageSize` owns renderer dimensions.

Headless Canvas2D intents live in `src/canvas.rs` and dispatch to
`ds-network/crates/ds-canvas`. `canvas::evaluate` accepts a bounded, closed
JSON camera intent. The CLI registers this as `map.canvas.camera`, including
MCP discovery. Spatial algorithms, numeric buffers and live pointer sessions
remain in ds-network, exported through ds-network-wasm; the command kernel
does not own interactive hit testing or snapping.

## Client state on Rust signals

`client_state` (feature `client-state`, on in the WASM build) holds client
application state as signals and memos of Leptos's reactive core
(`reactive_graph`), with the same semantics in the browser's WASM and
natively. Svelte keeps only presentation state (focus, layout, animation).
ds-web reads each memo as a store through `src/lib/runtime/rust-state.ts`
(`rustState(initial, (state) => state.signal())`) and sends intents through
typed commands; it never sets a value. To move a store here:

1. Put its state in a domain module under `src/client_state/`. Inputs are
   `ArcRwSignal`s that only typed commands change; everything shown is an
   `ArcMemo`, which notifies only on change. A value read for the current
   scope is a `ScopedRead`: it reads only while wanted and when the scope is
   complete, never reads the same scope twice, waits for a read in flight
   and then reads only the latest scope, and counts an answer only for the
   scope it was read for. Expose what it derives as `OnDemand`.
2. Reach the host only through `Effects` (`client_state::browser` for the
   page): gateway calls are the domain's `gateway_exchange` operations, and
   durable reads go through `durable_store`. No memo calls a host.
3. Export each value from `browser.rs` as a `StateSignal` with
   `unchecked_return_type = "StateSignal<T>"` and a tsify type, and each
   command as a method. A subscription wants its read until disposed.
4. In ds-web, replace the store with `rustState` and its writers with the
   commands, and delete the TypeScript load, cache and in-flight logic in the
   same commit.
5. Test the graph with `scenario!` in `client_state_tests.rs`. It runs on
   futures' local pool natively (`cargo test --features client-state
   client_state`) and on wasm-bindgen-futures under wasm-bindgen-test
   (`wasm-pack test --node --features wasm`). Test the store in Vitest
   through the real vendored WASM.
