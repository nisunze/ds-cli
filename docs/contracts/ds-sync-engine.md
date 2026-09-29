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
whose `sha256` or `replay_key` is already recorded (`already_recorded`), the
handoff is re-runnable, and `work/publish` restates one declaration for at most
three rounds. A cancelled row is never resurrected.

## 5. The stage a reader sees

`ds_command_kernel::report::PublicationStage` is the one vocabulary every
producer answers in — **sealed → queued → drained → acknowledged**, plus the
two terminal states `conflicted` and `refused`.

It is *derived*, never persisted. `sync_store::ArtifactState` is a SQLite
column and the report activity projection validates its five tokens, so
renaming it would be a data migration that strands every queue already on
disk. `publication_stage()` maps the tokens one-to-one
(`held → queued`, `uploading → drained`, `published → acknowledged`) and
refuses an unknown one rather than guessing.

`sealed` is the stage that had no reader at all, and its absence is what let
593 committed, digest-verified artifacts look exactly like reports that never
ran. It is a committed batch in the producer store with no queue row, and
`ds_sync_runtime::reports::census` counts it: `batches - queued`, with every
displaced batch named. Nothing about a publication is invisible any more —
`reports::inventory` still yields one row per room, because that is what a head
means, but the batches it displaces are reported instead of dropped.
