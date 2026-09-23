# `ds feedback`

`ds feedback` is the agent's end of the shared product backlog — the same
deduplicated ledger DS GridDesign's `fb` tab reads and writes. It is not an
issue-file generator and it does not introduce an MCP or another API client.

Commands default to the headless signed-in user (`--target server`), using the
same fixed feedback API. Neither Desktop nor a selected project is required.
`--target desktop` retains the paired application's compatibility route.
Credentials remain inside the protected native client or paired application.

```text
submit → (a coding session addresses the gap) → list → close
                                                  ↘ note (what it waits on)
```

## `submit`

Records an agent's observed product gap. Both adapters pin `reporter_kind` to
`agent`; project selection is not authority for feedback.

Submit only after live `ds capabilities` discovery establishes that a
capability is absent or broken. Include bounded non-secret evidence, the
expected behavior, and an observable acceptance condition. Repeated sightings
of the same OPEN gap are deliberately merged by the feedback service.

A settled report is never merged into. Once a report is `resolved` or
`wont_fix`, the next sighting of the same obstacle becomes a NEW report whose
`supersedes` names the closed one, and the closed one gains `superseded_by`. A
link, not a merge: both histories survive, and the closure that was already
proven is not undone.

## `list`

Reads the backlog the `fb` tab shows, as a DIFFERENCE.

```
ds feedback list --output json
```

The backlog remembers how far this account has read. A visit where nothing
moved answers:

```json
{ "view": "open", "total": 0, "count": 0, "reports": [],
  "changed": false, "complete": true, "cursor": "fb1.…", "cursor_source": "reader" }
```

`changed: false` is the whole answer. Stop there. It is a field, not an empty
list to interpret, and reaching it cost no local file, no timestamp and no
token the caller had to keep.

* `--cursor <token>` reads the difference since a token the backlog issued. The
  token is opaque and carries its own integrity digest and the fingerprint of
  the query it answered; a hand-built one, or one replayed under a different
  filter, is `feedback_cursor_rejected`.
* A read that NARROWS — `--component` or `--query` — is a lookup, not a chunk
  of the sweep. "Which reports are about this?" has the same true answer every
  time it is asked, so it is answered in full, and it neither reads nor advances
  where the sweep left off. Draining it instead would answer it once and answer
  `changed: false` ever after, which is how a second triage session goes blind
  to the reports it came to match against its own work. A caller that really
  wants to drain a narrowed question echoes that read's `cursor`.
* `--all` ignores the watermark and reads the newest matching reports from the
  top. It is a peek, not an enumeration: it returns `--limit` rows and neither
  reads nor advances where this account has read, so calling it again returns
  the same rows. The way to see every matching report is the drain below. It is
  also what an older client does implicitly: opting into the watermark is an
  explicit flag on the wire, so a client that predates it sees no change.
* `--view open` (the default) is what is legitimately open: unsettled and
  waiting on nothing. A report noted `--blocked-on` a release, a deploy or a
  ruling is `waiting` and stops coming back in every sweep; it returns to `open`
  when a note unblocks it. `not_addressed` is both halves, then `addressed` and
  `all`. Each view drains under its own watermark, narrowed by `--component` or
  `--query`. Every filter goes to the backlog — nothing is narrowed after the
  answer arrives, because a row hidden here would still have been counted as
  read.

`--since` is gone. It took an RFC3339 timestamp the CALLER had to remember,
which meant a state file on one machine and a full rescan on every other one.

`scan_incomplete` is gone too, and so is the 200-record horizon behind it. The
backlog enumerates completely: `total` is how many reports match, `complete`
says whether they all came back, and `truncated` means only that `--limit` held
rows back — never that older records were out of reach. `feedback_backlog_too_large`
refuses rather than presenting a partial answer as a whole one.

A `truncated` answer on the unfiltered sweep is a DRAIN, not a rescan. The watermark only advances past
reports it actually handed over, and a limited read hands over the ones it has
gone longest without showing this account. So listing again returns the NEXT
chunk, never the rows already delivered, until the backlog answers
`changed: false` — and each visit costs less than the one before. A backlog
larger than `--limit` is therefore read once in bounded pieces; it is never
re-read in full. The rendered answer says which of the two it is (`drains`), so
the line under the rows reads `list again for the next chunk` on a drain and
`narrow with --component or --query` on a lookup or an `--all` read — narrowing a drain
would start a different question, with its own watermark, and abandon it.

### A row decides on its own

Each row carries `status`, `severity`, `component`, `title`, plus `blocked` and
`blocked_on`, `note_count` and `latest_note`, `supersedes` and `superseded_by`,
and the `id` and `version` a close or a note takes. That is enough to decide
"do I care about this" without opening anything.

Without `--detail` a row carries the first 240 characters of the report and
says `detail_truncated: true`. That number is the mountain, measured: the long
acceptance text of 124 reports at 1200 characters each is about 37 000 tokens
for ONE visit, which is how a handful of visits came to cost 1.5 million. The
watermark stops the re-reading; the excerpt stops the first read from being a
mountain on its own.

`--detail` returns each report's full text — including the acceptance condition
its author wrote down. Reach for it only for the handful of ids about to be
verified or closed; that is the moment the acceptance condition matters, and
it is the single most expensive thing this command can be asked for.

A full read also returns `backlog` counts: `total`, `open`, `blocked`,
`settled`.

## `note`

Records what an open report is waiting on, without closing it.

```
ds feedback note --id <id> --text '<what is now known>' --blocked-on '<the dependency>' --yes
```

With three verbs the only way to say anything about a report was to close it,
so a report waiting on a deploy, a terraform apply, an owner ruling or another
report carried no record of that — and every later visit re-read its full text
to rediscover the same blocker.

A note is append-only and never touches the submitter's own sighting: that
stays exactly as filed, and a note is a later observation ABOUT it. At most 20
notes per report and 1000 characters each; past that the answer is
`feedback_note_limit` — refused, never truncated, because silently dropping the
newest note loses exactly the observation somebody bothered to write.

`--blocked-on` marks the report BLOCKED, which shows on every later listing row
and needs no rediscovery. `--unblock` lifts it; one note cannot do both.
`--expect-version` is the same optimistic fence a close uses.

A settled report takes no notes at all: `feedback_settled`, whose remedy is a
new report that names the closed id.

Unlike `close`, `note` needs no platform triage capability. The sessions that
discover a blocker are mostly agents that cannot close anything, and the
alternative to letting them write it down is that nobody does.

## `close`

Marks one report addressed: `--status resolved` (the default) or `wont_fix`,
with a `--resolution` that says what changed. It is the `fb` tab's own triage
mutation, so it needs the same platform triage capability a person needs
(`feedback_not_permitted` when the account only reads the backlog), and it
carries the same optimistic version — a report edited since it was listed is
refused as `feedback_conflict` rather than silently overwritten. Pass
`--expect-version` to pin the version explicitly; without it the close uses the
version read at the moment it runs.

Close only what the session can show is addressed, and verify against the
acceptance condition in `detail` — not against the title. A report closed
without the fix costs more than one left open, because the next sighting starts
a new report instead of raising the occurrence count on this one.

When a report cannot be closed, `note` is not optional politeness — it is the
other half of the contract. A touched report left silent is what forces the
next reader to rescan the whole backlog.

Reopening is deliberately absent. Returning a report to the open backlog stays
a human triage decision in the `fb` tab.
