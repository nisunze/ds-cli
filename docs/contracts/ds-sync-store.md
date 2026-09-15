# The sync store — one durable queue and artifact store per execution context

**Status:** contract, 2026-09-11, under `ds-application-runtime.md` §1
("Durability and ownership"). Slices R1 (the store), R4 (the verified handoff)
and R8 (fences, leases, lifecycle) of the sync gate campaign.

## 1. What it is

The durable state the sync gate needs between two runs of the host loop,
shaped by the kernel and persisted by the host of each execution context:

| Context | Persistence | Owner of the bytes |
|---|---|---|
| Linux/Windows desktop, server, CLI | SQLite through the portable crate `crates/ds-sync-store` (kernel dependency only; no engine) | the native host |
| Web | IndexedDB rows of the same shape, written by the browser adapter from kernel-returned rows | the browser host |
| Android/iOS | the same rows, checkpointed through the platform's storage | the app host (not yet verified) |

The kernel module `sync_store` owns the ROW SHAPES and every TRANSITION over
them as pure functions (`evaluate(&[u8])`, WASM `syncStore`): recording a
local artifact, recording the remote heads and when they were read, holding a
grant, applying an action receipt, taking and releasing a worker lease,
fencing, and importing legacy pending work. Hosts perform the writes the
transition returns and nothing else; no host re-decides a state.

## 2. Rows

| Row | Key | Fields |
|---|---|---|
| `artifact` | scope + identity | `scope {project | personal}`, `engine`, `operation`, `variant`, `sha256`, `size_bytes`, `produced_at_ms`, `base_revision?` (output-head OCC identity), `input_base_fingerprint?` (source-input digest for a derived artifact), `engine_release`, `engine_build_manifest_sha256?`, `bytes_locator` (host-opaque), `readable`, `state` (`held | uploading | published | conflict | refused`), `state_reason?`, `replay_key` (idempotency across attempts), `updated_at_ms` |
| `head` | scope + identity | `revision` (output-head OCC identity), `head_revision` (monotonic counter), `input_base_fingerprint?` (sealed source-input digest), `sha256`, `published_at_ms`, `read_at_ms` |
| `grant` | scope + `grant_id` | `install_id`, `engines[{name, release, manifest_sha256?}]`, `expires_at_ms`, `raw` (the signed document, opaque) |
| `receipt` | `receipt_id` | `action`, identity, `outcome`, `detail?`, `work_id?`, `head_revision?`, `committed_bytes?`, `at_ms` — append-only evidence |
| `lease` | scope | `worker_id`, `taken_at_ms`, `expires_at_ms` |
| `fence` | context | `account`, `deployment` (lane + origin), `install_id` — every row is stored under one fence; a row from another fence is never executed |

## 3. Transitions (pure)

`Event` → `Transition { rows: [RowChange], effects: [Effect], refusals: [Refusal] }`:

- `LocalProduced { artifact }` → upsert `artifact` (`held`); the next plan is a `local_change` trigger (uploads never wait).
- `HeadsRead { heads, read_at_ms }` → replace the scope's `head` rows; `read_at_ms` becomes the plan's `remote_read_at_ms`.
- `GrantOpened { grant }` → upsert `grant`; superseded grants of the same scope are dropped.
- `ActionReceipt { receipt }` → `upload` committed ⇒ `artifact.state = published` and a `head` row from the receipt (the head is carried, never re-read); `conflict` ⇒ `artifact.state = conflict` with `state_reason`; `refused` ⇒ `refused` with the reason; `download` committed ⇒ `artifact` row for the downloaded head (`held`, readable); every receipt appended.
- `LeaseTake { worker_id, now_ms, ttl_ms }` → granted iff no live lease for the scope, else `refused: lease_held{worker_id, expires_at_ms}`. `LeaseRenew`, `LeaseRelease`. A lease is the ONLY thing that lets a worker drain: two workers never execute the same work.
- `Logout` / `AccountSwitch { account }` → rows of another account are not visible; nothing is deleted (held bytes belong to the project); drains stop.
- `Cancel { identity }` → `held`/`uploading` ⇒ `refused: cancelled` with the receipt; committed work is never undone.
- `HandoffLegacy { rows: [legacy per-user outbox entries] }` (R4) → for each legacy row: an `artifact` row preserving identity, the original `bytes_locator`, digest, `replay_key` = the legacy row id, acknowledgements folded into `state`, unresolved failures as `state_reason`; a `receipt` per imported row naming its origin; **never silent loss**: a legacy row the shape does not admit becomes a `refused` artifact row with `handoff_unmapped` and the raw row kept in `detail`.

## 4. Fences and identity

Every row is stored under `{account, deployment, install_id}`. The plan is
asked per `{scope}` within one fence. Server hosts run under an explicit
device identity (`ds` headless sign-in); ambient credentials are never used.

## 5. Recovery

The store is the truth after UI closure, crash, offline, suspension or
restart: a host restarts by reading its rows, taking the lease, and asking the
plan with trigger `startup`. Transfers resume from the transport's own
persisted `TransferState`, kept beside the artifact row (`bytes_locator` +
`transfer_state`), so completed work is never restarted and never falsely
reported.
