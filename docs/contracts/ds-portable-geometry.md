# Portable geometry — the lightweight layer beneath the kernel

**Status:** implemented, 2026-09-11. This is the contract for the severance
required by `ds-kernel-network-boundary.md` §4–5. §1, §2 and §4 were rewritten
when the owner ruled the destination; §3 is the judgement made before the code
and is unchanged.

The ruling forbids `ds-command-kernel ──► ds-network` and permits "a lightweight
dependency beneath it". This document names where that lightweight geometry
lives, how it stays honest, and what the severance did and did not buy.

## 1. The destination — the owner's ruling

> **Source stays maintained in `ds-network`. It is compiled into the kernel at
> build time. NO Cargo edge in either direction. Digest-pinned with a drift
> check.**

`ds-network` is **not edited**. It keeps `ds-geo`, `ds-geo-ops` and `ds-canvas`
exactly as they were, with their own tests. The kernel carries a **snapshot** of
the files it needs:

| | |
|---|---|
| `vendor/ds-geo-lite/` | the snapshot — ds-network bytes, plus `MANIFEST.json` |
| `crates/ds-geo-lite` | compiles `vendor/ds-geo-lite/geo/*` as `ds_geo_lite` |
| `crates/ds-canvas-lite` | compiles `vendor/ds-geo-lite/canvas/*` as `ds_canvas_lite` |
| `tests/geo_lite_drift.rs` | the proof that the snapshot is still the source |
| `scripts/sync-geo-lite.sh` | the only sanctioned way to move bytes across the line |

`geo`, `rstar`, `serde` and `serde_json` are declared by those two crates inside
this repository. `ds-geo`, `ds-canvas` and `ds-geo-ops` are gone from
`Cargo.toml` and from `Cargo.lock`.

### Why the snapshot is not a fork

A copy that nobody checks is a fork; a copy that is checked on every `cargo test`
is a build artefact with a long shelf life. `MANIFEST.json` pins, per file, the
ds-network path, the ds-network commit, the sha256 of the vendored bytes, and
the sha256 of every source they were cut from. `tests/geo_lite_drift.rs`:

1. **always** re-hashes the vendored files against the manifest, and asserts the
   vendor tree holds exactly the manifest's files — so an in-place "quick fix"
   to the snapshot fails immediately, on a machine that has never heard of
   ds-network;
2. **when `../ds-network` is on disk** re-hashes every source the manifest names
   and diffs every whole-file entry against the live bytes — so upstream moving
   is reported, with the exact path, by whoever builds next;
3. guards the two ways the edge comes back: a manifest that names a
   `ds-network` path, and a `.rs` file that reaches one through `include_str!`,
   `include_bytes!` or `#[path]`. The second is not hypothetical — see §5.

Parity needs no corpus for the whole-file entries, because parity is by
construction: the bytes are the same bytes, and the unit tests that ship
inside those files run here too (`cargo test --workspace`). The drift test is
the named proof. The partial entry below is the one place
that argument does not reach, so it carries a corpus of its own.

### Why two crates

`ds-canvas`'s files use crate-root paths (`crate::Bounds`, `crate::Point`,
`crate::drag_started`). Grafted under `ds-geo-lite` as a module tree they would
all need editing, and byte-identity is the whole proof — so the snapshot of
`ds-canvas/src/**` is a crate of its own whose `[lib] path` IS the vendored
`lib.rs`. `ds-geo-lite`'s own crate root is authored (it is not a snapshot of
anything) and mounts the four ds-geo modules by `#[path]`.

Both are edition 2024, because `ds-network` is and `comparison.rs` uses a
let-chain. Compiling the bytes as they were written is part of snapshotting
rather than rewriting them.

### The one partial entry

`ds-geo-ops::haversine_m` is three items in three files
(`primitives/haversine.rs::distance`, `lib.rs::haversine_m`,
`types.rs::EARTH_RADIUS_M`) whose siblings pull the whole primitives and
operations tree. The kernel calls one function, so the cut is at item level:
`vendor/ds-geo-lite/geo/haversine.rs` is marked `partial: true`, lists the items
it carries, and is checked against its three SOURCES' digests rather than by
byte comparison. Upstream touching any of the three fails the drift test and
asks for a re-derivation by hand.

