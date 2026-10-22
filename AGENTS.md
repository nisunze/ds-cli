# DS application workspace agent guide

Follow the workspace AGENTS.md. For delegated, cross-repository or release work,
use [collaboration](crates/kernel/ds-command-kernel/docs/agent-guidance/collaboration.md)
and [context management](crates/kernel/ds-command-kernel/docs/agent-guidance/context-management.md).
Read the applicable task contract; do not preload all guides.

## Owners and dependency boundaries

- `crates/kernel`: deterministic commands, application state, contracts, storage,
  sync and named native effect owners. Engineering, Solar and reporting algorithms
  stay in their independently released engine repositories.
- `crates/cli`: the ds executable, terminal/MCP adapters and native Agent Skills
  under skills/. Adapters consume Kernel operations and never Interactions.
- `crates/interactions`: transient hit testing, snapping, pointer/selection,
  camera, Canvas and Profile rendering/intents. No durable state, engineering,
  credentials, project acquisition, filesystem or network effects.
- `crates/shared/ds-geo-lite`: neutral portable geometry used by all groups and
  engines. It depends on none of the three groups.
- Kernel and Interactions are independent in both directions. Neither imports
  CLI. ds-web consumes Kernel and Interactions, never CLI. A gesture ends in a
  Kernel command; it never performs a durable write.

## Invariants

- One operation/refusal vocabulary across native and WASM. Typed Rust owners,
  explicit principal/project/object/revision context, stable IDs, units and
  digest admission; preserve dirty held data and atomic revision checks.
- Hosts execute declared effects; Svelte/TypeScript only bind and render. No
  mutable TypeScript fallback, alternate route catalogue or Tauri dependency
  in shared policy. Server builds must not depend on ds-web or Tauri.
- Reads are demand-driven, bounded and offline first. Held SQLite inputs remain
  useful without incidental network/freshness reads. Reuse captured inputs,
  coalesce consumers, retain invalidation until consumed, and prove read counts.
- Reveal CLI progressively: root domains, domain commands, then one complete
  Command descriptor with summary, purpose, effects, authority, inputs, outputs,
  examples and refusals. Bound output and state truncation.
- Call owning Rust operations, fixed typed process contracts or the protected
  native user client. Never expose caller-supplied argv, shell, SQL, code,
  ambient credentials or unregistered operations. MCP projects the live catalog.
- Small neutral geometry is allowed in Interactions; authored model commits,
  authorization, persistence and compute stay with their owners. Preserve numeric
  behavior, units, identity, ordering, admission bounds and refusals when moving.
- Skills teach discovery/workflows; keep descriptions short and load SKILL.md on
  demand. They do not copy commands, implement transport or create private tools.
- Prove actual installed CLI/MCP behavior first for a delivery gap; fix its Rust
  owner, test the exchange/refusal, then bind the UI and verify its user path.
- Independent units retain their package/unit IDs and releases. Actual typed
  contracts and capabilities decide compatibility, not source/version equality.

## Task routing

| Task | Contract |
| --- | --- |
| CLI command/owner selection, auth, blind acceptance | [CLI workflow](docs/development/cli-agent-workflow.md), [process boundary](docs/contracts/process-boundary-contract.md) |
| MCP | [reference](docs/reference/mcp.md), [protocol boundary](docs/contracts/protocol-boundary-contract.md) |
| Kernel/application authority | [application Kernel](crates/kernel/ds-command-kernel/docs/contracts/ds-application-kernel.md) |
| Native effects, authorization, demand reads | [headless runtime](crates/kernel/ds-command-kernel/docs/contracts/ds-headless-application-runtime.md) |
| Network boundary | [Kernel/Network](crates/kernel/ds-command-kernel/docs/contracts/ds-kernel-network-boundary.md) |
| Transport and held models | [transport](crates/kernel/ds-command-kernel/docs/contracts/ds-transport-authority.md), [held models](crates/kernel/ds-command-kernel/docs/contracts/desktop-held-models.md) |
| Transient interactions | [interaction boundary](crates/interactions/docs/interaction-boundary.md) |
| Portable geometry | [geometry](crates/kernel/ds-command-kernel/docs/contracts/ds-portable-geometry.md) |
| Cross-component acceptance | [Architecture queue](../ds-architecture/backlog/DECOUPLING.md) |
| Unfinished code/decisions/seeding | [principles](crates/kernel/ds-command-kernel/backlog/PRINCIPLES.md), OPEN.md / OPEN-X.md / SEEDING.md beside it |

## Proof and landing

Work on canonical run, preserve concurrent files/indexes/processes/caches, stage
only owned paths, and land after focused proof. No speculative framework or
duplicate algorithm. Coding findings are fixed or recorded in owning backlogs,
never product feedback or private instruction/handover files.

Use installed ~/.cargo/bin when PATH omits Cargo. This repository has one root
workspace/lockfile; select the affected package and target. Run its real consumer
test and meaningful refusal first. Metadata/dependency checks do not compile:

- `node scripts/check-workspace-boundaries.test.mjs`
- `node scripts/check-workspace-boundaries.mjs`
- `cargo test --locked -p <affected-owner> <test-filter>`
- `cargo fmt --all --check`; affected strict Clippy and portable wasm32 checks.

Native/process boundaries require real host tests; UI changes require actual
WASM execution/rendered interactions. Missing dependencies and inapplicable
checks report a named refusal/skip. A helper pass is not installed acceptance.
For an affected map/report delivery, run one bounded blind installed CLI/MCP
trial after implementation, with the job/output/mutation bounds and no source
or syntax hints. Documentation-only work checks links/diff without builds.
