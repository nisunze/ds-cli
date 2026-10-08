# The sync engine — one decision, one gate

**Status:** contract, 2026-09-11. Owner rulings (verbatim) it implements:
"all of the sync center in the kernel"; "the root fix is simply removing
TypeScript out of the way and we use and prove resumable uploads on weak
networks"; "no fallback, no TypeScript in the syncing engine — one function:
should I upload, should I download"; "the desktop should not make the data
personal; it is just project data"; "ds-brain has to check and give
credentials — and then the desktop uses that credential to do the uploads in a
resumable way regardless of the size. The Sync Center should be the only gate
that everyone that communicates to the cloud uses … it checks remote conflict,
freshness and might poll to pull the fresh things."

## 1. The one function

`ds_command_kernel::sync::plan(&Input) -> Plan` (`sync::evaluate(&[u8])`,
WASM `syncPlan`). Pure: no IO, no clock, no randomness. Input is what the host
observed; output is the ordered list of actions the host performs, and when to
plan again.

| Input | Meaning |
|---|---|
| `project`, `personal`, `install_id` | The project being synced and the install doing it — or `personal: true` with an empty project for the signed-in account's own bytes (user data, personal notes, personal media), which go through the same gate under their own scope: "project bytes and even personal bytes of course" (owner). Never an account as the key of a project row. |
| `now_ms` | The host's monotonic high-water mark of server-observed time (edge contract §3). |
| `online` | Connectivity as the host sees it. |
| `trigger` | Why the host asks now: `startup`, `navigation`, `local_change`, `publish_completed`, `download_completed`, `reconnect`, `remote_push`, `manual`, `grant_opened`. A plan is never asked for because time passed. |
| `remote_read_at_ms` | When the held `remote` heads were last read from the record; absent when never. |
| `grant` | The work grant in hand (`work/open`), parsed not verified: `grant_id`, `install_id`, `project`, `engines[{name, release}]`, `expires_at_ms`. |
| `local[]` | What this machine holds: identity (`engine`, `operation`, `variant`), `sha256`, `size_bytes`, `produced_at_ms`, `base_revision` (the remote head revision the result was computed against, if any), `readable`, `engine_release`. |
| `source_grants[]` | Resource-scoped grants with `snapshot_base`, used for derived reports whose `input_base_fingerprint` pins source inputs separately from output heads. |
| `remote[]` | What the shared record holds: identity, `revision`, `sha256`, `published_at_ms`. |

| Action | When |
|---|---|
| `offline` | Not online. Nothing else is planned; re-plan on reconnect or after `POLL_OFFLINE_MS`. |
| `open_grant{reason}` | An upload is needed and the grant is absent, expired, for another project or install, or does not cover the artifact's engine release. Always first; the host calls `work/open` and re-plans. |
| `upload{identity, sha256, size_bytes, reason}` | Local exists and remote does not (`absent_remotely`); or both exist, digests differ, and the local result was computed on the current head (`local_newer`). |
| `download{identity, revision, sha256, reason}` | Remote exists and local does not (`absent_locally`); or the local bytes are unreadable (`local_unreadable`). Downloads need no grant: expiry degrades to read-only. |
| `conflict{identity, base_revision, current_revision, reason}` | Both exist, digests differ, and the local result was computed on an older base — or on no base at all (`base_moved`). Surfaced, never overwritten. |
| `nothing{identity, reason}` | Digests equal (`in_sync`). |
| `refused{identity?, reason}` | A row that takes no part: `identity_malformed`, `duplicate_identity`, `local_unreadable` with no remote copy. One bad row never poisons the queue. |

Derived reports compare their `input_base_fingerprint` with the operation's
source revision in a valid work grant, not with the input fingerprint of an
older published output. Missing grants, or held grants that disagree with newly
computed inputs, require fresh admission. After `grant_opened`, a mismatch is
`source_base_moved` and moves no bytes. The host re-plans after admission and
before uploading. The publish service still checks that the source remains
current transactionally. Ordinary artifacts retain the output-head
`base_revision` conflict rule.

