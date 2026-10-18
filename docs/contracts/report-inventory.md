# Report inventory and refresh

`report::inventory` projects `ds.report.inventory/v1` through the same native
and browser WASM `reportCommand` function. It performs no IO, authenticates no
principal and authorizes no generation, reuse, publication or deletion. Native
hosts supply facts after fencing every read to the captured owner, project,
transformer and variant. Foreign scoped facts refuse the complete projection.

Committed sets carry immutable batch, client publication/run, producing engine,
input fingerprint and output locator/digest/size/presentation identities. Their
order comes from the native committed catalog, current first. The existing
`current_output_indices` rule replaces only matching canonical outputs; retained
outputs stay in their original producing set. The projector never sorts dates
to select a generation. A single exactly matching native queue row identifies
the active generation; multiple candidates report ambiguity. Each output keeps
its own generating run and timestamp. A partial iteration never claims that
its latest run produced the entire surviving inventory.

Generation timestamps and cloud publication observations are different facts.
Legacy cloud status/URLs remain a separate observation. Their old timestamp,
success token or file presence cannot replace a local generation or establish
its publication. Cloud absence cannot delete committed local bytes. No receipt
or byte directory is a second publication queue; native sync state remains the
one queue.

A local output is `published` only when its committed batch/publication/input
and complete output pins match the native published row, the validated current
head declares identical output bytes and format/presentation, and that output's
origin names the exact acknowledged work, same input and producing engine/build.
The native upload receipt must bind its work ID to this client publication;
an identity-only historical receipt cannot supply that binding. Missing binding
remains unknown. Older heads without
per-output origins remain `acknowledgement_unverified`. Head declarations prove
record metadata at observation time, not a fresh download/rehash of cloud bytes.
Cloud-only outputs carry their own head/origin and never acquire a local run or
generation timestamp. A changed current input is explicitly `changed`/`stale`;
publication does not grant engineering or report freshness.

## Freshness and automatic reuse

