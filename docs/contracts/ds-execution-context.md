# The execution context — one verified record per admitted operation

**Status:** the kernel owner is implemented and proven on
`fable/server-isolation`, 2026-09-12; the compute runtime, Server host and CLI
that consume it are the rest of the slice. Parent:
[project and instance isolation](../../../ds-web/docs/project-and-instance-isolation-contract.md);
refines [Headless application runtime](ds-headless-application-runtime.md) §6.

## 1. The question this settles

One Server admits Solar for one project, printing for a second and a layer write
for a third, concurrently, for one authenticated account. None of it may come
from a process-wide "current project", a selection captured at `serve` start, or
an id a caller holds. So: **what is an admitted operation about, who decided
that, and what may change it?** Answered once in `src/execution_context.rs`
(`ds.execution-context/v1`, WASM `executionContext`):

| Question | Op |
|---|---|
| May this request be admitted, and under which project? | `admit` |
| May this caller see this job at all? | `visible` |
| Does this lifecycle event change the scope? (never) | `retain` |
| Is there room for this project's work right now, fairly? | `capacity` |
| What context does a row written before this slice have? | `recover` |

Pure: `now_ms` is an input and every list bounded, so the same request always
has the same answer; no transport, store or credential enters it.

## 2. The record

Immutable once admitted, serialised snake_case as the durable job row's
`context` member, so these are the on-disk names.

```
ExecutionContext { principal_uid, lane, deployment, install_id, // the connection
  project,                                          // the AUTHORIZED project
  client, operation, job_id,                    // who asked, for what, as what
  idempotency_key, input_sha256, admitted_at_ms }  // retry + payload identity
```

`operation` is closed: `transformer_processing`, `solar_processing`,
`report_export`, `layer_read`, `layer_write`; `lane` is `stable` or `canary`;
`input_sha256` is a 64-character lowercase digest. Every text field is
non-empty, trimmed, control-free and bounded (`client` ≤ 120, `idempotency_key`
≤ 200, `project` ≤ 500, `layer_document`'s bound). One that is not is
`context_corrupt`, named.

`compute_jobs::Job` gains `#[serde(default, skip_serializing_if)] context:
Option<ExecutionContext>`. Rows are one JSON blob, so there is **no SQL schema
change**: a released Server's row deserialises with `None` and re-serialises
byte-identically, proven on a real SQLite file in
`crates/ds-sync-store/tests/compute_jobs_rows.rs`. `transition` never touches it.

## 3. The rules

- **A saved selection is a caller default, never Server state**, verified
  against a fresh membership snapshot exactly like a requested project; outside
  it, or with it expired, `project_not_visible`.
- **Sealed bytes outrank a request, which outranks a default.** A prepared Solar
  input names its own project; a disagreeing default is not used, a caller who
  *names* a contradicting project is `scope_mismatch`. Bytes name a project;
  they never authorize one.
- **Membership is verified before any stored key is consulted**, so key probing
  cannot enumerate where work lives.
- **A retry keeps its scope and identity.** Same key, project, operation and
  digest returns the stored context unchanged (`existing`), not re-stamped;
  another project or operation is `scope_mismatch_for_key`, other bytes are
  `payload_changed_for_key`, another principal/lane/deployment holding it is
  `principal_mismatch`.
- **Nothing rewrites scope.** `retain` is the identity function and it takes
  `now_ms` and ignores it; every event returns the record it was given.
- **The refusal vocabulary is closed**, all ten proven: `project_required`,
  `project_not_visible`, `scope_mismatch`, `scope_mismatch_for_key`,
  `payload_changed_for_key`, `principal_mismatch`, `not_visible`,
  `capacity_exhausted`, `context_corrupt`, `context_unrecoverable`.

## 4. Non-disclosure

`visible` answers one object — `{"code":"not_visible","message":"job not
found"}` — to a foreign principal, lane or deployment, a caller narrowing to the
wrong project, an unreadable context, a malformed caller, and an id that never
existed, so an id in a log discloses nothing and a guess is indistinguishable
from a hit. `install_id` is recorded for provenance, never as a fence.
`project_not_visible` likewise says one sentence whether the caller is not a
member or the snapshot expired, and a capacity refusal names counts and limits
but never a project — all three asserted over the recorded answers.

## 5. Capacity and fairness

`capacity` takes only host-owned inputs — configured limits and the store's
census — so a malformed input is a hard fault, never a client refusal.
`Intent::Admit` bounds the queue (`global_queued`, then `per_project_queued`); a
full *running* pool never refuses a submission, the job queues. `Intent::Claim`
bounds the running pool (`global_running`) and the fairness share: a project may
exceed `per_project_running` while nothing else waits and may not while another
project has queued work — room for printing during a long Solar run without
idling the machine to defend an unwanted share. `retry_after_ms` is a second per
unit of work that must go first, capped at sixty. No clock, no randomness.

## 6. Recovery provenance

A row carrying a context returns it (`stored`). A row written before this slice
takes its project from the sealed input if it is Solar (`sealed_input`,
authoritative — the bytes name it) and from the saved selection if it is
transformer work (`saved_selection`; such requests carry no project by design).
With neither it is `context_unrecoverable` and the host asks rather than
guesses. A reconstructed record keeps the row's own `created_at_ms` and records
`recovery:legacy-row` / `recovered` for the client and key — the durable id
digests `(owner, lane, key)` and cannot be inverted, so reuse is decided by
project, operation and digest, never by key text. Recovery reconstructs, it does
not authorize: the host re-verifies membership before the next effect.

## 7. Decisions to challenge

1. **`context_corrupt` covers caller-supplied field bounds**, not only a damaged
   record, so an over-long client label is a typed refusal, not a 500. Host
   configuration stays a hard fault.
2. **One sentence for stale and for unauthorized membership**; separating them
   helps an operator but discloses more than the contract asks, and the host
   holds `fetched_at_ms` already.
3. **`scope_mismatch_for_key` covers a changed operation** — the durable id
   digests `(owner, lane, key)` and would otherwise collide.
4. **Three deliberate shape changes.** `admit` gained `sealed_project`
   (acceptance item 2 refuses a Solar submit whose bytes name A while the caller
   names C); `capacity` gained `intent`, or one op serving both moments would
   refuse to claim out of an exactly full queue; `deployment` is fenced on
   recovery too, so an audience rename needs a migration, not a silent reopen.

## 8. What hosts keep

Hosts perform I/O on these answers and decide nothing: `ds-compute-runtime`
stores the admitted context in the job row and asks `visible` before answering
`jobs/job/job_input/job_result`, `capacity` before claiming, `recover` when
opening a store; `ds-cli-server` opens per-(principal, project) sessions from an
admitted context instead of a process-wide selection; `ds` re-raises refusals
literally. The kernel holds no credential material, so the owner digest stays
the store's fence; `recover` checks the lane it can see. Proof:
`tests/fixtures/execution-context-cases.json`, 72 recorded cases, byte-identical
to ds-web's `src/tests/fixtures/execution-context/cases.json` and replayed there
through the vendored WASM, plus the unit tests above.
