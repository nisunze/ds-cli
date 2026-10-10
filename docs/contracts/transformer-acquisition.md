# Transformer acquisition

A consumer asks for one complete transformer room in an explicit project and
principal scope. The shared native planner (`pinned_context::plan` and
`room_is_fresh`) decides reuse from observed revisions and saved class counts.
Project Control uses the same rule. Status observations come from navigation,
explicit refresh or push; consuming a room never creates a polling loop or a
per-transformer head probe. Missing evidence is not a newer revision.

The Web/Desktop host owner is `ds-web/src/lib/design/transformer-acquisition.ts`.
Pinning, map opening, paired CLI design reads, room materialization, and individual
and combined report preparation use it. Complete rooms alone enter the canonical
store. A renderer can project a room in memory, but cannot cache that projection
as editable truth. A partial historical pin cache is not a canonical room.
Native saved-room execution lives in `ds-project-data::room_hold::acquire`.
Project exports and headless pinned previews share this owner and its complete
principal/project cache. It verifies saved-content digests before reuse, checks
observed revisions and class inventories through the same planner, shares
concurrent reads using a per-room OS file lock, and compares the captured room
under the room/index lock before publication. A late response cannot replace a
newer held room. It never substitutes a projected context for saved bytes.
Native pinned views read governed transformer nature once per requested set only
when an authored style consumes it. The kernel projects assigned values and clears
unassigned values in the view; the exact saved cache remains untouched. Missing
definitions preserve saved values. This metadata read never reacquires geometry.
A pin receipt distinguishes actual acquisition counts from hypothetical `--held`
planning counts; local retention failures remain explicit. Missing legacy
saved digests require refresh, and unavailable reads cannot bless stale bytes.
The other native headless readers and derivative-worker migration remain in [OPEN §16](../../backlog/OPEN.md#16-map-data-datasets-and-screen-styling).

A complete unchanged room needs no source download. Saved inventory includes all
classes, not only the transformer point; missing poles/custom classes and reduced
or removed classes require repair. The newest known revision wins over responses
arriving out of order. An older head cannot invalidate a newer held snapshot.
Dirty and offline-created rooms are preserved even under force. An explicit
local-discard intent instead captures the exact local room before acquisition;
it does not discard any edit that arrives later.

The host coalesces in-flight reads by principal, project, transformer and intent,
including overlapping single/batch reads and a consumer arriving during Refresh.
One caller's cancellation releases that caller, not other consumers. An account
transition permanently invalidates an old read, including A→B→A; a newly admitted
A request cannot revive the old flight. Per-call windows are bounded at 64 rooms,
with at most 128 active shared reads. Scheduling consumers retain their native
resident/request bounds; pinning uses windows of eight.

Before publication, the host rechecks the returned revision and class inventory
against the newest observed head without a network probe. A response overtaken
by a newer head refuses. Downloaded bytes then publish through a single IndexedDB transaction comparing the
captured room with the current room and checking current authority and any pending
save. A newer edit, save, analysis attachment, replacement, deletion or save marker
refuses the publication without overwriting local state. Storage failures stay
failures. A failed refresh keeps old bytes but cannot certify them current.
An offline consume can use an unchanged held room; a known stale or missing room
requires a connected refresh. Another account's dirty cache is never consumed or
replaced; a foreign clean cache requires this account's authenticated acquisition.

Authoritative save readback, conflict comparison and computed-result reads are
explicit cloud-snapshot intents through this owner. They do not reuse local
editable data or publish over it. Their hosts retain the existing permission,
project and exact-save/content checks. Ordinary consume never takes this route
merely to check freshness.

Derived-output reuse follows [report inventory's freshness contract](report-inventory.md#freshness-and-automatic-reuse).
A published output is reusable across authorized users only with complete relevant
input provenance and verified bytes. Room acquisition alone does not prove print
freshness, and reports never wait for tiling.

Owning proof: kernel pinned-context and Project Control tests; ds-web's
`transformer-acquisition.test.ts`, `transformer-acquisition-boundary.test.ts`,
room executor, report preparation, pinning and authored-style materialization tests.

## Measured headless acceptance

The locally built CLI on Gisagara `agahehe` returned six classes and 435
features, including 76 `lv_poles`, on both cold and warm acquisition (2026-10-10).
The cold invocation took 17.782 s and fetched one room. The next invocation took
7.853 s, reused that room and fetched zero room geometry. Both returned no
per-transformer failures. These are end-to-end connected CLI measurements,
including authenticated status/style reads; they are not browser cache latency
or a claim of zero total API calls. The proof command is `ds design pinned preview --project czgmdwth_gisagara
--transformer agahehe --lane canary --output json`, built from CLI `174a137c`
and kernel `65ad6b6`. Owning tests retain the acquisition assertions; installed Desktop map acceptance is separate.
