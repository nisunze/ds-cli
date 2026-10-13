# ds-command-kernel agent guide

Read [the shared working agreement](docs/agent-guidance/collaboration.md)
and the nearest scoped AGENTS.md before changing code.

## Mandatory agent instructions

Every agent, regardless of model or tool, must read and follow the workspace,
repository and nearest scoped AGENTS.md before working. AGENTS.md is the sole
agent-instruction filename. Do not create or recreate CLAUDE.md, MEMORY.md,
vendor-specific rule files, private instruction/memory indexes or handover ledgers
to bypass these rules, including in tool folders, home directories or other repos.
Use the linked owning contracts for durable decisions and the shared backlogs for
unfinished work. Tool configuration and temporary scratch may exist, but must not
become competing product authority. A necessary scoped AGENTS.md adds local rules
and links to its parent; it must not duplicate or weaken the shared agreement.
If a tool cannot use AGENTS.md, report that limitation instead of inventing another
instruction file.

## Owns

Shared deterministic command decisions and state transitions, plus named native
application/transport/storage crates. Hosts own their credentials, scheduling,
filesystem/network effects and rendering; they consume one closed decision contract.
Engineering, Solar and report algorithms remain with their respective owners.

## Invariants

- One operation and refusal vocabulary across native and WASM consumers. No mutable
  TypeScript fallback, browser requirement or Tauri dependency in shared policy.
- Explicit project/principal/object/revision context; stable IDs, exact digests and
  typed admission. Preserve dirty held state and atomic transitions.
- Acquisition plans are demand-driven and bounded. Offline retained inputs must not
  trigger incidental network or freshness reads. Prove reads/requests when touched.
- Follow [prompt queue sync and cheap input freshness](docs/contracts/ds-headless-application-runtime.md#prompt-queue-sync-and-cheap-input-freshness): reuse captured inputs across sessions and use the shared cheap source-basis comparison for intentional online reads. Keep examples within the authorized implementation slice.
- A new shared slice serves a real consumer and deletes the duplicated decision;
  avoid speculative framework expansion or a second copy of an owning algorithm.
- Compatibility/schema changes need explicit consumer/migration evidence. Do not
  re-pin a test or weaken an integrity fence merely to bless a breaking change.
- Unfinished code, owner decisions and seeding/operational acceptance use the three
  owning backlog files; no model-specific handover or completed-work archive.

## Route the task

| Task | Read |
| --- | --- |
| Agent collaboration and completion | [Shared implementation loop](docs/agent-guidance/collaboration.md#small-implementation-loop), [verification and landing](docs/agent-guidance/collaboration.md#verify-then-land) |
| Kernel/application authority | [Application kernel](docs/contracts/ds-application-kernel.md) |
| Server/native effects | [Headless runtime](docs/contracts/ds-headless-application-runtime.md) |
| Network boundary | [Kernel/network boundary](docs/contracts/ds-kernel-network-boundary.md) |
| Transport and held models | [Transport authority](docs/contracts/ds-transport-authority.md), [held models](docs/contracts/desktop-held-models.md) |
| Portable geometry | [Portable geometry](docs/contracts/ds-portable-geometry.md) |
| Open work and governing principles | [Principles](backlog/PRINCIPLES.md), [code](backlog/OPEN.md), [owner decisions](backlog/OPEN-X.md), [seeding](backlog/SEEDING.md) |

## Checks and workspaces

- Use the installed ~/.cargo/bin toolchain when a non-interactive PATH omits Cargo.
- Root changes: affected cargo test --lib filters; use ./build.sh for the full
  native/WASM test and build path when that path is actually affected.
- crates/ds-client-core and crates/ds-project-data have independent manifests and
  lockfiles. Use --manifest-path for their checks; a root workspace selector does
  not test them. scripts/test-detached-workspaces.sh verifies this boundary.
- Native-host or cross-repo wire changes require the affected host/consumer tests,
  not only a successful pure-kernel test. Keep schema/digest/parity evidence explicit.
- Documentation: review links, authority and the diff; no product build/deploy.

The shared collaboration document and check live here. Keep them model-neutral;
scope-specific engineering detail belongs in its owning contract, not this guide.
