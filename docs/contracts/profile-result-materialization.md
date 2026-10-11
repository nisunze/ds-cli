# Profile result fact materialization

Profile result queries consume retained native calculation evidence. They never
project a scene, solve a case, assemble demands, screen structures, acquire
project data, or write a package. The public query request, response identity,
freshness fences, paging cursor and 200-item/256-KiB limits remain the same.

After a complete signed Profile edit update, the Runtime registers the complete
fact corpus and entity indexes without serializing fact bodies. Native typed
predicates still determine usage, failures, negative loading, violating sections,
qualification and blockers. Counts and stable ordering precede pagination and
presentation caps; missing evidence remains unavailable, never zero failures.
The command view and display projection do not consume the result-query index.

The retained source is one immutable copy of the exact revision/request-matched
typed usage report plus the already retained calculated scene base. The dense
scene arrays are shared, not cloned. Querying cannot consult a later mutable
cache or reclassify an old report as current. Observation metrics report
`profile_result_fact_source_capture_ms`, captured report rows, registered deferred
facts and fact materializations separately from total `profile_result_index_ms`.
The capture cost includes the report clone and must remain part of elapsed-path
comparisons.

Each distinct fact has a shared `OnceLock`. Usage, qualification, failure and
negative-loading entries for the same row reuse the same cell. Repeated queries,
captured publications and concurrent consumers reuse its exact JSON. Only an
actually offered page item materializes its cell. Query metrics count new
materializations, already materialized offered cells and JSON materialization
time; these are presentation work, not calculated cases.

All existing model/history/input-root/source/case/entity/cursor/bounds and
publication checks occur before materialization. Invalid or stale queries,
unconfirmed publications, changed dependencies, absent envelopes, invalid page
offsets and an oversized response basis materialize no facts. Checking whether
an offered fact fits the wire cap necessarily materializes it: an oversized
item still refuses, and retrying reuses that cell. A page may materialize one
additional offered item when that item exceeds its remaining byte budget.

Publication checkpoints retain the same immutable index and cells. Confirming a
candidate preserves its exact source; discarding restores the prior index,
calculated base, view and history together. Failed and cancelled calculations
retain existing Runtime admission/rollback rules. Cells never grant permission
to read stale or unconfirmed evidence. Equal-input projection may share facts
while issuing a new result identity, so old cursors remain stale.

Owning proof is in `result_queries/lazy_fact_tests.rs` and the existing complete
query, analysis retention, repeated signed edit, publication, calculation-job,
and calculated scene/display parity tests. It includes unused-data and refused
query zero-materialization controls, shared-cell concurrency, mutable-cache
replacement, exact eager/lazy fact parity and honest oversized-item refusal.
These native checks do not prove Desktop elapsed-path improvement: matched
actual-package Desktop/headless edits must retain scene/root/history/checkpoint
parity and report complete draw timing before a performance gap is closed.

## Matched live edit acceptance

The Linux local Desktop witness has 417 structures and 63 alignments. The same
two type edits, case selections, Undo/Redo and headless checkpoint replay retained
all eleven scene/history/input-root/axis pins. All six public query kinds retained
their first page, next page, repeated items, basis, counts, roots and cases:
usage 417, failures 60, negative loading 117, violating sections 132,
qualification 417 and blockers 29,931. Checkpoint bytes remained identical.

| Elapsed work | First edit, before → after | Second edit, before → after |
| --- | --- | --- |
| Result index | 93.617 → 10.305 ms | 87.932 → 10.108 ms |
| Typed report capture, included in after index | 2.229 ms | 3.107 ms |
| Complete edit to canvas | 1,263 → 1,260 ms | 1,045 → 998 ms |

Both completed edits captured 417 report rows, registered 30,480 deferred cells
and materialized zero fact bodies. An immediate public Usage query after a fresh
completed edit materialized exactly two offered facts (0.088 ms); repeating it
and reading the same Qualification page each reused two cells with zero new
materializations. All three queries calculated zero cases and preserved history
and calculation state. Bounds, stale-cursor and absent-entity refusals remained
typed refusals. Native pending/rollback and changed-dependency zero-work checks
remain covered by the owning tests.

The immutable source package SHA-256 is
`e89b7a1c0416d65879aad8829c6a6faffc55105b746f4e683502620cec03bdc1`;
the identical before/after headless checkpoint SHA-256 is
`cc73134c0cca37afa9bb0603f361f13492032ac8202c3828ae41255a564826a5`.
The before native executable SHA-256 is
`b090b219d9506947d109b11c3af31b75638fb6eb958961a7cacde801c402acbf`;
the after executable is
`516cde9eaf8cb2aee02cf81539bfab9ebc5bf7b14e3ee36ffe9f0420b0bbc341`
(270,422,400 bytes), with CLI
`f73b3e58651cfd7e050495782ae8ead78d2d75171ead2a9f54396aa2edabfe0c`.
The Grid WASM stayed
`467b4edca0925b65f60520266b377954b6ca8266ac7a0dd3ca1800aab6d2f244`.
Evidence is in the owning Desktop job's
`out/profile-observer-20261010/lazy-facts-live-paired-proof.json`,
`lazy-facts-demand-v19-proof.json` and `lazy-facts-final-artifacts.json`.

This proves removal of unused fact serialization from completed live edits.
The first complete draw showed no meaningful improvement; two samples do not
establish percentiles or meet the 500-ms full-path target. The activation also
changed final debug stripping and CLI/Solar sidecar artifacts, so it is not an
isolated one-variable benchmark or packaged-release acceptance.

Initialization, full projection/explicit calculation and selected prepared cases
still use the eager constructor. Undo/Redo reconciles through full projection;
when revision/root/case/usage differs, equal-input index reuse fails and a new
eager index is built. Queries afterward correctly report zero newly materialized
lazy cells. Scene-base preparation alone does not change an index. These paths
remain separate from the accepted live-edit optimization.
