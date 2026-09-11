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
| `project`, `install_id` | The project being synced and the install doing it. Never an account. |
| `now_ms` | The host's monotonic high-water mark of server-observed time (edge contract §3). |
| `online` | Connectivity as the host sees it. |
| `trigger` | Why the host asks now: `startup`, `navigation`, `local_change`, `publish_completed`, `download_completed`, `reconnect`, `remote_push`, `manual`, `grant_opened`. A plan is never asked for because time passed. |
| `remote_read_at_ms` | When the held `remote` heads were last read from the record; absent when never. |
| `grant` | The work grant in hand (`work/open`), parsed not verified: `grant_id`, `install_id`, `project`, `engines[{name, release}]`, `expires_at_ms`. |
| `local[]` | What this machine holds: identity (`engine`, `operation`, `variant`), `sha256`, `size_bytes`, `produced_at_ms`, `base_revision` (the remote head revision the result was computed against, if any), `readable`, `engine_release`. |
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

**No polling.** Owner, 2026-09-11: "we cannot afford passive aggressive polling
when not needed — this should be triggered by dependencies and navigations, not
background hammering of Firestore." So the plan answers two more questions
per trigger and never runs on a clock:

| Output | Meaning |
|---|---|
| `refresh_remote{needed, reason}` | Whether the remote heads must be re-read for THIS trigger: `never_read` (no heads held yet), `trigger_reads_record` (startup, navigation, reconnect, manual), `record_changed` (a push from the messaging stream); never for a local change (`held_heads_suffice`), a completed publish (`receipt_carries_the_head`), a completed download or a freshly opened grant (`nothing_remote_changed`). |
| `wake` | `{kind: "event"}` — wait for the next trigger — or `{kind: "at", at_ms, reason: "grant_renewal"}`, the ONE timed wake: renew a valid grant `GRANT_RENEW_LEAD_MS` (10 min) before it expires while an upload is pending. The transport's own retry (`response_policy`) is the only other timer, and it is not this plan's. |

## 2. What hosts do

Desktop shell (native) and `ds` (native): list local artifacts from the local
stores; read remote heads through the native client; call `plan`; perform the
actions in order — `work/open` for a grant; resumable bytes through the native
transport host under `transfer` + `response_policy` (any size); `work/publish`
with attribution and `base_revisions` (the server re-authorizes live and checks
concurrency); downloads through the gateway read — then re-plan. The browser
holds no local compute artifacts: it shows remote heads and downloads.

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
