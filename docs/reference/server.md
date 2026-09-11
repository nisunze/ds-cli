# Native server

`ds server serve` hosts the shared Rust compute runtime under the identity
established by `ds auth login` or device linking. Run both under the same Linux
user and lane. No browser, paired Desktop, ADC or service-account impersonation
is involved. The current API is for that owner's control over loopback; remote
operators use SSH. Public multi-user web delegation is not yet implemented.

```bash
ds server serve --lane stable
# From another terminal under the same account:
ds server submit --input transformer-batch.json --key processing-001
ds server status
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
