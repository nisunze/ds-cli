# ds

The Data Solutions command line. One executable is the door into the whole
stack — for a person in a terminal, and for a coding agent that has never seen
it before.

```bash
ds --help
```

That screen is the domain list, and it is generated from the domain
declarations themselves. One row per domain — its name and a one-line summary —
then the calls that go a tier deeper, then the global flags, then the exit-code
legend. Nothing on it names a command.

This file deliberately keeps no copy of it. A pasted domain table is a second
description of the same declarations: it is right on the day it is written and
silently wrong from the next domain onwards, which is exactly the failure mode
the tiering exists to prevent. Run the binary; that answer is always current.

## The idea

Nobody has time to learn another geospatial tool. So the design constraint is
not "expose everything" — it is **reveal the stack progressively, so nobody
pays for the parts they are not using**.

Root help names domains. Domain help names commands. Command help is one
complete contract. Nothing prints the tier below it. A caller interested in
one domain never loads the rest, and adding a domain costs root help exactly
one line.

`ds capabilities` is the bounded machine-readable companion to those help
tiers. It never replaces `--help`; both remain first-class views generated from
the same command declarations.

That is enforced, in bytes, by `crates/ds/tests/context_budget.rs`. An agent
that has never seen `ds` reaches a specific command's full contract in **three
calls, and pays for one domain rather than all of them**: the domain index,
one domain's command list, one command's descriptor. Measured for
`dsgrid.inspect` at this commit, that is 1 721 + 813 + 2 984 = **5 518 bytes**
of JSON, or 2 043 + 492 + 2 737 = **5 272 bytes** of human help.

Only the first of the three grows with the rest of the stack, and it grows by
one summary line per domain. On a `ds` with twice as many domains the second
and third calls cost exactly what they cost here — which is the property that
matters, and the one the budget test asserts.

See [`docs/contracts/discovery-contract.md`](docs/contracts/discovery-contract.md).

## Build and run

`ds` links the authoritative engine crates from the sibling `ds-network`
workspace by path, so both repositories must be checked out side by side.
`ds-web` is not built, but `tests/bridge_parity.rs` reads its source to prove
`ds map` sends operations the paired application actually implements:

```
data-solutions/
  ds-cli/       ← here
  ds-network/   linked by path; required to build
  ds-web/       read by the bridge parity suite; DS_WEB_DIR overrides
```

```bash
cargo build --locked --release
./target/release/ds --help
```

Verification, all three of which must pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## For a coding agent

Start here. Three calls and you have a complete contract:

```bash
ds capabilities --output json                    # which domains exist
ds capabilities dsgrid --output json            # which commands, and can they run
ds capabilities dsgrid.inspect --output json    # inputs, effects, refusals, examples
```

Then invoke, and branch on the envelope:

```bash
ds dsgrid inspect --model ./model.dsgrid --output json
```

```json
{"v":1,"command":"dsgrid.inspect","contract":1,"status":"ok","data":{ }}
```

Rules you can rely on:

- **stdout is the answer**, stderr is everything else. In `--output json`,
  stdout is parseable in every outcome including failure.
- **exit code names a class**: `0` ok · `2` invalid input · `3` unavailable ·
  `4` unauthorized · `5` conflict · `6` failed · `1` a bug in `ds`.
  `error.class` always agrees with it.
- **`error.code` is stable** and is the field to branch on. Every code a
  command can emit is listed in its help under `REFUSALS`, with a remedy — so
  failure can be planned for, not discovered.
- **results are bounded**. A command returns its cheapest useful projection and
  names the rest in `more`. Truncation is always reported.
- **no command guesses your intent.** Anything that writes a durable artifact
  of record, changes software or integration settings on this machine, or
  mutates shared project state requires `--yes`. Those are the three effect
  classes `needs_confirmation` covers, and the descriptor reports the answer
  as `confirmation_required`.

Full rules: [`docs/contracts/cli-output-contract.md`](docs/contracts/cli-output-contract.md).

## Native agent skills

The canonical Codex, Claude Code and GitHub Copilot skills live under
`skills/`. They contain workflow guidance only: both people and agents operate
the stack through the same `ds` executable, and every command contract is
discovered from the installed binary at run time.