Solar city input bases use `solar_snapshot::input_base::{encode,fingerprint}`:
Brain-compatible sorted UTF-8 objects, exact integers, binary64 floats including
negative zero, and HTML-safe strings over the four governed sections (config's
path manifest only). New kernel snapshots use that encoding for the body too;
historical browser and authenticated Brain snapshot bytes remain readable.
`tests/fixtures/solar-input-base-vectors.json` is the shared Rust/Go authority
corpus, with the copy and numeric typing rules beside it. Report bases name exact
verified calculation report-input bytes; portfolio bases retain membership order.
Client snapshot transport hashes continue to name the exact submitted bytes.

**No polling.** Owner, 2026-09-11: "we cannot afford passive aggressive polling
when not needed — this should be triggered by dependencies and navigations, not
background hammering of Firestore." So the plan answers two more questions
per trigger and never runs on a clock:

| Output | Meaning |
|---|---|
| `refresh_remote{needed, reason}` | Whether the remote heads must be re-read for THIS trigger: `never_read` (no heads held yet), `trigger_reads_record` (startup, navigation, reconnect, manual), `record_changed` (a push from the messaging stream); never for a local change (`held_heads_suffice`), a completed publish (`receipt_carries_the_head`), a completed download or a freshly opened grant (`nothing_remote_changed`). |
| `wake` | `{kind: "event"}` — wait for the next trigger — or `{kind: "at", at_ms, reason: "grant_renewal"}`, the ONE timed wake: renew a valid grant `GRANT_RENEW_LEAD_MS` (10 min) before it expires while an upload is pending. The transport's own retry (`response_policy`) is the only other timer, and it is not this plan's. |

**Uploads never wait.** Owner, 2026-09-11: "uploads are a must, as fast as you
can, but always in the background and offline first." A finished local artifact
is a `local_change` trigger: the plan lists its upload at once (no read of the
record first), the host runs it in the background under the grant, and if the
link is down the artifact is held locally and the reconnect event — an OS
signal, never a poll — drains it. Resume is the transport's own; it is not a
reason to wait either.

## 2. What hosts do

Desktop shell (native) and `ds` (native): list local artifacts from the local
stores; read remote heads through the native client; call `plan`; perform the
actions in order — `work/open` for a grant; resumable bytes through the native
transport host under `transfer` + `response_policy` (any size); `work/publish`
with attribution and `base_revisions` (the server re-authorizes live and checks
concurrency); downloads through the gateway read — then re-plan. The browser
holds no local compute artifacts: it shows remote heads and downloads.

The Desktop sync door sends the signed-in webview's validated `x-api-key`
alongside its bearer. An empty key is allowed only for a local stack that does
not require one. A 401 from the gateway or the record is a credential blocker:
the pass records `credential_refused`, stops after that first refused call, and
keeps every sealed artifact readable in its held queue. It never settles a
publication or retries the other rows under the same refused credential in
that pass. A later pass with a working credential records a newer delivery
receipt and clears the blocker. A 403 remains the record's publication verdict.

While that project's fenced queue is credential-blocked, a `local_change`
pass makes no gateway call and appends no durable retry receipt. Its returned
`session:blocked` receipt says why it did no work. `startup`, `navigation`,
`reconnect` and `manual` still try the gateway. A successful heads read after
a block records `session:connected`, clearing the old blocker even if this
pass produces no upload receipt and queued rows remain; a fresh 401 blocks it
again. Another project's or account fence's queue is unaffected.

The Sync Center renders the plan and the receipts of executed actions. It
computes nothing.

## 3. Hard rules

1. **No second decider.** No TypeScript, no host code, no server code re-derives
   upload/download/conflict/freshness. `compute_artifact_engine` decides how a
   publication proceeds; `transfer` decides how bytes move; nothing else decides.
2. **Project data, not personal.** No account identity keys a sync row; the
   grant is bound to install + project; membership is the server's grant.
3. **No fallback.** One transport path per host. Unavailable transport is a
   typed refusal, never a different door.
4. **Credential first, admission once.** Engine admissibility is decided when
   `work/open` issues the grant, from the governed release registry; per-row
   checks are claim checks against that grant. (Server side: lane
   `sync-delivery-root`; `ds-brain/docs/contracts/edge-admission-and-publish.md`.)
