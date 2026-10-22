# Profile view

This crate owns the MV profile panel's Leptos DOM, Canvas2D drawing, camera,
selection gestures, hit index, paint coalescing and typed intent payloads. The
permanent Interactions WASM wraps it; CLI and Server do not acquire its UI
dependency. The view never depends on the Kernel. Its owned scene projection
accepts already-calculated engineering facts and native display encodings; it
does not solve engineering or persist edits.

One `mountProfileCanvas` export on the existing Interactions WASM creates a retained
handle. `update` replaces the scene only when its identity changes, with small
presentation updates separately. `command`, `payload`, `reply`, `read` and
`invalidate` serve the existing typed host boundary. `unmount` cancels RAF,
releases observers/listeners/gesture state, drops cached surfaces and drops the
Leptos mount owner. The renderer uses the permanent Interactions instance; its
host retains the existing Kernel application client.

Model commands, project authorization, native worker requests and the shared
application tools opened by menus retain their existing owners. The host
rechecks its captured project, model, revision and selection around asynchronous
native work. Rust owns construction of station, ground, creation, lasso, sag,
delete and display payloads. A provisional native scene patch is display-only
and admitted only under the exact source schema, revision and axis.

`cargo test -p ds-profile-view` exercises independent hit/viewport/selection,
payload refusal and scene-patch controls in separate test files. Browser tests
exercise the actual mounted Interactions WASM. `cargo tree -p
ds-interactions-wasm --target wasm32-unknown-unknown -d` must not contain a second
`reactive_graph` version; the view uses 0.2.16.
