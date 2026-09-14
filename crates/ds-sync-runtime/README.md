# Shared native synchronization runtime

`ds-command-kernel::sync` owns the plan, conflicts and wake decisions. This
crate executes that plan through three host adapters: `Gateway`, `Producer`
and `Reads`. `StoreHost` keeps rows, grants, receipts and project leases in
`ds-sync-store`, fenced by account, deployment and installation.

Desktop's `src-tauri/src/sync_loop.rs` consumes this runtime. Linux Server's
`ds-cli-server` also consumes it directly, using native user authentication,
registered installation identity and sealed prepared Solar jobs. Solar publishes
through its existing Compute Artifacts protocol under the same StoreHost lease,
rows and receipts. It does not use Network Reporter's work-grant protocol.
`reports::ReportProducer` is the shared Desktop/Server inventory over
`ds-report-artifacts`, which owns sealing, promotion, recovery and verified
output handles outside Tauri. Server drains sealed exports through `run_reports`,
registering the exact reporter release before each work grant. That scoped pass
never submits Solar rows to the report protocol.

`StoreOnlyProducer` and `UnavailableGateway` explicitly refuse missing effects;
other producers require their own real adapters before their effects are enabled.

This crate is Tauri-independent despite its current location in `ds-web`.
Kernel modules own deterministic sync decisions and store transitions; this
native crate executes IO, and each host supplies its platform adapters. Neither
Server nor CLI depends on the Tauri application to run synchronization.

Each active pass has a unique lease identity, renewed during blocking IO.
Loss of authority or lease fences later effects and successful receipts. A
host's availability callback must observe its actual authorization as well as
connectivity. The loop releases its lease on unwinding. Renewal does not add a
sync polling policy; the kernel remains responsible for sync wake decisions.

Downloaded replicas are checked against the current remote head and their
output hashes after restart. They are observations, never producer or outbox
rows: a changed head replaces its cached files, a removed head cannot be
republished from the cache, and corrupted bytes are downloaded again. The
private head receipt is committed atomically after the verified outputs.

## Native artifact I/O

`VerifiedReads` is the one native artifact download owner. It accepts only a
bounded HTTPS signed ticket on `storage.googleapis.com`, refuses redirects and
userinfo, stages bytes privately, and places an output only after its declared
size and SHA-256 match. Desktop and Server give it different private cache
roots; neither keeps a second downloader.

`native_transfer::drive_verified_output` is the corresponding one native
resumable upload driver for receipt-sealed, seekable artifact handles. It pins
the same storage origin, sends no control-plane credential to the object
session, refuses redirects, re-proves the sealed SHA-256 while streaming, and
asks `ds-command-kernel::transfer` for every retry, resume and terminal state.
Desktop's Tauri command is only an IPC/progress wrapper; Server calls this
driver through its producer adapters.

`ds-cli-auth::Transport::upload_bytes` remains a narrow adapter for
`ds-client-core`'s arbitrary `Read` upload contract. It has no Sync Center
publication entrypoint or producer routing: its sole purpose is translating
that older client-core callback into the same kernel transfer protocol until
client-core can require a receipt-sealed seekable handle.

The shared tests cover durable reopen, duplicate worker exclusion, lease
renewal and ownership loss, interrupted transfer, cancellation, revocation
before publication, admission failures and verified downloads. Desktop tests
also drive its real resumable transfer adapter through this loop.

```sh
cargo test --manifest-path crates/ds-sync-runtime/Cargo.toml
cargo clippy --manifest-path crates/ds-sync-runtime/Cargo.toml --all-targets --all-features -- -D warnings
```

The product destination is
[the headless runtime contract](../../../ds-command-kernel/docs/contracts/ds-headless-application-runtime.md).
