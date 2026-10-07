# Profile display control authority

`profile_display::DisplayState::patched` owns display validation and atomic
patching. `profile_controls::groups` owns the declared grouping, label keys,
initial section expansion, numeric values, limits and complete current request
contexts. Both the renderer projection and bounded command receipt emitted by
`ds-profile-runtime` consume that declaration. The runtime is independent of
TypeScript, Svelte, browser stores and Tauri.

Each visibility row retains the native option, current checked value and
resolved governed pen. Each numeric control emits a complete typed
`DisplayPatch` request and its `value_path`; a host replaces only that value
and submits the request. Omission retains state. Terrain requests include all
four current fields. A zero gap is refused by the owner even though HTML cannot
express an exclusive numeric minimum. Numeric inputs must not clamp, round or
choose defaults in a host.

Cable pens include the exact engineering name, current encoding and native
`can_reset` state. Setting a cable colour uses the existing `cable_colors`
patch; null returns it to its governed/default pen. Hosts display the native
ink and reset availability. The legend explains emitted symbols and has no
settings, colour editor or independent mutation path.

The live model Profile Display surface and CLI/MCP use the same revision-pinned
runtime. The existing host captures identity, project, model and revision and
refuses stale results. Runtime view scopes remain isolated; malformed patches
cannot alter the admitted view. A missing native projection is an unavailable
control surface, never a reason to write browser settings instead. Package
label authoring is separate from transient Profile display visibility; a
browser-owned Plan/Profile label editor must not substitute for that authority.

The unit gate in `profile_controls` proves complete unique option coverage and
that numeric requests use the owner's validation. The runtime integration gate
`cli_and_renderer_share_native_controls_and_colour_state` proves matching
receipts, state retention after invalid combined patches, colour normalization
and reset, and isolation between view scopes.

Local appearance uses `DisplayPatch.styles`: exact native option or semantic symbol
role to `{color?, width_mm?, line_type?, symbol?}`; null resets the role. Rust
merges omitted fields, validates the entire request and converts physical pen
units to screen encodings. Symbols apply to strain/suspension/junction/unknown
and structure/terrain points. Allowed line types are solid, dashed, dotted and
dash_dot. CLI receipts and drawing carry the same native styles, JSON request
and compiled schema. These transient overrides never publish global styles.

`profile_layout::resolve` receives the available Map/Profile area. Omitted height
opens equal halves; explicit user/CLI resize must leave both panes visible.
Persisted pixel height must never override the opening half-height policy.

Shared Plan/Profile selection controls execute `grid_selection_controls` through
the native runtime. Initial relation is Within; initial layers are All. Explicit
layer choices remain in the principal/project/model scope for the Desktop
lifetime and are filtered against each held surface's native available families.
Only configure changes remembered preferences; one-shot admission never changes them.
Read/configure/admit/compose are separate typed requests; omission retains the
scoped preferences, and null families restores All. The toolbar renders relation,
combination operation, then layer choice. Rust admits queries and composes
ordered selected identities; host project/model/revision/scene/cancellation fences
remain in force. Outside means disjoint, including all occurrences, never merely
"not wholly within".
