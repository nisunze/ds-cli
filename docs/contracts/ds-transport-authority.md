# DS transport authority — one Rust communication and sync client

**Status:** contract adopted 2026-09-10 by owner decision. Implementation in
flight; the "Implementation ledger" at the end records exactly which slices are
landed, which are partial, and which are not started. Do not read an unmarked
section as shipped.

## 1. The decision

Every DS request to API Gateway, and every byte transfer to a DS-minted storage
session, passes through **one shared Rust client**. Rust owns:

operation validation · identity and lane binding · authorization headers ·
request coalescing · cancellation · retries · streaming · resumable transfers ·
durable scheduling · acknowledgement handling.

GUI, CLI and MCP consume the *same* operation contracts and the *same* state
machines. There is no second interpretation of a publication, an upload
acknowledgement, or a retry decision.

TypeScript keeps Svelte presentation, DOM and file-picker events, rendering, and
thin typed bindings. It does not keep a transport engine.

### 1.1 What this supersedes, explicitly

| Superseded statement | Where it lives | New position |
|---|---|---|
| "`ds-client-core` … There is no Tauri command: the consumer remains the independent `ds` sidecar." | `ds-web/docs/native-client-core-contract.md` §Status | The Tauri shell is now a first-class consumer of the same client authority. It is still not a second product. |
| "The crate is at repository root, not under `src-tauri`, and `src-tauri` does not depend on it in this slice." | same, §Narrow architecture exception | `src-tauri` may depend on the shared transport/contract layer. It still may not fork product logic. |
| "The crate can do exactly twenty-eight things" (closed surface framed as a permanent ceiling) | same, §Closed surface | The surface is closed *per reviewed operation registry entry*, not capped at a number. Adding an operation is a descriptor edit plus a review, never an arbitrary-URL escape. |
| "No product logic lives in Rust." (read as "no communication logic in Rust") | `ds-web/CLAUDE.md` §Desktop app | Unchanged for map/layer/design/product logic. Communication and sync policy are explicitly NOT product logic and are Rust-owned. |
| TypeScript chunk/retry/probe engine (`src/lib/api/resumableUpload.ts`) | ds-web | Deleted. Its behaviour is replaced by §4. |

This decision does **not** authorize moving rendering, engineering, project
policy, Firestore encoding, or lifecycle authority into Rust or into the shell.
ds-brain remains the server-side authorization, lifecycle, publication and
data-authority owner. The Tauri shell remains an IO host.

## 2. Layering (no cycles)

```
ds-command-kernel            deterministic CONTRACTS, zero IO, zero HTTP
  ├── gateway::registry      the closed operation table (§3)
  ├── transfer               resumable-transfer protocol state machine (§4)
  ├── compute_artifact_engine durable publication state machine (already owned)
  └── failure classification typed causes (§5)
        ▲                 ▲                        ▲
        │                 │                        │
ds-client-core        src-tauri                ds-web WASM
(identity, lane,      (native IO host:         (browser IO host:
 profile, session,     ureq + files)            fetch + IDB)
 typed operations)
        ▲
        │
      ds-cli
```

- `ds-command-kernel` has **no** HTTP dependency and gains none. It is the only
  crate all three hosts already link, which is why the shared contracts live
  there.
- `ds-client-core` already depends on `ds-command-kernel`. It keeps identity,
  lane/audience binding, the refresh-token store port, and the typed operation
  bodies; it gains the registry as its route/authority source of truth.
- Hosts supply IO ports only. A host never re-decides protocol, retry,
  ordering, or acknowledgement meaning.

**A native failure must never select the browser adapter as a recovery path.**
Placement is decided once, before the operation runs, and is not revised by a
failure.

## 3. The operation registry

One reviewed descriptor, `ds-web/routing/operations.json`, is the single source.
The generator emits two artefacts from the same bytes:

1. the existing content-hashed TypeScript route manifest, and
2. a Rust table compiled into `ds-command-kernel::gateway`.

A parity test fails the build when they diverge. Feature code names an
operation id; it never names an origin, a method, or a path.

Each entry declares exactly:

| Field | Meaning |
|---|---|
| `id` | logical operation, e.g. `compute_artifacts.open` |
| `method` | HTTP method |
| `path` | gateway path |
| `action` | body `action`/`operation` key when the path multiplexes |
| `transport` | `json` \| `ndjson` \| `blob` \| `sse` \| `resumable_upload` \| `anonymous` |
| `authority` | `gateway` (user bearer) \| `release` (anonymous) \| `storage_session` (session URI is the credential; **no DS bearer**) |
| `principal` | `user` \| `anonymous` |
| `lane_header` | whether `X-DS-Processing-Lane` applies |
| `idempotency` | `idempotent` \| `idempotent_with_key` \| `unsafe` |
| `retry_class` | `none` \| `transport_only` \| `transport_and_5xx` |
| `timeout_s`, `response_cap_bytes` | bounds owned by the registry, not the caller |

