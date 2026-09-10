# Keep the multi-repository workspace root small

The workspace root is a navigation and planning anchor. It is not a package,
script directory, knowledge dump, output directory, or archive.

## Choose a home before writing

| Material | Home |
|---|---|
| Workspace instructions, current cross-stack plan, navigation | Root `AGENTS.md`, `PLAN.md`, `README.md` only |
| Typed automation, native agent skills, MCP projections | `ds-cli`; MCP remains a projection of the live executable |
| Engineering, native format parsing and characterized writers | `ds-network` and its owning crates |
| APIs, permissions, catalogs and persistence | `ds-brain` |
| Desktop/web design workflows and bridge implementation | `ds-web` |
| Offline field collection | `simple_notes` (DS Field) |
| Report calculation / solar calculation | `ds-network-reporter` / `ds-solar` |
| Host monitoring / platform reliability | `ds-server` / `ds-sre` |
| Reusable contributor setup and agent troubleshooting | `ds-cli/docs/development/` |
| Project inputs, receipts, deliverables and retained evidence | `_shared/<task>/` |
| Disposable scripts, logs, screenshots, probes and temporary outputs | `_discardable/<task>/` or an OS temporary directory |
| Linked Git worktrees | `_worktrees/`, managed through Git |

Run package managers and generators from their owning repository. Always pass
an explicit output path for task artifacts. Do not run a root package install
or leave root `node_modules`, `__pycache__`, `tmp`, `output`, `outputs`, capture
files, plans, envelopes, or handovers. One-off code stays scratch; reusable code
is reviewed with its owner before promotion. Never promote raw experiments as
supported commands or copy runtime schemas into a second catalogue.

The retained root infrastructure directories are `.github`, `.release`,
`_shared`, `_discardable`, `_worktrees`, and `_deprecated_stack`. `.git` is
allowed if the planning anchor is itself versioned. Live `ds-*` and
`simple_notes` checkouts must have a Git directory or worktree marker.
`ds-system` and `_deprecated_stack` remain excluded from routine work.
The retired `ds-mcp` and `ds-cli-skills` checkouts are explicitly rejected at
the active root, even when they contain Git metadata. Their canonical owner
is `ds-cli`. Do not infer retirement from a repository's name alone: a current
top-level checkout can differ from an older namesake in the historical stack.

## Reuse the knowledge already maintained by its owner

The Huye investigations must not become a second PLS implementation. Use the
[terrain workflow](../../skills/ds-pls-cadd-terrain-roundtrip/SKILL.md),
[backup delivery workflow](../../skills/ds-pls-cadd-backup-delivery/SKILL.md),
[library provenance workflow](../../skills/ds-library-seeding/SKILL.md), and
[PLS owner reference](../reference/pls.md). They preserve the durable boundaries:
exact native bytes and digest-pinned resources, closed workspaces, characterized
writers, explicit operator-owned alignment scope, and native Restore/reopen
acceptance distinct from successful DS readback.

Use the [MCP reference](../reference/mcp.md) for integrations. The retired
`ds-mcp` repository is not a destination for new knowledge or implementations.
Use [Remote-SSH Codex recovery](remote-ssh-codex.md) for the recorded extension
host failure. Historical handovers are evidence, not current authorization,
live capability descriptions, or proof that an old blocker remains open.
Read the deployed command contract again; verified gaps belong in the typed
feedback backlog, not in new root or repository gap ledgers.

## Enforce the boundary

From the workspace root, run at session start and before handoff:

```text
python ds-cli/scripts/check-workspace-root.py .
```

The check lists root entries only, reports unexpected names, returns a failing
exit status, and never reads their contents, deletes, moves, or repairs them.
It does not descend into repositories, native workspaces, or the retired stack.
It detects sprawl at verification time; it is not a filesystem write sandbox.

Put `<!-- ds-workspace-root:v1 -->` in the workspace `AGENTS.md` to opt into
the same check automatically when `python scripts/check.py` runs in `ds-cli`.
Unmanaged standalone checkouts and CI parents are explicitly skipped; CI runs
the gate's fixture tests. Add a new root exception only as an explicit owner
decision, with a corresponding gate and documentation change.

For cleanup, inventory exact sources and destinations first. Keep unique
evidence under a task directory with a checksum manifest. Preserve originals
when extracting reusable lessons, update navigation links, and label archived
scripts as historical and unsuitable for replay. Do not relocate live native
workspaces or registered worktrees as ordinary folders.