Those digests prove upstream has not moved. They prove nothing about whether the
transcription was faithful the day it was cut — a transposed argument, a dropped
halving or a different Earth radius would pass every digest in this contract. So
the partial entry is additionally checked against closed forms computed WITHOUT
the haversine formula (an exact meridian arc, and the spherical law of cosines)
in `tests/geo_lite_drift.rs::the_transcribed_haversine_matches_an_independent_yardstick`.
Two independent formulations of the same distance on the same sphere agree with
a correct transcription and disagree with a wrong one. **Any future partial
entry owes the same kind of proof**, because for a partial entry the bytes are
ours, not ds-network's.

## 2. What is snapshotted

15 files, `vendor/ds-geo-lite/MANIFEST.json`, taken at ds-network `a6288ad`
(`577ec83` until geometry admission was added, 2026-09-27):

| Vendored | ds-network source | Whole |
|---|---|---|
| `geo/viewport.rs` | `crates/ds-geo/src/viewport.rs` | yes |
| `geo/spatial_predicate.rs` | `crates/ds-geo/src/spatial_predicate.rs` | yes |
| `geo/identity.rs` | `crates/ds-geo/src/identity.rs` | yes |
| `geo/comparison.rs` | `crates/ds-geo/src/comparison.rs` | yes |
| `geo/admission.rs`, `geo/admission_corpus.json` | `crates/ds-geo/src/admission{.rs,_corpus.json}` | yes |
| `geo/haversine.rs` | `crates/ds-geo-ops/src/{primitives/haversine.rs,lib.rs,types.rs}` | **partial** |
| `canvas/{lib,viewport,selection,measured_path,free_layout,packed,layout_session}.rs` | `crates/ds-canvas/src/**` | yes |
| `network/processor_presets.json` | `crates/ds-network/src/network/processor_presets.json` | yes |

The last one is not geometry — it is the processor catalogue
`process_settings` is pure over, and §5 explains why it is here. The directory
is named after its first tenant rather than its contents; renaming it is one
string in `scripts/sync-geo-lite.sh` and three `#[path]`s.

`geo/admission.rs` is the one rule every input door applies to a geometry
(owner ruling, 2026-09-27: what BigQuery rejects is rejected at our input
boundary, and a globe-covering polygon never exists in our system). Its source
stays in ds-network so the file importers there (`ds-io`) and the kernel's doors
judge with the same bytes. Its parity is with BigQuery rather than with
ds-network, so it ships its own corpus: `geo/admission_corpus.json` records
`ST_GEOGFROMGEOJSON`'s answer for every case, and the tests inside the file
assert each one here too.

## 3. Judgement: `compare_geojson_layers` is not heavy

The ruling asked for a decision rather than an assumption. The decision is that
it moves with the rest, on this evidence:

- **It performs no geometric computation at all.** Geometry equality is exact
  JSON value comparison — `ds-geo/src/comparison.rs:447-448` reads
  `local_feature.get("geometry") != cloud_feature.get("geometry")`. There is no
  predicate, no projection, no tolerance, no intersection.
- **It imports nothing heavy.** `comparison.rs:7-10` imports `crate::identity`,
  `serde`, `serde_json` and `std::collections`. No `geo`, no `rstar`, no
  `geographiclib-rs`. Its one internal dependency, `identity.rs`, is 88 lines
  over `serde_json` and `HashSet`.
- **Its algorithm is a diff, not a solve.** Identity matching is union-find over
  alias strings (`comparison.rs:374-405`, `UnionFind` at `:919`); the rest is
  counting and a coordinate walk for change bounds (`BoundsAccumulator`, `:860`)
  that is the same shape as `geojson_bounds`.
- **The litmus test answers yes.** Comparing two revisions of a design is
  synchronization semantics — explicitly the kernel's column in the ruling's
  ownership table. A surveyor offline in the field must be able to see what
  changed between the revision they hold and the one they were sent. That is
  the case *for* portability, not against it.

It is a large function — 1,440 lines with tests, 954 without — but size is not
the criterion. The criterion is whether the operation is inherently heavyweight
engineering computation. It is not; it is a bounded data diff.

The two kernel call sites (`src/design_versions.rs:509` and `:527`) therefore
stand as written and need no re-examination.

## 4. `ds-design-workspace` has its own workspace