DS GridDesign ships a receipt-bound copy of this skill tree beside `ds`. Its
`pt` shortcut copies a short setup prompt for any chatbot on that machine. The
chatbot runs `ds doctor --output json`, follows the exact installer path under
`.data.skills.installers`, and verifies its native skill directory is current.
That key exists only when a receipt-bound bundle is found beside `ds`, which is
the installed case this describes; on a source checkout `.data.skills.status`
is `missing` with its own remedy, and there is no `installers` key to follow.
Nothing is injected into a conversation. For MCP-only hosts, `ds mcp install`
writes an entry for the same executable. Its compact default publishes stable
chapter routers; optional role profiles publish bounded typed command views.
Both are generated from live `ds capabilities`, never a separate surface.

For a source checkout, the same ownership-safe installers are:

```bash
scripts/install-skills.sh install
scripts\install-skills.ps1 install
```

They install owned copies into `${CODEX_HOME:-~/.codex}/skills`,
`~/.claude/skills`, and `~/.copilot/skills`, refuse same-name skills they do
not own, and require the literal `uninstall` action for removal. Verified
product gaps go through `ds feedback submit` to the same shared, deduplicated
backlog as the app's `fb` shortcut, and a session that fixes one closes it
there with `ds feedback list` and `ds feedback close`. There is no Markdown gap
ledger.

## Reaching `ds` from your shells

The Linux package installs `ds` into `/usr/bin`, which every shell already
searches. The Windows installer puts it beside the app, in a directory no
shell searches — so the installer's post-install hook runs `ds shell register`,
which appends that directory to the user's own `HKCU\Environment\Path` and
broadcasts the change. PowerShell, cmd and Git Bash windows opened afterwards
resolve `ds`; windows already open keep the PATH they started with.

```bash
ds shell status        # this shell, and a new one: where does `ds` resolve from?
ds shell register      # after a source build or a copied binary; idempotent
ds doctor              # folds the two answers into one word: reachable · registered · unreachable
```

The desktop's `cl` shortcut opens a terminal the other way round: this
install's directory *leads* that session's PATH and `DS_DESKTOP_DESCRIPTOR`
pins it to the window that opened it, so it always runs the build that opened
it and never has to guess between Stable and Canary. See
[`docs/reference/shell.md`](docs/reference/shell.md).

## Authority

The default interactive architecture is **paired desktop reuse**, not a second
login. When DS GridDesign is running, `ds` discovers its private bridge
descriptor, authenticates to a random-loopback bridge with a short-lived
pairing secret, and uses the session the application already holds.

The bridge never returns a JWT or refresh token, and accepts only a closed set
of named semantic operations. Possession of the descriptor proves a transport,
never a person — it can never authorize a project write on its own.

```bash
ds desktop status          # paired? signed in? which project?
```

"Not paired" is an answer, not a failure. See
[`docs/reference/desktop.status.md`](docs/reference/desktop.status.md).

That same borrowed session is what `ds map` acts through. The map is a
MapLibre instance inside the running application, so there is no file to open
instead — drawing a local layer, running a vector tool over one, or staging a
property change on a transformer's design layers is one named bridge
operation each, performed by the application under the identity it holds.

```bash
ds map view                                                   # what is on the map
ds map draw --name AOI --geometry Polygon --features aoi.geojson --zoom
ds map points-along --layer sketch:abc --interval-m 25
```

Local layers need only that the application is running. Design-layer commands
need a signed-in project session, they stage locally, and the one command that
pushes to the project — `ds map design save` — requires `--yes`. See
[`docs/reference/map.md`](docs/reference/map.md).

## Architecture

`ls crates/` is the inventory, and the naming is the map: `ds` is the binary
that registers domains, dispatches and renders, `ds-cli-<domain>` owns one
domain and nothing else, and each crate's own header says what it may reach.
A list here would be a second inventory that nothing checks, so there is not
one — but three crates carry a boundary the name does not give away:

- **`ds-cli-contract`** is the CLI's own contract: command metadata, tiered
  help, the output envelope, error classes, exit codes and argument parsing.
  It links no domain crate, so the contract is testable without building an
  engine.
- **`ds-cli-exec`** is the typed process boundary — the audited set of files
  from which a sibling DS executable can be reached at all.
- **`ds-cli-desktop`** is the paired-desktop authority surface every domain
  needing a signed-in principal borrows. It is separate so that nothing in it
  is reachable from a domain that did not ask for it.

Two boundaries hold this together.

**The CLI coordinates; domain owners compute.** No engineering, geometry,
electrical, solar, reporting or conversion logic lives here. A second
implementation would be a second answer to a question that must have one.

`ds` reaches an owner one of two ways, and the choice is not stylistic:

- **Link the crate** where it is a pure library with a clean boundary.
  `ds dsgrid inspect` calls `ds_grid_exchange` and `ds_grid_model` — the same
  functions the desktop application links.
- **Call the typed process contract** where the owning workspace deliberately
  chose process separation and wrote down why. `ds-report` and `ds-solar` are
  both in this category; `ds-report`'s own header states the rule — one named
  subcommand per call, a typed request file, a machine-readable result
  document. No caller-supplied argv reaches an owner — every spawn site passes
  a statically known argument list — so a subcommand no `ds` command names
  stays unreachable.

**Every fact about a command lives once.** Help, the JSON descriptor, argument
validation and dispatch all read the same `Command` value. A command that
gains a flag without declaring it cannot receive it, and help cannot drift
from behaviour — `contract.rs` proves it by comparison.

Domains are separate crates so "domain discovery must not initialize or probe
unrelated engines" is a structural property rather than a promise.

## Status

`ds --help` names the domains this build registers, and `ds doctor` reports how
many of their commands can run on this machine and why not for the rest. Both
answers are generated; neither is copied here.

`dsgrid`, `dsgrid-exchange` and `pls` **link** the authoritative
`ds-network` crates, so they work on a machine with no sidecar installed and
an empty `PATH` — asserted by `domain_smoke.rs`. `solar` and `report` **call** the typed process contracts
their workspaces published, and report `unavailable` with a remedy when those
binaries are absent. `desktop` reaches the paired application session.

`ds` is a **core bundled component of the desktop application**. The Linux
`.deb` installs it into `/usr/bin` alongside `ds-report`, which is how
`ds report` finds its engine with no configuration. The Go `ds-mcp` and
`ds-grid-mcp` sidecars are gone from the package, from the build, from the
component manifest, and from the dev launcher.

That completed the runtime cutover. What it changed is recorded, as closed
history, in
[`docs/migration/cutover-history.md`](docs/migration/cutover-history.md);
historical `ds-mcp` source is not a runtime, a fallback, or a source of live
contracts. Remaining capability gaps are recorded in the reference document of
the domain that owns them, and a gap found while working goes to
`ds feedback submit` rather than into a Markdown ledger.

## Documentation

| Document | What it settles |
|---|---|
| [`docs/contracts/discovery-contract.md`](docs/contracts/discovery-contract.md) | tiers, byte budgets, availability |
| [`docs/contracts/cli-output-contract.md`](docs/contracts/cli-output-contract.md) | envelope, exit codes, effects, authority |
| [`docs/contracts/process-boundary-contract.md`](docs/contracts/process-boundary-contract.md) | when to link, when to call, and the rules for calling |
| [`docs/reference/sre.md`](docs/reference/sre.md) | global Reliability authority, bounds, projections, and refusals |
| [`docs/recommendations/cli-ergonomics-2026-08-24.md`](docs/recommendations/cli-ergonomics-2026-08-24.md) | prioritized discovery and convenience improvements that preserve the active contracts |
| [`docs/reference/dsgrid.md`](docs/reference/dsgrid.md) | validate's two questions, and the engine catalog |
| [`docs/reference/dsgrid-exchange.md`](docs/reference/dsgrid-exchange.md) | why the sequence is the contract, the write rules, contract-1 gaps |
| [`docs/reference/dsgrid.inspect.md`](docs/reference/dsgrid.inspect.md) | the `.dsgrid` read, and its cost model |
| [`docs/reference/pls.md`](docs/reference/pls.md) | digest pinning, task bounds, why a task's code stays in `detail` |
| [`docs/reference/report.md`](docs/reference/report.md) | why it calls a binary; the must-not-exist and blockers rules |
| [`docs/reference/solar.md`](docs/reference/solar.md) | the two-phase split, and why the token is never a flag |
| [`docs/reference/map.md`](docs/reference/map.md) | local map layers, vector tools, and staged design-layer edits |
| [`docs/reference/desktop.status.md`](docs/reference/desktop.status.md) | pairing, discovery, what is never printed |
| [`docs/reference/shell.md`](docs/reference/shell.md) | this shell versus a new one, what `register` writes, who runs it |
| [`docs/reference/mcp.md`](docs/reference/mcp.md) | serving `ds` to an MCP host: chapters, profiles, and what they may not change |
| [`docs/migration/cutover-history.md`](docs/migration/cutover-history.md) | closed history: what the `ds-mcp` → `ds` cutover changed |
