# Native server

`ds server engine --output json` inspects the Solar engine linked into this
executable, without login, a running server or network access. Release builds
return the owner's `ds.engine-build/v1` identity. The release publisher registers
this manifest separately from the bundled standalone `ds-solar` manifest:
their Cargo dependency closures can differ. Development builds omit release
provenance and cannot be admitted as release engines.

`ds server serve` hosts the shared Rust compute runtime under the identity
established by `ds auth login` or device linking. Run both under the same Linux
user and lane. No browser, paired Desktop, ADC or service-account impersonation
is involved. The current API is for that owner's control over loopback; remote
operators use SSH. Public multi-user web delegation is not yet implemented.

```bash
ds server serve --lane stable
# From another terminal under the same account:
ds server submit --input transformer-batch.json --key processing-001 --project <exact-id>
ds server solar submit --input pala.server-submission.json --key solar-001
ds server status --project <exact-id>
ds server activity --lane stable --project <exact-id> --output json
ds server status --job <id> --project <exact-id>
ds server result --job <id> --out result.json --project <exact-id>
```

## One project per call, named by the caller

Every `ds server` command that names or reads a job or a project takes
`--project <exact-id>`. It is optional, and what makes it optional is the
saved selection from `ds auth project use`: absent the flag, the client reads
that selection from its own protected context and sends it as if you had typed
it. Either way exactly one project name reaches the Server on every request.

A saved selection is therefore a **client default**, never Server state. The
Server verifies whichever name arrives against freshly fetched membership for
the authenticated account and refuses on its own authority; it does not read
this machine's selection, its state directory, a browser cache or any other
client path. Two consequences worth stating plainly:

- **`ds server serve` needs no selected project.** An account that has never
  run `ds auth project use` can host. Callers name their project per request.
- **One Server serves many projects at once.** Nothing switches, and nothing
  needs a restart to move between them.

With neither `--project` nor a saved selection there is nothing to verify and
nothing worth guessing, so the call refuses `project_required` locally rather
than admitting a job with no scope. The one exception is `ds server solar
submit`: the sealed `ds.solar.server-submission/v1` envelope names its own
project and that name is authoritative, so it can be submitted with nothing
named. Passing `--project` there is still worth doing — it is how you learn
you prepared the wrong city, as a differing name refuses `scope_mismatch`.

### Working two projects side by side

```bash
# Two projects, one Server, no switching and no restart.
ds server solar submit --input kigali.server-submission.json --key solar-a-001    # sealed: project A
ds server submit --input b-transformers.json --key lv-b-001 --project project-b
ds server status --project project-a --output json      # A's jobs only
ds server status --project project-b --output json      # B's jobs only
ds server activity --project project-b --output json    # B's publication state only

# Changing the saved selection changes only the DEFAULT, never work in flight.
ds auth project use --project project-c
ds server status --output json                          # now defaults to C
ds server status --project project-a --output json      # A's jobs, still running, unchanged
```

Nothing about a running job's project can be rewritten after admission —
retries, worker restarts, host restarts and cancellation all retain the context
the job was admitted under. `ds auth project use` moves the default for the
*next* call and nothing else. Asking about another project's job answers
`job not found`, identically to an id that never existed.

`ds server activity` answers one envelope,
`{"schema":"ds.server-activity/v1","projects":[{"project","activity"}]}`, with
one entry per project the request covers.

### Capacity between projects

`ds server serve --workers N` bounds the whole host. `--per-project M` bounds
what one project may hold while another has work queued; it defaults to
`max(1, N / 2)`, so a busy project cannot starve a second one on a host with
room for two. Exhaustion is a typed answer, not a hang: `capacity_exhausted`
carries `retry_after_ms` and whether the `global` or the `project` scope filled.
Cancelling a job releases its capacity immediately.

### Refusals, by their own names

The Server's typed refusals are re-raised by the CLI under the same code, so an
operation fails identically whichever host executed it.