5. **Dangling data is ignored.** The retired per-user outbox is not an input and
   is not migrated.
6. **One gate for every project byte.** Compute artifacts, survey photos and
   entry attachments, and project assets are all artifacts to this plan
   (engines such as `network_reporter`, `solar`, `survey_media`,
   `project_assets`), uploaded under the same grant through the same resumable
   transport, with `transfer` + `response_policy` owning every retry. No
   per-outbox engine, budget or retry policy exists beside it. Owner,
   2026-09-11: "confirm that even survey photos and attachments will use the
   same thing." State on that day: project assets already move through the one
   transport but decide what to upload on their own; survey photos moved that
   day onto ds-brain's governed resumable route through a separate outbox with
   its own ten-attempt budget (`survey_photo_outbox.rs`). Both fold under this
   plan next; until then they are the named exceptions, not the design.

## 4. Ordering: which of two disagreeing results is current

A report is a *derived* artifact: a pure function of the source inputs pinned
in `input_base_fingerprint`, not an authored document with a revision line. It
therefore has no base revision to compare, and two machines never merge one.
`sync::derived_winner` is the whole rule:

| local vs head | decision | action |
|---|---|---|
| same `sha256` | the same report | `Nothing { in_sync }` |
| different source revisions | the source authority decides, not a clock | the existing admission branch (source grant / `source_base_moved`) |
| same source revision, local produced later | the later computation is the report of record | `Upload` |
| same source revision, head published later or at the same instant | the shared copy already carries a later computation | `Nothing { superseded_by_head }` |
| same source revision, local production time unknown | not guessed | `Conflict { production_time_unknown }` |

**Last-writer-wins, and only between renderings of the same source revision.**
The clock is the weakest evidence available, and there are two of them — a
directory mtime here and the server's `published_at_ms` there. Where the two
sides declare different source revisions there is better evidence, and it is
already used: the grant pins the current source revision and a result computed
against a revision that has moved is refused admission. Deciding *those* by
timestamp would let a device with a fast clock publish a report of an old room
over a report of the current one.

A tie goes to the head, so the outcome cannot depend on which machine drained
first. `superseded_by_head` is recorded as a persisted receipt rather than a
silent no-op: the operator's computation lost, and they are told which batch
and why. It is not an error — the local bytes may be discarded.

**Replay is idempotent** and always was: `Event::LocalProduced` refuses a row
whose `sha256` or `replay_key` is already recorded (`already_recorded`), and
`work/publish` restates one declaration for at most
three rounds. A cancelled row is never resurrected.

## 5. The stage a reader sees

`ds_command_kernel::report::PublicationStage` is the one vocabulary every
producer answers in — **sealed → queued → drained → acknowledged**, plus the
two terminal states `conflicted` and `refused`.

It is *derived*, never persisted. `sync_store::ArtifactState` is a SQLite
column and the report activity projection validates its five tokens, so
renaming it would be a data migration that strands every queue already on
disk. The activity projection maps the tokens one-to-one
(`held → queued`, `uploading → drained`, `published → acknowledged`) and
refuses an unknown one rather than guessing.

`sealed` is the stage that had no reader at all, and its absence is what let
593 committed, digest-verified artifacts look exactly like reports that never
ran. It is a committed batch in the producer store with no queue row, and
`ds_sync_runtime::reports::census` counts it: `batches - queued`, with every
displaced batch named. Nothing about a publication is invisible any more —
`reports::inventory` still yields one row per room, because that is what a head
means, but the batches it displaces are reported instead of dropped.

## 6. Where each publication rule lives

**Owner clarification, 2026-10-08:** Solar outputs and Network Reporter
artifacts are conceptually the same: computations produce artifacts which must
sync through the same logic. Prefer downloading and verifying a valid shared
artifact, including one another user computed, over repeating that computation.
Preserve governed source data, authored parameters and provenance so recomputation
is available when needed; preserving them does not reduce artifact synchronization
or justify unnecessary compute. Reclaiming derived bytes never authorizes deleting
or rewriting their source inputs.

