# The kernel / engine boundary — Android and iOS are the litmus test

**Status:** owner ruling, 2026-09-10. **RESOLVED 2026-09-11** by the
ds-command-kernel commit that introduced `vendor/ds-geo-lite/` (lane
`fable/sever-network-edge-v2`). §3 is kept as the measurement of the violation,
now historical; §3.1 records what it measures after. `ds-portable-geometry.md`
is the implementation contract.

## 1. The rule

```
                 ds-command-kernel
                lightweight / portable
                         │
        ┌────────────────┼────────────────┐
        │                │                │
     ds-web            CLI           Android/iOS
      WASM            native        field/survey use
        │                │                │
        └────────────────┴────────────────┘
                         │
                  optional boundary
                         ▼
                     ds-network
               heavy engineering engine
                 WASM / native compute
```

**`ds-command-kernel` ──X──► `ds-network`.** Strict. The command kernel must
compile, run and remain useful with `ds-network` **completely absent**. Higher
level hosts may invoke `ds-network` separately when a command genuinely requires
heavy engineering compute.

Portable vector primitives have one maintained implementation in
`crates/ds-geo-lite` under the kernel repository. Both the application kernel
and Network's `ds-geo-ops` use that small crate. The permitted edge is
`ds-network → ds-geo-lite`, never `ds-network → ds-command-kernel`. The portable
crate has no application-kernel or Network dependency. Its types, six primitive
modules, and buffer/sample/crossing operations replace the Network copies;
the unchanged geometry/canvas snapshots keep their separate drift fence.

### The question to ask of every piece of logic

> Would we expect this capability to work on an Android or iOS field device
> without shipping the heavy network engine?

**Yes, and it decides application behavior** → `ds-command-kernel`, or a
lightweight neutral dependency beneath it. Transient UI interaction has its own
owner, defined below.
**No, because the operation is inherently heavyweight engineering computation**
→ `ds-network`.

### Ownership

| `ds-command-kernel` — the portable DS application | `ds-network` — optional heavy compute |
|---|---|
| canonical project/application state | advanced electrical/network analysis |
| commands and queries | optimization |
| admitted selection and authored editing semantics | large engineering graph/spatial workloads |
| survey workflows | terrain/profile computation |
| attributes/forms | FEM |
| filters | PLS-related engineering computation |
| lightweight geometry | other expensive algorithms that should not burden a field deployment |
| validation | |
| offline mutations | |
| synchronization semantics | |
| derived/application/view state | |
| domain behaviour that must exist independently of MapLibre or an open map | |

### Transient UI boundary (owner, 2026-10-14)

`ds-interactions` permanently owns transient map and canvas interaction:
pointer hit testing, screen-space snap candidates, selection gestures, viewport
transforms, Canvas2D layout and UI interaction indexes. These implementations
belong in neither CLI nor Server, and depend on neither the application kernel
nor Network engineering. Small shared geometry/value primitives can remain
neutral dependencies with neither dependency of their own. See the
[Interactions contract](../../../ds-interactions/docs/interaction-boundary.md).

Interaction produces transient intent; applying that intent to authored data
still passes through the existing Kernel or Network admission boundary. Network
topology snapping, engineering profiles and persisted model edits remain
engineering/application operations, not UI algorithms. CLI may request an
existing typed UI effect from an admitted UI host without compiling the UI's
interaction algorithm into its own dependency closure.

The accepted browser target is exactly three WASM units: permanently available
Kernel orchestration, permanently available neutral Interactions, and Network
engineering loaded when processing is requested or deliberately imminent. The
engineering unit uses the same owning Rust computation crates as the native
Network addon. Ordinary map movement, selection and snapping must cause zero
engineering-module loads. Existing Network/Grid package names and current
startup behavior do not establish that this consolidation is already complete.

Desktop and Server use the admitted native engineering executable. Tauri stays
outside the native application runtime. CLI/MCP adapts one shared command
contract; a future separate CLI executable or repository merge is a packaging
decision and does not authorize moving product decisions into an adapter.

