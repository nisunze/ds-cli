# ds-interactions agent guide

Read the [workspace agreement](../AGENTS.md), the [shared collaboration
agreement](../ds-command-kernel/docs/agent-guidance/collaboration.md), and any
nearer scoped AGENTS.md before editing.

## Owns

Transient UI interaction algorithms: pointer hit testing, snap candidate ranking,
selection gestures, viewport transforms, Canvas2D layout and interaction indexes.
Read [the interaction boundary](docs/interaction-boundary.md).

## Invariants

- No command-kernel, engineering, credentials, project acquisition, filesystem or
  network dependency. Small neutral geometry primitives are allowed.
- These crates serve UI hosts. CLI and Server must not depend on them. Headless
  Rust tests exercise UI algorithms without making them server capabilities.
- Authored model changes, authorization, persistence and computation stay with
  their existing owners. Pointer intent is not an admitted model mutation.
- Move existing implementations and their tests mechanically; preserve numeric
  behavior, units, stable identity, ordering, admission bounds and refusals.
- Work on canonical run, stage only owned paths, and preserve concurrent authors.

## Checks

Run affected Rust tests and strict Clippy, then portable wasm32 checks. Browser
adapter changes need actual WASM execution and owning interaction tests, including
a control proving ordinary map use does not load Network engineering.
Use the shared backlog for unfinished work; do not create a handover archive.
