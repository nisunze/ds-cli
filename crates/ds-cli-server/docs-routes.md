# The Server's routes under one verified execution context (isolation slice 1)

Written by the Rust-native writer for the CLI-client writer. Normative source:
`ds-command-kernel/docs/contracts/ds-execution-context.md` and the slice
contract §2/§3. Everything below is what `crates/ds-cli-server/src/host.rs`
and `layers.rs` actually accept and answer after this slice; `lib.rs` (the CLI
side) is yours.

## 0. The one rule

A Server operation is about **one project, named by the caller and recorded
by the kernel at admission**. The Server executes what its authenticated owner
hands it for the project named; the gateway enforces entitlement at
publication and sync. The Server is the desktop's core and stands on the
desktop's side of the one boundary with ds-brain: it holds no project
directory, fetches none, caches none and refreshes none to admit work, and it
carries no online/offline conditional — admission, queueing, execution,
restart recovery and capacity are local and are proven with no upstream
present at all. It reads `ds auth project use`'s saved selection nowhere at
all — not at `serve`, not at recovery — because the saved selection is the
*client's* default, which the client sends explicitly on every call. So:

> `ds server …` MUST send `--project`'s value (defaulting to the client's saved
> selection) on every submit, layer and, where it narrows, every read.

A request that names no project where one is required is refused
`project_required`; the Server never substitutes one.

## 1. Refusal wire (unchanged shape, new codes)

Every refusal below is the existing typed body `layers.rs::refusal` already
emits, so `typed_refusal` in `lib.rs` re-raises it unchanged:

```json
{"error": "<sentence>", "class": "<invalid_input|unauthorized|unavailable|conflict|internal>",
 "code": "<code>", "retryable": <bool>, "remedy": "<sentence>"}
```

`capacity_exhausted` additionally carries `"retry_after_ms": <u64>` and
`"scope": "global"|"project"`, and is answered under HTTP 429.

New codes to add to the `typed_refusal` match and to the command `Refusal`
rosters (class → HTTP):

| code | class | HTTP | when |
|---|---|---|---|
| `project_required` | invalid_input | 400 | no project named and none in the sealed input |
| `scope_mismatch` | conflict | 409 | the sealed Solar input names another project than `--project` |
| `scope_mismatch_for_key` | conflict | 409 | that key, **in this project**, already names another operation |
| `payload_changed_for_key` | conflict | 409 | that key was admitted with other bytes |
| `principal_mismatch` | conflict | 409 | that key/row belongs to another authenticated identity |
| `not_visible` | conflict | 409 | `job not found` — the ONE sentence for every invisible job |
| `capacity_exhausted` | unavailable | 429 | queue or worker admission is full; `retry_after_ms` |
| `context_corrupt` | invalid_input | 400 | a caller field is out of bounds (e.g. an over-long project) |
| `context_unrecoverable` | conflict | 409 | a pre-slice row names no project, and nothing outside its own bytes may name one |

`not_visible` says **`job not found`** and nothing else, for a foreign
principal, a foreign lane, the wrong project and an id that never existed.
Do not add prose to it, and do not print the requested project back.

## 2. Routes

### `POST /v1/transformer-processing/:key?project=<id>`
`project` — REQUIRED in practice (transformer inputs carry no project by
design). Body: `ds.fast-lv.request/v1` **bytes**, ≤ 64 MiB — the desktop's own
transformer processing takes bytes, so this route does too. 202 →
`{"job": <Job>}` where `<Job>` now carries `"context": {…}` (see §3).
Refusals: `project_required`, `scope_mismatch_for_key`,
`payload_changed_for_key`, `principal_mismatch`, `capacity_exhausted`,
`context_corrupt`. A project named for the first time is admitted on the
owner's word: nothing is fetched to allow it.

### `POST /v1/solar-processing/:key?project=<id>`
**Body changed: the route takes the PATH, not the bytes.**

```json
{"input_path": "/absolute/path/to/pala.server-submission.json"}
```

The prepared `ds.solar.server-submission/v1` envelope is a workspace file, and
the Server accesses the filesystem exactly as the desktop does — same machine,
same user, same homes — so it opens that path itself, reads it under the owner's
identity and digests the bytes it read for idempotency. `ds server solar submit
--input <path>` passes the path straight through; nothing copies 64 MiB through
a socket to the same machine's own user. The path must be absolute (the client
and the host share a filesystem but not a working directory) and name a regular
file of at most 64 MiB; anything else is `server_refused` (400) with the remedy.

`project` — OPTIONAL: the sealed envelope names its own project and that name
wins. Sending it is still recommended (it is how a caller learns it prepared the
wrong city): a `project` that differs from the sealed input is `scope_mismatch`.
202 → `{"job": <Job>}`.

