# The execution context — one verified record per admitted operation

**Status:** the kernel owner, the durable jobs table (`ds-sync-store`) and the
compute runtime are implemented and proven on `fable/server-isolation`,
2026-09-12; the Server host and the CLI that consume them are the rest of the
slice. Third pass, same day, from the second adversarial verdict: a project id
is one path segment (§2), a per-project limit must divide the host limit it
shares (§5), and the corpus no longer records a cross-project
`scope_mismatch_for_key` no host can produce (§6). Third-pass review, same
day: that segment is also one no filesystem reads as a drive, a device or a
stream (§2), and every refusal but
`not_visible` says what to do about it (§3), `context_unrecoverable` included
(§6). Parent:
[project and instance isolation](../../../ds-web/docs/project-and-instance-isolation-contract.md);
refines [Headless application runtime](ds-headless-application-runtime.md) §6.

## 1. The question this settles

One Server admits Solar for one project, printing for a second and a layer write
for a third, concurrently, for one authenticated account. None of it may come
from a process-wide "current project", a selection captured at `serve` start, or
an id a caller holds. So: **what is an admitted operation about, who decided
that, and what may change it?** Answered once in `src/execution_context.rs`
(`ds.execution-context/v1`, WASM `executionContext`):

| Question | Op |
|---|---|
| May this request be admitted, and under which project? | `admit` |
| May this caller see this job at all? | `visible` |
| Does this lifecycle event change the scope? (never) | `retain` |
| Is there room for this project's work right now, fairly? | `capacity` |
| What context does a row written before this slice have? | `recover` |

Pure: `now_ms` is an input and every list bounded, so the same request always
has the same answer; no transport, store or credential enters it.

## 2. The record

Immutable once admitted, serialised snake_case as the durable job row's
`context` member, so these are the on-disk names.

```
ExecutionContext { principal_uid, lane, deployment, install_id, // the connection
  project,                            // the project the work names, immutably
  client, operation, job_id,                    // who asked, for what, as what
  idempotency_key, input_sha256, admitted_at_ms }  // retry + payload identity
```

`operation` is closed: `transformer_processing`, `solar_processing`,
`report_export`, `layer_read`, `layer_write`; `lane` is `stable` or `canary`;
`input_sha256` is a 64-character lowercase digest. Every text field is
non-empty, trimmed, control-free and bounded (`client` ≤ 120, `idempotency_key`
≤ 200). One that is not is `context_corrupt`, **named and told the rule it
broke** — `execution context field client is not valid: 1..=120 characters,
trimmed, with no control character` — with the closed vocabularies spelled out
rather than named, because a caller cannot look up a constant it does not have.