| code | when |
|---|---|
| `project_required` | no `--project` and no saved selection to default to |
| `context_corrupt` | a project id outside its bound (empty, padded, over 500 characters, control characters) |
| `project_not_visible` | the account's freshly verified membership does not contain that project |
| `not_visible` | `job not found` — one answer for an unknown id, a foreign principal, a foreign lane and the wrong project |
| `principal_mismatch` | the stored job belongs to another account, lane or deployment |
| `membership_revoked` | membership was lost while the job was queued or running; only that project's work stops |
| `scope_mismatch` | the sealed Solar input names one project and `--project` names another |
| `scope_mismatch_for_key` | that idempotency key already admitted a job in a different project |
| `payload_changed_for_key` | that key already admitted a job with different input bytes |
| `capacity_exhausted` | the global or per-project queue is full; carries `retry_after_ms` and `scope` |
| `context_unrecoverable` | a job stored by an older Server names no project and none can be recovered without guessing |
| `multi_principal_unsupported` | the request's credential names an account other than the Server's |

`not_visible` says `job not found` and nothing more. It never names the
project, the owner or whether the id exists — the class, the code and the
sentence are identical in all four cases, because anything that differed would
itself be the disclosure.

### Multi-principal is explicitly not supported

One `ds server serve` serves **one authenticated native account** and as many
of that account's projects as it may reach. A request whose credential maps to
a different principal is refused `multi_principal_unsupported`; nothing falls
back, and no request is served under a second identity. A second account needs
its own `ds server serve` with its own `--state-dir` and `--listen`. This is a
stated limitation of this slice, not an oversight: per-connection principals
would need an authenticated host transport for callers other than the owner,
which is a separate authority change (see the browser note below).

## Jobs

Input is the existing `ds.fast-lv.request/v1` contract documented in
[design.md](design.md). Settings and configuration are explicit captured inputs.
Submission acknowledges only after durable storage; retries with the same key
and bytes return the same job. Changed bytes under that key refuse, and so does
the same key under a different project — a key never moves a job between
projects. Independent requests execute concurrently up to kernel CPU/memory
admission; the engine uses its shared native Rayon pool inside each request.
`--workers` can reduce that capacity. Results preserve per-transformer
engineering outcomes, including failures; `completed` means the complete result
document was produced. Each job receipt carries the context it was admitted
under, including its project.

`server solar submit` accepts one private `ds.solar.server-submission/v1`
envelope. It contains the existing prepared calculation fields (`prepared`,
optional `render_charts`, optional `run_id`) and the exact matching
`ds-solar.prepared-publication-claim/v1` as `publication_claim`. The Server
checks the claim's project, city, city-content digest and prepared-input digest
against the prepared calculation, and seals the claim's snapshot SHA-256,
input-base fingerprint and actor-bound receipt into the durable job. The
existing compute-artifact authority revalidates that sealed claim under the
job's own project before publication. A missing, malformed, expired or
stale claim is never published: missing or malformed claims refuse at
admission, and an expired or stale claim is recorded by the publication
authority. A server never stamps a newly fetched snapshot onto an older
prepared input. The envelope is owner-private because the claim carries the
actor-bound receipt. It is a sealed request body, not a server-readable client
path or a browser cache reference.

## Layers on the running Server

`ds server layers list|show|hide|reorder` are **retired**. One operation has
one command id whichever host executes it, so the layer drawer's catalogue,
visibility and order are `ds map layer list|show|hide|reorder` with an explicit
`--target server|desktop[:instance]` (the routing default decides when you pass
none). A second set of ids that differed only by which host answered was the
thing to remove: the same request, the same arguments and the same answer
should not have had two names.

Nothing about the operation moved. Both hosts call ONE shared Rust owner
(`ds-layer-ops` over `ds-command-kernel::layer_state`); the HTTP host and the
CLI embed no rule of their own. What `ds-cli-server` keeps is the transport
that genuinely is the Server's: the protected owner-only loopback connection
(`connection.json` bearer, native authority renewed and revocation observed as
for jobs), with `--target server` sending an explicit `project` on every layer
request. Remote operators use SSH. Preferences never become another account's
visibility because both reached one host: a restart under another account reads
that account's own scope and leaves the previous one untouched.

