# The sync store — one durable queue and artifact store per execution context

**Status:** contract, 2026-09-11, under `ds-application-runtime.md` §1
("Durability and ownership"). The store, fences, leases and lifecycle of the sync gate campaign.

## 1. What it is

The durable state the sync gate needs between two runs of the host loop,
shaped by the kernel and persisted by the host of each execution context:

| Context | Persistence | Owner of the bytes |
|---|---|---|
| Linux/Windows desktop, server, CLI | SQLite through the portable crate `crates/ds-sync-store` (kernel dependency only; no engine) | the native host |
| Web | IndexedDB rows of the same shape, written by the browser adapter from kernel-returned rows | the browser host |
| Android/iOS | the same rows, checkpointed through the platform's storage | the app host (not yet verified) |

The kernel module `sync_store` owns the ROW SHAPES and every TRANSITION over
them as pure functions (`evaluate(&[u8])`, WASM `syncStore`): recording a
local artifact, recording the remote heads and when they were read, holding a
grant, applying an action receipt, taking and releasing a worker lease, and
fencing. Hosts perform the writes the transition returns and nothing else;
no host re-decides a state.

## 2. Rows

| Row | Key | Fields |
|---|---|---|
| `artifact` | scope + identity | `scope {project | personal}`, `engine`, `operation`, `variant`, `sha256`, `size_bytes`, `produced_at_ms`, `base_revision?` (output-head OCC identity), `input_base_fingerprint?` (source-input digest for a derived artifact), `engine_release`, `engine_build_manifest_sha256?`, `client_publish_id`, `acknowledged_work_id?` (exact native commit evidence), `bytes_locator` (host-opaque), `readable`, `state` (`held | uploading | published | conflict | refused`), `state_reason?`, `replay_key` (idempotency across attempts), `updated_at_ms` |
| `head` | scope + identity | `revision` (output-head OCC identity), `head_revision` (monotonic counter), `input_base_fingerprint?` (sealed source-input digest), `sha256`, `published_at_ms`, `read_at_ms` |
| `grant` | scope + `grant_id` | `install_id`, `engines[{name, release, manifest_sha256?}]`, `expires_at_ms`, `raw` (the signed document, opaque) |
| `receipt` | `receipt_id` | `action`, identity, `outcome`, `detail?`, `failure_code?` (exact authority code), `work_id?`, `client_publish_id?` (captured publication binding), `head_revision?`, `committed_bytes?`, `at_ms` — append-only evidence |
| `lease` | scope | `worker_id`, `taken_at_ms`, `expires_at_ms` |
| `fence` | context | `account`, `deployment` (lane + origin), `install_id` — every row is stored under one fence; a row from another fence is never executed |

## 3. Transitions (pure)

`Event` → `Transition { rows: [RowChange], effects: [Effect], refusals: [Refusal] }`:

- `LocalProduced { artifact }` → upsert `artifact` (`held`); the next plan is a `local_change` trigger (uploads never wait).
- `HeadsRead { heads, read_at_ms }` → replace the scope's `head` rows; `read_at_ms` becomes the plan's `remote_read_at_ms`.
- `GrantOpened { grant }` → upsert `grant`; superseded grants of the same scope are dropped.
- `ActionReceipt { receipt }` → a row-changing action binds the selected `client_publish_id` under its fence/scope/identity; a replaced row retains its state and only the attempt's receipt is appended. A cancelled row cannot be restated, but an exact `reclaimed` receipt may mark its bytes gone while preserving cancellation. `upload` committed additionally requires a nonempty `work_id` and matching explicit head digest, and records `artifact.acknowledged_work_id` with `artifact.state = published` and a `head` row from the receipt (the head is carried, never re-read); `conflict` ⇒ `artifact.state = conflict` with `state_reason`; `refused` ⇒ `refused` with the reason; `download` committed ⇒ `artifact` row for the downloaded head (`held`, readable); `reclaimed` ⇒ the row's bytes are gone (`readable = false`, `bytes_locator` cleared) and its verdict stays as evidence; `reclaim_failed` changes no row; every receipt appended.
- `LeaseTake { worker_id, now_ms, ttl_ms }` → granted iff no live lease for the scope, else `refused: lease_held{worker_id, expires_at_ms}`. `LeaseRenew`, `LeaseRelease`. A lease is the ONLY thing that lets a worker drain: two workers never execute the same work.
- `Logout` / `AccountSwitch { account }` → rows of another account are not visible; nothing is deleted (held bytes belong to the project); drains stop.
- `Cancel { identity }` → `held`/`uploading` ⇒ `refused: cancelled` with the receipt; committed work is never undone.

