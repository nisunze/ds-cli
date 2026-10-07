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

Native Profile sessions initialize once with `operation: initialize`, receiving
the worker's read-only `ProfileSessionSeed` and options/style/patch/analyze inputs.
The opened snapshot, full retained journal and exact cursor are independently
verified before projection. The admitted receipt names `model_id`,
`model_revision` and native `history`. `operation: refresh` contains that model
identity, the held `base_revision` and `base_history`, the worker's
`expected_revision` and `target_history`, its full retained typed `JournalEntry`
list, and the same display inputs. There are no package bytes in refresh.
`operation: observe` uses the same fenced replay inputs but never prepares a
scene, weather case, usage screen or analysis. It retains the calculated scene
and axis, advances authored RAM history and demand-cache invalidation only, and
returns `deferred:true`, `ran:false`, and native `calculation` metadata: explicit
policy, authored/computed revisions, whether calculation is required, and the
union of engine recalculation scopes since the held scene. Display patches and
Analyze are refused on observation. No dense scene is serialized for this read.
Calculation is intentional only (owner, 2026-10-07): edits, idle time, opening
Issues, leaving editing and Save never schedule it. Report visibility remains
independent of this policy; `analysis_state:due` is informational. Explicit
Refresh/Analyze requests consume accumulated receipts; unchanged alignment
surfaces and section demands remain reusable. History navigation retains the
old scene and reports that explicit reconciliation is needed.
The runtime finds the exact common command prefix, navigates retained history,
and applies only new entries with parent and resulting-revision fences. Native
discard/navigation pins install the exact cursor and truncate only the worker's
discarded tail. Command identity, history pins and depths distinguish repeated
content revisions, including a branch after undo; an old request whose content
revision repeats still refuses against a changed history.

Replay and projection admission are one transaction. A stale base, replacement
model, unknown journal origin, invalid command, divergent result or failed
projection leaves the held session, redo tail, display and analysis unchanged.
These named refusals never trigger an automatic package-checkpoint fallback.
Initialize a replacement model in a fresh scope; reinitializing an admitted scope
refuses. The renderer captures its own scope/generation and journal cursor before
work and installs a result only while those
pins are still current; a stale display result does not undo an admitted native
session advance. The next request uses the native receipt's admitted head.

The session retains package assets by shared ownership and opens with the same
package-derived protection as other native consumers. Demand caches advance
only from engine command receipts: unaffected section demands survive forward
edits, while navigation or unmatched parents make old evidence non-current.
Old analysis is never projected as current at a different authored revision.
Initialization must use the worker's opened snapshot so its entire active
journal, including undo before the first Profile display, remains navigable.

Profile scene engineering and Analyze share the same receipt-advanced native
demand cache. Scene preparation populates it before default analysis and the
issue screen; repeated reads reuse those native section cases. Applied-load and
whole-support report evidence stays identical to uncached native analysis; the
issue screen reuses retained section cases. The cache has 16,384
section/case/basis slots; inputs evicted above that bound must be solved again.
Clearance remains a separate design-case calculation. This change alone is not scoped-band or release-latency acceptance.

Native Profile scopes are leases. The runtime admits up to eight and refuses a
new scope with `profile_replay_capacity` before seed/package work if all slots
are held. It never evicts another admitted client by UUID ordering or recency.
`release` takes exact `scope` and `model_id`, refuses a mismatched held model,
and idempotently drops its session, retained source assets, display and analysis
state. It never changes the worker's authored session or Save metadata. Hosts
release on replacement/disposal and may initialize a fresh read-only seed once
if a restarted runtime reports `profile_replay_base_unavailable`; that recovery
is not a per-edit package fallback.

A recovery initialization may forward its prior admitted `display_state` as
`restore_state`. Rust validates this with the ordinary DisplayPatch controls,
then applies the new request patch. Restore, seed replay and scene admission
are transactional: invalid state or projection refusal leaves no new scope.
The renderer transports these choices unchanged; it does not reconstruct
native defaults or merging policy.

Native Profile journal replay passes its actual command outcomes into
`ProfileReviewView::scene_after_with_cache`. Surface reuse starts only at the admitted
cursor, follows the complete native parent chain, and stops at the requested
cursor; commands in a retained redo tail do not contribute to its scope.
Undo/redo and a divergent prefix reconcile without forward receipts. The
runtime's scope/model/history fences continue to own isolation, and no web
input can supply a recalculation scope. A refused replay retains the previous
session, view and analysis together. Analyze with an unavailable style never
advances the retained review against an unadmitted model head. Native runtime
transaction tests prove surface work and exact fresh-scene parity for local
and coalesced edits, navigation, branching and style refusal. Runtime checks
also prove that a local refresh computes only affected section cases, Analyze
reuses those demands with exact uncached report parity, and undo reconciles
instead of retaining forward-edit evidence.

Seed and journal handovers use opaque `seed_json` and `journal_json` strings.
Renderers may parse identity/cursor metadata but forward the original text: JS
number reserialization loses signed zero and can change native revision hashes.
Native deserialization and revision/history validation remain authoritative.
Initialization admits at most 1 GiB of seed JSON (including encoded assets);
refresh admits at most 64 MiB of journal JSON and 16,384 entries. Old object
handover fields refuse rather than falling back to package reopening.
