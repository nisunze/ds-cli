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
Observation also stages an independent native `ProfileAuthoredView` and returns
`authored_delta` at the retained axis. Its fields identify the previous authored
revision, current authored root, calculated revision and axis digest. Workers
admit it against exact history before updating their current geometry/pick index;
renderers transport native field changes without deriving geometry. The calculated
review and its input root remain unchanged. An unavailable authored axis is named
in `authored_unavailable`, with no package or calculated-scene fallback.
`metrics` reports journal admission, native replay and authored projection
milliseconds, delta bytes and actual applied/retained entry counts. These phase
measurements exclude host transport and worker installation and do not constitute
release-performance acceptance. Native surface work separately counts rebuilt
bands and rebuilt ground, distinguishing reused terrain from placement work.

Native display projection partitions `authored_structures` by canonical support
identity; static `symbols` and `primitives` retain terrain and calculated labels.
Observation returns only changed support groups in `authored_display_delta`, with
null tombstones for deletion. Renderers apply these native replacements after
worker geometry admission. Canonical staking comments and structure descriptions
are distinct fields; changing a description does not invent a staking comment.
`authored_display_delta_bytes` measures this additional transfer independently.

Calculation is intentional only (owner, 2026-10-07): edits, idle time, opening
Issues, leaving editing and Save never schedule it. Report visibility remains
independent of this policy; `analysis_state:due` is informational. Explicit
Refresh/Analyze requests consume accumulated receipts; unchanged alignment
surfaces and section demands remain reusable. History navigation retains the
old scene and reports that explicit reconciliation is needed.

Weather selection and prepared-layer transfer cannot calculate. Native
`case_layer` retains the `cached_only` wire field solely to suppress geometry
bytes; false never authorizes preparation. Both reads and selection require
the captured model, full native history (including undo/redo tails), exact
current revision, coordinate options and a valid prepared case. Receipt identity
includes the same scope, model and history.
Valid missing/stale preparation refuses `profile_calculation_required`;
unknown/disabled cases refuse `profile_case_unavailable`. Explicit Refresh or
Analyze carries the requested `patch.display_case` and prepares it. Save is
independent. A pure read does not change display state, weather sequence, job
context, result identity or a confirmed publication rollback checkpoint.
Selection changes the native display/job context and admits a result index for
that prepared clearance case, invalidating older query cursors. It returns
native case decorations even when geometry bytes are suppressed.

Case receipts expose `case_ms`, `materialize_ms`, `decorations_ms`,
`layer_encode_ms`, `layer_bytes`, `index_ms`, `index_rebuilt` and
`calculated_cases:0`. These include native scene materialization and index
reconstruction, which remain broad; they exclude final JSON encoding, IPC and
paint. The cached case path does not claim bounded projection or installed
latency merely because it does no new engineering.

Explicit background calculation uses four native operations, available to a
headless `ds-profile-runtime::Runtime` without a Desktop scheduler:

- `start_calculation` takes `{job_id, request: RefreshRequest}`. First Observe
  the exact worker history; both refresh histories and revisions must equal
  that admitted native cursor. The request requires a new `publication_id`.
  Rust forks only that scope's retained RAM inputs, demand cache and review,
  then solves on its own thread after returning a bounded job receipt.
- `calculation_status` takes `{scope, model_id, job_id}` and returns only the
  captured history, running/ready/failed/stale/cancelled state, timing and
  refusal evidence. It neither returns the scene nor publishes engineering.
- `cancel_calculation` uses the same identity. It discards the result and
  checks cancellation before and after solving. `solver_interruptible:false`
  describes the current solver boundary. `runtime_worker_active` truthfully
  reports whether any physical calculation worker still occupies the slot.
- `admit_calculation` adds the exact start `publication_id` to that identity.
  Rust checks the captured authored history and native display context before
  installing the ready result under the ordinary publication checkpoint.
  Worker/render admission must still Confirm or Discard it.

Observe and native selection remain executable while a job runs. Authored
history pins distinguish repeated-content cursors; native context identity
also distinguishes display changes away and back, calculation, release and
replacement. A stale, cancelled, pending or failed result cannot replace the
live review or erase pending edits. Interleaved scopes retain their own inputs.
One physical worker per Runtime is admitted, including cancelled work until
it exits; there are at most eight retained results. Job IDs cannot be reused
within a Runtime lifetime, with a bounded 16,384-ID ledger. No job writes or
packs a model. First initialization still projects synchronously; these controls
address calculation in an already retained editable Profile.

