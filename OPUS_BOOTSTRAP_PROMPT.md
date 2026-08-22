# Opus bootstrap prompt: replace DS MCP with a Rust CLI

You are the principal engineer responsible for beginning and carrying forward a
deliberate migration of Data Solutions' agent tooling from `ds-mcp` (Go + MCP)
to `ds-cli` (Rust + ordinary command-line contracts).

This is an implementation assignment, not a request for a speculative design
document. Inspect the repositories, establish the facts, make a small coherent
first slice work end to end, test it, document it, and leave the tree in a state
from which another capable coding agent can continue without reconstructing
your reasoning.

## Product direction

The owner has made the decision:

- MCP is being removed from Data Solutions.
- Go-based `ds-mcp` tooling is being removed from deployment.
- The deployed local agent/tooling surface will be 100% Rust.
- `ds-cli` is the new direct interface for humans and coding agents.
- An LLM coding session is treated like a capable coworker operating a normal
  executable. It should not need a protocol server to discover or invoke DS
  capabilities.
- The CLI must be unusually well documented and self-describing. A fresh agent
  should be able to run `ds --help`, discover the command tree, understand
  authority and effects, invoke a command safely, consume structured output,
  and recover from a failure without reading implementation source.

Do not implement an MCP server in Rust. Do not reproduce MCP concepts under new
names. Do not retain Go or MCP in the shipping path as a fallback. The desired
end state is a normal Rust CLI with explicit commands and stable machine-readable
contracts, delegating domain work to the Rust engines and application/API
surfaces that actually own it.

Migration may be incremental while the replacement is proved, but every slice
must move toward deletion. Compatibility code needs a named, temporary consumer
and a deletion condition; otherwise do not add it.

## Workspace and starting state

The workspace root is:

```text
/home/magese/data-solutions
```

The new repository is:

```text
/home/magese/data-solutions/ds-cli
```

At the time this prompt was written, `ds-cli` contained only `README.md` and the
initial Git commit. Confirm the current state rather than assuming it remains
unchanged.

Important sibling repositories include:

- `ds-mcp`: the existing Go implementation and the best inventory of current
  tool contracts. It is reference material and a migration source, not the
  target architecture.
- `ds-network`: the Rust engineering kernel/workspace and existing `ds-grid`
  CLI. Deterministic network, electrical, routing, geometry, conversion, and
  `.dsgrid` behavior belongs here.
- `ds-solar`: the Rust Solar workspace and prepared-batch execution path.
- `ds-network-reporter`: Rust reporting functionality.
- `ds-web`: the Tauri/Svelte desktop application, native bridge, packaging,
  release scripts, component catalog, and many current `ds-mcp` deployment
  references.
- `ds-brain`: authentication, project membership, durable business data,
  auditing, jobs, and public API ownership. The CLI is a client; it gets no
  direct database authority.
- `ds-deploy`, `ds-sre`, `ds-system`, and `ds-apis-tf`: deployment and
  operations surfaces that may contain stale hosted-MCP wiring.

Read repository-local `AGENTS.md`, `CLAUDE.md`, and governing contract documents
before changing files in each repository. Check `git status` in every repository
you touch. Existing changes belong to the operator; preserve them. Do not reset,
overwrite, or clean unrelated work.

## Governing intent already present in the workspace

Start by reading these sources and resolve contradictions using current code,
Git history, and the owner's decision above:

- `ds-web/docs/humble_objectives.md`
- `ds-web/docs/handover/FINAL-DESKTOP-CONSOLIDATION-HANDOVER-20260820.md`
- `ds-web/docs/desktop-shell-contract.md`
- `ds-web/docs/run-launcher.md`
- `ds-web/docs/deployment-contract.md`
- `ds-web/docs/desktop-distribution-contract.md`
- `ds-web/desktop-components.json`
- `ds-mcp/README.md`
- `ds-mcp/docs/contracts/runtime-language-and-ownership.md`
- `ds-mcp/docs/CLIENT_BOUNDARY.md`
- `ds-network/Cargo.toml` and its relevant CLI/contracts
- `ds-solar/Cargo.toml` and its prepared-batch interfaces