Hybrid placement never makes calculation depend on publication admission.
Retained prepared inputs and their declared cached dependencies can compute
while Brain is unreachable. Completion seals the local queue without a gateway
session; verified local results remain available for preview and download.
A reconnect drains that same queue. The shared runtime proves this for both
engines across a crash/restart, with zero gateway reads, opens or transfers
while offline. Credential expiry holds publication, rather than deleting its
bytes or requiring calculation to repeat. Acquiring a missing governed input
still requires its declared authority; offline execution never invents it.

`sync::publication_verdict` is the pure record-answer interpretation. HTTP 401
holds the credential, 403 refuses the row, 409 at open conflicts and 409 at
finalize stores stale, 400 at open refuses, and finalize 400, offline, incomplete
transfer and 5xx hold. Successful `published` and `stored_stale` answers are
preserved. A transport refusal takes precedence over a response's state. The
native executor uses this function both for its receipt and for determining
whether the receipt settled the row. All native callers use the common publication sequence and verdict mapping.
A 401 holds Solar exactly like a report; a successful signed-in retry resumes
the same durable replay key. Desktop producer coverage remains a separate boundary.

`sync::engines` holds the native source-fence and dependency policy. Only the
two exact reporter aggregate operations are source-fence-free; individual
exports and Solar publications require source inputs. An unavailable engine
policy holds its bytes. A Solar report waits on the published calculation's
exact `report-input` output hash in this project's observed heads. Missing,
wrong-city, mismatched or ambiguous declarations answer
`nothing:waiting_on_dependency`, counted separately from `in_sync`, with an
event wake. A counter or inventory hash cannot satisfy that dependency.

The common native upload executor now accepts either engine and calls that
preflight. Source tests prove common open/transfer/finalize execution and
verified remote-copy reuse for both. Native hosts now call that executor without engine scoping. Server calculations
use it; complete Desktop and derived-output coverage must be proved separately.

`sync::declaration::validate` owns the closed publication request before native
credentials attach: selected project, replay identity, registered engine,
source fingerprint, unique output identities, allowed output metadata and
bounded bytes. The engine table selects the declaration policy. A Solar
calculation requires its exact snapshot digest and UUID receipt; derived
artifacts cannot carry calculation snapshot fields. Report presentation uses
the existing artifact inventory owner. The native client's `publication` entry
accepts either engine through this validator; there is no report-scoped entry or typed Solar publication adapter.
The Server uses the common executor over its combined producer set.

The Solar migration keeps one decision family (`sync`, `sync_store`, `transfer`)
and one native executor (`ds-sync-runtime`). The following inventory distinguishes
the current owners from the required destination; it does not claim cutover is
complete. Engine authority is data in one table on each side. Producer adapters
supply bytes and declarations, never verdicts. ds-brain rechecks every local
preflight and its answer settles the row through `sync_store::ActionReceipt`.