`crates/ds-design-workspace` is a native host crate — rusqlite, fs2, no WASM —
and it depends on `ds-network` directly. That is permitted: the ruling lets a
higher-level host invoke `ds-network` separately.

But cargo resolves **every workspace member's manifest** before building any of
them, so while it sat in the kernel's `members` the *kernel* could not build
with `ds-network` absent no matter what the kernel's own dependencies said. It
is therefore excluded from `members` and carries its own `[workspace]` table and
`Cargo.lock` at an unchanged path. `ds-cli`'s path dependency on it is
unaffected.

That has one consequence which is easy to miss and was: **a detached crate is
invisible to `cargo test --workspace`.** It is not a member here any more, and
it is not a member of ds-cli's workspace either — it sits outside that
directory and declares its own `[workspace]`, so `cargo test --workspace` there
builds it as a dependency and runs none of its tests. Detaching it silently took
its six tests out of every gate.

They run in `scripts/test-detached-workspaces.sh`, which is part of the kernel's
gates wherever `../ds-network` is on disk (it prints `SKIPPED` and exits 0 where
it is not — a checkout without the engine is the severance working, and this is
the one crate that legitimately cannot build there).
`tests/geo_lite_drift.rs::every_detached_workspace_is_covered_by_the_gate_script`
fails if a second crate is ever detached and not listed in it.

## 5. `include_str!` is an edge too

`src/process_settings.rs`'s test module read ds-network's
`processor_presets.json` off the filesystem. The library compiled fine without
ds-network; `cargo test` did not. A filesystem edge fails later and louder than
a Cargo edge, and nothing was watching for one.

The catalogue is snapshotted with everything else, and
`no_kernel_source_reaches_ds_network` now fails on any `.rs` file under `src/`
or `tests/` that names a `ds-network/` path.

## 6. What the severance bought — measured, 2026-09-11

Measured in the lane worktree at kernel `8d38e89` before, and with this change
after, using the same command both times
(`CARGO_TARGET_DIR=$PWD/target-wasm wasm-pack build --target web --release
--features wasm`, rustc 1.97.0, wasm-opt 130):

| | before | after | delta |
|---|---|---|---|
| `Cargo.lock` packages | 189 | 154 | **−35** |
| `dist/ds_command_kernel_bg.wasm` (wasm-pack) | 10,282,760 B | 10,403,695 B | +120,935 B |
| after `wasm-opt --all-features -Oz --strip-debug --strip-producers` | 7,530,016 B | 7,646,634 B | +116,618 B |
| `cargo check --features wasm` with ds-network **absent** | fails | **succeeds** | |
| `cargo test` with ds-network **absent** | fails | **succeeds (529)** | |
| `wasm-pack build` with ds-network **absent** | fails | **succeeds** | |

**The artefact did not shrink, and that is the honest finding.** The ruling's §3
measured 22,238 lines of engine source in the build graph and implied the 7.9 MB
artefact carried them. It did not: wasm-ld already stripped every unreachable
function, so the eleven symbols' code was the only ds-geo/ds-canvas code in the
binary before, and it is the only such code now. The +1.6 % is codegen-unit
repartitioning — the same functions, compiled in a small crate instead of a
large one, inline and merge differently (`code` section +118,878 B, +17 B of
function-section index, everything else within 64 B). The linked geometry
surface is unchanged; the binaries carry the same 101 `ds_geo*` symbol strings.

So the severance buys **deployability and the build graph**, not bytes:
35 fewer packages, no ds-network checkout required, no engine in the dependency
tree, and an Android/iOS build that cannot accidentally acquire one. If the
artefact size itself becomes the goal, the lever is `[profile.release]`
(`codegen-units = 1`, `lto`), which is a product-wide decision and not part of
this severance.

## 7. Refreshing the snapshot

```
scripts/sync-geo-lite.sh [path-to-ds-network]     # default ../ds-network
cargo test --test geo_lite_drift
```

It refuses a dirty ds-network checkout, copies the whole-file entries, rewrites
`MANIFEST.json` at the current ds-network commit, and never overwrites the
partial entry — for that it only re-reads the source digests, and the drift test
tells you the items to re-derive.

**Do not edit `vendor/ds-geo-lite/` by hand.** `cargo fmt` will happily reformat
those files, because they are real modules of a real crate; the drift test is
what catches it. A fix belongs in `ds-network`, followed by a sync.