Some documents reflect intermediate decisions and will say that `ds-mcp` is a
core desktop sidecar or that a hosted `ds-mcp` survives temporarily. Those
statements are superseded by this decision: the replacement shipping surface is
`ds-cli`, implemented in Rust, with no MCP transport and no Go requirement.
Update governing documentation as behavior changes; do not leave contradictory
architecture prose behind.

## Architecture boundaries

Preserve these boundaries during the migration:

1. **The CLI coordinates; domain owners compute.** Do not copy engineering,
   geometry, electrical, Solar, report-generation, or conversion algorithms
   into `ds-cli`. Use Rust libraries when a clean library boundary exists, or
   call a versioned native/application contract when process separation is the
   correct ownership boundary.

2. **No hidden privilege.** The CLI is comparable to the signed-in desktop or
   web client. It must not read Firestore directly, use an ambient service
   account to impersonate a project user, accept a user/project claim as proof
   of authorization, or bypass `ds-brain`/the paired desktop authority surface.

3. **Local-first does not mean authority-free.** Local deterministic discovery
   and computation can work offline where their contracts allow it. Project
   reads and effects still require an explicit, verified current principal and
   project. Possession of a file, handle, bridge descriptor, or project ID is
   not authorization.

4. **The desktop owns its semantic operations and session.** Where Reporter,
   Solar, project selection, collision work, or artifact publication already
   has an app-owned semantic function, use that same function through the
   authenticated native bridge. Do not build a second implementation for the
   CLI.

5. **Cloud ownership remains with `ds-brain`.** Use its ordinary public/API
   Gateway contracts with the signed-in user's authority. Do not change API
   Gateway or add private endpoints merely to make the CLI convenient.

6. **Effects are explicit.** A command that writes a file, opens UI, queues a
   job, publishes an artifact, spends model tokens, or changes remote state must
   say so in help and structured metadata. Destructive or remote mutations need
   explicit operands and suitable confirmation/idempotency behavior; never
   infer consent from conversational prose.

7. **Fail closed and tell the truth.** Missing engines, sign-in, project
   binding, external tools, data assets, or optional document runtimes produce
   typed unavailable/refusal outcomes with a concrete remedy. Never silently
   substitute a Python path, cloud compute, synthetic result, different project,
   or approximate domain algorithm.

8. **No Python or Go runtime in the final desktop agent path.** External QGIS,
   PyQGIS, Pandoc, and LibreOffice may be detected and used only according to
   their existing contracts; the DS-owned CLI and compute path itself ships as
   Rust native binaries.

## Authentication, desktop cache, APIs, and shared Rust crates

The default interactive architecture is **paired desktop reuse**, not a second
CLI login.

When DS GridDesign is running, `ds` should discover its private
`agent-bridge.json`, authenticate to the random-loopback bridge with the
descriptor's short-lived pairing secret, and use the desktop's current signed-in
session and selected project. The bridge must never return the Firebase JWT or
refresh token. Authenticated API operations remain inside the existing frontend
API client, which already owns token refresh and API Gateway authorization.

This is also the correct route to the web application's local cache. The CLI
should consume cache-backed semantic operations through the running application;
it must not open, scrape, lock, copy, or reverse-engineer the WebView's physical
IndexedDB/LevelDB files. IndexedDB is an implementation detail, can be live and
locked, is partitioned by installation/profile/account/project, and is not an
authorization source. Expose any missing bounded cache read as a named semantic
bridge operation using the same TypeScript store/repository function as the UI.

Expected routing order:

1. Pure local discovery or deterministic computation uses the authoritative
   Rust crate directly and requires no login when the domain contract permits.
2. Desktop state, IndexedDB-backed working copies/outboxes/catalogs, current map
   context, user confirmation, and app-owned workflows use the paired loopback
   bridge.
3. Authenticated project reads/writes use the same public API contracts and
   signed-in user authority as ds-web. In paired mode, execute them through the
   desktop semantic operation so credentials never leave the app.