### `GET /v1/jobs?project=<id>`
`project` — OPTIONAL narrowing. Without it: every job visible to this
connection, whatever its project. With it: only that project's, and a project
that holds no work answers an empty list whether the owner ever named it or
not (never a disclosure). Answer shape is unchanged:
`{"jobs":[<Job>…],"more":<bool>}`.

### `GET /v1/jobs/:id?project=<id>`, `POST /v1/jobs/:id/cancel?project=<id>`, `GET /v1/jobs/:id/result?project=<id>`
`project` — OPTIONAL narrowing. An id belonging to another project answers
exactly `job not found` (`not_visible`, 409), identical to a guessed id, on
all three. Shapes unchanged (`{"job":…}`, `{"job":…,"publication":…}`, raw
result bytes). One deliberate difference: a job the caller CAN see that simply
has not finished answers `server_refused` / "job has no completed result", so
"not yours" and "not yet" stay distinguishable *inside* a project and
indistinguishable across projects.

### `GET /v1/activity?project=<id>`
**Shape changed.** One envelope, always:

```json
{"schema":"ds.server-activity/v1",
 "projects":[{"project":"<id>","activity":{…the previous /v1/activity body…}}]}
```

Without `project`: one entry per project this connection has durable work in
(ordered by project id), read by paging the whole durable queue rather than its
newest page. With `project`: exactly that entry, or an empty `projects` array
when the project is not visible. An entry whose Sync Center projection could
not be read carries `"unavailable": "<reason>"` and no `activity` INSTEAD of
failing the envelope — one project's gateway is never allowed to hide what the
others are doing, and the reason is what keeps it from reading as "no work".
Render accordingly.

### `GET /v1/layers?project=<id>&refresh=&limit=&zoom=`
### `POST /v1/layers/visibility?project=<id>`  body unchanged: `{"layers":[…],"visible":<bool>}`
### `POST /v1/layers/order?project=<id>`  body unchanged: `{"orders":[{"layer_id","order"}]}`

`project` — **REQUIRED**, as a query parameter on all three (the bodies stay
exactly the `ds_layer_ops` request types, so nothing about `ds map layer …`'s
shapes changes). Three answers, all proven through the real listener:

| named | answer |
|---|---|
| nothing | `project_required` (400) |
| a name outside the kernel's bound | `context_corrupt` (400) |
| any project this owner's account can read | that project's catalogue, its own remembered visibility, its own order |
| a project the account cannot read | the gateway's own refusal, raised where the account is established (`auth_rejected`) |
| a source that answers about another project than the one it was opened for | `project_context_changed` (409), remedy naming both |

The document source is opened FOR the named project
(`ds_layer_ops::Native::for_project` → `ds_cli_auth::layer_config_for_project`),
so this machine's saved selection is never read and two of the owner's projects
are served side by side through one running host. A refused layer request
writes nothing, under any project.

### Anything this host does not serve
A request under `/v1/map/…` or `/v1/invoke` answers the standing ruling's
typed refusal:

```json
{"error":"this host runs the operation but has no rendered map to run it against",
 "class":"unavailable","code":"needs_paired_map","retryable":true,
 "remedy":"run the same command with --target desktop, against a window open on this project"}
```

under HTTP 503. Every other unserved path answers `unsupported_operation`
(400) naming the path, so no request to this host ends in an untyped 404. The
same `needs_paired_map` code is what a map-bound command should raise from its
descriptor before it calls out at all; this is the host-side half of it.

### One owner (all routes)
One Server is signed in as exactly one owner, and its owner-only loopback
bearer is that owner's. A request with any other bearer is
`401 {"error": "server access denied"}` — that is the whole rule, and there is
no header that names an account: a second identity is not something this
process can have. Many users are many machines (`ds server serve` per machine,
its own `--state-dir` and `--listen`), which is the deployment model, not a
gap. The `x-ds-principal` header is **deleted**; a client that still sends it
is simply not read, and no route answers `multi_principal_unsupported` —
there is no request a second account can make here. That code survives in the
one place a second account really does meet one Server: `ds server serve`
refuses to adopt a protected state directory that already belongs to another
owner, by that name, with the remedy of a host of its own.

## 3. `Job` gains `context`

`compute_jobs::Job` now serialises

```json
"context": {"principal_uid","lane","deployment","install_id","project","client",
            "operation","job_id","idempotency_key","input_sha256","admitted_at_ms"}
```

(absent on a pre-slice row that could not be recovered). `ds server status`
can render `job.context.project`; nothing else in the job shape changed.

## 4. `serve` — what `lib.rs` must construct

`host::App` gains ONE field:

```rust
pub sessions: Arc<crate::server_sync::sessions::ServerSessions>,
```

built in `serve` with

```rust
let limits = ds_command_kernel::execution_context::Limits {
    global_running: workers,
    per_project_running: per_project,          // --per-project, default max(1, workers / 2)
    per_project_queued: per_project_queued,    // default 512
    global_queued: global_queued,              // default 4096
};
let sessions = crate::server_sync::sessions::ServerSessions::native(
    connection.clone(), directory.join("store.sqlite"), limits)?;
```

