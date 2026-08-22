# ds

The Data Solutions command line. One executable is the door into the whole
stack — for a person in a terminal, and for a coding agent that has never seen
it before.

```
$ ds --help
ds — the Data Solutions command line.

USAGE
  ds <domain> <command> [--flags]

DOMAINS
  dsgrid           Canonical .dsgrid models: identity, inventory, validation.
  dsgrid-exchange  Import, export, compose: classify, plan, convert.
  pls              PLS-CADD workspaces: structures, capacity, references, DONs.
  solar            Solar batches: prepare inputs, run them offline, verify weather.
  report           Deliverables: transformer and combined report artifacts.
  map              The paired map: local layers, vector tools, design-layer edits.
  desktop          The paired DS GridDesign session: pairing, sign-in, project.

DISCOVERY
  ds <domain> --help             commands in one domain
  ds <domain> <cmd> --help       one command's full contract
  ds capabilities [<domain>|<id>]  the same, as JSON
  ds capabilities --search <text>  find a command by words
  ds doctor                      what works here, and why not
…
```

## The idea

Nobody has time to learn another geospatial tool. So the design constraint is
not "expose everything" — it is **reveal the stack progressively, so nobody
pays for the parts they are not using**.

Root help names domains. Domain help names commands. Command help is one
complete contract. Nothing prints the tier below it. A caller interested in
one domain never loads the rest, and adding a domain costs root help exactly
one line.

That is enforced, in bytes, by `crates/ds/tests/context_budget.rs`. An agent
that has never seen `ds` reaches a specific command's full contract in **three
calls totalling ~3.1 KB**.

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
  or mutates shared state requires `--yes`.

Full rules: [`docs/contracts/cli-output-contract.md`](docs/contracts/cli-output-contract.md).

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

```
crates/
  ds-cli-contract   command metadata, tiered help, output envelope,
                    error classes, exit codes, argument parsing.
                    Links no domain crate — the contract is testable
                    without building an engine.
  ds-cli-exec       the typed process boundary: the one place a process
                    can be spawned, and the only way to reach a sibling
                    DS executable.
  ds-cli-desktop    the paired-desktop authority surface.
  ds-cli-map        the map domain: local layers, vector tools and
                    design-layer edits, entirely over the paired bridge.
                    Links no engine — the map is inside the application.
  ds-cli-dsgrid     the canonical-model domain, linking ds-network's
                    crates. Discovery and read-only throughout.
  ds-cli-dsgrid-exchange
                    classify, plan, convert. Split from ds-cli-dsgrid by
                    effect: it holds the only command in either that
                    writes a file.
  ds-cli-pls        the PLS-CADD domain, over ds-grid-tasks' typed tasks.
  ds-cli-solar      the solar domain, over the ds-solar contract.
  ds-cli-report     the reporter domain, over the ds-report contract.
  ds                the binary: registers domains, dispatches, renders.
```

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
  document. There is no generic `run(binary, argv)` anywhere in this
  repository, so a subcommand no `ds` command names stays unreachable.

**Every fact about a command lives once.** Help, the JSON descriptor, argument
validation and dispatch all read the same `Command` value. A command that
gains a flag without declaring it cannot receive it, and help cannot drift
from behaviour — `contract.rs` proves it by comparison.

Domains are separate crates so "domain discovery must not initialize or probe
unrelated engines" is a structural property rather than a promise.

## Status

Seven domains, thirty-nine domain commands plus three root metadata commands.

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

This completes the runtime cutover. The migration matrix remains the evidence
ledger and capability backlog; historical `ds-mcp` source is not a runtime or
fallback.

## Documentation

| Document | What it settles |
|---|---|
| [`docs/contracts/discovery-contract.md`](docs/contracts/discovery-contract.md) | tiers, byte budgets, availability |
| [`docs/contracts/cli-output-contract.md`](docs/contracts/cli-output-contract.md) | envelope, exit codes, effects, authority |
| [`docs/contracts/process-boundary-contract.md`](docs/contracts/process-boundary-contract.md) | when to link, when to call, and the rules for calling |
| [`docs/reference/dsgrid.md`](docs/reference/dsgrid.md) | validate's two questions, and the engine catalog |
| [`docs/reference/dsgrid-exchange.md`](docs/reference/dsgrid-exchange.md) | why the sequence is the contract, the write rules, contract-1 gaps |
| [`docs/reference/dsgrid.inspect.md`](docs/reference/dsgrid.inspect.md) | the `.dsgrid` read, and its cost model |
| [`docs/reference/pls.md`](docs/reference/pls.md) | digest pinning, task bounds, why a task's code stays in `detail` |
| [`docs/reference/report.md`](docs/reference/report.md) | why it calls a binary; the must-not-exist and blockers rules |
| [`docs/reference/solar.md`](docs/reference/solar.md) | the two-phase split, and why the token is never a flag |
| [`docs/reference/map.md`](docs/reference/map.md) | local map layers, vector tools, and staged design-layer edits |
| [`docs/reference/desktop.status.md`](docs/reference/desktop.status.md) | pairing, discovery, what is never printed |
| [`docs/migration/matrix.md`](docs/migration/matrix.md) | what moves, what is deleted, in what order |
