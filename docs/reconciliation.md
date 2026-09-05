# Durable reconciliation

`reconciliation::apply` / WASM `reconciliationCommand` own the versioned
`ds.reconciliation/v1` state machine. This is a pure command boundary: hosts
perform authentication, storage and domain transport. A successful HTTP request
is not a publication acknowledgement.

Each immutable operation binds principal, credential audience, deployment lane,
project, a closed domain operation, object identity, server-base semantics,
input fingerprint, content sizes/digests, dependencies and policy revision.
Local hashes cannot supply a missing server base. Unknown base or unavailable
domain transport refuses a claim. Dependency admission is ordered and acyclic;
delivery waits for each dependency's exact acknowledgement.

Worker claims carry monotonically increasing epochs and bounded leases.
Expired workers cannot settle reclaimed work. Interruption preserves uncertain
remote outcomes. Transient failures record a durable retry deadline; expired
credentials, authorization refusal and conflicts stop automatic claims.
Cancellation before any attempt is final locally. After an attempt it remains
requested because a lost response may hide a committed remote result. An exact
authenticated publication acknowledgement wins that race.

An acknowledgement must match the scope, operation/request digest, input,
payload, complete artifact set and policy revision. Tombstones additionally
require an exact recoverable-payload reference and Recovery Center record ID.
All inputs, artifacts, receipts and history stay retained, including cancelled
and acknowledged Survey work. This module currently authorizes **no garbage
collection or history compaction**. A future cleanup command must prove both
retention authority and live-reference safety; removing a queue from a screen
is never that authority.

Inspection is network-free and remains available after clock rollback.
Mutation refuses a backwards clock. Queue bounds are 4096 operations, 128
attempts per operation, 1024 history events per operation and 16 MiB serialized
metadata. A bound refuses additional work; it never drops retained evidence.

## Connected consumers

Offline Design version capture derives publication pins in the Rust worker,
then commits its rooms, manifest, catalog and shared queue atomically in
IndexedDB. Retained comparison artifacts use the same queue. The shared queue
is fenced by principal, audience, lane and project. Sync Center and the paired
dev CLI's `design sync status/cancel/resume` call the same host functions and
kernel transitions. No TypeScript transition fallback exists.

The local capture policy identifiers describe immutable capture semantics;
they are **not signed offline licenses or permission to publish**. No upload
worker is attached yet: entries truthfully remain `pending_transport`.
Old version/comparison intents migrate only when that exact object is captured
again. Untouched legacy intents remain retained and are not yet included in the
new queue view. The native SQLite workspace, Solar/Survey/settings and artifact
outboxes are not yet migrated. Exact-content server admission, signed product
grants and server-backed compaction remain separate required closure work.

## Evidence and production code accounting

Rust tests exercise changed-input retries, foreign scope/revision fences,
dependencies, expired/reclaimed workers, upload versus publication, cancellation
races, credential expiry, clock rollback, corrupt acknowledgement history and
Recovery Center evidence. Real-WASM/IndexedDB tests prove transaction rollback,
retention after cancellation/restart, offline version batches and comparison
reopening without downloading source versions.

Local validation on 2026-09-05: kernel workspace tests (62), the five targeted
web suites (65 tests), Svelte checking (zero errors/warnings), the complete CLI
workspace suite with all features, and kernel/CLI Clippy gates passed. The
native desktop operation allowlist and CLI semantic inventory both include the
three reconciliation commands; tests now check those inventories explicitly.
The official local sidecar preparation completed. The source-backed dev CLI
successfully invoked `design sync status --limit 5 --output json` against the
running DS GridDesign Dev application. Its active-project queue was empty;
this proves live dispatch and local inspection, not legacy queue migration or
authenticated publication. No cloud deployment or GCP outage experiment was
performed.

## Next transport boundary: confirmed Solar findings

Read-only tracing of the Solar input consumer confirmed these remaining gaps:

- `ds-web/src/lib/solar/input-mutations.ts` coalesces pending edits by changing
  their payload under the existing operation ID. Its sender returns `void`,
  and completion of that call marks the row synchronized. It retains only the
  newest 100 synchronized rows without exact publication receipts.
- `ds-brain/internal/handlers/solar.go` / `SolarService.SaveInput` return target
  identifiers, not an operation-bound publication receipt. Regular documents
  use Firestore merge; `network_data` uses its own media-validation transaction
  and whole-value replacement of `maps`. The local recursive patch must not
  resurrect omitted media references when this path is migrated.
- The existing `CommitProjectCity` already has a server-observed fingerprint
  check and transactional writes for a complete project city. Reuse that
  admission foundation where applicable; do not turn a locally derived hash
  into evidence of a server base or replay an editor patch as an unconditional
  write. Global templates and the special network-media path require their
  own explicit coverage.

No Solar production code was changed in this checkpoint. Its queue migration,
retained legacy evidence, immutable dependent edits, exact authenticated
receipts and server transaction tests remain required. These are inspected
findings; they must not be reported as already fixed or as a verified outage
test.

Against `ds-web` baseline `cc1bb5b2`, affected production TypeScript/Svelte:

| File under `src/lib/` | Before | After | Added | Deleted | Net |
|---|---:|---:|---:|---:|---:|
| components/dialogs/SyncCenterDialog.svelte | 440 | 482 | 44 | 2 | +42 |
| design/transformer-version-comparison.ts | 160 | 175 | 18 | 3 | +15 |
| design/version-archive.ts | 107 | 113 | 9 | 3 | +6 |
| desktop/cli-bridge.ts | 2266 | 2272 | 7 | 1 | +6 |
| desktop/cli-design.ts | 1158 | 1180 | 22 | 0 | +22 |
| reconciliation/command.ts | 0 | 50 | 50 | 0 | +50 |
| stores/db.ts | 4861 | 4687 | 5 | 179 | -174 |
| **Total** | **8992** | **8959** | **155** | **188** | **-33** |

Rust, tests, documentation and ignored generated WASM are excluded. Most
deletion is removal of unused automatic quota-eviction helpers, not migration
of an existing domain scheduler. The removed helpers could evict inactive
design rooms and synchronized Survey entries without retained receipt evidence;
no production callers existed. The old version/comparison intent writers are
replaced by kernel queue admission. Remaining TypeScript performs storage,
worker invocation, host identity checks, bridge argument parsing and rendering.
