# The kernel / engine boundary — Android and iOS are the litmus test

**Status:** owner ruling, 2026-09-10. The boundary is **currently violated**;
§3 measures it exactly. This document states the rule, the measurement, and what
severing it requires.

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

### The question to ask of every piece of logic

> Would we expect this capability to work on an Android or iOS field device
> without shipping the heavy network engine?

**Yes** → `ds-command-kernel`, or a lightweight dependency beneath it.
**No, because the operation is inherently heavyweight engineering computation**
→ `ds-network`.

### Ownership

| `ds-command-kernel` — the portable DS application | `ds-network` — optional heavy compute |
|---|---|
| canonical project/application state | advanced electrical/network analysis |
| commands and queries | optimization |
| selection and editing semantics | large graph/spatial workloads |
| survey workflows | terrain/profile computation |
| attributes/forms | FEM |
| filters | PLS-related engineering computation |
| lightweight geometry | other expensive algorithms that should not burden a field deployment |
| validation | |
| offline mutations | |
| synchronization semantics | |
| derived/application/view state | |
| domain behaviour that must exist independently of MapLibre or an open map | |

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

## 3. The violation, measured (2026-09-10)

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
field build would carry.

One thing that is **not** a problem, recorded so nobody spends effort on it:
`polars` is *not* in the graph. `ds-geo` declares `default = []` and puts polars
behind `enable_dataframes`, which the kernel does not enable.

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