| Rule | Current owner / observable behavior | Required home |
| --- | --- | --- |
| Matching owner, project, engine reader; nonempty unique inventory and bounded input | `compute_artifact_engine::validate_row`, `ComputeArtifactDrainPlan::new`; browser rejects malformed/cross-account rows | `sync_store` fence and inventory validation; common engine table |
| Archived rows never enter a drain | `compute_artifact_engine::validate_row`, queue `next` | `sync_store` retained-history projection |
| Exact producing release, build digest, reporter batch and publish id | engine `start`; `artifact-sync.ts` enqueue validates native receipts | Common declaration validation, with release validators in the engine table; retain producer provenance |
| Explicit recovery for terminal rows | engine `start` accepts only pending/syncing | `sync_store` retry transition; conflicts and stored-stale are not automatic retry |
| One lease and attempt count, clock bounds, renewal and expiry | engine `apply` persists browser worker lease before each IO effect | Native project lease and `sync_store` receipts; preserve installation/account/project fences |
| Persist before IO, compare-and-swap, identity changes and cancellation stop the attempt | `artifact-sync-row.ts`, `artifact-sync-kernel.ts`, browser storage transition; native `StoreHost` active/cancel fences | `sync_store` transaction plus native host guard before each effect and receipt |
| Prepare final bytes; digest/size must match receipt, nonzero bounded bytes | engine `transition::Bytes`, browser engine-owned locator readers | Producer verified reads; common inventory validator and `transfer` integrity checks |
| Closed output inventory, generated reporter filenames, declaration field whitelist | engine `opening`; native `rows::{open_declaration,finalize_declaration}`; Solar auth builder | One native builder from row plus `compute_artifact_inventory`; engine-table declaration data |
| Open work id/head and output target inventory must match; duplicate/undeclared targets refused | engine `Opened`, native gateway decoder | Native decoder plus common publication sequence; no engine-specific target parser |
| Restart reuses run id, work id and bytes; one expired-session reopen budget | engine `next`, `Probed`; native uploader reopens once | Native common sequence and `transfer` session facts |
| Probe keeps session when progress is inconclusive; bounded safe cause messages | engine `transition::describe_probe_cause`; browser transfer port | `transfer` typed causes, native receipt rendering |
| Committed offset cannot exceed declared length; verify local bytes before resume | engine `Bytes` and `Probed`; `artifact-transfer.ts` | `transfer` plus native producer verified stream |
| Finalize only after all outputs commit; response work id/head must match | engine `Uploaded` / `Finalized`; native `run::upload` | One native publication sequence and decoder |
| Published advances held head; stored-stale preserves server verdict; unknown state is held | engine `Finalized`; native `run::upload` / `settled_verdict` | Pure `sync::publication_verdict`, `sync_store::ActionReceipt` |
| Integrity conflict; terminal/authority/API/transport failure classes; redacted bounded messages and failure code | engine `failure`; `artifact-sync-row.ts` transport facts | Common native verdict function and safe typed receipts |
| 401 pauses entire project pass, 403 refuses, 400 open refuses, finalize byte visibility failure holds, offline/5xx hold | Native `run::record_refusal`; Solar `classify_solar_error` currently terminally blocks 401 and finalize 400 | Common verdict function for every engine; working sign-in clears credential hold |
| Same project/run report waits for calculation; failed pass never spins | `compute_artifact_queue::{new,next,settle}`; dependency keyed by project/run/operation | `sync::plan` dependency facts and `waiting_on_dependency`; native pass receipts |
| Terminal old reporter row must not strand a newer inventory; transient failure fences stream | Queue `next` / `settle` | `sync` / `sync_store` publication selection and pass ordering |
| Retry backoff starts at 2 s, capped at 5 min; lease-busy wake; no autonomous retry of terminal result | engine `failure::retry_delay`, browser retry wakes | `transfer` / native wake projection; `sync_store` retry eligibility |
| Exact retained reporter build-admission or historical storage-comparison failure can retry; no fuzzy error matching | `compute_artifact_sync::assess` | `sync_store` assessment against native retained receipt, engine-table release validator |
| Sanitation sorts/validates snapshot, hashes owner/project/row fingerprints; apply must match preview digest | `compute_artifact_sanitation`, `artifact-sanitation.ts` | `sync_store` preview/apply under native fence and exact snapshot digest |
| Archive published or fully superseded failed inventory; partial replacement cannot archive unseen outputs | Sanitation latest-by-operation/variant/output rule | `sync_store` retained history; preserve provenance and locally owned bytes |
| Sanitation keeps active, unresolved, archived and other-workflow rows; retry only eligible retained failures | Sanitation decision ordering | `sync_store` native eligibility and history projection |
| Digest-pinned cache identity includes owner/origin/project/locator/digest/size; immutable reads need no TTL | `compute_artifact_cache::Identity`, `artifact-byte-cache.ts` | Native verified read/cache ownership; retain display download capability |
| Cache file 128 MiB, total 256 MiB, bounded unique inventory; oldest-first eviction | `compute_artifact_cache::Retain` | Native read-store retention under its own fence; never delete producer-owned bytes |
| Solar completes durably before sealing, no session needed; never adopt unsealed legacy Solar jobs | Server `SolarActivity::seal_completed`, `solar::seal`; `SolarProducer::retire_unsealed` | Shared native completion observer and producer; keep the no-adoption ruling |
| Solar job enumeration is project-fenced, paged, newest 512 with explicit truncation | Server `solar_jobs_in`, `publication`, `publication_metadata`, `sealed_jobs` | `ds-sync-runtime` Solar producer over `ds-compute-runtime` |
| Losing Solar bytes are retired from job result column; job and result digest remain evidence | Server `SolarProducer::reclaim` | Shared Solar producer reclaim; native store receipt |
| Reporter and Solar share one producer set and executor | Server `with_producers`, one project wake and `ds-sync-runtime::run`; auth forwards the common closed declaration | One native pass, one store transition and credential recovery |
| Startup, new sealed inventory, reconnect, kernel deadline trigger the report pump; idle scan is not a gateway trigger | Server `SyncWake`; Solar pump recovery thread | Shared native scheduler; host only forwards wake/trigger facts |
| Browser success/failure mirrors Solar result/report state; clear dirty only after all rows publish | `artifact-sync.ts::{reflectSolarResultSync,clearDirtyWhenFullyPublished}` | Native Solar application projection from durable store, rendered by UI |

