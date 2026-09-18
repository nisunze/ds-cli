---
name: ds-feedback-triage
description: Read the DS feedback backlog as a difference, then close what is proven or note what blocks it.
metadata:
  ds-chapters: operations
  ds-mcp-profile: operations
---

# Visit the backlog, and never leave it silent

Everything here goes through `ds`. Never edit the backlog through an API, a
store, or the `fb` tab on the agent's behalf.

## 0. The backlog is a difference, and the API owns it

```
ds feedback list --output json
```

That is the whole first step. The backlog remembers how far this account has
read, so a visit where nothing moved answers:

```json
{ "changed": false, "total": 0, "reports": [], "cursor_source": "reader" }
```

**`changed: false` means stop.** Do not read rows, do not re-list with another
filter, do not "check anyway". It is a field, not an empty list to interpret.

Keep **nothing** locally. No corpus file, no index, no timestamp. There is no
`--since` any more — it made remembering the caller's job, which meant a state
file on one machine and a full rescan on every other one. The watermark lives
in the API, so every machine, every agent and every fresh checkout inherits it.

- A read that NARROWS — `--component` or `--query` — is a LOOKUP, and it is
  answered in full every time. It never reads or moves the sweep's position, so
  searching for the reports this session touched (step 1) always returns them,
  however often you ask.
- `--all` ignores the watermark and reads the newest matching reports from the
  top. It is a peek: calling it again returns the same rows, so it is never how
  you read the whole backlog — the drain below is. Use it only to re-see
  something already read.
- `--cursor <token>` reads the difference since a token the backlog issued —
  useful for handing an exact position to another agent. Never build one.
- Enumeration is complete. `total` is how many reports match; `truncated`
  means only that `--limit` held rows back, never that older reports were out
  of reach. `scan_incomplete` no longer exists.
- `truncated: true` on the unfiltered sweep is a DRAIN, not a rescan: list again
  and the next chunk comes back, never the rows already delivered, until
  `changed: false`. That is how the whole backlog gets read. Do NOT narrow the
  filter to make the sweep fit — a narrower question is a lookup, not the next
  chunk, and the sweep is left half-drained.

**Work from the rows.** Each row carries `status`, `severity`, `component`,
`title`, `blocked` and `blocked_on`, `note_count` and `latest_note`, and the
`id` and `version`. That is enough to decide what a task touches.

**`--detail` is the expensive thing here.** A row carries only the first 240
characters of the report and says `detail_truncated`. Pull the full acceptance
text only for the handful of ids about to be verified or closed.

**Fan out by partition, never by repetition.** Give each agent a disjoint slice
of ids. Never hand the whole corpus to every agent.

## 1. Find what this session touched

```
ds feedback list --component <repo-or-area> --detail --output json
ds feedback list --query '<words from the work>' --detail --output json
```

`--view` is `not_addressed` by default; pass `addressed` or `all` to see what
is already closed. Match reports to the work by component and by what the
report actually describes — never by title resemblance alone.

Keep every report's returned `version`, and record `reporters`,
`reporter_kind` and `updated_by` so the person who raised or last maintained it
stays visible.

## 2. Verify against the acceptance condition

Read `.data.reports[].detail`. The submitter wrote an expected behavior and an
observable acceptance condition; that condition, not the diff, is what decides
whether the report is addressed:

```
ds capabilities --search '<the words the report used>' --output json
ds capabilities <command-id> --output json
```

If the fix is unmerged, undeployed, or in a build this `ds` is not running, the
report is not addressed yet. Go to step 3b, not step 3a.

## 3a. Close it with the record

```
ds feedback close --id <id> --expect-version <version> --resolution '<what changed, and the evidence>' --yes --output json
```

Use `--status wont_fix` with a resolution that says why, when the gap is real
but will not be acted on. The resolution is what the next reader sees instead
of reopening the investigation.

## 3b. Or say what it waits on

```
ds feedback note --id <id> --expect-version <version> --text '<what is now known>' --blocked-on '<the exact dependency>' --yes --output json
```

**A session that touches a report either closes it or notes it. Silence is the
one thing not allowed**, because silence is what forces the next reader to
rescan the mountain. Name the dependency or the exact unmet condition — a
deploy, a terraform apply, an owner ruling, another report id. It then shows on
every later row and nobody rediscovers it.

`--unblock` lifts a blocker once the condition is met. At most 20 notes per
report, 1000 characters each; past that it refuses rather than truncating.

A note needs no triage capability, so leave one even when `close` would refuse.

## What can refuse, and what to do

- `feedback_not_permitted` — closing is an admin action. Do not retry or switch
  identity: **leave a note instead**, then report the ids and resolutions so a
  capable account can close them.
- `feedback_conflict` — the report moved. List it again, confirm it is still
  addressed, then act.
- `feedback_settled` — the report is closed and is never revived. Submit a NEW
  report naming that id; it will link to it automatically.
- `feedback_cursor_rejected` — drop `--cursor`, or pass `--all`.
- `feedback_not_found` — list again with `--view all`.
- `headless_signed_out` — `ds auth login --email <address>`.

## Rules

Close only what this session can show is addressed, one report at a time, each
with its own resolution. Do not close to tidy the backlog, by severity, by age,
or what someone else is working on. Reopening stays a human decision in the
`fb` tab.

When asked to close the latest N, finishing with fewer than N closures is
correct whenever some acceptance conditions are not proven in the deployed
`ds`. Return two explicit sets: closed ids with evidence, and remaining ids
each with a note already left naming the exact unmet condition. Include each
report's `reporters` in both sets; never replace a missing reporter with an
inference.
