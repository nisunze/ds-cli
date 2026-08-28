# Working in `ds-cli`

Read this before changing anything here. It is short on purpose.

## What this repository is

The `ds` executable: one door into the Data Solutions stack, for a person in a
terminal and for a coding agent. It replaces `ds-mcp` (Go + MCP) as the
supported local agent surface.

It is **not** an engine. No engineering, geometry, electrical, solar, routing,
reporting or conversion logic lives here, and none may be added. The CLI parses
arguments, calls an owner, and shapes the answer.

## The one rule that shapes everything

**Reveal the stack progressively.** Root help lists domains. Domain help lists
commands. Command help is one contract. Nothing prints the tier below it.

A caller interested in one domain must never pay the context cost of the rest.
This is why the whole stack can live behind one binary. It is enforced in bytes
by `crates/ds/tests/context_budget.rs`, and the load-bearing assertion is
`root_help_scales_with_domains_not_commands`.

If you find yourself adding text to root help, you are almost certainly solving
the problem at the wrong tier.

## Before you add a command

1. **Who computes it?** Find the owning crate or binary. If the answer is "I'll
   write the logic here", stop — that is the one thing this repository does not
   do.
2. **Which domain?** A new domain costs every caller a line of root help
   forever. Prefer an existing one. Nest instead of proliferating:
   `ds pls oracle submit` is fine, a `pls-oracle` domain is not.
3. **Declare it once.** Everything — summary, purpose, effect, authority,
   inputs, output, examples, refusals — goes in the `Command` value. Help, the
   JSON descriptor, validation and dispatch all read that. There is no second
   place to describe a command, and adding one would be the first thing to
   reject in review.
4. **Enumerate its refusals.** Every way it declines, with a stable code and a
   remedy. `refusals_are_named_and_actionable` requires the remedy;
   `runnable_examples_run_and_fail_only_as_documented` checks the codes are
   real.
5. **Bound its output.** What is the cheapest useful default? Everything larger
   is an explicit projection with a `--limit`, and truncation must be reported
   in `more`.

## Reaching an owner

Three routes, and the choice is not stylistic.

**Link the crate** when it is a pure library with a clean boundary — as
`ds-cli-dsgrid` links `ds-grid-model` and `ds-grid-exchange`, the same crates
the Tauri desktop links. Use a path dependency to the sibling repository,
declared in the workspace `Cargo.toml`.

**Call the typed process contract** when the owning workspace has deliberately
chosen process separation and documented why. Three already exist:

| Binary | Owner | Shape |
|---|---|---|
| `ds-grid` | `ds-network/crates/ds-grid-cli` | one named subcommand per call |
| `ds-report` | `ds-network-reporter/src/bin/ds-report.rs` | typed request file, machine-readable result file |
| `ds-solar` | `ds-solar/apps/ds-solar-cli` | prepare/run split; `run` may not touch the network |

`ds-report`'s own header states the rule: one named subcommand per call, never
a caller-supplied argv, a typed request file rather than flags built from model
output. Honour it. Never build a generic `run_<binary>(args)` shape — a
subcommand no command registers must stay unreachable.

A sibling DS executable is reached through `ds-cli-exec`, which takes a
`&'static str` subcommand. The rule across the whole repository is that **no
caller-supplied argv reaches any owner**: every spawn site passes a statically
known argument list, and the set of non-test files permitted to create a
process is pinned by `crates/ds/tests/process_boundary.rs`. There are four such
owner classes — a sibling DS executable, the platform package manager,
an already-installed executable being probed, and `ds` itself re-invoked by the
MCP adapter — and each is named in
[`docs/contracts/process-boundary-contract.md`](docs/contracts/process-boundary-contract.md).
A generic `run(binary, argv)` reachable from a command is still forbidden, and
so is a fifth spawn site that has not been added to that test deliberately.

**Ask the paired application** when the owner is the running desktop itself —
its map, its local layers, its signed-in session, its transformer rooms. None
of that is reachable from a file or a sidecar, so `ds map` is entirely this
route. The same rule shape applies: `ds-cli-desktop::bridge` sends one named
semantic operation from a closed set, the application performs it under the
identity it already holds, and what comes back is an outcome — never a
credential and never the ability to run code inside the app. A generic
`invoke(operation, args)` reachable from anywhere would be the same mistake as
a generic argv, so `ds map` declares every operation and argument key it can
send in `BRIDGE_OPS` and refuses to send anything else.