4. If the app is not running or is signed out, paired/authenticated commands
   fail with a typed remedy; they do not fall through to ambient ADC, a service
   account, raw cache files, or a different identity.

Provide commands such as `ds desktop status` and `ds auth status` (exact naming
may change after inspecting conventions) that clearly report whether the app is
paired, signed in, and project-bound without exposing credentials. Automatic
descriptor discovery should cover Stable/Canary/dev profiles deterministically,
refuse ambiguity, and allow an explicit `--desktop-descriptor <path>` override.

A separate headless login may be designed later for CI or machines without the
desktop, but it is not the first slice and must not be necessary for normal
operator use. If implemented, use a real user OAuth/device authorization flow,
store secrets in the OS credential vault, make identity visible/revocable, and
use the same public APIs. Never read browser credential databases or store a
refresh token in plaintext config. ADC remains deployment/developer tooling,
not end-user project identity.

For native computation, `ds-cli` should link the same authoritative Rust crates
already used by the Tauri application. Current examples to verify include
`ds-io`, `ds-cleaning`, and `ds-network` from `ds-network`;
`ds-solar-contracts` and `ds-solar-runtime` from `ds-solar`; and the appropriate
pure reporting cores from `ds-network-reporter`. Do not depend on the complete
`ds-web-desktop` Tauri shell as a library. When Tauri and CLI need the same
native orchestration, extract the Tauri-independent portion into a small crate
owned by the relevant domain and have both binaries call it. Frontend/IndexedDB
semantics remain behind the bridge until deliberately moved to an authoritative
shared Rust store; do not duplicate them in Rust merely to avoid the bridge.

## Workflows and scripting

Make individual `ds` commands composable enough that Bash, PowerShell, CI, and
LLM agents can build workflows from stable JSON and exit codes. Ship useful
canonical multi-step workflows as first-class Rust subcommands when DS owns the
semantics—for example, prepare then calculate then inspect results—not as opaque
shell strings.

Allow repository-local/custom workflow recipes once the primitive commands and
effect model are stable. Prefer a small declarative, versioned format (for
example `ds-workflow/v1` in TOML or YAML) with named CLI steps, typed inputs,
explicit output references, conditions, bounded retries/timeouts, and a complete
preflight effect/authority summary. A workflow runner may call only registered
`ds` commands; it must preserve each command's authorization, confirmation,
idempotency, and output bounds.

Do not embed Python, Node, a shell evaluator, arbitrary Rust compilation, or a
general-purpose code-execution plugin system inside `ds`. Do not accept workflow
steps as raw command strings. Initially, external scripts can invoke `ds`
normally, while checked-in examples demonstrate safe composition. Add a native
`ds workflow run` only when at least one real workflow proves the abstraction.

## What “LLM-friendly CLI” means

Design for both a person in a terminal and an agent invoking commands without
an MCP schema exchange.

The executable should ultimately be named `ds`. Use a conventional, predictable
command tree. Do not flatten dozens of unrelated verbs into one namespace.

Every shipped command must provide:

- excellent `--help` text with purpose, authority requirement, effect, inputs,
  defaults, examples, output description, and common refusal/remedy cases;
- stable stdout behavior: successful machine output goes to stdout; diagnostics
  and progress go to stderr;
- a global machine-readable mode such as `--output json`, with JSON as the
  automation contract and human-readable output as presentation;
- a versioned response envelope or another documented compatibility strategy;
- stable, documented exit codes grouped into useful classes (success, invalid
  input, unavailable dependency, unauthenticated/unauthorized, conflict/stale
  state, execution failure, and internal error);
- strict input parsing, bounded payload/file sizes, bounded output, timeouts,
  cancellation behavior, and no secret leakage;
- deterministic ordering where practical;
- `--no-color` and non-interactive behavior that works in CI and agent shells;
- an explicit `--yes`/confirmation policy for commands that truly require it,
  while read-only commands remain frictionless;
- examples that are executable and tested where possible.

Provide a discoverable capability inventory without recreating MCP. A command
such as the following is appropriate:

```bash
ds capabilities --output json
ds help <command>
ds doctor --output json
```