Rules:

- **No arbitrary URL, method, origin or token may cross a host boundary.**
  Generated bindings are welcome; an `invoke('http', {url, method, token})`
  shaped door is not, and the static gate in §7 fails it.
- `authority: storage_session` operations attach **no** `Authorization`,
  **no** `x-api-key`, and **no** `X-User-Email`. The session URI is the whole
  credential. This is what makes it safe for the native host to speak to
  Google storage directly.
- A user bearer is attached only after the target origin has been validated
  against the compiled profile/manifest origin. Redirects are disabled for
  every authenticated call and for every resumable call (§4.2).

## 4. Resumable transfer protocol

`ds-command-kernel::transfer` is a **pure** state machine. It performs no IO,
allocates no buffers for file bodies, and is identical on every host.

```
        ┌──────────── Probe ─────────────┐
start ─▶│  (empty body, bytes */total)   │──▶ Committed(n) ─▶ SendChunk(n..)
        └────────────────────────────────┘        │
                                                   ├─ AlreadyComplete ─▶ Done
                                                   ├─ Expired ─▶ Reopen (once)
                                                   └─ Inconclusive ─▶ Retry probe
```

### 4.1 Events the host must be able to report truthfully

The host reports **facts**, never conclusions:

| Event | Carried facts |
|---|---|
| `ProbeResponse` | status, `Range` header present?/value, body length |
| `ChunkResponse` | status, `Range` header present?/value, bytes the host actually wrote to the socket |
| `TransportFailure` | typed cause (§5) and the phase it occurred in |
| `Cancelled` | cancellation source |

### 4.2 Truthful transfer facts — required behaviours

These are the boundaries the machine must get right. Each has a deterministic
test in `ds-command-kernel`.

1. **308 with a `Range` header** → committed prefix is `range_end + 1`.
2. **308 with no `Range` header** → the server has committed **zero** bytes of
   this session so far. That is a *fact*, not an ambiguity: resume from 0. It
   must not be conflated with "unknown".
3. **200/201** → the object is already complete. Finalize only; never re-send.
4. **404/410** → the session is *proven* expired. Reopen at most once per row
   (the existing `reopens` budget in `compute_artifact_engine`).
5. **Ambiguous acknowledgement** (socket closed after the bytes were written,
   read timeout after the last byte, empty/malformed status) → **probe**, never
   restart, never assume success, never assume failure.
6. **Inconclusive probe** → bounded retry of the probe with the typed cause
   preserved. A session is **never** restarted because a probe was
   inconclusive. This is the exact defect being fixed (§9).
7. **Truncation** — server-committed prefix shorter than the bytes the host
   wrote → resume from the server's number, not the host's.
8. **Digest / length mismatch** at finalize → terminal `conflict`, never a
   retry loop.
9. **Malformed `Range`** → typed `RangeMalformed`, treated as inconclusive
   (rule 6), never parsed permissively into a number.
10. **Redirect following is disabled.** A 308 here means *resume incomplete*,
    not *permanent redirect*. A native client that follows redirects turns a
    resumable status into a wrong request, and can move a credential to an
    unvalidated origin.
11. **Bytes sent ≠ bytes committed.** Progress is reported as two separate
    numbers. `100%` is reserved for an acknowledged commit. Local readiness is
    never reported as sync.
12. **Cancellation** is honoured before dispatch, during transfer, and during
    backoff, in each case leaving durable state that a later resume can use.
13. **Bounded retries with coalesced wakeups.** Progress/idle semantics: a
    transfer that is making progress is not killed by a wall-clock deadline;
    a transfer making no progress fails with `Timeout{phase}`. There is no
    blanket 30-second operation timeout on a byte transfer and no endless
    invisible retry.

### 4.3 Memory and backpressure

- The host streams **bounded chunks** straight from the verified artifact.
- There is **no whole-file buffer**, no base64, and no JSON IPC of file bytes.
- A chunk-size or preview limit is **not** an export/transfer limit. The
  former 32 MiB and 64 MiB ceilings are removed from the transfer path; a
  separate, clearly named preview bound may remain for previews only.
- The digest is computed **streaming**, in the same pass that feeds the socket
  where possible, so a large PDF is never materialised twice.

## 5. Typed causes

Transport results are typed and safe. `ds-command-kernel::transfer::Cause`:

`HttpStatus(u16)` · `Dns` · `Connect` · `Tls` · `Timeout{phase}` ·
`ConnectionClosed` · `Offline` · `Cancelled` · `Truncated` ·
`DigestMismatch` · `LengthMismatch` · `RangeMalformed` · `SessionExpired` ·
`BlockedByHostPolicy` · `Inconclusive`

Rules:

- A cause is derived from an observed fact. `Inconclusive` is a real value, not
  a default for "we did not look".
- Causes are **safe to surface**: they never contain a session URI, an
  `Authorization` value, a query string, or a response body that could carry a
  credential. Status codes and coarse transport classes are safe and required.
- The browser host may additionally report `BlockedByHostPolicy` when the
  platform refuses the request without a status (the browser's opaque
  CORS/permission failure). That is an honest classification of an observed
  refusal, not a diagnosis of the server.

## 6. Durability, ownership and serialization

| Host | Durable publication store | Serialization |
|---|---|---|
| Browser tab | IndexedDB (`ds-web` `outbox`) | one tab wins by the kernel's `sync_lease_id` / `sync_lease_until` CAS; a stale lease is superseded, never duplicated |
| Installed desktop | the WebView's IndexedDB, plus the native crash-durable publication batch receipts under the reporter/solar workspaces | native batch receipts are the recovery authority; the kernel lease still serialises drains, and the native transfer runs in the shell process so it survives a component unmount |
| Headless `ds` | native store owned by the CLI | same kernel lease semantics |

- The native runtime executes independently of any Svelte component staying
  mounted. `installComputeArtifactSync()` is app-lifetime, and the byte
  transfer itself runs in the shell process.
- Late callbacks cannot revive cancelled work: every step re-asserts owner uid,
  lease id and generation before it commits.
- Queue sanitation never destroys source artifacts.

## 7. Static gate — one authority, provably

`ds-web` ships a check that enumerates every remaining gateway/remote call site
and fails on any unreviewed bypass:

- allowed: the generated client entry points, and the registry-driven native
  transport;
- refused: `fetch`/`XMLHttpRequest`/`axios`/`EventSource` reaching a DS or
  Google origin from feature code, any raw URL/method/token IPC, any second
  Rust HTTP client that is not the shared adapter.

The check reports **exact remaining exceptions** with a reason. Remaining DS
gateway bypasses mean the goal is not complete, and the report says so rather
than passing quietly.

## 8. What must NOT happen

- No desktop fallback to browser fetch/XHR for DS traffic.
- No second Rust crate that also owns transport.
- No hidden JS fallback behind a deleted TS engine.
- No legacy transport fallback or silent dual-write period at completion.
- No Firestore polling for progress, leases, integrity, or previews.
- No printing gateway, cloud PDF generation, new upload route, or invented
  artifact namespace.
- No proxying of bytes through API Gateway or ds-brain.

## 9. The defect this replaces

On 2026-09-10 an installed Stable desktop reported, repeatedly:

```
gpkg: resumable session could not be checked
```

Read the two sides together:

- `ds-web/src/lib/api/resumableUpload.ts` `probeResumableSession()` wraps the
  whole probe in `try { … } catch { return { status: 'error', offset: 0 } }`.
  Every distinct cause — an offline-mode refusal, a DNS failure, a TLS failure,
  a platform refusal with no status, a 403, a malformed `Range` — collapses
  into the single token `error`.
- `ds-command-kernel/src/compute_artifact_engine/transition.rs:153` maps that
  token through a catch-all `_ => Err(retry(…))`.

The result is a permanent, uninformative retry loop that establishes nothing.
The cause of the Stable failure has **not** been established and this contract
does not assert one; the fix is to make the cause observable and to move the
transfer to a host where the browser's opaque failure modes do not apply.
Weakening the `Attachment probe (v0)` validation, or admitting the known-bad
`tables/alignments.arrow` fixture, is not part of this and is forbidden.

## Implementation ledger

| Slice | Owner repo | State |
|---|---|---|
| S1 transfer protocol state machine + typed causes + tests | ds-command-kernel | see repo `docs/contracts/` and commit log |
| S2 native transfer host (streaming, ureq, no redirects) | ds-web `src-tauri` | " |
| S3 browser/WASM host adapter over the same protocol | ds-web `src/lib` | " |
| S4 operation registry + parity + static bypass gate | ds-web + ds-command-kernel | " |
| S5 removal of the TS upload/retry engine and duplicate authority | ds-web | " |
| S6 cold/warm, offline, interruption, cross-machine acceptance | all | " |

`HANDOVER.md` at the workspace root carries the live per-slice status, commit
IDs, gates run, and unresolved blockers.
