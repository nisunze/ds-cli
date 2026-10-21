# Report generation and publication activity

The pure `report::activity` projection owns generation/publication counts,
per-transformer regeneration reasons and the no-progress assessment. Native
CLI and browser WASM consume this same owner. Hosts supply IO facts for one
explicit project; no saved active project can redirect the request. Foreign
project rows in the native store or receipt window refuse the entire projection.
This read-only command grants neither project access nor export authority.

Generation progresses queued → preparing → generating → generated or failed.
A valid durable local export increments generated immediately. Cloud publication
is separate and advances only from the native sync store for the exact
`client_publish_id` returned by that generation. Historical published files
never increment the current run's published count. Failures retain the name,
timestamps and error. Host callbacks record facts, not count or policy decisions.

The outbox summary describes supplied native sync-store publications, not every
retained batch directory. It includes pending batches/bytes, oldest pending time,
conflicts/refusals and latest issue. Upload/publish attempts are counted only in
the supplied receipt window, explicitly not lifetime totals. The UI must surface
inventory-read failures rather than show an empty healthy queue.

Current native export regenerates from current held input. Legacy cloud success
or file presence cannot claim native report reuse. Each row reports
revision_conflict, missing_outputs (including requested PDF IDs), or
local_regeneration. Required outputs come from the existing Rust format plan for
captured project settings and selection; the host must not infer paper IDs from
filenames. Hosted cloud freshness is unchanged.

Local rendering may reuse the exact previously admitted transformer-nature
projection and saved analysis held with the project inputs. These captures
retain the user, lane, credential audience and original sheets digest; analysis
also retains the saved room revision and content digest. Missing, altered or
crossed captures refuse. A service-confirmed stale or missing analysis removes
the retained copy. Publication still requires current governed tag reads and
its existing source and authority fences.

A configurable 30–900 second threshold (default 120) detects absence of generation
phase progress while work remains. Sync traffic cannot reset this clock. The
result identifies waiting active work, or the next queued transformer if no worker
has reported a phase. This is an observable no-progress signal, not a diagnosis
that a process died. Completed generation waiting for sync is not a renderer stall.

Snapshots are bounded to 500 rows, 5,000 artifacts, 5,000 receipts and 32 requested
outputs. Project IDs, transformer identities, publication IDs and error lengths
are validated. The CLI's existing bounded report-plan action adapts this owner;
the UI only displays the projection and executes transport reads.

## The publication queue's own reading and its lock

`report::activity` projects a snapshot a host supplies. The queue itself is
read separately, by `ds_report_artifacts::queue_status`, because the question
it answers — "is anything stuck on this machine?" — must be answerable with no
session, no project selection and no running Server. It reports queued batches
and bytes per project, the age of the oldest sealed batch, and the state of
the pending-upload lock. `ds report outbox status` renders exactly that
reading and derives nothing of its own.

The lock carries an owner record: pid, the boot it was taken in, the holder's
process start time, the executable name, and a heartbeat renewed while the
holder works. A pid alone is not evidence of
liveness, because pids are reused; a lock may be released automatically only
when the recorded holder is proved gone — an earlier boot, an exited process,
or a live pid whose start time differs from the record — AND its heartbeat is
older than `LOCK_STALE_AFTER_MS`. A holder that is live, or that this platform
cannot be asked about while its heartbeat is fresh, is never broken.

Every automatic release is appended to a bounded journal beside the marker and
surfaced by both commands. A silent reclaim is a defect: two writers inside
the crash-durable promotion section is the failure the lock exists to prevent,
so the evidence that a lock was broken must outlive the process that broke it.

`stuck` is true only for a lock nobody can take whose holder is neither
working nor provably gone. A queue holding work is not stuck; that is what a
queue is for.
