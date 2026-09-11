# Native compute runtime

This native host runs the existing `ds.fast-lv.request/v1` engine contract.
`compute_jobs` in the portable kernel owns lifecycle and CPU/memory admission;
`ds-sync-store` owns atomic persistence; `ds-network` owns engineering. The
host performs no backend calls and accepts no browser or cloud credentials.
The calling platform supplies an `Authorizer` and retains native authentication.

Submission retains exact input bytes before acknowledgement. A caller key is
bound to owner and lane; different bytes cannot replace an accepted request.
Workers claim jobs transactionally, renew 30-second leases and commit results
atomically. Expired workers and cancelled jobs cannot commit. Authority loss
leaves inputs recoverable under the same identity. Cancellation fences results;
it does not interrupt the engine midway through an algorithm.

`completed` means that a result document exists. Each transformer still carries
its own engineering outcome. Cloud acquisition, project configuration capture,
publication and browser delegation must use their existing governed owners;
this host does not supply those operations or certify full domain migration.

The crate is a detached native workspace so the portable kernel can still build
without `ds-network`. Its real-engine/recovery tests run through
`scripts/test-detached-workspaces.sh`, alongside the existing design workspace.