Where a command translates its own flags into an owner's typed request, those
field names are a hand copy and must be **checked against the installed
engine**, not assumed. The first version of `ds solar verify-weather` sent
`--dataset` to an engine that only accepts `--file`; it compiled, it read
plausibly, and it failed at runtime. Prove every mapping against the real
binary.

**Never** depend on the `ds-web-desktop` Tauri shell as a library. Where Tauri
and `ds` need the same native orchestration, extract the Tauri-independent part
into a crate owned by the relevant domain and have both call it.

## Authority

`ds` has no hidden privilege. It is comparable to the signed-in desktop or web
client. It does not read Firestore, does not use an ambient service account to
impersonate a user, and does not accept a project id as proof of anything.

Routing order:

1. Pure local discovery or deterministic computation → the authoritative Rust
   crate directly, no login where the domain contract permits.
2. Desktop state, app-owned workflows, the web app's local cache → the paired
   loopback bridge (`ds-cli-desktop`).
3. Authenticated project reads and writes → the same public API contracts
   ds-web uses, executed *through* the desktop so credentials never leave the
   application.
4. App not running, or signed out → a typed refusal with a remedy. Never a
   fallback to ADC, a service account, raw cache files, or another identity.

The web application's IndexedDB is an implementation detail. Do not open,
scrape, lock, copy or reverse-engineer it. A missing cache read is a named
semantic bridge operation using the same store function the UI uses.

## Verification

All of these must pass, and CI runs all of them:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The suite links `ds-network` by path and binds to its real `.dsgrid` fixture.
That is deliberate: a vendored copy would keep passing after the format moved
on, reporting parity that no longer exists.

These suites carry most of the weight, and it is worth knowing what each one
will refuse to let you do:

| Suite | Refuses |
|---|---|
| `context_budget.rs` | help or a result growing past an allowance that already scales with what the command declares |
| `contract.rs` | a command that is not fully described, or whose help and descriptor disagree |
| `refusal_coverage.rs` | an error code a handler can emit that no command documents |
| `engine_parity.rs` | a hand-copied schema field that the installed engine does not actually have |
| `bridge_parity.rs` | the same, for the paired application: an operation it does not implement, an argument key it does not accept, a bound or snapshot field that moved |
| `process_boundary.rs` | a new file creating a process without being added to the audited set deliberately |
| `mcp.rs` | an MCP surface that has stopped being a projection of the live CLI — a tool the registry does not back, or an envelope that differs from the same call made directly |
| `domain_smoke.rs` | a command that compiles, helps correctly — and returns the wrong answer on real data |

`domain_smoke.rs` is the one that catches what the others cannot. Two bugs got
past every shape-level check while this CLI was being built: a `--limit`
default of 50 against a task bounded at 32, so every call refused; and a
capability filter comparing a `Debug` spelling to a variant name that does not
exist, so ten ready conversions reported as none. Both compiled, both had
correct help and documented refusals. **Add a smoke assertion for every new
command, and assert something specifically true** — "the response parses" is
not a test, "a real PLS workspace offers at least one conversion" is.

`engine_parity.rs` and any command over a process contract need the engine
present. Set `DS_REPORT_BIN` / `DS_SOLAR_BIN` to run them locally; they say so
loudly rather than passing quietly when the engine is absent.

`bridge_parity.rs` needs the `ds-web` checkout. It defaults to the sibling
directory and takes `DS_WEB_DIR` when that is not the layout — a git worktree
of this repository is two levels deeper, and the first run of that suite from
one skipped every check and reported green. A skipped parity suite is worse
than no parity suite, so the skip names the path it looked in and CI fails if
it appears.

## Budgets

Raising a byte budget in `context_budget.rs` is allowed. Raising one silently
is not — change the number in the same commit, with the reason. A test that
only ever gets relaxed is not protecting anything.

## Things that will be rejected

- Domain logic implemented here rather than called.
- A second description of a command anywhere.
- A refusal without a remedy, or an availability check without a code.
- An error code no command documents.
- Silent truncation.
- Root help that names a command.
- A generic argv, shell, SQL or code-execution route to any owner.
- A flag whose engine mapping was never run against the real binary.
- MCP that is anything other than a projection of live `ds capabilities`: a
  hand-authored command schema, a second contract surface, a transport `ds`
  does not own and run itself, or a tool with no registered command behind it.
  `ds mcp serve` is this executable answering JSON-RPC on stdio and `ds mcp
  install` writes a host's own entry for it; `ds` is an ordinary executable and
  stays one. `crates/ds/tests/mcp.rs` is what holds that line.
- Go, anywhere in the shipping or local-development path.