**A project id is one path segment** (`valid_project`): 1..=200 characters, no
`/` and no `\`, no whitespace anywhere, no control character, no trailing dot
(which also refuses `.` and `..`), none of `: * ? " < > |`, and not a Windows
device name (`NUL`, `con.json`; the stem before the first dot, case ignored).
200 is the kernel's own project bound, as `survey_query` and
`survey_entries_changes` already read it — deliberately not `layer_document`'s
500-character free-form bound, because this id is durable and hosts partition
by it: store rows, dataset caches, staging directories, report names. The last
three rules are Windows and only Windows, and the product ships a Windows
desktop: `Path::join("C:")` **replaces** the whole path rather than appending
to it, so `state_dir.join(project)` for a project called `C:` leaves the state
directory altogether; a trailing dot is stripped, so `A.` and `A` would be two
projects in one directory; and a device name is a device at any depth, so a
project called `NUL` silently discards everything written for it. An id that
could mean a different place on disk (`..`, `A/../B`, `C:`, `A.`, `NUL`) or
that a shell, a log line and a path each read differently (`A B`) is refused
`context_corrupt` at every door it can arrive through — named by the caller,
defaulted from a saved selection, sealed in the bytes, offered to a legacy row
on recovery, stored in a record, or counted in a census (there it is the
host's own bug, a hard fault). The refusal states the grammar, so an operator
reads the rule rather than guessing which character was the problem. The
Server's and `ds-cli-auth`'s own restatements of the bound (500) are theirs to
narrow; the kernel refuses what they let through.

`compute_jobs::Job` gains `#[serde(default, skip_serializing_if)] context:
Option<ExecutionContext>`. Rows are one JSON blob, so there is **no SQL schema
change**: a released Server's row deserialises with `None` and re-serialises
byte-identically, proven on a real SQLite file in
`crates/ds-sync-store/tests/compute_jobs_rows.rs`. `transition` never touches it.

## 2b. Boundary

The owner's standing ruling of 2026-09-12, verbatim, because it is the reason
this owner has no directory in it at all:

> The desktop wraps the Server; the Server is NOT something else. It has exactly the boundary the desktop has with ds-brain, and it stands on the same side of that boundary as the desktop does: entitlement is decided by ds-brain at that boundary, never re-decided inside the Server. Consequences, and they are simple:
> - **The Server never fetches, caches, persists or refreshes a project directory to admit work.** There is no membership snapshot, no TTL, no refresh-on-refusal, no "if online then … else …". Delete every such conditional. Admission is: the ONE authenticated owner of this Server (principal, lane, credential audience — the connection identity) hands it work that names a project explicitly (a sealed Solar input's project outranks the named one); the kernel records the immutable context and refuses only principal mismatch, a key reused with another scope or other bytes, a missing project, or a job that is not visible to the caller's (principal, lane, project) — "not found", the same sentence an invented id gets.
> - **Entitlement is enforced by the gateway (ds-brain) where the effect crosses to the cloud** — publication, sync, a document fetch — exactly as the desktop already behaves. A publication the gateway refuses for project A stops A's publication and nothing of B's. There is no `membership_revoked` fault inside the runtime; revocation is a gateway answer on an effect, not a Server decision.
> - **Offline-first falls out of that:** admission, queueing, execution, retry, restart recovery, capacity and the jobs table are local and are proven with NO upstream present at all.
> - Wording: descriptors, reference docs and contract docs say "the Server executes what its authenticated owner hands it for the project named; the gateway enforces entitlement at publication and sync". The phrase "freshly fetched/verified membership" must not survive anywhere.
> - **The Server accesses the filesystem just as the desktop does (owner, 07:45 UTC).** It runs under the owner's identity on the owner's machine model and reads/writes the same local homes the desktop does — the prepared layer store, the sync store, project dataset caches, report artifacts, downloads. A request may name a local path exactly as the desktop's own commands do (a prepared Solar input is a workspace file: the route takes its path and the Server reads it, digesting the bytes it read for idempotency); "a remote Server never reads a client path, a browser cache or this machine's selection" is withdrawn as a rule — remote administration is SSH, so the path is on the Server's machine, which is the same statement. Keep bytes-in-the-request only where the desktop itself takes bytes. Not in this slice, recorded for slice 2: one on-disk home for desktop and Server (the Server state dir and the desktop data dir are still two roots).
> - **Never a container (owner, 07:55 UTC).** "I know it's a way to turn it into a container, but we'll never do that. We'll use the distributed system we have right now. Don't optimize for that. It's simply the desktop core to constrain Tauri and TypeScript. That's all." So: the Server is the local desktop core on the owner's machine; the cloud stays ds-brain and the existing backends. Nothing in this slice is designed for a remote or containerised Server: no remote-administration framing ("Remote administration uses SSH" goes), no transport abstractions that exist only so the core could run elsewhere. **The right limit on users (owner, 08:00 UTC): "many users can access the server but we don't need that complexity right now; we can evolve the server any way we want."** And the deployment model (owner, 08:05 UTC): "if I want to set up that server on the cloud, I rent the machine, I auth, I work and shut down the machine — running on a VM. If many users, they'll have to use many virtual machines. That's all." So: ONE authenticated owner per Server, always — one user per machine, scale-out is more machines, never multi-tenancy inside one process; `multi_principal_unsupported` is the honest one-sentence refusal; the context record carries its principal because the record does, not as a multi-user design; nothing is built for many users in one Server: no per-principal limits, no multi-user auth, no per-client fairness beyond the owner's own projects sharing one machine. What the slice keeps is exactly the local need: one user, several projects at once, from the UI, the CLI and MCP, without a project switch and without cross-project effects.
> - **Long-lived, and the long-term direction (owner, 08:20 UTC): "one can leave the server running as long as he wants — and the long-term objective is to connect a version of the browser on top of it (wasmless). But not now."** So the Server is a process the owner may keep up indefinitely (restart recovery, the lease and the durable jobs table already serve that; nothing more now), and one day the browser UI will talk to this Server instead of running the kernel in WASM. Not in scope for this slice or the next; only a constraint: do not build anything in the Server that assumes a short life or that a webview is the only client.
> - Slice 2 consequence: a Desktop instance IS a Server instance with a webview; the desktop descriptor and the Server `connection.json` converge into one instance registry, and the two on-disk roots converge into one home.

One line of that quote is superseded by the same day's later reading of it:
`multi_principal_unsupported` is **deleted**, not kept as an honest refusal.
One owner per Server is enforced by the owner-only bearer alone — any other
bearer is `server access denied` — so there is no second identity for a
request to name and no way to reach such a refusal. That is the whole rule,
and a code nothing can raise is not one. The bearer itself is `ds-cli-server`'s
(`host.rs`, where it is proven); this owner records the consequence, which is
that its refusal vocabulary stays at the nine codes below.

What that means here, concretely: `admit` takes the connection identity, the
project the work names (`requested_project`, the caller's own `saved_project`
default, and `sealed_project` from the bytes), and the key with its input
digest. There is no membership input, no TTL and no freshness rule, and
`project_not_visible` is not in the vocabulary. "Verified project membership at
admission" is satisfied **at the ds-brain boundary** — the same boundary the
desktop stands on — and nowhere inside this kernel or the Server. See §7.2: it
is a decision to challenge, not an omission.

## 3. The rules

- **A named project is the operation's project.** `saved_project` is the
  caller's own default, used exactly like a name it typed; it is a client-side
  convenience and never Server state.
- **Sealed bytes outrank a request, which outranks a default.** A prepared Solar
  input names its own project; a disagreeing default is not used, a caller who
  *names* a contradicting project is `scope_mismatch`. Bytes name a project;
  entitlement to it is the gateway's answer on the effect that leaves the
  machine, never this owner's.
- **The project is resolved before any stored key is consulted**, so what a key
  already holds never decides what a request is about.
- **A project id is one path segment** (§2), wherever it is named.
- **A retry keeps its scope and identity.** Same key, project, operation and
  digest returns the stored context unchanged (`existing`), not re-stamped;
  another operation is `scope_mismatch_for_key`, other bytes are
  `payload_changed_for_key`, another principal/lane/deployment holding it is
  `principal_mismatch`. Another *project* never reaches that refusal at all:
  the durable id digests the project, so the same key in two projects is two
  ids and two jobs (§6).
- **Nothing rewrites scope.** `retain` is the identity function and it takes
  `now_ms` and ignores it; every event returns the record it was given.
- **The refusal vocabulary is closed**, all nine proven: `project_required`,
  `scope_mismatch`, `scope_mismatch_for_key`, `payload_changed_for_key`,
  `principal_mismatch`, `not_visible`, `capacity_exhausted`, `context_corrupt`,
  `context_unrecoverable`.
- **A refusal says what to do about it, or it is a dead end.** Every one but
  `not_visible` carries a second clause after the `;` or the `:` — the rule
  the value broke, or the thing the caller does instead; `capacity_exhausted`
  carries `retry_after_ms` in the answer rather than in the sentence. The
  exception is deliberate and is §4 itself: `job not found` says nothing else
  because anything else would disclose that there is something to find. Held
  over the recorded corpus, code by code
  (`every_refusal_but_the_invisible_one_says_what_to_do_about_it`).

## 4. Non-disclosure

`visible` answers one object — `{"code":"not_visible","message":"job not
found"}` — to a foreign principal, lane or deployment, a caller narrowing to the
wrong project, an unreadable context, a malformed caller, and an id that never
existed, so an id in a log discloses nothing and a guess is indistinguishable
from a hit. `install_id` is recorded for provenance, never as a fence. A
capacity refusal names counts and limits but never a project — both asserted
over the recorded answers.

## 5. Capacity and fairness

`capacity` takes only host-owned inputs — configured limits and the store's
census — so a malformed input is a hard fault, never a client refusal.

**A share must divide the pool it shares.** Each limit is 1..=4096, and
`per_project_running` must be smaller than `global_running` and
`per_project_queued` smaller than `global_queued`; limits that are not are
refused as the host configuration fault they are, in a sentence naming both
numbers. A share equal to the pool is not a share — the first project to
arrive holds every worker, or fills the whole queue, while a second waits,
which is precisely what the share exists to prevent — and it is not a client's
refusal to receive, so it is a hard fault. **The one exception is a pool of
one:** the smallest rented VM (`--workers 1`) has nothing to divide, whatever
runs holds the machine, and its `per_project_*` must therefore be exactly 1.
Such a host has no share and is still bounded: its one slot is full while it
runs, and the second project is told so with `scope: "global"` and retry
guidance. Until this pass the invariant lived only in `ds-cli-server`'s flag
parser, so any other host — a test, a route building its own `Limits`, a
future embedder — could configure the fairness away and this owner would
apply it as written.
`Intent::Admit` bounds the queue (`global_queued`, then `per_project_queued`); a
full *running* pool never refuses a submission, the job queues. `Intent::Claim`
bounds the running pool (`global_running`) and the fairness share: a project may
exceed `per_project_running` while nothing else waits and may not while another
project **under its own share** has queued work — room for printing during a
long Solar run without idling the machine to defend an unwanted share. Two
projects both at their share and both waiting never refuse each other: the share
yields only to a project that could take the slot, otherwise queue order decides
(cases `capacity-claim-every-waiting-project-at-its-share`,
`capacity-claim-yields-only-to-a-project-under-its-share`). `retry_after_ms` is
a second per unit of work that can go first, capped at sixty. No clock, no
randomness. The host asks inside the store transaction that claims, from a
census read there — a census read before the transaction describes a pool that
no longer exists, and under several workers it lets one project take every slot
the burst offers. The census counts only work a worker could actually take: a
row that predates contexts, and a row whose stored context does not read, are
in neither list, or a row nothing can ever claim would spend a project's queue
bound for ever and stand for ever as another project's waiting work.

**The limit of the share, stated (2026-09-12 review).** It is enforced at claim
time and there is no preemption. A project alone on the machine may therefore
take every running slot, and it keeps them until those jobs finish: a long
Solar run started while nothing else was queued does *not* leave room for
printing submitted an hour later, however small `per_project_running` is. From
that moment on the share bites every further claim, so the pool drains towards
fairness rather than away from it. The alternative — refusing an over-share
claim in case someone submits later — idles a worker on speculation, which the
concurrency section forbids as plainly as it forbids the starvation. Taking a
running job away from one project to give the slot to another is a product
decision (preemption, priorities, or a reserved slot the operator configures),
not something this owner may invent.

## 6. Recovery provenance, and the durable id

A row carrying a context returns it (`stored`). A row written before this slice
takes its project from its own sealed input if it is Solar (`sealed_input`,
authoritative — the bytes name it) and **from nothing else at all**. A
transformer row carries no project by design, so it is `context_unrecoverable`:
readable by its own connection, never claimed, never executed, never published.
It is not recovered into whichever project happens to be selected when the host
reopens the queue — that would run one owner's retained work, and publish it,
somewhere it was never admitted for. The one remedy, and it travels **inside
the refusal** (`UNRECOVERABLE_REMEDY`, appended to every
`context_unrecoverable` sentence the kernel and the store produce, and printed
by the runtime when a reopened queue finds such rows): **read its stored input
and submit it again naming the project**. Not its result — such a row was never
claimed and never ran, so it has none; what it has is the bytes it was admitted
with, which a host returns (`GET /v1/jobs/:id/input`, `ds server input`), and
submitting them under a named project is a new job with its own id. A
reconstructed record keeps the row's own `created_at_ms` and records
`recovery:legacy-row` / `recovered` for the client and key.

The durable job id is `digest(owner, lane, project, key)` for every admission
this build makes (`ds_compute_runtime::job_id`), so the same key in projects A
and B is two pieces of work with two ids rather than a collision, and
`scope_mismatch_for_key` is what a key reused **within one project** for another
operation or other bytes still gets. That is the stronger statement, and it
replaces the weaker one the corpus used to record: a host looking a key up in
project B finds nothing and admits fresh, so no request can reach the
cross-project half of that refusal, and a host that somehow handed over another
project's record would already have been stopped by the job-id check before it.
The kernel keeps the check — it assumes no host's id scheme — but the corpus
records no case for it, because in this build none can be produced. Rows written before this build keep the ids
they have (`digest(owner, lane, key)`); those ids can no longer be re-derived
from a key, so a later submission never adopts such a row — it is refused
`context_unrecoverable` and left exactly as it is.

## 7. Decisions to challenge

1. **`context_corrupt` covers caller-supplied field bounds**, not only a damaged
   record, so an over-long client label is a typed refusal, not a 500. Host
   configuration stays a hard fault.
2. **"Verified project membership at admission" is satisfied at the ds-brain
   boundary, not here.** This owner records the project the authenticated owner
   named and refuses nothing on entitlement; a project the owner may not act in
   is stopped where its effect crosses to the cloud — publication, sync, a
   document fetch — exactly as the desktop already behaves. The cost is honest:
   work can be admitted, queued and computed locally for a project whose
   publication will later be refused, and the refusal names one project's
   effect without touching another's. The alternative, a directory inside the
   Server, is the conditional the owner deleted.
3. **`scope_mismatch_for_key` covers a changed operation** — within one project
   the durable id digests `(owner, lane, project, key)` and would otherwise
   collide. Its cross-project half is a fence with no reachable door in this
   build (§6): kept in the code, deleted from the corpus, because a proof of
   something no host can ask for overstates what is proven.
4. **Three deliberate shape changes.** `admit` gained `sealed_project`
   (acceptance item 2 refuses a Solar submit whose bytes name A while the caller
   names C); `capacity` gained `intent`, or one op serving both moments would
   refuse to claim out of an exactly full queue; `deployment` is fenced on
   recovery too, so an audience rename needs a migration, not a silent reopen.

## 8. What hosts keep

Hosts perform I/O on these answers and decide nothing: `ds-sync-store` writes
the admitted context with the job row, narrows `jobs/jobs_page` to a caller's
project in the query itself, asks `visible` before answering
`jobs/job/job_input/job_result`, and asks `capacity` **inside the transaction
that inserts (`Intent::Admit`) or claims (`Intent::Claim`)** from a census read
there, so racing submissions cannot both take the last place and racing workers
cannot each take the same room; its census counts a running row whose lease has
expired as waiting and a row nothing can claim as neither; and it reopens a
queue by counting the rows that carry a context and reading only the ones that
do not, so a long-lived Server does not pay its whole queue in memory to
restart. `ds-compute-runtime` admits through the kernel, asks `recover` when it
starts its workers, and makes no network call: a claimed job runs what was
admitted, and the one thing it calls that a host *could* implement over the
network — the `Authorizer` that says whether this Server's owner is still the
authenticated one — gates only starting work. Neither the lease refresh nor the
terminal commit asks it again, so an unreachable moment delays new work and can
never destroy work already done. `ds-cli-server` opens per-(principal, project)
sessions from an admitted context instead of a process-wide selection, and it
is where one owner per Server is actually enforced — the owner-only bearer of
its `host.rs`, which is also where that fence is proven; `ds` re-raises refusals
literally. The
kernel holds no credential material, so the owner digest stays the store's
fence; `recover` checks the lane it can see. Proof:
`tests/fixtures/execution-context-cases.json`, 86 recorded cases, byte-identical
to ds-web's `src/tests/fixtures/execution-context/cases.json` and replayed there
through the vendored WASM, plus the unit tests above.