Routes: `GET /v1/layers?project=&refresh=&limit=&zoom=`,
`POST /v1/layers/visibility?project=` `{"layers": [canonical ids], "visible": bool}`,
`POST /v1/layers/order?project=` `{"orders": [{"layer_id", "order"}]}`. The
bodies are exactly the shared owner's request types, so nothing about the
command's shapes changes with the host; `project` is required on all three and
travels in the query. Refusals are typed on the wire (`class`, `code`, `error`,
`remedy`) and re-raised by the CLI under the same code: `unknown_layer`
(runtime ids are never accepted), `duplicate_layer`, `invalid_order`,
`invalid_number`, `local_layer_refused`, `project_context_changed` (the
Server's document is on another project than the one named),
`auth_identity_mismatch`, `headless_signed_out`, plus the project refusals
above. Nothing here pretends a renderer mounted anything: `writes` name the
layout word a renderer would apply to each runtime layer.

Browser-to-Server layer control is **not** provided: the connection bearer is
an owner-only local control credential and must not reach a web visitor.
Remote presentation waits on an authenticated host transport for browsers,
which is a separate authority change.

## Protected state and authority

The default state root is `$XDG_STATE_HOME/ds/server/<lane>` or
`~/.local/state/ds/server/<lane>`. `--state-dir` overrides it explicitly. It must
be owner-only. `connection.json` is a local control credential and must never
be printed, copied into logs or exposed to a web visitor. It is distinct from
the protected upstream DS login. Jobs are fenced by UID, lane, credential
audience and project. Native authority is renewed at most every 15 seconds
across workers; sign-out/account changes fence work immediately when observed.
Upstream device revocation is observed on renewal. Loss of authority pauses
execution and fences result commits. Restoring the same identity permits pending
work to recover. Losing membership of ONE project fails that project's work with
`membership_revoked` and leaves every other project running.
Each running host also retains its starting provider/device binding. It cannot
silently fall back from a revoked or removed device to a stored password login
for the same UID. Changing that binding requires restarting the host; retained
jobs still belong to the same canonical account, lane, audience and project.

Worker leases renew during computation. After a killed process, an expired
30-second lease can be reclaimed; completed jobs are never rerun. A job
recovered after a restart keeps the exact context it was admitted under. A job
row written by a Server released before per-job context recovers its project
from the sealed input (Solar) or from the recorded saved selection
(transformer); when neither exists it refuses `context_unrecoverable` rather
than guessing. Cancellation retains input, fences late results and releases the
job's capacity. The native engine finishes its current computation before
releasing CPU; cancellation does not promise mid-algorithm interruption.
Complete interactive Desktop workflow delegation, device management UI and
public browser authentication remain separate migration gates.

Development: `ds-web/run-linux-server.sh` builds locally and restarts its own
server after successful Rust rebuilds. Failed builds preserve the running
server. Durable state is outside the checkout and survives reloads. It uses
an installed, digest-bound native profile catalog, or the explicit debug-only
`DS_NATIVE_CLIENT_PROFILE_BUNDLE`. `./run-linux-server.sh --cli auth status`
uses the exact development executable; `--cli auth login --email <email>` or
`--cli auth link begin` establish its native identity when needed.

MCP exposes submit, status, cancel and result through the existing command
catalog, including `--project`. Starting the foreground host is a
terminal/service operation and is excluded from MCP so it cannot block a tool
response indefinitely.

`server activity` reads the server's shared Sync Center state, including native
job lifecycle and artifact publication receipts, grouped by project. It uses the
same protected connection and native identity as other Server commands; it
creates no queue. A completed calculation and a published artifact are separate
states.

## Report publication without Desktop

`ds report project export --transformer <name> --out-dir <fresh-directory>
--publish --lane canary` runs the native report engine and seals verified outputs
into the same report artifact store consumed by Desktop. The foreground Server
publishes queued reports in the background through the existing WorkGrant
authority, under the job's own project. `queued_for_server_sync` is an enqueue
receipt, not proof of remote publication; inspect
`ds server activity --lane canary --project <exact-id> --output json`.

Both commands must use the same user, lane and state directory. If Server uses
`--state-dir`, export must receive that exact path as `--server-state-dir`.
Report names are partitioned by project, so the same report name in two
projects does not collide. Without `--publish`, export remains local-only.
Development engines cannot claim release publication authority. This command
computes before queueing; it does not claim a durable background report-compute
job.

The development launcher isolates config, cache, data and Server state below
`DS_SERVER_DEV_ROOT` (default `~/.local/state/ds/server-dev`). Use its `--cli`
mode to authenticate that development environment. It neither copies the
installed Server login nor allows a development state override into installed
Server state.
