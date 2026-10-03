# Offline Design workspace

`src/design.rs` owns deterministic LV snapshot validation, source precedence
and feature-edit transitions for native and WASM hosts. The separate native
`ds-design-workspace` crate adds SQLite persistence, captured background jobs
and an outbox. Neither owns HTTP, project authorization, engineering algorithms
or rendering. `ds-network` computes; `ds-report` renders.

## Revisions and source authority

Initialization atomically creates a private directory. The project label grants
no remote authority. No application cache is opened. A complete snapshot pins
geometry, explicit settings, network configuration and source datasets. Only
`EPSG:4326` longitude/latitude is accepted; convert other CRS explicitly.

A revision is SHA-256 of recursively key-sorted compact JSON after
normalization; arrays preserve order. Missing feature IDs are deterministic;
provided IDs must be unique strings within a layer. The enclosing transformer
owns row identity. Source collection hashes are verified before persistence.
Unknown envelope fields and unsupported native settings are refused.

An edit carries its expected local revision and operation ID. The head,
immutable revision, retry receipt and outbox entry commit in one SQLite
transaction. Identical retries return the original receipt; reusing the ID for
different inputs conflicts. Restore creates a new operation and retains all
historical revisions. Batch edits index identities once per touched layer,
rather than scanning the entire layer for each ID.

Declared project sources win over browser draft choices. Absence permits the
explicit user choice, then the Rust default. An empty pole list means no
borrowed poles. An empty customer declaration is invalid. Unavailable project
configuration must never be converted into an absent declaration. Browser
source addresses are opaque identifiers; the native snapshot carries actual
dataset bytes. Labels never establish dataset identity.

## Background computation

A run transaction captures 1–32 explicitly named transformer revisions, at
most 64 MiB in aggregate. A snapshot admits at most 100,000 aggregate features
and 64 layers/sources. Reusing a run preserves its exact inputs and ordered
selection. Resumption is fenced by the native executable SHA-256, since the
engine package version alone cannot identify a build.

One compute lock prevents duplicate workspace workers while leaving edits
available. Independent jobs use a native Rayon pool bounded by the requested
CPU budget and job count. `--workers` lowers memory/CPU pressure. These are
input/result bounds, not an operating-system RSS limit. Complete job outputs
are persisted atomically, capped at 256 MiB each, and never inlined in status.

The plan's `concurrency` is an upper bound supplied to the execution host. On
Desktop, the native host applies a second, mandatory resident-memory admission
gate before a transformer starts; queued transformers do not own a WASM heap.
The host may therefore execute fewer jobs at once than this plan permits. On
hosted web, the bounded Web Worker pool is the resident-memory envelope. Both
hosts run the same `ds-network` transformer kernel and neither may fall back to
cloud processing after local admission fails.

Killed workers leave unfinished jobs pending/running. After obtaining the
workspace lock, restarting the same run recovers interrupted running jobs and
retries only unfinished jobs. Cancellation is sticky and stops new jobs at boundaries;
already running jobs may complete. Use a new run after explicit cancellation.
Failed jobs retain diagnostics. Results never overwrite newer authored edits.

## Reports and printing

The workspace creates a fresh report-input directory from a completed run:
plain local layers, captured configuration, the exact saved producer
voltage-drop JSON member and a digest manifest. Its SHA and final-layer pin
travel together. A missing source or invalid envelope, identity, digest or
final-layer/settings binding refuses before staging. The reporter still owns
deep saved-run content and presentation admission; its blockers prevent
complete retention. Reporting never calculates a missing result or reprocesses
approved geometry.
The CLI
calls the existing typed `ds-report export-transformer-report` boundary.
Country and formats are explicit; every individual selection includes the
kernel's mandatory saved voltage-drop JSON. The reporter requires usable nonempty
configuration and, for Rwanda, its verified local admin reference. Unresolved
media or unsupported requirements remain visible blockers.

Print selections use `pdf__<layout-id>` from the captured governed printing
configuration, with its layout, style and renderer documents. The native vector
renderer needs neither GCP nor a map window. This is distinct from the experimental
print-iteration task. Printing here means generating a printable PDF; physical
printer selection/spooling remains an OS operation.

Completed artifacts are read once, checked against the reporter's size and
SHA-256, retained inside the workspace and queued transactionally. Artifact
paths must lie directly in the expected output directory. Partial exports are
not queued as complete. A completed delivery must contain each requested
format exactly once, including the mandatory JSON whose bytes must equal the
captured producer member. Changed report inputs or omitted artifacts refuse
before the outbox transaction. Failed exports can be retried into a new directory
from the same immutable run; existing output is never overwritten.

## Pending reconciliation

The outbox retains edits, restores, process results and report bytes. It
reports `pending_transport`; there is no fake acknowledgement or unauthenticated
upload action. Local hashes are not server versions. The subsequent transport
must bind the workspace to an admitted project and protected native principal,
lane and audience, then reconcile explicit server revisions idempotently.
Conflicts and rejected authorization must preserve local drafts.

The existing UI edit room and governed collaboration transport are not migrated
to this store yet. Browser source precedence now calls Rust with no TypeScript
decision fallback. Assisted routing should eventually submit reviewable,
version-fenced proposals; route inference and physics changes are outside this
slice.