`calculation_control` accepts `{scope, model_id, expected_history?, request}`.
Its shared native `CalculationArguments` is a closed action DTO: `start` takes
`job_id`, optional `display_case` and `analyze` (default false); `status`,
`cancel` and `admit` take only `job_id`. Start requires exact observed history.
Rust captures the admitted options, style document and retained native journal;
callers cannot replace scope, project, publication pin or calculation inputs
inside the action. The single-use job ID owns its native publication ID.
Admit retrieves that pin from the native job and normalizes into ordinary
publication admission before executing. Hosts still confirm/discard after
worker/render installation. Status/cancel return bounded receipts without
calculating or transferring scenes. Save remains independent.

The native control gate proves closed JSON/identity inputs, missing/stale history
and wrong-model refusals, continued observation while a paused worker runs,
cross-model cancel refusal, stale admission, noninterruptible worker capacity,
full fresh calculation/Analyze parity and publication rollback without saving.
This retained Runtime is headless; authenticated project/principal capture and
paired CLI/MCP transport are separate host boundaries.

The admitted result adds `job_start_ms` (capture/fork/thread launch),
`job_worker_ms` (native calculation/serialization on the fork), and
`job_elapsed_ms` (capture through admission, including scheduling and polling).
The existing `publication_checkpoint_ms` measures admission rollback staging.
These are native phases, excluding transport, worker picking and painting.
RAM cloning and complete calculated-scene transfer remain measured costs;
background scheduling alone does not prove a faster solver. Wire encoding is
outside these phases.

The ignored `dense_native_job_costs_and_fresh_parity` gate measures twelve
alternating description edits with 10,004 terrain points, 403 supports and one
three-support tension section. Each admitted scene and analysis equals a fresh
runtime. The debug median is 168.25 ms apply, 249.47 ms Observe, 11.61 ms job
capture, 395.37 ms calculation and 16.63 ms admission. The admitted response is
15,611,679 bytes: calculated-scene transfer is still a broad boundary. The
receipt is `ds-network/out/profile-native/native-job-cost-debug.json`. This is
a headless owning-crate measurement, excluding wire encoding/transport, worker
picking and painting; it is not installed Desktop or large-connected-model
acceptance. Set `DS_PROFILE_JOB_COST_RECEIPT` to an absolute disk path and run
the named ignored gate through the runtime manifest to reproduce it.

Ordinary release on the same fixture has medians of 6.59 ms apply, 28.82 ms
Observe, 5.19 ms capture, 95.49 ms worker calculation and 9.17 ms admission
(6.50 ms of admission is publication checkpoint staging). Native job elapsed
time, including polling, is 108.72 ms, range 104.33–113.30 ms. The result still
transfers 15.61 MB. The source receipt is
`ds-network/out/profile-native/native-job-cost-release.json`; the timing
exclusions and single-section limitation above remain in force. The transitive
release build requires the exact local Solar source SHA and this runtime's
Cargo.lock path in `DS_SOLAR_SOURCE_SHA` and `DS_SOLAR_CARGO_LOCK_PATH`.
`operation: present` admits display-only patches against current, prepared
results. It refuses `profile_scene_unavailable` when edits or new coordinate
inputs would require calculation; callers must explicitly Refresh or Analyze.
Visibility/style controls cannot become hidden engineering calculation paths.
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
pins are still current. Hosts that install results across asynchronous boundaries
stage a native publication before adopting its replay head.

`publication_id` on Initialize/Refresh/Observe/Present stages a disposable result.
Its `publication` receipt pins the exact model, native authored history,
calculated revision and retained axis. `confirm_publication` accepts only that
entire receipt; `discard_publication` names the captured publication identity,
model and target history, so even a lost response can be discarded. An
unconfirmed result blocks another projection in that scope with
`profile_publication_pending`; another model scope remains independent.
Discard restores the prior native session cursor, display, analysis, pending
engineering receipts and geometry together. It does not alter worker-authored
edits, run calculation, write a package or Save.

