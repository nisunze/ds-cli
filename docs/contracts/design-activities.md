# Design Activities: the ledger and the cross-project fold

**Owner:** `design_activities` (`src/design_activities.rs`), schema
`ds.design-activities/v1`, native `evaluate` and WASM `designActivities`.
One entry, two actions: `ledger` and `fold`. The fold is the single authority
behind `ds design activities read`, the MCP surface and the Governance page;
none of them counts, ranks or diffs on their own.

## Why

The owner saw unauthorised activity on their system and needs, across every
project they reach, who designed what, where and when. There is no event log
for design writes. The only record is the per-project transformer status list
(ds-brain `/report list_transformers_status`), whose rows carry a stamp —
status, time, user — per phase (sketch, draft, process, report, combined) and
a document updater. Those stamps are overwritten in place by the next run.
So the record has to be MADE: captured per project, compacted, retained, and
diffed between captures.

## What a ledger is

`action: "ledger"` takes one project's raw rows, special rows included, and
returns `ds.design-activity-ledger/v1`: the project, the capture time, the row
count, and one entry per transformer (special rows — `mv_data`,
`combined_transformer`, `collisions`, `all_transformers` — are counted in
`row_count` and never recorded). Each entry holds the transformer's district
and sector, its document creator and updater, every phase stamp it carries,
its latest action (`design_status_row`'s), its health tone, error and warning
counts and the user the Dashboard's attention note charges, and `designed` —
the Dashboard's own design-stamp verdict (any successful compute phase whose
output was saved, attributed to its operator, then to the document updater).
The ledger is COMPLETE: every stamp of every transformer, not the Dashboard's
`recent[8]`. Nothing in it is decided here; every reading is the Dashboard's,
the status row's or `design_health`'s, called in-process.

## What hosts must capture and retain

- Capture the full status list of each project the account can reach, one
  project at a time, and call `ledger` with the project id, the capture time
  (`captured_at_ms`, epoch millis, the host's clock at capture) and the rows.
- Retain the returned ledger per project, keeping at least the previous one:
  the fold diffs each project's current ledger against its `previous`, and
  the differences between sweeps are the only history there is. A host that
  keeps only the latest ledger loses every overwritten stamp.
- Pass every retained pair to `fold` with `now_ms`, the host's UTC offset,
  the project's display name and status, its roster (`members`) when the
  host holds one, and the counts it retained from the Dashboard model
  (`dashboard_summary`, of which `health_score` reaches the reply).
- The embedded ledgers are read leniently — members added later are allowed,
  missing ones default — so a ledger written by a newer kernel still folds.

## What is NOT claimed

- **No device or installation attribution.** Rows carry an account on a
  stamp and nothing else. The fold names accounts; it cannot say which
  machine, browser or installation acted.
- **Nothing between two captures is visible.** A stamp overwritten twice
  between sweeps records only its last value; a transformer created and
  deleted between sweeps never appears. `since_previous` and `changes`
  describe the difference between two captures, not the events in between.
- **An action that never stamps a row is invisible.** Reads, downloads,
  exports, map views, edits held in a browser room and never saved, and
  anything else that leaves no phase stamp or document update on the status
  row do not exist to this module.
- A capture older than 24 h against `now_ms` is reported `stale`; the fold
  still reads it, it just says so.

## Sweep etiquette

- Sweep projects **sequentially**, never in parallel: ds-brain serialises full
  status scans to one slot per instance, so concurrent captures queue behind
  each other and time out rather than run faster.
- One capture per project per sweep; the ledger's `captured_at_ms` is the
  sweep's evidence of when the rows were read.
- The ledger request is bounded at 32 MiB and 2000 rows (the client-core
  bound on one project's list); the fold at 64 MiB. `limits` (users,
  timeline, projects, changes) default to 500/200/500/500, may be raised to
  5000, and `more` says which lists were cut.

## Label keys

Everything this module emits is an i18n key. Its own set, prefixed `gda_`, is
listed in the module rustdoc and is the source the web catalogue is authored
from. Timeline, change and `latest` label keys are the status row's phase
action keys (`dsr_action_*`), reused as-is.

## Parity

`tests/design_activities_corpus.rs` replays
`tests/fixtures/design-activities-corpus.json` with exact equality: ledgers
over the Dashboard corpus's real-shaped fleet and folds over those ledgers,
with a roster, a previous capture, a stale capture, tiny limits and a ledger
carrying members from the future. Re-record deliberately with
`DS_RECORD_CORPUS=1`.
