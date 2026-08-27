# `ds feedback`

`ds feedback` is the agent's end of the shared product backlog — the same
deduplicated ledger DS GridDesign's `fb` tab reads and writes. It is not an
issue-file generator and it does not introduce an MCP or another API client.

Every command uses the paired application's signed-in session. `ds` sends one
typed operation; DS GridDesign calls its existing `/api/v1/feedback` client. No
credential leaves the application.

```text
submit → (a coding session closes the gap) → list → close
```

## `submit`

Records an agent's observed product gap. DS GridDesign pins `reporter_kind` to
`agent` and adds the active project only as optional triage context.

Submit only after live `ds capabilities` discovery establishes that a
capability is absent or broken. Include bounded non-secret evidence, the
expected behavior, and an observable acceptance condition. Repeated sightings
of the same open gap are deliberately merged by the feedback service.

## `list`

Reads the backlog the `fb` tab shows: `--view not_addressed` (the default),
`addressed`, or `all`, narrowed by `--component` or `--query`. Rows carry the
`id` and `version` a close takes, and `--detail` returns each report's full
text — including the acceptance condition its author wrote down.

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
without the fix costs more than one left open, because the next sighting merges
into a new report instead of raising the occurrence count on this one.

Reopening is deliberately absent. Returning a report to the open backlog stays
a human triage decision in the `fb` tab.
