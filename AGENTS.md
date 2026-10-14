# ds-cli agent guide

Follow the workspace AGENTS.md and any nearer scoped guide. Use the task table
below to open only the relevant section. For cross-repository, release or
delegated work, use the [shared working agreement](../ds-command-kernel/docs/agent-guidance/collaboration.md)
and [context guide](../ds-command-kernel/docs/agent-guidance/context-management.md).

## Owns

The ds executable, its CLI/MCP projection and canonical native Agent Skills under
skills/. This repository adapts owning Rust operations; it does not own engineering,
geometry, electrical, Solar, routing, reporting or conversion algorithms.

## Invariants

- Reveal the surface progressively: root help lists domains, domain help lists
  commands and one command descriptor carries its complete contract.
- Declare summary, purpose, effects, authority, inputs, output, examples and
  refusals once in the Command value. Bound default output and report truncation.
- Call an owning crate, a reviewed typed process contract, or the protected native
  user client. Never expose caller-supplied argv, shell, SQL, code execution,
  ambient credentials or an unregistered owner operation.
- Keep project and principal context explicit and audience-fenced. A project ID or
  saved selection is an address to authorize, never proof of authority.
- MCP is a projection of live ds capabilities, not a second schema or transport.
  Skills teach discovery and workflows; they do not copy commands or implement them.
- Prove a delivery gap through the installed CLI/MCP before a broad refactor.
  Repair the smallest owning slice, prove it headlessly, then bind the UI.
- Server owners must not compile ds-web or Tauri. Shared native behavior belongs
  with its Rust owner and is consumed by both CLI and Desktop.

## Route the task

| Task | Read |
| --- | --- |
| Add or change a command | [CLI workflow: before adding a command](docs/development/cli-agent-workflow.md#before-you-add-a-command) |
| Owner/process selection | [CLI workflow: reaching an owner](docs/development/cli-agent-workflow.md#reaching-an-owner), [process boundary](docs/contracts/process-boundary-contract.md) |
| Authentication and project context | [CLI workflow: authority](docs/development/cli-agent-workflow.md#authority), [headless runtime](../ds-command-kernel/docs/contracts/ds-headless-application-runtime.md) |
| Delivery or refactor | [Evidence before refactoring](docs/development/cli-agent-workflow.md#evidence-before-refactoring) |
| Blind workflow acceptance | [Blind CLI/MCP validation](docs/development/cli-agent-workflow.md#blind-climcp-workflow-validation) |
| MCP surface | [MCP reference](docs/reference/mcp.md), [protocol boundary](docs/contracts/protocol-boundary-contract.md) |
| Multi-repository layout | [Workspace layout](docs/development/workspace-layout.md) |
| Agent skill change | This guide, the affected SKILL.md and scripts/check.py |

## Skills and discovery

Skill descriptions stay short and selective. Full SKILL.md content loads only
after selection. Keep flags, schemas, enums, outputs and refusals in the live
Command declaration and discover them with ds capabilities. Reject a skill-local
executable, copied command contract, direct API route or separate gap ledger.

Coding findings are fixed or recorded in the owning shared backlog; coding
sessions do not submit product feedback. Product-use sightings outside coding
use the governed feedback surface.

## Checks

Run the affected crate/unit/integration test and a meaningful refusal first.
Use the full checks only when the changed boundary warrants them:

- cargo fmt --all --check
- cargo clippy --workspace --all-targets --all-features -- -D warnings
- cargo test --workspace --all-features
- python3 scripts/check.py
- bash scripts/test-install-skills.sh

Process contracts require their real engine. Bridge parity requires the ds-web
checkout. Missing dependencies or inapplicable checks must refuse or report a
named skip; they do not silently pass.

For map/report workflow changes, run one blind installed CLI/MCP trial after the
implementation is ready. The trial gets the user's job, allowed project/output
and mutation bounds, but no source, copied syntax or implementation hints.

Documentation-only work reviews links, authority and the diff. It does not run
product builds or deployment.
