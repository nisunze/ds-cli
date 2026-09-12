# The Server's routes under one verified execution context (isolation slice 1)

Written by the Rust-native writer for the CLI-client writer. Normative source:
`ds-command-kernel/docs/contracts/ds-execution-context.md` and the slice
contract §2/§3. Everything below is what `crates/ds-cli-server/src/host.rs`
and `layers.rs` actually accept and answer after this slice; `lib.rs` (the CLI
side) is yours.

## 0. The one rule

A Server operation is about **one project, named by the caller and verified
against fresh membership at admission**. The Server no longer reads
`ds auth project use`'s saved selection as execution state — `serve` starts
without a selection, and the saved selection is the *client's* default, which
the client sends explicitly on every call. So:

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
| `project_not_visible` | invalid_input | 400 | not a member, or the membership snapshot expired |
| `scope_mismatch` | conflict | 409 | the sealed Solar input names another project than `--project` |
| `scope_mismatch_for_key` | conflict | 409 | that key already names another project or operation |
| `payload_changed_for_key` | conflict | 409 | that key was admitted with other bytes |
| `principal_mismatch` | conflict | 409 | that key/row belongs to another authenticated identity |
| `not_visible` | conflict | 409 | `job not found` — the ONE sentence for every invisible job |
| `capacity_exhausted` | unavailable | 429 | queue or worker admission is full; `retry_after_ms` |
| `context_corrupt` | invalid_input | 400 | a caller field is out of bounds (e.g. an over-long project) |
| `context_unrecoverable` | conflict | 409 | a pre-slice row has no project to recover into |
| `membership_revoked` | conflict | 409 | membership was lost while the job was queued/running (job `error`) |
| `multi_principal_unsupported` | unauthorized | 401 | the request names a principal other than the Server's |

`not_visible` says **`job not found`** and nothing else, for a foreign
principal, a foreign lane, the wrong project and an id that never existed.
Do not add prose to it, and do not print the requested project back.

## 2. Routes

### `POST /v1/transformer-processing/:key?project=<id>`
`project` — REQUIRED in practice (transformer inputs carry no project by
design). Body: `ds.fast-lv.request/v1`, ≤ 64 MiB. 202 →
`{"job": <Job>}` where `<Job>` now carries `"context": {…}` (see §3).
Refusals: `project_required`, `project_not_visible`, `scope_mismatch_for_key`,
`payload_changed_for_key`, `principal_mismatch`, `capacity_exhausted`,
`context_corrupt`.

### `POST /v1/solar-processing/:key?project=<id>`
`project` — OPTIONAL: the sealed `ds.solar.server-submission/v1` envelope names
its own project and that name wins. Sending it is still recommended (it is how
a caller learns it prepared the wrong city): a `project` that differs from the
sealed input is `scope_mismatch`. 202 → `{"job": <Job>}`.

### `GET /v1/jobs?project=<id>`
`project` — OPTIONAL narrowing. Without it: every job visible to this
connection, whatever its project. With it: only that project's, and a project
outside membership answers an empty list (never a disclosure). Answer shape is
unchanged: `{"jobs":[<Job>…],"more":<bool>}`.

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
(ordered by project id). With `project`: exactly that entry, or an empty
`projects` array when the project is not visible. Render accordingly.

### `GET /v1/layers?project=<id>&refresh=&limit=&zoom=`
### `POST /v1/layers/visibility?project=<id>`  body unchanged: `{"layers":[…],"visible":<bool>}`
### `POST /v1/layers/order?project=<id>`  body unchanged: `{"orders":[{"layer_id","order"}]}`

`project` — **REQUIRED**, as a query parameter on all three (the bodies stay
exactly the `ds_layer_ops` request types, so nothing about `ds map layer …`'s
shapes changes). Three answers, all proven through the real listener:

| named | answer |
|---|---|
| nothing | `project_required` (400) |
| a project outside membership | `project_not_visible` (400) |
| a member project that is not the one the Server's document source is on | `project_context_changed` (409), remedy naming both |
| the project the document is for | the catalogue, unchanged |

A refused layer request writes nothing, under any project.

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

### Principal header (all routes)
An optional `x-ds-principal: <uid>` is honoured only when it equals the
Server's own account uid; any other value is `multi_principal_unsupported`
(401). One Server serves one authenticated principal and many of its projects;
a second principal needs a second `ds server serve` with its own
`--state-dir`. `ds` never needs to send this header — it is the explicit,
tested statement of the boundary.

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

`ServerSessions::native` performs **no** network call and requires **no**
saved project, so `ds server serve` now starts for an account that has never
run `ds auth project use`. Add `--per-project <count>` to `SERVE` (optional,
1..=workers, default `max(1, workers / 2)`) and drop
`headless_project_not_selected` from the server refusal rosters where it only
described the old startup requirement.

## 5. Two additive `ds-cli-auth` exports this slice adds

```rust
pub struct HeadlessPrincipal { account_uid, deployment, install_id }   // accessors
pub fn headless_principal(lane_value: &str) -> Result<HeadlessPrincipal, Failure>
pub fn headless_projects(lane_value: &str) -> Result<Vec<String>, Failure>
```

The Server's connection identity **without** a saved selection —
`headless_sync_context` minus the project, which is what a host that admits a
project per operation actually needs — and the exact project directory a
membership snapshot is made of (the same one `ds auth project use` verifies a
selection against, both credential paths). `headless_sync_context` is
unchanged; the Server calls it in exactly one place, `serve`, to learn the
saved selection a *pre-slice* transformer row can be recovered into, which is
the one use the execution-context contract §6 gives it.

## 6. What this slice does not do yet — say it, do not discover it

1. **A layer request can name only the project the Server's account has
   selected.** The project is required, verified against membership and
   fenced against the document that comes back, so nothing is ever served
   under the wrong project — but `ds_cli_auth::layer_config_fenced` and
   `capture_layer_scope_fence` still read the saved selection to decide which
   project's document to fetch, so naming a second authorized project answers
   `project_context_changed` instead of that project's catalogue. Closing it
   is a ds-cli-auth slice (an explicit-project layer fence), not a Server one.
2. **Multi-principal is refused, not supported** (`multi_principal_unsupported`),
   as the contract asks. A second account needs a second `ds server serve`.
3. **`/v1/activity` needs a gateway session per project**, so its per-project
   envelope is proven for its scope selection (`project_scopes`) and its
   pre-startup refusal, not for a live Sync Center projection.