One rollback checkpoint survives confirmation until the next operation in that
scope. This permits a renderer refusal after worker admission to discard the
same publication. Repeated-content histories, wrong calculated/axis pins and
foreign model discards refuse. Release clears the checkpoint with its lease.
Large authored and calculated geometry are shared immutable Arcs during local
staging; `publication_checkpoint_ms` exposes the remaining checkpoint cost.
The native gates prove dropped observation followed by a coalesced edit, stale
calculation followed by another edit with its old engineering and accumulated
scope preserved, initialization discard and interleaved contexts. Worker
rollback and Web/Desktop publication bindings must consume these controls;
the native gates alone do not establish those transport races are closed.

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

Replay validates every new retained command, including future redo commands,
with native apply and result-revision fences. Only receipts at or before the
requested cursor advance demand caches. An undone weather command cannot clear
unrelated current-cursor section cases. The deterministic future-redo gate
reduces recomputed cases from six to the three affected cases, reuses the other
three, retains the exact redo history, and matches a fresh native scene.

Observe and explicit Refresh/Analyze return bounded numeric `metrics`.
`admission_ms`, `journal_ms` and `replay_ms` separate request admission, native
journal decoding/chain validation and staged replay/navigation. The summed
`replay_apply_*_ms` phases measure actual native command admission, snapshot
clone, command, stored geometry, outer validation, revision, delta, commit and
total apply. `replayed_entries` includes validated future-tail commands;
`applied_entries` counts receipts admitted at the current cursor, and
`retained_entries` is the complete journal length.

Explicit calculation additionally reports `calculation_ms`, including native
scene preparation, analysis when requested, and result construction.
`last_surface_*` counters describe the retained surface's last preparation;
`last_demand_*_cases` describes the final demand consumer. These last-work
counters are not a sum of all calculation stages and may describe retained
preparation on a cache hit. `total_native_ms` ends after native result
construction, before host transport, worker installation and painting. Debug
revision phases include the full-snapshot incremental-token audit; the guard
is unchanged. Metrics never alter journal, authored or engineering identity.

Terrain-only native receipts retain candidate demand cases until explicit
calculation resolves their current support geometry. Bit-identical finite
support/attachment coordinates and effective ground admit reuse; unavailable
paths or any coordinate change require fresh solving. Observe advances only
invalidation and authored geometry, leaving demand counters and calculated
revision unchanged. Refresh exposes `last_demand_terrain_geometry_checks` and
`last_demand_terrain_geometry_reused_cases`, with the same last-consumer semantics
as other demand counters. The native runtime gate proves exact fresh-scene parity
and independent Save, including a rounded endpoint whose tiny interpolation
change correctly defeats reuse. Terrain-only reference reuse has its separate
native span-window fence. Unchanged selected-case payloads share that final
reference scope and actual selected-case identity; independent design clearance
and query freshness retain their broader boundaries.

The owning ordinary-release terrain demand receipt uses 10,017 terrain points,
three cases and twelve edits. Six sections/seven supports: native apply median
10.853 ms; checked calculation and final JSON 1.443 ms versus 1.459 ms fresh.
120 sections/121 supports: apply 12.253 ms; checked calculation and final JSON
5.847 ms versus 6.155 ms fresh, solving six cases and reusing 354. Every report
matches fresh bytes. The guard avoids solves but the six-section total benefit
is negligible. See `ds-network/out/profile-native/terrain-demand-geometry-cost-release.json`.
This excludes runtime replay, Profile reference/case/layout, design clearance,
job capture, IPC and painting; installed sub-0.5-second editing remains separate.

`last_demand_resolved_support_paths` counts actual geometry-guard resolutions;
named cases share one immutable path per section within the last explicit walk.
It excludes path resolutions inside fresh solving and is not a count of total
terrain work. No derived path survives as a current fact at another revision.

Explicit calculation also reports `last_reference_computed_sections`,
`last_reference_reused_sections`, `last_reference_terrain_checked_sections`,
`last_reference_terrain_reused_sections`, `last_reference_terrain_inspected_ground_edges`
and `last_reference_terrain_inspected_observations`. These describe the last
reference preparation, including index build and candidate visits, rather than
the entire calculation. A cache hit can retain prior work counters. Observe
does not enter the reference guard. Changed endpoint bases can reject a candidate
before visiting ground windows; zero inspected edges does not imply no dependency
checking. AutoSag, unavailable references, changed offsets and mixed receipts
retain conservative preparation. Exact finite comparisons use no tolerance.

