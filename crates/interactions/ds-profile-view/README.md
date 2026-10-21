# Profile view

This crate owns the MV profile panel's Leptos DOM, Canvas2D drawing, camera,
selection gestures, hit index, paint coalescing and typed intent payloads. The
Kernel opts into it with `profile-view` (included by `wasm`); native Kernel
consumers do not acquire a UI dependency by default. The view never depends on
the Kernel. Its owned scene projection accepts already-calculated engineering
facts and native display encodings; it does not solve engineering or persist edits.

One `mountProfileCanvas` export on the existing Kernel WASM creates a retained
handle. `update` replaces the scene only when its identity changes, with small
presentation updates separately. `command`, `payload`, `reply`, `read` and
`invalidate` serve the existing typed host boundary. `unmount` cancels RAF,
releases observers/listeners/gesture state, drops cached surfaces and drops the
Leptos mount owner. There is no second WASM module or Kernel client instance.

Model commands, project authorization, native worker requests and the shared
application tools opened by menus retain their existing owners. The host
rechecks its captured project, model, revision and selection around asynchronous
native work. Rust owns construction of station, ground, creation, lasso, sag,
delete and display payloads. A provisional native scene patch is display-only
and admitted only under the exact source schema, revision and axis.

`cargo test -p ds-profile-view` exercises independent hit/viewport/selection,
payload refusal and scene-patch controls in separate test files. Browser tests
exercise the actual mounted Kernel WASM. `cargo tree --target
wasm32-unknown-unknown --features wasm -d` must not contain a second
`reactive_graph` version; both this view and Kernel use 0.2.16.
