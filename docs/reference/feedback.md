# `ds feedback`

`ds feedback submit` records an agent's observed product gap in the same
deduplicated backlog as DS GridDesign's `fb` shortcut. It is not an issue-file
generator and it does not introduce an MCP or another API client.

The command uses the paired application's signed-in session. `ds` sends one
typed `feedback.submit` operation; DS GridDesign pins `reporter_kind` to
`agent`, adds the active project only as optional triage context, and calls its
existing `/api/v1/feedback` client. No credential leaves the application.

Submit only after live `ds capabilities` discovery establishes that a
capability is absent or broken. Include bounded non-secret evidence, the
expected behavior, and an observable acceptance condition. Repeated sightings
of the same open gap are deliberately merged by the feedback service.