The owning indexed reference receipt measures 121 supports, 120 sections and
10,015 terrain points over twelve edits, with alternating fresh/retained order.
Ordinary-release apply median is 12.502 ms; complete native scene calculation
and JSON is 40.285 ms versus 44.896 ms fresh, preparing two references and
retaining 118. About 8.27 MB matches fresh bytes. Index build/candidate visits
inspect 39,960 edges and 20,030 observations, compared with about 2.36 million
each in the measured linear guard. That guard was slower than fresh (46.916 ms
versus 44.028 ms), demonstrating the need to include dependency work. See
`ds-network/out/profile-native/terrain-reference-cost-release.json` and its
`terrain-reference-linear-cost-release.json` comparison. Includes full current
surface, reference/case/election, uncached usage, layout/markers and encoding;
excludes runtime replay, independent Analyze/design-case clearance, job capture,
IPC, worker installation and painting. Installed interaction remains unqualified.

Explicit calculation additionally reports `last_case_computed_sections`,
`last_case_reused_sections`, `last_case_attempted_transitions`,
`last_case_cached_sections`, `last_case_cached_samples` and
`last_case_cache_admitted`. These describe the final selected-case preparation,
not all election or engineering work. Admission is numeric `1` or `0`, preserving
the metrics' numeric contract. Prepared unavailable rows count without
attempting a transition. One actual selected case is retained within native
16,384-section/262,144-sample logical bounds; overflow discards future reuse and
leaves current calculated results unchanged. Internal rows do not authorize
cached-only reads or selection. Observe preserves this work and the calculated
revision. Cache hits can retain earlier counters, and metrics never alter
history, query freshness or publication identity.

The ordinary-release selected-case gate on the same 121-support/120-section,
10,015-point fixture measured apply 12.362 ms and retained full-scene
calculation/JSON 37.217 ms versus 43.991 ms fresh. Two reference and case
sections prepare, 118 reuse, with 2,040 cached sampled points. All twelve outputs
match fresh bytes (8.27–8.67 MB). See
`ds-network/out/profile-native/selected-case-cost-release.json`. This includes
election, copying, uncached usage and layout/markers; runtime replay, separate
Analyze/design clearance, job capture, IPC, installation and painting remain
outside the measurement. Native runtime gates preserve no-solve Observe and
independent Save, full fresh scenes and stale async admission.

Terrain observations return `authored_delta.work.local_terrain_point_projections`
and `local_ground_rebuilds` without calculation. These count the receipt-local
path, not fresh preparations. Explicit calculation also exposes these as
`last_surface_local_terrain_point_projections` and
`last_surface_local_ground_rebuilds`, with the same last-preparation semantics.
The runtime gate checks A/B/A/B terrain edits, exact worker history, no scene or
analysis generation, unchanged calculated base, and native work-count transport.
## Cached native result queries

`ds-profile-runtime::Request::QueryResults` reads an admitted in-memory result
index. It does not project, screen, assemble demand, calculate or save. The
request carries scope/model, exact expected history and authored engineering
input root; `kind` is `usage`, `failures`, `negative_loading`,
`violating_sections`, `qualification`, `blockers` or `findings`. The optional
`source` defaults natively to `profile`; `model_analysis` reads a separate
admitted model-bound Analyze index. `findings` requires `model_analysis`.
Optional `entity_ids`
focuses up to 128 native identities. Empty means the full calculated corpus.
`limit` defaults to 50 and admits 1–200. Each encoded response is at most
256 KiB; native byte limits can end a page before its item limit.

Usage and qualification return the native screening row's typed facts with
counts for blockers, unevaluated checks and advisories; blocker messages are
queried separately. Failure uses the native fail status. Negative loading uses
the native uplift witness or negative minimum signed weight span and retains
`uplift_failure` separately: negative loading alone is not a failure. Counts
include all typed cached rows before the analysis report's 2,000-row cap.
The complete matching native usage envelope is retained; a missing envelope
refuses these four queries rather than reporting a false zero.

`map.profile.usage` contract 2 is the single-support binding of this same query
(kind usage, one entity, one item). Its `usage` member is the unchanged native
page, replacing the former selected `screening` receipt. Optional type choices
remain a separate read-only library projection. The transport captures project,
principal, client, model and revision; the native query also fences full history,
engineering input root, entity family, result identity and publication state.
Selection, retry and warning/result inspection do not enter a solver or Save.
Renderer selection state only suppresses obsolete delivery; it holds no LRU of
engineering results. Scene admission at equal authored history starts a fresh
read so a prior calculation receipt cannot remain current by a revision-only
cache key. Native JSON carries known failure with incomplete qualification and
signed loading separately; the presenter does not label every result an extreme
envelope. Query block messages separately from row evidence counts. The owning
native refusal tests are in `result_queries.rs`; CLI declaration and Desktop
transport/selection tests cover the matching contract and stale capture paths.