The native Store acquires its SQLite writer transaction before reading the
transition snapshot and receipt replay check; the same transaction persists
all changes. This fences a late publication A receipt against a concurrent
replacement or cancellation of publication B, including another connection.
Optional publication/acknowledgement fields default to absent for historical
rows and receipts. An identity-only old receipt never supplies a publication
ACK. A newly sealed row cannot supply its own acknowledgement; native committed
receipt/physical bytes and exact cloud per-output origin pins remain necessary
for the report inventory to report verified publication.

## 4. Fences and identity

Every row is stored under `{account, deployment, install_id}`. The plan is
asked per `{scope}` within one fence. Server hosts run under an explicit
device identity (`ds` headless sign-in); ambient credentials are never used.

## 5. Recovery

The store is the truth after UI closure, crash, offline, suspension or
restart: a host restarts by reading its rows, taking the lease, and asking the
plan with trigger `startup`. Transfers resume from the transport's own
persisted `TransferState`, kept beside the artifact row (`bytes_locator` +
`transfer_state`), so completed work is never restarted and never falsely
reported.

## 6. The store is the queue (2026-09-20)

`Held` is QUEUED. A producer seals a publication — bytes on disk, integrity
receipt — and records `LocalProduced` in the SAME acknowledged step
(`ds-sync-runtime::reports::seal`); if the store refuses or cannot be written,
the bytes are removed and the seal is not acknowledged. No file index is
enumerated to decide what is queued. `StoreHost::local` adopts a batch that
is on disk with no row exactly once (`adopted` receipt, legacy batches sealed
before this rule) and is otherwise a no-op over the producer.

