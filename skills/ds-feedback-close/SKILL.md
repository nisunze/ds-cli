---
name: ds-feedback-close
description: Close verified DS feedback through the typed `ds` backlog.
metadata:
  ds-chapters: operations
  ds-mcp-profile: operations
---

# Close the loop after a coding session

A gap reported with `ds feedback submit` stays open until someone closes it.
When a session has just fixed one, closing it is part of the work: an open
report that is already fixed makes the next sighting merge into a stale entry
and makes the backlog count work that is done.

Everything here goes through `ds`. Never edit the backlog through an API, a
store, or the `fb` tab on the agent's behalf.

## 0. Read the backlog once, then read the difference

The backlog is large and every report carries a long acceptance text. Pulling
all of it into a context window on every visit is the single most expensive
mistake available here, and it buys nothing: the reports that mattered last
time have not changed.

Read it incrementally.

- **The first read is the only full read.** Keep what comes back — id,
  version, status, severity, component, title, and the detail — outside the
  conversation, in a file.
- **Every later read is `--since`**, with the newest `last_seen_at` from the
  previous pull:

  ```
  ds feedback list --since <RFC3339> --detail --output json
  ```

  That returns only what was seen or updated after that moment. Merge it into
  what you kept, by `id`, and advance the stored timestamp. A report whose
  `version` is unchanged needs no re-reading.
- **Work from a one-line-per-report index**, not from the detail. Severity,
  component and title are enough to decide what a task touches. Open the full
  `detail` only for the handful of ids you are actually about to verify or
  close — that is the moment the acceptance condition matters.
- **Fan out by partition, never by repetition.** When several agents triage
  the backlog, give each one a disjoint slice of ids. Do not hand the whole
  corpus to every agent.
- **Closing is what makes this cheap.** Every report closed with evidence
  leaves `not_addressed` permanently. A backlog nobody closes is re-read in
  full forever.

The same discipline governs any shared, slow-changing list this CLI exposes:
read once, keep a timestamp cursor, then read only the difference.

## 1. Find what this session touched

```
ds feedback list --component <repo-or-area> --detail --output json
ds feedback list --query '<words from the work>' --detail --output json
```

`--view` is `not_addressed` by default; pass `addressed` or `all` to see what
is already closed. Match reports to the work by component and by what the
report actually describes — never by title resemblance alone.

For a bounded newest-first closure campaign, obtain the complete acceptance
text for exactly the requested window before changing anything:

```
ds feedback list --limit 10 --detail --output json
```

Treat this only as a review queue. There is intentionally no bulk-close step:
verify and close each report independently, keep every report's returned
`version`, record `reporters`, `reporter_kind`, and `updated_by` so the human
operator who raised or last maintained it stays visible, and leave unverified,
undeployed, or unrelated reports open.

## 2. Verify against the acceptance condition

Read `.data.reports[].detail`. The submitter wrote an expected behavior and an
observable acceptance condition; that condition, not the diff, is what decides
whether the report is addressed. Prove it the way the report states it — the
usual proof is live discovery plus one real invocation:

```
ds capabilities --search '<the words the report used>' --output json
ds capabilities <command-id> --output json
```

If the fix is unmerged, undeployed, or in a build this `ds` is not running,
the report is not addressed yet. Leave it open.

## 3. Close it with the record

```
ds feedback close --id <id> --expect-version <version> --resolution '<what changed, and the evidence>' --yes --output json
```

Use `--status wont_fix` with a resolution that says why, when the gap is real
but will not be acted on. `--expect-version <version>` pins the version read
in step 1, so a report someone else edited meanwhile is refused rather than
overwritten.

The resolution is what the next reader sees instead of reopening the
investigation. Name the command or behavior that now exists and how it was
verified; at most 1000 characters, no secrets, no customer data.

## What can refuse, and what to do

- `feedback_not_permitted` — closing is an admin action on shared state. The
  signed-in DS GridDesign user must hold the platform triage capability. Do
  not retry, do not switch identity: report the ids and resolutions to the
  user so a capable account can close them.
- `feedback_conflict` — the report moved since step 1. List it again, confirm
  it is still addressed, then close it.
- `feedback_not_found` — the id is wrong or the report was deleted; list
  again with `--view all`.
- `desktop_signed_out` — sign in to DS GridDesign first.

## Rules

Close only what this session can show is addressed, one report at a time,
each with its own resolution. Do not close a report to tidy the backlog, do
not close by severity or age, and do not close what someone else is working
on. Reopening is deliberately not available here; it stays a human decision
in the `fb` tab.

When asked to close the latest N reports, finishing with fewer than N closures
is correct whenever some acceptance conditions are not proven in the deployed
`ds`. Return two explicit sets: closed ids with evidence, and remaining ids
with the exact unmet condition or deployment gap. Include each report's
`reporters` in both sets; do not replace a missing reporter with an inference.

Stops at: deployment — an acceptance only a newer deployed `ds` can meet stays
open for whoever deploys it.