With the default Profile source, violating sections are distinct native section
IDs with proven structure capacity contributions, displayed-case Profile clearance failure or native
section-state failure. Contributors are counted before their 50-ID display
cap. This is Profile clearance, not the separate model-bound design clearance
report. Qualification describes structure usage screening. Blockers cover
Profile curve/projection/clearance and structure usage evidence, with explicit
basis metadata and evidence-record counts; they are not a complete query of
the default analysis's resistance checks or design-clearance point findings.
Messages are presented, never parsed for an engineering verdict. A message
retains its original character count and truncation flag after 1,024 characters.

The response names authored and computed revisions/roots separately. Equal
engineering inputs admit the cached facts at a later authored history. Native
forward receipts also permit focused reads outside affected structure/incident
section/alignment dependencies (`freshness: unchanged_dependencies`). Global
reads across pending affected inputs, affected focused reads, unknown lineage
and unindexed displayed clearance cases refuse `profile_calculation_required`.
Whole-model invalidation stays conservative. Querying an unconfirmed publication
refuses; discard restores the prior index alongside its prior calculated view.

`next_cursor` pins the exact result identity, scope/model, history, source, kind,
canonical entity selection and offset. Recalculation at equal content creates
a new identity; old pages refuse. History ABA, redo-tail replacement, release,
foreign models and unknown entities are tested. `total` counts matching native
facts before paging; `truncated` means another page exists. `query_ms` includes
native bounded response construction and byte-limit verification, excluding
transport. The dense measurement test includes request DTO decode and final
JSON byte encoding separately. Owning tests live in `result_queries.rs`; the
engine cache's immutable exact-envelope accessor has its own negative controls.

The ordinary release dense-query gate retains 10,004 structure rows, one
three-support tension section and 10,001 unstrung supports. Median times for
50-item pages (including DTO decode and final JSON encoding) are 0.268 ms for
usage, 0.260 ms for qualification and 0.076 ms for blockers. Usage transfers
43,093 bytes; blockers transfer 12,298 bytes. Three proven failures take
0.023 ms and transfer 4,479 bytes; the negative-loading witness takes 0.012 ms
and transfers 2,731 bytes. The complete blocker corpus contains 40,008 evidence
records before paging. The receipt is
`ds-network/out/profile-native/result-query-cost-release.json`; the prior debug
receipt uses the same row count with an unpinned capacity fixture and is not a
failure-performance comparison. These are source/headless query measurements,
excluding IPC, renderer, calculation/index construction and installed artifacts.


## Complete cached model-bound Analyze queries

Explicit Analyze admits the paired engine `DefaultAnalysisFacts`, before report
caps, into an immutable native index beside the Profile display index. It never
reconstructs facts from capped analysis JSON. `source: model_analysis` uses that
index's computed revision/root, actual native default criterion/cases/basis and
resistance/clearance classifications. The response declares its source and has
`display_case: null`; changing or reading a prepared Profile case cannot narrow
or relabel these design cases. Native queries default to the Profile source for
compatibility. The existing bounded report bytes and engineering calculations
remain unchanged; queries do not call either analysis or issue calculation.

Model-analysis usage returns complete native structure screening rows with
bounded evidence counts. Qualification additionally returns each native support
resistance case check. Failures retain native structure fail rows and failed
resistance checks; negative loading retains signed section weight-span witnesses
and complete net support uplift separately. Missing resistance-input lists are
represented by counts in check facts and by bounded native messages in blockers.
Counts are native evidence facts, not distinct structures; an incomplete check
never becomes a pass or zero-valued usage. Blockers also retain every unavailable
section demand, design-clearance section/case, warning and calculation refusal.
A missing/refused model envelope permits blocker inspection but refuses other
queries; missing clearance findings refuse `findings`, rather than false zero.

