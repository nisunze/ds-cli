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
  the project's display name and status, and the counts it retained from the
  Dashboard model (`dashboard_summary`, of which `health_score` reaches the
  reply).
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
- **`first_seen_ms` is not a first appearance and `last_seen_ms` is not a
  presence.** Both are stamps inside the retained captures. A user whose
  older stamps have all been overwritten since the first sweep has an
  earliest retained stamp of yesterday and is not a new actor; a user who
  read, downloaded and exported all day stamped nothing and does not move.
  A host must spell both as what they are, never as arrival or activity.
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

## Where a capture lives

**Owner:** `design_activities_store` (`src/design_activities_store.rs`),
schema `ds.design-activities-store/v1`, native `evaluate` and WASM
`designActivitiesStore`. The bytes and the atomic write are the host's; the
identity, the admission, the retention, the selection and the order of the
next sweep are the kernel's, so the Linux server's store under
`$XDG_STATE_HOME/ds/design-activities/…` and the Governance page's IndexedDB
hold the same history of the same captures.

A retained capture is one envelope, `ds.design-activities-snapshot/v1`:
`{schema, ds_project, project_name?, display_name?, status?, lane, captured_at_ms,
captured_by, source:{rows, diagnostics_source}, ledger, dashboard?,
digest}`. `digest` is `sha256:` over the canonical (compact) bytes of the
ledger; `captured_by` is the account the capture was taken as — **not** a
device, because the rows carry none.

- `validate` admits an envelope only when it is internally true: the snapshot
  schema, a valid project, a positive capture time that fits a 13-digit name,
  a named capturer, the ledger's schema, its project and its capture time
  equal to the envelope's, its `transformer_count` equal to what it carries,
  `source.rows` no smaller than the ledger's `row_count`, and the digest
  recomputed from the ledger it carries. A refusal names the field.
- `digest` returns that digest for a ledger, so a host never has to reproduce
  the canonical bytes itself.
- `key` returns the segments `["design-activities", lane, <32 hex of
  sha256(principal_uid + "\n" + audience_sha256)>, ds_project]` and the file
  name `<captured_at_ms zero-padded to 13 digits>.json`. The account is
  hashed, never spelled; two accounts, or one account on two credential
  audiences, are two stores; every segment must be one safe path component.
- `retain` keeps the newest `keep_last` (30) captures whole **plus** the
  newest capture of each UTC day within `keep_days` (90), and drops the rest.
  Thinning is not cache eviction: the record is the difference between
  captures, so the older shape of the history is kept a day at a time. It is
  idempotent and never drops the newest capture.
- `select` names the pair the fold diffs: the latest capture and the newest
  strictly older one.
- `plan` takes the visible projects, the captures already held, `now_ms`, a
  bucket (`active` by default; a project with no status counts as active),
  `max_age_ms` (6 h), `limit` (500) and `pause_ms` (2000), and returns the
  sweep: `refresh` ordered oldest capture first and then the projects with no
  capture at all, `skip` naming every project left out and why (`fresh`,
  `bucket`, `limit`), and the etiquette — `sequential: true` and `pause_ms` —
  that a host must obey.

**Nothing here schedules anything.** There is no timer, unit file, cron or
background refresh in this contract and there is not meant to be one: a sweep
happens because a person or an agent asked for one. A reader is stale-first
and says which moment it is reading.

`tests/design_activities_store_cases.rs` replays
`tests/fixtures/design-activities-store-cases.json` with exact equality —
every action, its refusals, and the retention edges (empty, one, exactly
thirty, thirty-one spanning more than ninety days, several captures a day) —
and re-records under `DS_RECORD_CORPUS=1`.

## Label keys

Everything this module emits is an i18n key. Its own set, prefixed `gda_`, is
listed in the module rustdoc and is the source the web catalogue is authored
from. Timeline entries, changes and `latest` each carry a `label_key`: the status
row's own phase action keys (`dsr_action_*`), reused as-is. A change carries
both `phase` (the raw token, for a machine) and `label_key` (for a reader);
a host that prints `phase` prints English on a French page.

## Parity

`tests/design_activities_corpus.rs` replays
`tests/fixtures/design-activities-corpus.json` with exact equality: ledgers
over the Dashboard corpus's real-shaped fleet and folds over those ledgers,
with a previous capture, a stale capture, tiny limits and a ledger carrying
members from the future. Re-record deliberately with
`DS_RECORD_CORPUS=1`.