### 6.1 Solar publication kinds and Server coverage

| Desktop queue kind | Current headless Server coverage | Server-first migration obligation |
| --- | --- | --- |
| `calculate-<city>` / `default`: exactly `report-input` JSON | Prepared jobs and captured native city completion seal through the common store/pass | Bind Desktop to the same captured native completion authority |
| `report-<city>` / language-specific APD draft plus chart PNGs | Captured native city completion retains closed draft/chart bytes through the common store/pass | Bind Desktop to the same completion; preserve calculation-digest dependency |
| `report-<city>` / language-specific interpreted final Markdown | `solar.final.submit` exists headlessly; common sync-store publication is **missing** | Forward the final through common seal/pass before Desktop cutover |
| `portfolio-<id>` / portfolio variant: `portfolio_result` JSON, optional draft | Governed v3 native completion retains immutable outputs through the common store/pass | Migrate legacy v2 city-batch aggregate and the Solar Project command workflow |
| Explicit city batch | Captured native completion seals successful calculation and draft constituents, with separate per-city publication errors | Bind Desktop to the same completion and remove its queue expansion |

Standalone finished PDF/DOCX uploads are manual report assets, not these
compute publications. `enqueueLocalComputePublication` is the only generic
enqueue; its closed inventory does not authorize an additional Solar kind.

### 6.2 Cloud and transfer consumers

Reporter cloud preparation and publication are
`ComputeArtifactsService::{PrepareNetworkReporterCloudPublication,PublishNetworkReporterCloudArtifacts}`,
called by `internal/proxy/ds_system.go`. That proxy file is inside ds-brain;
it does not authorize deploying the separate ds-system repository. Publication
calls service `Open`/`Finalize` in process, so admission must live in that service,
not only in the public handler.

`ds-solar/apps/ds-solar-cloud` accepts prepared calculations and returns bytes;
preparation/publication belong to ds-brain. It contains no compute-artifact
publisher. Hosted web Solar remains closed by the routing descriptor. Future
hosted publication must use the same service admission, without a new door.

`transfer-host.ts` also serves `api/uploadQueue.ts` (assets) and
`api/survey-media-upload.ts`; these consumers stay. `artifact-transfer.ts` also
serves `desktop/gateway-transfer.ts` and must be traced at cutover before removing
its non-publication transport exports. `api/computeArtifacts.ts` supplies
read/list/download to `solar/published-documents.ts` and engine types to
`assets/origins.ts`; retain display/download. Its open/finalize calls are used by
`artifact-sync-row.ts` only. Removing compute publication never removes the
operator's verified result views or downloads.

### Legacy Desktop queue cutover

Owner ruling, 2026-10-08: show every affected city, discard its legacy queued
publication, and require a fresh run. Remove the old queue in this release.
Never re-seal or adopt those legacy Solar outputs. Retain a visible cutover
notice with city identities before deleting the queue; governed inputs remain.