Owner ruling, 2026-10-10: transformer acquisition and derived-output reuse use
one shared native freshness authority. Pinning, editing, printing, report export,
collision analysis and tiling consume its admitted snapshots. Hosts transport
facts and render decisions; they do not implement separate `isFresh` policies.
The unfinished migration is owned by [OPEN.md §16](../../backlog/OPEN.md#16-map-data-datasets-and-screen-styling).

Before deciding to reuse an output instead of computing it, that authority must
bind its producing provenance to the requested dependency closure: transformer
revision and content, saved analysis, project configuration, seeds and reference
data, styles, layout, extent and context layers, media, output format/options,
and the admitted engine/build. Only dependencies relevant to that output belong
in its closure; changing an unrelated transformer or output must not invalidate
it. The source snapshot remains captured through execution and publication;
an edit during acquisition or generation cannot silently bless stale work.

The existing v2 server input-base fingerprint does not by itself prove this
closure: it binds transformer/version, project sheets and country/reference.
Inventory `source_state=matched` therefore is not permission to skip rendering.
Historical missing provenance remains unknown. Name, date, report success,
head-level fingerprint and byte equality alone cannot establish current reuse.
Each surviving output keeps its own producing provenance, including when a head
combines several runs. Unknown or changed inputs refuse automatic current reuse
without hiding an otherwise authorized historical download.

Project publication is reusable across users and machines. The producer is
provenance, not an extra freshness dependency. Access must be authorized for the
requesting principal and exact project through the existing publication owner;
another account's private local cache is not project publication. Bind the
selected immutable output, verify its size/digest when acquiring bytes, and
recheck the captured authority before returning them. Revoked access, moved
identity or invalid bytes cannot be repaired by borrowing the producer's account
or returning an unverified local copy.

Navigation, explicit refresh and admitted push events supply observations;
there is no polling or per-consumer freshness probe. Shared acquisitions are
coordinated so a freshness check cannot race an in-flight download or overwrite
local edits. Intentional regenerate/Refresh remains a new run; opening an
existing retained preview preserves [its read-only contract](retained-printing-previews.md).
Report generation and artifact reuse never wait for tiling.

## Inventory observations

Read facts are `ready`, `held` (previous verified same-scope facts retained after
a refresh error), `unavailable`, or `not_observed`. Held/unavailable facts retain
their bounded error. Only all-ready reads establish complete observed inventory;
required missing outputs, mismatched native pins and changed source remain
explicit. A host must retain the previous projection during a transient read,
display held local files immediately, and refresh in the background with an
owner/project generation guard. The native IO binding is
`ds_report_artifacts::inventory::read_snapshot`, called by
the native host under its existing fence. It hashes
selected current bytes, checks immutable commit/output pins, and performs no
recovery, adoption, reclaim or remote call. A missing/corrupt current output
refuses the fresh read rather than resurrecting an older shadowed output.
Historical generation times and ACK work IDs remain null: directory mtime and
sync state transition times are not generation or acknowledgement authority.
The Desktop read and background presentation are consumers of this snapshot.

The native catalogue can retain only a subset of an older committed batch. Its
sync state applies to those outputs only after the full original stored output
declaration matches the committed receipt uniquely; unrelated, duplicate or
changed pins cannot disappear when that subset is projected. The current sync
store retains one artifact row per operation/variant, not publication history.
An older surviving output without a still-observed exact row stays `sealed`;
it does not inherit the newer row's queued or published state. A host may retain
previous exact observed facts, but a fresh store read cannot invent them.

The Server CLI/MCP `report.outbox.inventory` reads the same native snapshot,
using the existing protected account/deployment/install and explicitly named
project/transformer. Identity observation does not create state or locks,
change permissions, refresh a credential or read saved project selection.
The host uses the sync store's read-only exact-fence snapshot API, then the
shared committed catalogue and physical-byte reader; it rechecks the captured
principal before returning the projection. Missing install/identity refuses;
a missing sync store is `not_observed`. The physical receipt binds owner/project
while the install fence governs observed queue facts. This read does not touch
project publication, call a provider, pump, adopt or reclaim. It supplies no
fresh cloud observation or reconstructed ACK work ID.

Read-only store observation does not initialize a missing database or change
its schema, rows or leases. Normal SQLite WAL reads include current committed
WAL content and may create or maintain the exact `store.sqlite-wal` and
`store.sqlite-shm` coordination files. Read-only does not promise an unchanged
directory tree. Protected account/install state, primary database content,
existing WAL content and committed report bytes remain outside those
coordination effects; an immutable SQLite connection is not used to hide WAL.

Snapshots admit at most 4,096 committed sets, 4,096 native sync facts, 32 outputs
per set and 32 required IDs. Known renderer metadata and output presentation are
validated by their existing owners. Public `report.plan --action inventory`
accepts this supplied JSON snapshot without authority, with its existing 4 MiB
request-file bound. It does not claim to read the installed native store.

## Native status view and retained acquisition

`report.inventory` writes the final `ds.report.inventory/v1` view to a bounded
private file and returns its exact path, SHA-256, byte count and captured
owner/project/transformer/lane. It does not return an unbounded snapshot through
Desktop stdout. Its default read has no provider call, queue transition or
freshness probe. Explicit `--refresh` runs the existing project Sync pass and
includes its receipt; that pass may publish queued work and acquire remote
outputs. The Sync pass still reads its project heads exactly once. Only when
that pass publishes the selected transformer does this explicit refresh read
back that exact full declaration, through the existing publication authority,
and merge it transactionally without replacing unrelated heads or regressing
an already observed revision. A failed or blocked pass or targeted readback
cannot label previous cloud facts current.

The Sync store retains complete authenticated publication-head documents only
after the bounded cursor walk completes. These additive SQLite observations are
not queue rows. They retain per-output origins, format and presentation exactly;
a reduced Sync head row is never expanded into invented publication facts.
The report consumer validates its exact selected full declaration through the
published artifact owner before projecting it. Old stores with no observation
table are unobserved; reading them creates no table. Older compatible writers
ignore this additive observation table and remain the sole queue owner.

The native view adds `outputs[].retained`: null, `source=committed`, or
`source=sync_download` with the exact head revision. A downloaded replica requires
its existing Sync receipt and physical output hashes. This is byte availability,
not a local producing generation: an acquired cloud-only output keeps null
local-run, generation-time and committed-artifact fields.

`report.inventory.copy` explicitly selects one retained source and exact
output SHA/size; downloaded replicas also require their selected head revision.
It performs no network call and never switches sources. The existing producer
receipt gate opens committed files; the existing verified Sync reader opens
replicas. A missing, corrupt or moved source refuses. Copying rehashes the
stream into a fresh private destination and rechecks captured identity before
returning its receipt.

`report.publication.download` reauthorizes one exact project and selected
engine/operation/variant/output/revision/SHA/size, checks that the governed ticket
still declares that full head and output, and uses the existing verified-byte
transport. It never substitutes a newer revision, recalculates a report or
falls back to another local source. The retained A4 preview and this operation
share the same exact-ticket acquisition implementation. Both byte-copy commands
leave no acknowledged output on failed identity or integrity verification.