Solar is the same rule on the same tables (`ds-sync-runtime::solar::seal`):
a completed prepared job's row is recorded at completion under the host's
local fence, with no session and no gateway. Its bytes are the job's `result`
column in this file, so the seal writes none. That seal is the ONLY way a
Solar row is born (owner, 2026-09-20: "the legacy jobs in solar are not good
— we should remove them"): a completed job the store holds no row for — one
completed before the seal existed, or one whose seal the store could not
record — is not a publication and is never adopted. A restart replays no
completion through the observer; the producer retires such a job's result
on its next observation (`Store::retire_job_result`, the job row stays as
evidence) and the job can be run again. The report side keeps its adoption
(`adopted:from_producer`, once) only for batches committed before the report
seal wrote rows — every Desktop export until 2026-09-20 and every `ds report
project export` before its seal — and a batch sealed today is never adopted.
The producer, completion observer, publication metadata and bounded job
projection are owned by `ds-sync-runtime::solar_producer`. The Server supplies
its captured `HostIdentity`, database path and completion wake; it delegates
sealing to that native observer and uses the native producer in its one producer
set. Neither observer nor producer depends on a session, browser or Tauri.
Reclaim clears derived result bytes through the compute store while retaining
the job's sealed input, context and producing provenance. Server publication uses the common native executor and verdict mapping;
credential refusal holds either engine until an accepted session resumes it.
Both seals share one choreography
(`seal::record`): the identity's previous result on this machine is freed
through the producer first (`reclaimed:superseded_locally`; for Solar,
`Store::retire_job_result`), then the new row takes the identity. A second
job of the same city with the SAME bytes is `already_recorded`: the standing
row stands and the duplicate result's retention is the owner's ruling
(`ds-cli-server/docs-routes.md` §6), not the seal's. The city's row is its
NEWEST completion whatever order completions reach the seal in: two workers
can complete two jobs of one city out of order, and a completion sealed
behind a later one of its city does not take the identity — its own result
is freed (`Superseded`, `reclaimed:superseded_locally` with no identity on
the receipt, so the standing row keeps its bytes). A completion whose result
is already gone seals nothing.

A host runs ONE producer set over every engine it runs
(`ds-sync-runtime::Producers`, keyed by the engine a row's identity names):
the producer asked to adopt, move or free a row is always the one whose
engine produced it. Before this rule the Server's report pass freed a lost
Solar row through the report producer, which answered 0 for a batch it never
held, and the store cleared the row's locator while the result stayed in the
compute store — the desktop, built with a report producer only, did the same
to every Solar row. A lost row of an engine this host has no producer for is
`reclaim_failed` with the engine named, and keeps naming its bytes: no
reclaim is silent, and no producer says another engine's bytes are gone.

`sync_store::queue_status(rows, leases, receipts, project?, now_ms)` is the queue's
reading, pure over one fence's rows, leases and newest-first delivery receipts:
queued = `held | uploading`
(count, bytes, oldest `produced_at_ms`, rooms, distinct hold reasons),
published copies = `published`, reclaimable = `conflict | refused` whose bytes
are still here (`readable` and a `bytes_locator`), per project. Liveness is
the store's lease: a live lease on a project is its pump. `stuck` = queued
work older than `ABANDONED_AFTER_MS` (60 min) on a project with no live
lease. A project with queued rows whose newest upload or session delivery
receipt is `credential_refused` has an output-only `blocked` verdict
(`cause`, `sinceMs`, `detail`); `stuck` is true and `next` names the sign-in
remedy. A newer successful delivery receipt clears that verdict. This does not
add a persisted row field or change an artifact's five states. A refused
credential leaves all held bytes readable and stops the pass after one refused
call. The bytes directory's file lock
(`.publication.lock`) serialises writers of that directory and is not a
liveness signal. `Store::queue(fence, project?, now_ms)` reads it;
`Store::open_read_only(path)` opens an existing store for that read with no
session, no schema write and no lease.

## 7. Power loss

The store runs `journal_mode=WAL` with `synchronous=FULL`: every acknowledged
transition — a seal's row, a commit receipt, a lease — is fsynced before the
caller is told. The trade is one fsync per transition against the loss of an
acknowledged seal in a power cut (owner, 2026-09-20). `ds-design-workspace`
already runs FULL without WAL and is outside this rule; the Solar compute
table shares this file and this pragma.

## 8. Survey media records (2026-09-20)

The same file holds the Server's survey photo cache index: one
`survey_media` row per `(fence, project, object path)`, shaped by
`ds_command_kernel::survey_moments::MediaRecord`, beside the bytes under
`<state>/survey-media/<project>/<media_key(path)>/` (`original.bin`,
`thumbnail.jpeg`, `manifest.json` — exactly a `survey photo rotate`
bundle, digest-pinned by the record). The table is additive (`CREATE TABLE
IF NOT EXISTS`); a store written before it holds no media and answers
empty, never an error; `SCHEMA_VERSION` does not move.

A record is `waiting` when `ds survey photo rotate` prepared it (the
rotated original and its regenerated thumbnail, pinned to the source
generation it was derived from) and `synced` once `ds survey photo publish`
replaced the head with those exact bytes (the published generations are
recorded). `waiting` is retained — it is this machine's only copy;
`synced` is cleanable — it equals the head. Reads (`ds survey moments
list|read`, `ds workstation local-data status`) go through
`Store::open_read_only` like the report queue: no session, no gateway, no
running Server, every fence of the one owner's file. Writes go under the
native fence. The decisions over the records — which are this project's
originals, filters, order, page, the rotation gate — are
`survey_moments`'s and `local_data`'s; the store persists.

What is NOT here, said rather than discovered: the Server acquires a
survey photo only by rotating it (the one headless acquisition that
existed); the browser's read-through display cache and its capture outbox
stay in the browser (they are its IO); entry facts (capture clock,
location) are `None` on a Server record until the Server holds survey
entries. Whether the Server should pull a project's photos on demand, or
the window should hand its read-through copies to the Server, is an owner
ruling this contract does not pre-empt.

## 9. Retained native publication outputs

Completed native batches can retain a closed producer receipt and derived output
BLOBs in additive `publication_payloads` / `publication_outputs` tables in this
same file. They are fenced by account, deployment, installation, project and
replay key. They hold bytes, not queue states: `artifacts` remains the sole queue.
`Store::seal_publication_payload` runs the existing kernel `LocalProduced`
transition and inserts the immutable receipt and outputs in one FULL SQLite
transaction. A refused transition stores no payload; a failed output write rolls
back the row and receipt. Replay cannot replace its receipt or bytes.

`ds-sync-runtime::retained_publication` validates the common closed declaration,
exact output digests, byte counts and inventory before this atomic seal, without
a gateway or sign-in refresh. Its producer offers no inventory adoption and
checks the retained closure and byte digest before upload or a local download.
The same functions serve every engine. `SolarProducer` dispatches only on storage
locator: prepared jobs retain their result column, native derived publications
retain their payload BLOBs. Both run the same sync decisions and transfer owner.
Retirement deletes only derived output BLOBs and keeps the immutable receipt as
evidence. Authored workspaces, copied seeds and inputs are independent of this
payload store. Old stores answer an empty retained inventory without migration
or relabeling of legacy results; `SCHEMA_VERSION` stays at 2.

Every engine uses `ArtifactRow::matches_publication` to identify a duplicate:
equal inventory bytes, source-input fingerprint, producing release and build
manifest. A rerun whose source or producer changed is a fresh publication even
when its output bytes happen to match; it retains the new provenance and asks
Brain for the new verdict. The engine table declares explicit preview refreshes
and the additive legacy source-in-base projection. Neither the transition nor
Solar's completion seal has a digest-only exception.

Native gateway receipts preserve the authority's exact optional `failure_code`
independently of display text, through open/finalize refusals and credential or
fetch failures. Runtime results expose the same value as `failureCode`; the
SQLite receipt uses the kernel's `failure_code` spelling. Omitted codes on old
receipts remain absent. This additive evidence changes neither verdict mapping
nor publication identity, and does not authorize retry or reopen a terminal row.

## Explicit native recovery

`sync_store::recovery::Control` is the closed public JSON request: schema
`ds.sync-recovery/v1`, explicit `project`, and `command` (`status`, `retry`,
`sanitize_preview`, or `sanitize_apply`). Hosts inject their captured owner,
deployment and installation fence and read durable facts; callers cannot supply
a fence, snapshot, producer label or local path. The pure owner returns the same
path-free result on every host. Read-only status/preview neither create a store
nor migrate it. Apply reads and persists in one IMMEDIATE SQLite transaction.

Retry preserves publication id, replay key, output inventory, source fingerprint,
producer release, exact manifest and immutable payload. A complete readable row
with an exact typed `ENGINE_BUILD_NOT_ADMITTED` refusal may be retried explicitly.
The historical exact typed storage-comparison conflict is also recoverable.
Unregistered engines, incomplete provenance, prose-only errors, cancellation,
stale source, integrity and permission failures are refused. Recovery uses the
engine table's release identity data and never overrides Brain's next verdict.
The common runtime retains recoverable bytes while leaving their row terminal;
restart or newly admitted builds never reopen it implicitly. The queue counts
these bytes as held, not reclaimable. A live project lease refuses mutation.

Sanitation hashes the complete fenced artifact history, standing identities,
latest bound publication attempts and lease, independent of display truncation.
Any changed snapshot refuses its earlier digest. Published history and terminal
rows fully superseded by a readable standing output inventory may be archived;
partial inventories cannot archive unselected outputs. Active publications and
unresolved current failures remain. Explicit archival sets an additive
`artifact.archived` flag; it deletes no file, payload, input, head or provenance.
Archived rows remain inspectable and are never readopted as legacy producer work.

Recovery uses the explicit `execution_context::SYNC_RECOVERY` operation. The
headless Server's owner-only local transport binds it without opening a gateway
session. A committed change wakes the existing common publication pump; the pump
rechecks authority and bytes through its ordinary publication sequence.
