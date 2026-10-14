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
