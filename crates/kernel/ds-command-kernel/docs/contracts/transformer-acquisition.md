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

## Metadata observations, without consumer probes

`ds-project-data::metadata` keeps complete authenticated style/status observations
under principal, lane, credential audience and explicit project. Its native store
checks the scope and checksum, bounds bytes, and shares a cold read across
processes under a slot lock. An empty inventory is an observation too. Ordinary
headless pinning consumes these observations and reports their cache/network
source. `--force` refreshes metadata and rooms. Explicit status/style authoring
reads replace the observations; native saves, retirement and style mutations
invalidate the affected slots. Shared style changes invalidate every project
catalogue for that identity. Failed refreshes never certify old observations.
These local observations are last-known data, not membership grants or proof
that an unseen remote change did not happen. Writes, publication and remote
artifact acquisition retain their authenticated gateway admission.

The browser's style transport host is `styles/catalog-cache.ts`: printing,
previews and map exports reuse one admitted catalogue, including metadata already
returned by layer navigation. A successful publication invalidates its durable
account/backend epoch without starting another request. Explicit Refresh bypasses
reuse. Atomic epoch checks fence in-flight reads across windows; account changes
permanently supersede an old read. Returned documents are cloned so consumers
cannot mutate held bytes. The layer loader checks local style freshness and
signed-source age; a fresh panel remount makes no API request. Stale sources
revalidate only on a navigation, never a timer.

Browser status reads share in-flight requests and retain empty full inventories.
Keys include principal, Firebase project, gateway origin and explicit project;
legacy project-only entries are not adopted. Targeted subsets cannot establish
full inventory completeness. Printing's inventory consumes that observation
instead of forcing another whole-project status read. The room freshness owner
continues to compare existing head observations; it never probes from each room.

Request-count regressions live in `metadata` native tests and Web
`style-observation-cache.test.ts`, `design-api-routing.test.ts` and
`layers-loader.test.ts`: eight cold consumers share one read, warm consumers
perform zero style/status reads, and refresh/mutation/supersession controls prove
changed inputs cannot be silently served as fresh. Remaining consumer and
artifact-provenance migration is tracked in OPEN §16.

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

After metadata observation reuse, an isolated native cache on the same project
measured 17.169 s cold, then 0.820 s and 0.793 s warm. Cold used network status,
network styles and one room acquisition. Both warm receipts used cached status
and styles and fetched zero rooms. The complete merge receipts were identical:
six classes, 435 features, including 76 poles. This proves removal of those warm
metadata/source reads and an approximately 90% reduction against the earlier
7.853 s warm invocation; it is not a measured cloud-billing total. Request-count
unit tests guard the read reduction independently of wall-clock variability.