`ServerSessions::native` performs **no** network call — it reads the
protected native state on this machine and nothing else — and requires **no**
saved project, so `ds server serve` now starts for an account that has never
run `ds auth project use`, and starts with no upstream. `--per-project <count>`
is optional and **1..workers-1** on a host with more than one worker (a share
equal to the worker count would let one project hold every worker while
another waits, which is not a share); the default is `max(1, workers / 2)`.
`headless_project_not_selected` is not in the server refusal rosters: it only
ever described the old startup requirement.

## 5. One additive `ds-cli-auth` export this slice adds

```rust
pub struct HeadlessPrincipal { account_uid, deployment, install_id }   // accessors
pub fn headless_principal(lane_value: &str) -> Result<HeadlessPrincipal, Failure>
```

The Server's connection identity **without** a saved selection —
`headless_sync_context` minus the project, which is what a host that admits a
project per operation actually needs — read from the local probe, never
refreshed over the network to obtain. There is no project-directory export:
the Server has no use for one. And no saved selection is read anywhere in the
Server, for any purpose: `serve` does not call `probe_headless_identity`.

## 6. The kernel seam, and two notes for the writers after this one

The seam is gone rather than satisfied. `AdmitRequest` has no `membership`
member (`deny_unknown_fields` refuses one), the runtime has no
`MembershipSource`, `still_admitted`, `membership_holds` or
`membership_revoked`, and `WorkerContext` has no saved selection. Admission is
the connection identity, the named project (a sealed input's outranks it) and
the key with its input digest. Nothing in the runtime touches the network at
all, so a claimed job runs exactly what was admitted.

**The durable job id changed.** `ds_compute_runtime::job_id` now digests
`(owner, lane, project, key)`. Equal keys in projects A and B are two pieces of
work with two ids rather than a `scope_mismatch_for_key`; inside one project a
key reused for another operation or other bytes still refuses. Rows written
before this scheme keep the ids they have, which can no longer be derived from
a key, so such a row is never adopted by a later submission.

**For the proof writer.** `tests/isolation.rs` and `tests/fixtures/mod.rs`
assert some of the deleted behaviour and are yours to repair, not mine:
`WorkerContext` has lost `membership` and `saved_project`;
`runtime::recover_contexts(path, identity)` takes two arguments and
`Recovery::from_saved_selection` is gone; a legacy transformer row now stays
`context_unrecoverable` instead of recovering into project C
(`item5_a_restart_recovers_every_context_including_rows_a_released_server_wrote`);
`multi_principal_access_is_refused_by_name_rather_than_served_quietly` tests a
deleted mechanism; and the Solar submissions must post
`{"input_path": …}` rather than the envelope bytes. The one remedy sentence for
`context_unrecoverable` is now identical in `sessions.rs` and in `ds server`'s
rosters: *read that job's result and resubmit under an explicit --project*.

**`ds-compute-runtime/Cargo.lock` gained `rusqlite` as a dev-dependency** (one
line, commit `8d07f80`). It is legitimate and stays: writing a row exactly as a
released Server wrote it — no `context` member — is the only honest way to
prove recovery, and that needs the same SQLite the store itself uses. It was
already in that lock through `ds-sync-store`, so nothing new is compiled.

## 7. What this slice does not do yet — say it, do not discover it

1. **CLOSED (2026-09-12).** A layer request named only the project the
   Server's account had selected. `ds-cli-auth` now has the explicit-project
   layer fence (`capture_layer_scope_fence_for_project`,
   `layer_config_for_project`, `layer_reorder_for_project`), `ds-layer-ops`
   opens its native source with `Native::for_project`, and the routes open
   theirs for the project the kernel recorded. Any project the owner's account
   can read is served; the saved selection is not read on this path at all.
   `capture_layer_scope_fence` / `layer_config_fenced` stay for the one caller
   whose subject IS the selection: `ds map layer …` with no `--project`.
2. **One owner per Server is the model, not a gap.** One authenticated owner,
   enforced by the bearer alone; a second account is a second `ds server serve`
   (its own `--state-dir` and `--listen`) or, in the owner's deployment model,
   a second machine. Nothing here is built for many users in one process: no
   per-principal limits, no multi-user auth, no fairness beyond the owner's own
   projects sharing one machine.
3. **`/v1/activity` needs a gateway session per project**, so its per-project
   envelope is proven for its scope selection (`project_scopes`) and its
   pre-startup refusal, not for a live Sync Center projection.
4. **The Solar route takes the path now** (§2), which closes what this list
   used to record as open. Where the desktop's own command takes bytes, the
   route still takes bytes: transformer processing.
5. **Two on-disk roots.** The Server state directory and the desktop data
   directory are still two homes; converging them into one is slice-2 work,
   alongside the instance registry.