The capability inventory should describe CLI commands, not “tools,” and may
include stable command ID, contract version, effect class, authority, execution
mode, availability, and concise remediation. It must be derived from the same
command definitions used by dispatch/help where practical so it cannot drift.

### Progressive discovery and context economy

This requirement is paramount. The principal consumers are agentic desktop and
editor products—ChatGPT Desktop, Claude Code/Desktop, GitHub Copilot in VS Code,
and equivalent coding-agent shells. They can execute ordinary commands, inspect
files, and continue across turns. The CLI should exploit that strength instead
of injecting its entire surface into every model context.

`ds` represents the full Data Solutions stack to these agents, but it must reveal
that stack progressively:

```text
ds --help
  -> compact list of domains and discovery instructions

ds network --help
  -> compact network command index

ds network inspect --help
  -> complete contract for exactly one operation

ds network inspect --help --output json
  -> machine-readable schema/metadata for exactly one operation
```

Do not print every command, schema, example, availability probe, or long-form
explanation from root help or default capability discovery. A caller interested
in Solar should not pay the context cost of QGIS, Reporter, survey, feedback,
and network contracts. Domain discovery must not initialize or probe unrelated
engines.

Design a tiered documentation surface:

1. **Root:** identity, one-line domain summaries, global flags, and how to drill
   down. Keep it stable and short.
2. **Domain:** command names with one-line purpose/effect/availability summaries.
3. **Command:** complete inputs, authority, effects, examples, output contract,
   refusal cases, and remedies for that command only.
4. **Deep reference:** versioned local Markdown/JSON Schema files available by
   explicit command or path, never pushed into normal help automatically.
5. **Runtime result:** only the requested projection plus a compact envelope and
   a clear continuation mechanism when more data exists.

Capability discovery must therefore be filterable and cheap. Prefer forms such
as:

```bash
ds capabilities                         # compact domain index
ds capabilities network                 # compact network command index
ds capabilities network.inspect         # one full command descriptor
ds capabilities search "transformer report" --limit 10
```

Exact names may change, but the behavior may not collapse into one giant JSON
catalog. Search results should contain command IDs and one-line summaries; the
agent explicitly requests the full descriptor for a selected result.

Keep output bounded by default. Large tables, geometries, logs, reports, and
documents require explicit projections, limits, filters, and resumable
continuation/cursor commands. Return artifact paths or handles instead of
inlining large payloads. Provide compact JSON by default in machine mode and an
explicit pretty-print option for humans. Errors should be short but carry a
stable code, retryability, and the next relevant command; do not emit stack
traces unless diagnostic verbosity is requested.

Documentation should be excellent without being duplicated. Generate CLI help,
machine descriptors, reference pages, and shell completions from shared command
metadata where practical. Keep examples adjacent to the command they teach and
test them. Use short cross-links rather than copying architecture prose into
every subcommand. Long guides belong in versioned local docs that agents open
only when needed.

Treat context size as a testable interface budget. Add golden tests or explicit
bounds for root help, domain help, search results, default JSON responses, and
error messages. A new command must not make unrelated help materially larger.
Measure bytes as well as correctness. The goal is high capability density: a
new agent discovers the one relevant command in a few small calls, learns its
exact contract, performs the work, and receives only the result needed for the
next decision.

Prefer composable explicit inputs:

```bash
ds network inspect --model ./example.dsgrid --output json
ds solar run start --context <id> --output json
ds report transformer export --transformer <id> --destination ./out --output json
```

These are illustrative, not commands you should invent without checking the
real domain contracts. Retain stable domain vocabulary and identifiers. For
large or nested requests, support a documented JSON request file or stdin rather
than an enormous collection of ambiguous flags. Never accept arbitrary shell
commands or model-authored SQL/code as a substitute for typed operations.

Structured output should be sufficient for an agent to decide the next step.
A refusal should identify what failed, whether retry is sensible, and a bounded
remedy. Do not require an LLM to scrape English logs or inspect source code.

## Migration inventory

Treat the current `ds-mcp` registry as an inventory to classify, not as a list
to port blindly. For each existing capability, record:

- current name and contract version;
- actual owning repository/service;
- current implementation and transport;
- effect and authority requirement;
- whether it is still a real product capability;
- proposed `ds` command path, if any;
- direct Rust library, native process, desktop bridge, or public API boundary;
- migration dependencies and parity proof;
- deletion targets in `ds-mcp`, `ds-web`, deployment, tests, and docs;
- disposition: migrate, merge, replace, defer with reason, or delete.

The existing catalog includes discovery, survey/network queries, layer-style
proposals, desktop project operations, Reporter exports, collision detection,
feedback, project-task/report proposals, LV routing, PLS Oracle jobs, QGIS
discovery/management, and Solar workspace/run/report/document operations. Verify
the live registry; do not assume the README is exhaustive or current.

Explicitly distinguish:

- capabilities that are merely MCP transport wrappers and disappear;
- useful application commands that should become direct CLI commands;
- model/chat orchestration that should not live in a deterministic CLI;
- functionality already better exposed by `ds-grid` or another Rust binary;
- stale/dead code that should be deleted rather than migrated.

Avoid turning `ds-cli` into a monolith. Reuse crates, but be alert to dependency
cycles between sibling workspaces. If a shared contract crate is warranted,
locate it with its real owner and explain the boundary. Do not casually vendor
or duplicate large sibling repositories.

## Deployment migration

The work is broader than creating a Cargo project. Find and eventually replace
all shipping references to the Go/MCP path, including at least:

- Go toolchain checks and `go build` steps;
- `go.mod`/`go.sum` completeness checks;
- `ds-mcp` and `ds-grid-mcp` sidecar builders;
- Tauri `externalBin` declarations and capability allowlists;
- component IDs, manifests, release pins, build identity contracts, and cache
  keys;
- Windows/Linux build, publish, install, repair, and verification scripts;
- local run launcher services, ports, health checks, environment variables, and
  logs;
- Cloud Run/Terraform/IAM/container/server-side assistant routing for hosted
  `ds-mcp`;
- `DS_MCP_*`, MCP URL/audience/port configuration;
- semantic smoke tests that launch or handshake with MCP;
- docs and handovers that describe MCP as the supported agent door.

Do not perform a blind global rename from `ds-mcp` to `ds-cli`. Many references
describe server assumptions—HTTP ports, stdio protocol, sessions, chat/model
orchestration, tool schemas, handshakes—that should be removed, not renamed.

Release provenance remains mandatory. The Rust binary must expose verifiable
build identity (contract/schema version, product/binary name, exact source SHA,
target, profile, and dirty-state policy) in a stable machine-readable command.
Packaging must build from an exact clean pin and verify the produced executable
before inclusion. Preserve platform parity for Windows and Linux.

There may temporarily be two Rust executables (`ds` plus an existing domain
binary) while library boundaries are rationalized. That is acceptable when the
ownership is clear. Shipping `ds-grid-mcp` is not acceptable in the end state;
the MCP crate/transport must be removed once its last proven consumer is gone.

## First execution objective

Begin with a vertical foundation slice that proves the architecture without
pretending the entire migration is complete.

1. Audit current repository instructions and dirty state.
2. Build a concise migration matrix from the live `ds-mcp` registry and actual
   call paths.
3. Decide and document the initial crate/workspace shape for `ds-cli` based on
   real dependency boundaries.
4. Scaffold a production-quality Rust CLI with locked dependencies, formatting,
   linting, tests, version/build identity, human and JSON output, stable error
   classes/exit codes, capability discovery, and `doctor`.
5. Implement one useful, low-risk vertical command backed by the correct
   existing Rust/domain owner. Prefer a discovery/read-only operation that
   proves real integration and structured refusal behavior; do not use a fake
   placeholder implementation.
6. Add tested examples and documentation sufficient for a fresh coding agent to
   build the binary, discover the command, invoke it, interpret success and
   failure, and locate the next migration work.
7. Integrate that slice into one real development or packaging path if it can be
   done coherently and proved. Do not declare deployment migrated from a local
   `cargo test` alone.
8. Identify the next smallest vertical slice and the exact obsolete code its
   completion will allow us to delete.

