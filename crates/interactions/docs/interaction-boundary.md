# Interaction boundary

Owner ruling: 2026-10-14.

`ds-interactions` permanently owns transient UI algorithms that do not belong in
CLI or Server: pointer hit testing, hover and selection gestures, snap candidates,
viewport transforms, Canvas2D layout and UI spatial indexes. It has no dependency
on the command kernel or Network engineering. Neutral geometry dependencies must
also have neither dependency. UI wrappers supply admitted geometry and screen
facts; interaction results are transient intent, never project authority.

The browser target contains exactly three WASM units:

1. Kernel orchestration, permanently available.
2. Interactions, permanently available for the UI.
3. Network engineering, loaded when processing is requested or deliberately
   imminent, using the same owning computation crates as the native addon.

Desktop and Server run engineering through the admitted native Network executable.
Ordinary map movement, selection and snapping must not instantiate that executable
or load the engineering WASM. CLI and Server dependency closures exclude the
Interactions implementation and its browser wrapper; neutral shared scalar or
geometric primitives remain separate dependencies where already justified.

This is an ownership and dependency target, not a claim that every current browser
export has migrated. Move one existing implementation and its actual consumers at
a time, remove replaced code, and preserve existing byte/numeric/refusal behavior.
Do not introduce a TypeScript algorithm fallback or keep a copied implementation.
Historical schema readers remain explicit compatibility, not parallel engines.

Persisted selection, authored edits, project authorization and mutation admission
stay with Kernel or Network according to their current contracts. Engineering
topology snapping is Network computation; screen-space snap candidate selection is
UI interaction. Pure UI intent must cross the existing typed admission boundary
before it changes a project or model.

Interactive DS Grid/Profile editing redesign and its engineering-aware session
migration remain separately recorded hard problems. Mechanical extraction of
neutral interaction primitives does not reopen their solving, engineering or
authoring semantics.

Verification requires owning regressions, dependency-closure exclusion, portable
WASM compilation and actual browser/WASM consumer execution. Touched acquisition
paths prove request counts and unused-data controls. Existing source tests alone
do not prove an installed host or published browser release.

## Numeric and consumer verification

Screen distance adapters preserve the existing JavaScript arithmetic at exact
hit thresholds; ordinary Rust `hypot` is not an interchangeable implementation.
Packed numeric records retain NaN, Infinity and signed zero where the original
UI accepted them. The polygon adapter reads only the reached ring and its two
coordinate axes, keeping ignored foreign members and later-ring short circuits
outside transport work. Browser projection errors retain their identity.

Profile label policy remains model-owned. The host supplies Canvas2D font metrics
and locale collation ranks; Interactions owns ordering, viewport nudging and
collision decisions. Gantt scene/task/dependency authority remains with its
existing owner; Interactions returns hit intent and owns only the retained
screen index. Hosts dispose replaced and unmounted retained index handles.

The Web cold consumer gate is `npm run test:interactions`. It initializes real
Interactions/Kernel WASM from supplied bytes, denies Network/Grid module imports
and checks unused acquisition controls. Its deliberate engineering admission
must trip the import sentinel. The separate
`vitest.interactions-consumers.config.ts` runs the affected existing UI suites
with the real permanent units. These narrow gates do not replace the advertised
frontend/global gates. Node/WASM or jsdom pointer execution is distinct from
Chrome or installed WebView acceptance, and an isolated consumer is distinct from
full application startup with retained layers, tables and edit sessions.

## UI build facts

The Interactions WASM export `uiEngineBuildIdentity()` returns JSON with exactly
the following fields: `schema` (`ds.ui-engine-build/v1`), `component`
(`interactions`), `protocol` (`ds.ui-wasm/v1`), integer `ui_contract` (`1`),
`source_sha`, `cargo_lock_sha256`, `rustc`, `target`, `profile`, and
`config_sha256`. These identify the producer; they grant no installation or
activation authority and contain no Core or engineering version requirement.
The receipt's manifest digest is SHA256 of the ten-key object encoded as compact
UTF-8 JSON with alphabetically sorted keys and no trailing newline.

Release compilation requires `DS_INTERACTIONS_SOURCE_SHA` as an exact lowercase
40-character committed Git SHA. As with the engine producers, the release driver
must verify that the supplied revision describes the source being built. The
adapter does not infer a sibling revision or verify a release checkout. Unstamped
debug builds report `development`, which is outside the installable exact-SHA
contract. A supplied malformed SHA refuses even in debug builds.

`cargo_lock_sha256` hashes the workspace's actual lock bytes. `rustc` records
`rustc -vV`; `target` and `profile` are Cargo's build facts. `config_sha256` hashes
the build script's named workspace and crate manifests, build metadata sources,
optional Cargo configuration and toolchain files (including declared absence),
Cargo's encoded Rust flags, feature environment, optimization level and debug
setting. Its framing is versioned and distinguishes an absent input from empty
bytes. It contains no timestamp, installation identity or ambient sibling HEAD.
The external neutral `ds-geo-lite` path source remains a release-driver input;
the lock digest does not pretend to pin its source bytes. WASM packaging tools and
final artifact digests belong to the release receipt, outside this Rust identity.

The component's committed `release/ui-wasm-source.json` pins its external neutral
geometry source. The producer admits its exact normal/build closure, captures
compiler and Cargo configuration, injects that configuration digest, and compares
the actual WASM export before minting a publication receipt.
