# Shared native synchronization runtime

`ds-command-kernel::sync` owns the plan, conflicts and wake decisions. This
crate executes that plan through three host adapters: `Gateway`, `Producer`
and `Reads`. `StoreHost` keeps rows, heads, receipts and project leases in
`ds-sync-store`, fenced by account, deployment and installation.

Desktop's `src-tauri/src/sync_loop.rs` consumes this runtime. Linux Server's
`ds-cli-server` also consumes it directly, using native user authentication,
registered installation identity and sealed prepared Solar jobs.

A report publishes on the shared record's one door, `compute_artifacts`:
`open` declares the row as the seal recorded it (inputs fingerprint, outputs,
and the engine release and build as provenance), the transfer moves exactly the
bytes the record asks for, and `finalize` is asked only once they committed.
The record passes every verdict: a 409 or `stored_stale` settles the row as a
conflict, a 403 or a 400 at `open` as refused, and the bytes leave in the same
pass; anything that reached no verdict keeps the row held with its cause.
There is no grant to obtain first — the session's own authority is asked,
on the one JWT (owner ruling 2026-09-18). Solar publishes through the same
door under the same StoreHost lease, rows and receipts, with its own
declaration.
The sync store is the queue (owner, 2026-09-20). `reports::seal` writes a
report batch's bytes and integrity receipts through `ds-report-artifacts` and
records its `held` row in the store in the same acknowledged step — a seal the
store cannot record leaves no bytes behind, and a room sealed again frees the
batch it supersedes. `reports::ReportProducer` moves and frees bytes the
store's rows name (`LocalRow::bytes_locator`); its inventory is read only to
adopt a batch sealed before the seal wrote its row (`adopted` receipt, once)
and to free a batch the store has settled as lost (`reclaimed`). Nothing
enumerates the artifact directory to decide what is queued;
`sync_store::queue_status` reads the rows. Server and desktop drain sealed
exports through `run_reports`, a scoped pass that never submits Solar rows.

Solar seals the same way. `solar::seal` records a completed prepared job's
`held` row at the job's completion, under the host's local fence, with no
session and no gateway — the job's `result` column in the same SQLite is the
bytes, so the seal has none to write — and a city computed again frees the
result it supersedes through the producer (`reclaimed:superseded_locally`) —
and a completion sealed BEHIND a later one of its city (a restart replays
completions newest first) frees its own result instead and leaves the newer
row standing (`SealOutcome::Superseded`). Both seals run one choreography,
`seal::record` and `seal::free_superseded`. That seal is the ONLY way a
Solar row is born: a completed job the store holds no row for (completed
before the seal existed, or one whose seal the store could not record) is not
a publication and is never adopted — the Solar producer's inventory offers
nothing, retires such a job's result on its next observation
(`Store::retire_job_result`; the job can be run again), and frees a result
the store has settled as lost the same way. `run_solar_publications` drains
the rows the store holds.

A host that runs several engines supplies ONE `Producers` set keyed by engine
— report + Solar on the Server and on the desktop — so a lost row is freed by
the producer that owns its bytes whichever pass observes it, and a row of an
engine the host has no producer for is `reclaim_failed` with the engine named,
never `reclaimed` with 0.

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
roots; neither keeps a second downloader. A failed read names its closed cause on
the failed `download` receipt (`failure_code`: the gateway transport's tokens,
plus `digest_mismatch` and `size_mismatch`; `read_cause`), and a retry is
unchanged.

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
before publication, the record's verdicts (stale, refused, held) and verified
downloads. Desktop tests
also drive its real resumable transfer adapter through this loop.

```sh
cargo test --manifest-path crates/ds-sync-runtime/Cargo.toml
cargo clippy --manifest-path crates/ds-sync-runtime/Cargo.toml --all-targets --all-features -- -D warnings
```

The product destination is
[the headless runtime contract](../ds-command-kernel/docs/contracts/ds-headless-application-runtime.md).
