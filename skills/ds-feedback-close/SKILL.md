---
name: ds-feedback-close
description: Verify and close the DS backlog reports a coding session fixed, through `ds feedback list` and `ds feedback close`.
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

## 1. Find what this session touched

```
ds feedback list --component <repo-or-area> --detail --output json
ds feedback list --query '<words from the work>' --detail --output json
```

`--view` is `not_addressed` by default; pass `addressed` or `all` to see what
is already closed. Match reports to the work by component and by what the
report actually describes — never by title resemblance alone.

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
ds feedback close --id <id> --resolution '<what changed, and the evidence>' --yes --output json
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