If the correct first command is unclear, investigate and choose using these
criteria: real user value, existing Rust owner, low authority/effect risk,
representative structured contracts, and ability to prove parity. Record the
reason; do not stop merely to ask the operator to choose between equally safe
implementation details.

## Quality bar

Use stable Rust unless a repository contract explicitly requires otherwise.
Prefer a small dependency set and explain significant dependencies. Pin and
commit the lockfile for an application. Deny or address warnings. Avoid
`unsafe`; if unavoidable, isolate and justify it. Never panic for user input or
expected dependency failure.

At minimum, establish and run the relevant equivalents of:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Add focused integration/golden tests for:

- help and command discoverability;
- JSON schema/envelope stability;
- exit-code mapping;
- unavailable/refusal behavior;
- build identity;
- the real vertical command;
- platform-neutral path/output behavior where relevant.

Use fixture-backed semantic tests where the command touches domain data. A mock
that proves only your own adapter is not parity proof. Keep tests bounded and
deterministic; never require live production credentials for the default suite.

Documentation is part of the interface. Include:

- a useful root README;
- installation/build instructions;
- command conventions and output/exit-code contracts;
- authentication/project-binding explanation;
- examples for humans and LLM agents;
- architecture and ownership boundaries;
- migration matrix/roadmap with evidence-based status;
- contribution workflow and verification commands.

Use generated CLI help as the source of truth where possible and test examples
to prevent drift. Avoid giant prose that restates code without teaching a caller
what to do.

## Working method

- Work autonomously and keep a short, current plan.
- Search broadly before editing; follow call sites across repositories.
- Make changes in coherent, reviewable slices.
- Preserve unrelated work and do not perform destructive Git operations.
- Do not commit, push, deploy, publish, or alter live cloud state unless the
  operator explicitly requests it.
- You may delete obsolete source/config/tests/docs when the replacement is
  genuinely proved and the deletion is in the requested scope. Before deletion,
  resolve the exact consumers and state the evidence.
- Do not weaken a test simply because it encodes the old architecture. Determine
  whether it protects an invariant that survives the migration, then rewrite or
  remove it accordingly.
- Do not claim completion based only on compilation or unit tests. Report what
  was actually exercised.
- When blocked by a missing credential or external system, complete all local
  work and tests possible, document the exact blocked proof, and provide the
  precise command the operator can run.

## Definition of the eventual migration outcome

The larger migration is complete only when:

- the supported local agent entry point is the Rust `ds` executable;
- its help and structured capability inventory are enough for a new LLM session
  to operate it productively;
- every retained command reaches its authoritative Rust engine, paired desktop
  semantic operation, or public `ds-brain` API without duplicated domain logic;
- authentication, project binding, effects, refusal, idempotency, and output
  contracts are explicit and tested;
- Windows and Linux packages build, verify, install, and exercise the pinned Rust
  CLI;
- no deployed/local agent path builds or runs Go;
- no `ds-mcp`, MCP endpoint, MCP stdio transport, MCP handshake, MCP SDK, MCP
  manifest, `ds-grid-mcp`, or `DS_MCP_*` configuration remains in shipping code;
- hosted `ds-mcp` infrastructure and server-side routing are removed;
- obsolete tests, docs, release pins, component entries, environment variables,
  and compatibility branches are deleted or rewritten;
- domain parity and at least one installed desktop smoke are proven, with no
  hidden Python, Go, MCP, or cloud-compute fallback.

Do not claim that eventual outcome during the first slice. Make the first slice
real, document exactly what it proves, and leave an evidence-based route to the
next one.

## Begin now

Start by inspecting instructions, Git state, the live `ds-mcp` registry, the
existing Rust workspaces, and the concrete `ds-web` packaging/launcher call
sites. Then state a short plan and execute the first vertical slice. Prefer
working code and verified contracts over an expansive proposal. At handoff,
summarize:

1. what changed;
2. what architectural facts were verified;
3. commands/tests run and their results;
4. what remains on Go/MCP;
5. the next vertical slice and the deletion it unlocks;
6. any external proof the operator must perform.
