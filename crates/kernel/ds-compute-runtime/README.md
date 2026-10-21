# Native compute runtime

This native host runs native LV and prepared Solar tasks through their detached
owners and retains historical `ds.fast-lv.request/v1` requests.
`compute_jobs` in the portable kernel owns lifecycle and CPU/memory admission;
`ds-sync-store` owns atomic persistence; `ds-network` owns engineering. The
host performs no backend calls and accepts no browser or cloud credentials.
The calling platform supplies an `Authorizer` and retains native authentication.

Submission retains exact input bytes, project context and the selected addon's
absolute path, SHA-256, identity and worker share before acknowledgement. Capture
occurs inside the atomic admission transaction after idempotency and capacity
checks. A caller key is bound to owner, lane and project; different bytes cannot
replace an accepted request. Restart and retry use the retained addon rather
than resolving a newly activated lane selection. Historical missing or misbound
pins remain readable and cancellable but are excluded from claims and capacity;
the owner exports their input and submits a new key naming the project.
Workers claim jobs transactionally, renew 30-second leases and commit results
atomically. Expired workers and cancelled jobs cannot commit. Authority loss
leaves inputs recoverable under the same identity. Cancellation fences results
and supervises the detached process through the owning cancellation contract.

`completed` means that a result document exists. Each transformer still carries
its own engineering outcome. Cloud acquisition, project configuration capture,
publication and browser delegation must use their existing governed owners;
this host does not supply those operations or certify full domain migration.

The crate is a detached native workspace so the portable kernel can still build
without `ds-network`. Its real-engine/recovery tests run through
`scripts/test-detached-workspaces.sh`, alongside the existing design workspace.
