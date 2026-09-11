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
ds server submit --input transformer-batch.json --key processing-001
ds server solar submit --input pala.server-submission.json --key solar-001
ds server status
ds server activity --lane stable --output json
ds server status --job <id>
ds server result --job <id> --out result.json
```

Input is the existing `ds.fast-lv.request/v1` contract documented in
[design.md](design.md). Settings and configuration are explicit captured inputs.
Submission acknowledges only after durable storage; retries with the same key
and bytes return the same job. Changed bytes under that key refuse. Independent
requests execute concurrently up to kernel CPU/memory admission; the engine
uses its shared native Rayon pool inside each request. `--workers` can reduce
that capacity. Results preserve per-transformer engineering outcomes, including
failures; `completed` means the complete result document was produced.

`server solar submit` accepts one private `ds.solar.server-submission/v1`
envelope. It contains the existing prepared calculation fields (`prepared`,
optional `render_charts`, optional `run_id`) and the exact matching
`ds-solar.prepared-publication-claim/v1` as `publication_claim`. The Server
checks the claim's project, city, city-content digest and prepared-input digest
against the prepared calculation, and seals the claim's snapshot SHA-256,
input-base fingerprint and actor-bound receipt into the durable job. The
existing compute-artifact authority revalidates that sealed claim under the
selected native project before publication. A missing, malformed, expired or
stale claim is never published: missing or malformed claims refuse at
admission, and an expired or stale claim is recorded by the publication
authority. A server never stamps a newly fetched snapshot onto an older
prepared input. The envelope is owner-private because the claim carries the
actor-bound receipt. It is a sealed request body, not a server-readable client
path or a browser cache reference.

## Layers on the running Server

`ds server layers list|show|hide|reorder` drive the layer drawer's catalogue,
visibility and order on the running Server with no Tauri process, browser,
paired map or rendering engine. The Server and `ds map layer …` call ONE
shared Rust owner (`ds-layer-ops` over `ds-command-kernel::layer_state`); the
HTTP host and the CLI embed no rule of their own. Requests travel over the
protected owner-only loopback connection (`connection.json` bearer, native
authority renewed and revocation observed as for jobs); remote operators use
SSH. The Server reads the assembled document under its native account and
selected project, remembers visibility in its native layer store fenced by
lane, account and project, and admits order overrides through the kernel
before the governed write. Preferences never become another account's
visibility because both reached one host: a restart under another account
reads that account's own scope and leaves the previous one untouched.

```bash
ds server layers list --lane canary --zoom 12 --output json
ds server layers hide --layer survey/poles --lane canary --output json
ds server layers list --lane canary --output json          # retained across restart
ds server layers show --layer survey/poles --lane canary --output json
ds server layers reorder --order survey/poles=100 --yes --lane canary --output json
```

Routes: `GET /v1/layers?refresh=&limit=&zoom=`, `POST /v1/layers/visibility`
`{"layers": [canonical ids], "visible": bool}`, `POST /v1/layers/order`
`{"orders": [{"layer_id", "order"}]}`. Refusals are typed on the wire
(`class`, `code`, `error`, `remedy`) and re-raised by the CLI under the same
code: `unknown_layer` (runtime ids are never accepted), `duplicate_layer`,
`invalid_order`, `invalid_number`, `local_layer_refused`,
`project_context_changed` (the document no longer matches the selected
project), `auth_identity_mismatch`, `headless_signed_out`. Nothing here
pretends a renderer mounted anything: `writes` name the layout word a
renderer would apply to each runtime layer.

Browser-to-Server layer control is **not** provided: the connection bearer is
an owner-only local control credential and must not reach a web visitor.
Remote presentation waits on an authenticated host transport for browsers,
which is a separate authority change.

The default state root is `$XDG_STATE_HOME/ds/server/<lane>` or
`~/.local/state/ds/server/<lane>`. `--state-dir` overrides it explicitly. It must
be owner-only. `connection.json` is a local control credential and must never
be printed, copied into logs or exposed to a web visitor. It is distinct from
the protected upstream DS login. Jobs are fenced by UID, lane and credential
audience. Native authority is renewed at most every 15 seconds across workers;
sign-out/account changes fence work immediately when observed. Upstream device
revocation is observed on renewal. Loss of authority pauses execution and fences
result commits. Restoring the same identity permits pending work to recover.
Each running host also retains its starting provider/device binding. It cannot
silently fall back from a revoked or removed device to a stored password login
for the same UID. Changing that binding requires restarting the host; retained
jobs still belong to the same canonical account, lane and audience.

Worker leases renew during computation. After a killed process, an expired
30-second lease can be reclaimed; completed jobs are never rerun. Cancellation
retains input and fences late results. The native engine finishes its current
computation before releasing CPU; cancellation does not promise mid-algorithm
interruption. Cloud input acquisition/publication, device management UI and
public browser authentication remain separate migration gates.

Development: `ds-web/run-linux-server.sh` builds locally and restarts its own
server after successful Rust rebuilds. Failed builds preserve the running
server. Durable state is outside the checkout and survives reloads. It uses
an installed, digest-bound native profile catalog, or the explicit debug-only
`DS_NATIVE_CLIENT_PROFILE_BUNDLE`. `./run-linux-server.sh --cli auth status`
uses the exact development executable; `--cli auth login --email <email>` or
`--cli auth link begin` establish its native identity when needed.

MCP exposes submit, status, cancel and result through the existing command
catalog. Starting the foreground host is a terminal/service operation and is
excluded from MCP so it cannot block a tool response indefinitely.

`server activity` reads the server's shared Sync Center state, including native
job lifecycle and artifact publication receipts. It uses the same protected
connection and native identity as other Server commands; it creates no queue.
A completed calculation and a published artifact are separate states.