Interactive DS Grid/Profile editing redesign and its engineering-aware session
migration remain separately parked in the
[owning hard-problem entry](../../backlog/hardproblems/OPEN.md#interactive-ds-grid-profiledesign-extraction).
Mechanical extraction preserves current behavior and does not reopen that work.

### Named refusals

Do not let convenience blur this. Specifically forbidden:

- duplicating command semantics in TypeScript;
- deriving application truth from MapLibre;
- **hiding a network dependency behind a feature flag** — an optional dependency
  that is on by default, or that the kernel's own tests require, has not been
  severed;
- forcing mobile/field deployments to carry `ds-network` because application
  logic leaked into it.

## 2. Why a feature flag is not a severance

Worth stating because it is the tempting shortcut. A Cargo feature that gates
`ds-network` still leaves the dependency in the lock file, the crates in the
build graph for anyone who does not know to disable it, two build
configurations to test, and a default that will drift back on. The boundary is a
**dependency edge that does not exist**, not one that is usually off.

## 3. The violation, measured (2026-09-10) — historical

`ds-command-kernel/Cargo.toml` lines 31–33:

```toml
ds-geo     = { path = "../ds-network/crates/ds-geo" }
ds-canvas  = { path = "../ds-network/crates/ds-canvas" }
ds-geo-ops = { path = "../ds-network/crates/ds-geo-ops" }
```

| Crate | Source lines | Its own dependencies |
|---|---|---|
| `ds-geo` | **17,838** | `ds-geo-ops`, serde, serde_json, `rstar`, `geo`, `regex`, `polars` *(optional)* |
| `ds-geo-ops` | **3,050** | `rstar` |
| `ds-canvas` | **1,350** | serde, `rstar` |
| **total** | **22,238** | |

**22,238 lines of the heavy engineering engine are linked into the lightweight
portable kernel — to obtain eleven symbols:**

| Symbol | Call sites |
|---|---|
| `ds_geo_ops::haversine_m` | 4 |
| `ds_geo::viewport::geojson_bounds` | 4 |
| `ds_geo::spatial_predicate::{selects_area, validate_area, intersects_area, covers_area, area_bounds}` | 6 |
| `ds_geo::comparison::{compare_geojson_layers, GeoJsonLayerComparisonInput}` | 4 |
| `ds_canvas::packed::{selection_buffer, camera_buffer}` | 2 |

Transitively this pulls `geo 0.33.1`, `geo-types`, `geographiclib-rs`, `regex`
and `rstar` into every kernel build, including the browser WASM package.

**Today's kernel WASM artefact: 7.9 MB.** That is the number an Android or iOS
field build would carry. (Measured again in the severance lane with one command
before and after: see §3.1 — the figure is dominated by the kernel itself, not
by the engine crates.)

One thing that is **not** a problem, recorded so nobody spends effort on it:
`polars` is *not* in the graph. `ds-geo` declares `default = []` and puts polars
behind `enable_dataframes`, which the kernel does not enable.

### 3.1 Measured after (2026-09-11)

Those three lines are gone. The kernel carries a digest-pinned snapshot of the
ds-network files the eleven symbols need, compiled as `crates/ds-geo-lite` and
`crates/ds-canvas-lite`; `ds-network` is unedited and there is no Cargo edge in
either direction. Full contract and method: `ds-portable-geometry.md`.

| | before | after |
|---|---|---|
| `Cargo.lock` packages | 189 | **154** |
| kernel WASM (`wasm-pack --release`) | 10,282,760 B | 10,403,695 B |
| same, after `wasm-opt -Oz --strip-debug` | 7,530,016 B | 7,646,634 B |
| `cargo check --features wasm`, ds-network absent | fails | **succeeds** |
| `cargo test`, ds-network absent | fails | **succeeds (529)** |
| `wasm-pack build`, ds-network absent | fails | **succeeds** |

The byte count is the honest surprise and is worth carrying forward: the
artefact never contained the 22,238 lines, because wasm-ld strips what nothing
reaches. The 7.9 MB a field device carried was *already* the portable subset.
What the severance removes is the **build graph** — 35 packages, and the
requirement that an engine checkout exist at all. `ds-portable-geometry.md` §6
has the section-level breakdown of the +1.6 % and why it is codegen-unit
repartitioning rather than new code.

## 4. What severing requires

The ruling explicitly permits "a lightweight dependency beneath it", so the
destination need not be *inside* the kernel.

The eleven symbols are all **lightweight geometry** — great-circle distance,
GeoJSON bounds, area predicates — plus two packed buffers that are derived view
state. Every one is squarely on the kernel side of the litmus test: a field
device must be able to ask whether a point is inside an area, or what a layer's
bounds are, without shipping an engineering engine.

So the shape is a lightweight geometry crate that **both** the kernel and
`ds-network` depend on. Note the arrow direction: `ds-network` → lightweight is
fine and expected. Only kernel → `ds-network` is forbidden.

`ds_geo::comparison::compare_geojson_layers` deserves a decision rather than an
assumption — layer comparison may be genuinely heavy. If it is, the kernel
should not be calling it at all and its two call sites need re-examining; if it
is not, it moves with the rest.

## 5. The acceptance test

`cargo build`, `cargo test` and the WASM build all succeed for
`ds-command-kernel` with `ds-network` **absent from the filesystem** — not
disabled, not feature-gated. Absent. Plus a recorded before/after WASM size, so
the field-deployability claim is a number rather than an assertion.

## 6. Ownership, stated as three layers (owner refinement, 2026-09-10)

> **Hosts perform effects; `ds-command-kernel` decides application meaning.**

| Host / Svelte | `ds-command-kernel` | `ds-network` |
|---|---|---|
| network transport | commands and queries | genuinely heavyweight engineering/network computation |
| IndexedDB / OPFS / durable storage | canonical application state | |
| credentials | selection | |
| filesystem / OS / browser APIs | field/survey semantics | |
| MapLibre rendering and pointer mechanics | editing semantics | |
| execution of external I/O | filters / attributes / forms | |
| | lightweight geometry | |
| | validation | |
| | deterministic transformations | |
| | change/dirty tracking | |
| | derived/view state | |
| | explicit effects/results for the host | |

Two failure modes, named:

- **Do not drag transport, permanent IO implementations, storage engines,
  renderer mechanics, or OS/browser concerns into the kernel.**
- **Do not leave application semantics in TypeScript merely because they
  currently sit next to IO or MapLibre code.** Split mechanics from meaning.

### 6.1 "Network transport" is a host concern — and the transfer protocol is not

These look contradictory and are not, so it is written down before someone
"corrects" one into the other.

The **socket, the TLS session, the fetch call, the credential** are transport
*mechanics* and belong to the host. **What an acknowledgement means** — whether
a 308 with no `Range` header proves zero committed bytes, whether an ambiguous
response permits a re-send, how long until the next attempt — is application
*meaning* and belongs to the kernel. That is exactly how
`ds-command-kernel::transfer` is built: it holds no HTTP client, opens no
socket, and the hosts report observed facts and execute one returned action.

The test: if two hosts could reasonably disagree about the answer, it is
meaning. If it is a platform API call, it is mechanics.

## 7. The dependency graph, rethought

Preferred shape when a shared contract is genuinely needed:

```
                 ds-domain / ds-types
                    ▲           ▲
                    │           │
          ds-command-kernel   ds-network
```

**`ds-network` ──X──► `ds-command-kernel`** must not be created for convenience.
`ds-command-kernel` is an application layer; `ds-network` should remain an
independently callable engineering engine that accepts explicit engineering
inputs and returns explicit deterministic results.

The shared lightweight dependency is `ds-geo-lite`. Its repository location
inside the kernel does not make it the application kernel: its dependency
closure excludes `ds-command-kernel`, `ds-network`, Solar and canvas. Network
may point only at this portable crate, and both hosts use the same primitive
code. The command kernel continues to build and test with Network absent.

Worth flagging, not acting on: **`ds-network-reporter` does depend on
`ds-command-kernel`.** That is the report worker, not `ds-network`, so it is
outside the letter of this rule — but it is a server-side engine depending on
the application layer, which is the shape the rule exists to prevent. Someone
should decide whether the spirit applies.

## 8. Crossing the boundary: coarse, not chatty

```
ds-command-kernel → complete engineering request → ds-network
                 → parallel heavy computation
                 → deterministic result → kernel incorporates it
```

Forbidden shape: `UI → ds-network → UI → ds-network → UI`. Normal selection,
editing, field inspection, survey, filtering, forms, lightweight geometry and
ordinary validation **must not continuously wake `ds-network`**.

## 9. CPU and power are part of the architecture

Adaptive, not maximal. Parallelism is spent where the work justifies it and
nowhere else.

| Ordinary UI / field / survey work | Explicit heavy engineering job |
|---|---|
| lightweight execution | use parallelism aggressively |
| bounded concurrency | use available CPU efficiently |
| low power | finish quickly |
| minimal background activity | return to idle |

Specifically forbidden on the lightweight path: unnecessary permanent worker
activity, busy polling, speculative heavy computation, waking multiple cores for
trivial interaction, unnecessary background work, and failing to return to idle
when work completes.

**Do not burn CPU or battery simply because parallelism is available.** This
matters because `ds-web` must remain viable as an offline-first Android/iOS
field application.

## 10. What the target is not

The target is **not** "Rust everywhere", and it is **not** "maximum TypeScript
removed". Deletion is a consequence of correct ownership, never the objective —
optimise for architectural ownership, runtime behaviour, portability and
performance, and let the line count fall out.

The target is: a lightweight portable `ds-command-kernel`; thin host effects; no
unnecessary dependency from `ds-network` to `ds-command-kernel`; neutral shared
crates only where genuinely needed; `ds-network` as an independently bounded
heavy engineering engine; aggressive multicore compute when heavy work justifies
it; low CPU and battery when it does not; and `ds-network` invoked explicitly and
coarsely, never as part of ordinary UI interaction.

`ds-command-kernel` + `ds-web` must be capable of providing the complete
ordinary/offline field experience with `ds-network` absent.
