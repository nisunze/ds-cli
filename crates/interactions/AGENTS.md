# ds-interactions agent guide

Follow the workspace AGENTS.md and any nearer scoped guide. Read the
[interaction boundary](docs/interaction-boundary.md) for behavior changes. For
cross-repository or delegated work, use the [shared working agreement](../ds-command-kernel/docs/agent-guidance/collaboration.md)
and [context guide](../ds-command-kernel/docs/agent-guidance/context-management.md).

## Owns

Transient UI interaction algorithms: pointer hit testing, snap candidate ranking,
selection gestures, viewport transforms, Canvas2D layout and interaction indexes.

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