`findings` pages every native design `PointFinding`, its original typed case and
deficits, selectable point ID and native XYZ. Its entity index also accepts exact
reported alignment, section and incident-support references. Filtering occurs
before query paging, not after the legacy 2,000-row report cap. Distinct violating
sections aggregate native capacity-failure contributors and each failed axis's
actual governing wire clearance witnesses. Contributor identities are capped at
50 after complete counts. Top-level section/support references describe the
vertical wire; optional horizontal_span supplies independent horizontal refs.
The basis names the independent-axis scope and native checked-corridor
qualification. Missing horizontal refs cannot be inferred from the vertical wire.

Model-analysis results survive pure case selection, presentation and unchanged
Profile refresh. New explicit Analyze creates a new result identity, including
at equal engineering content. After authored engineering inputs change, model
analysis queries conservatively require Analyze. Profile-only calculation cannot
renew old model-analysis facts. Local preservation of unchanged model-analysis
facts across such calculation remains unfinished. Source/history/root/model and
entity-family fences, native bounds and publication confirm/discard apply to both
indexes. Job capture shares the index; admission swaps it transactionally with
the calculated held state, and discard restores the previous exact index.

Headless gates in `result_queries/analysis_queries.rs` prove complete native
parity beyond 2,000 point findings, independent case selection, cursor source
refusal, missing/ambiguous envelopes, no-query math/state mutation, signed bounds,
incomplete native failure, stale authored heads, async admit/discard and stale
worker rejection. CLI/MCP declaration and replacing findings read bindings are
separate owning slices. The ignored release cost gate includes wire DTO decode,
native result construction and final JSON byte encoding; it explicitly excludes
IPC/rendering/installed acceptance and a dense connected model.


The ordinary-release query gate on 2026-10-08 used four placements, three strung
supports, one tension section and 2,031 added design points. Twelve repetitions
measured native DTO decode/query/result encoding: 50 design findings took a
0.536 ms median and 71,715 bytes; 44 usage/resistance qualification facts took
0.166 ms and 25,503 bytes; other query medians ranged 0.019–0.086 ms. Repeated
explicit unchanged Analyze, including native result construction and final JSON
encoding, took 96.707 ms and returned 7,836,915 bytes. These are source release
measurements of this fixture, not local-edit or dense connected-model/installed
acceptance. Broad calculated-result transfer, independent default-analysis
invalidation and duplicate explicit clearance work remain measurable boundaries.
The owning receipt is `ds-network/out/profile-native/analysis-query-cost-release.json`.


CLI `map.profile.results` and `map.profile.issues` contract 2 declare source and
Findings via the owning closed QueryArguments DTO. The Issues binding transports
that query and displays a bounded native page, basis and typed refusals. Opening,
paging and focusing do not calculate. Focus verifies exact page history before
selection. The old TypeScript layer filter, thresholds, schema assertion and
worker cache installation were deleted. Explicit browser reconciliation remains
a separately named native calculation until its async owner is bound.

Source gates: 24 CLI Profile parser tests, 34 bridge-parity and six refusal
coverage tests; 124 focused Web query/transport/dock/reconciliation tests;
Web check zero errors and four pre-existing SpanCheck warnings. Staged CLI
`1da077b5` discovers both contract-2 commands and refuses old calculation JSON
and unknown sources. MCP discovers both typed tools and refuses undeclared
arguments; valid calls stop at the unpaired Desktop gate on this host. This
proves no paired nested-JSON call or installed native activation. Receipts are
`ds-cli/out/stabilization/profile-controls/analysis-query-*` and
`ds-web/out/profile-native/analysis-query-*`; staged CLI remains unqualified for
installation (adjacent bundle absent and release pins deferred).


Model-analysis Findings now retain the optional native `horizontal_span` with
its own section, both support refs, attachment IDs and plan span. The native
entity index includes these refs. ViolatingSections attributes each failed axis
only to its actual governing section; a point failing both axes in one section
contributes once. The basis declares
`design_clearance_section_scope:native_independent_vertical_and_horizontal_wires`.
Missing legacy horizontal refs remain unattributed and do not borrow vertical
refs. Model/context/history/root/cursor and bounded transfer fences are unchanged.

Headless runtime gates: 53 unit and 13 integration tests pass (three cost gates
explicitly ignored). Actual native report/index parity proves different governing
sections and incident-support indexing. Negative controls prove absent horizontal
refs do not infer a section and a horizontal-only failure does not use its
vertical reference; shared-axis counts remain distinct points. Owning logs are
`ds-network/out/profile-native/horizontal-governing-{span,query}-*.log`.
