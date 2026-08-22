# `ds solar` — reference

Tier-4 reference. `ds solar <command> --help` is the contract.

## The two-phase split is the domain

`ds-solar` models the same two phases the product does, and the split is
enforced by types rather than convention:

- **`prepare` may reach the network.** It resolves each city's sealed
  reference bundle cache-first and commits prepared inputs. It touches the
  network only on a cache miss, and only when `--reference-url` is given;
  without it the frozen fixture bundles are the provider and preparation is
  fully offline.
- **`run` may not.** It performs no intake and no network call of any kind. It
  receives only prepared inputs, and the runtime it executes through holds no
  cache, no provider, no HTTP client and no credentials.

`ds` keeps these as two commands with two contracts rather than collapsing
them into one `ds solar batch`. Collapsing them would hide exactly the
property the desktop and cloud paths depend on, and would make an offline run
indistinguishable — from the outside — from one that quietly fetched.

`ds solar prepare` states it in the result:

```json
"network_permitted": false
```

That field is `--reference-url` having been given. A caller auditing a prepared
set can tell from the receipt alone whether those bytes could have left the
machine.

## What a reference bundle is, and why a cache hit is a complete skip

`ds-solar-weather` owns the whole location-specific PV model — the weather, the
pinned pvlib module and inverter records, the string design, the optimal tilt,
the loss configuration, and the 8,760-hour simulation of one reference inverter
and its strings. It seals that into four artifacts:

| file | holds |
|---|---|
| `weather.json` | the normalised weather dataset |
| `reference-unit.json` | resolved inputs, configuration, annual totals, digests |
| `reference-unit.parquet` | the 8,760-hour simulation |
| `manifest.json` | exact byte size and SHA-256 for the three above |

A reference unit is a **constant** of its coordinates and its pins: the same
place under the same pvlib, the same catalog rows and the same losses is the
same 8,760 hours forever. So a cache hit downloads nothing at all, and there is
no time-to-live anywhere in the path.

`--overwrite` is the deliberate refresh, and it is the *only* way to pick up a
producer-side pin change. That is not laziness: a pvlib upgrade or a revised
catalog row changes the producer's source key, and nothing about either is
visible from this side of the boundary without asking. Making the refresh a
flag is what keeps that asymmetry honest instead of hidden behind a heuristic.

## Which project, and who is asking

`--project` names the project prepared inputs are committed under. Without it,
`ds` asks the paired DS GridDesign session for its **selected** project — the
same one its window is showing — so a terminal and a window can never disagree
about what is being prepared. Neither available is a refusal (`project_unknown`)
with both remedies, never a default: "default" is the wrong project id to write
into a prepared input.

The result says which happened:

```json
"project": "arjgpydw_aderm_loc7", "project_source": "paired session"
```

## The token is never a flag, and never a value `ds` holds

There is no `--token`, and there is also nothing for one to be assigned to.

When `--reference-url` names an authenticated origin, the download is performed
**by the paired application**, under the identity it already holds, through one
closed bridge operation (`solar.reference.fetch`). The JWT is never fetched,
never passed as an argument and never held by this process. `ds` asks for an
outcome — "these cities' bundles, in this cache directory" — not for a
credential.

That is `ds-cli-desktop`'s crate invariant, and it is the reason the bridge is
safe to build on: possession of the pairing descriptor buys the ability to ask
the application to do a *known* thing, never the ability to borrow its
identity. A credential passed as an argument lands in shell history, in `ps`
output and in an agent's context, and there is no version of that which is
acceptable.

Without a paired application, preparation is offline and a cache miss is a
refusal with a remedy.

## Flag translation

`ds` owns its own vocabulary, and it does not always match the engine's:

| `ds` flag | engine flag | why |
|---|---|---|
| `ds solar verify-weather --dataset` | `ds-solar verify-weather --file` | "file" says nothing about what the file is |
| `ds solar prepare --project` | `ds-solar prepare --project-id` | `ds` resolves it from the paired session; the engine only records it |

Translating is the adapter's job. The risk is that a translation is *wrong* —
the first version of this one emitted `--dataset` to an engine that only
accepts `--file`, which failed at runtime and no test caught.

`solar_engine_flags_are_real` in `crates/ds/tests/engine_parity.rs` now checks
every flag `ds solar` forwards against the installed engine's own help, at the
version actually installed. It skips loudly when the engine is absent; set
`DS_SOLAR_BIN` to run it locally.

## Availability on a stock install, and why

`ds-solar` is **not** a bundled desktop sidecar, and that is a deliberate
position rather than an oversight.

**The solar engine is already packaged.** `ds-solar-engine` and
`ds-solar-contracts` are path dependencies of `ds-web/src-tauri`, and their
symbols are present in the shipped `ds-web-desktop-canary` binary. Solar
compute travels with the application.

**The `solar` component row is a documented leftover.**
`desktop-components.json` still carries a `solar` component, `planned` and
pin-free. `ds-web`'s own Windows packaging validator states what it is for:

> The Python-era compiled Solar artifact is gone. The native Rust Solar
> runtime is ordinary linked source, not an installed component, so a `solar`
> component row may only linger in a dormant, pin-free state until it is
> deleted from the catalog entirely.

So an earlier version of this page's remedy — "install the solar component" —
pointed a caller at something that will never install. It now says what they
can actually do.

**Bundling the CLI would cost 27 MB to duplicate an engine the app already
carries.** `ds-web`'s component rules only permit a bundled sidecar as a
*core* component, whose delivery allowlist is closed and whose contract test
is named "keeps the installed core small". Adding solar there would contradict
both the quoted comment and that test.

### The two real ways to make this available

1. **Extract the shared orchestration.** `ds-solar`'s CLI is "one of three
   adapters over the same runtime". The part `ds` needs — load prepared
   inputs, run a batch, write artifacts — is that adapter's orchestration, not
   the runtime's. Lifting it into a small crate that both `ds-solar` and `ds`
   link is the pattern for exactly this case, and it duplicates nothing.
2. **Keep calling the binary, and ship it.** Cheaper, but it means a core
   component row and a second copy of the engine in every package.

The first is better and is a change in `ds-solar`, not here.

### Today

```bash
DS_SOLAR_BIN=/path/to/ds-solar ds solar run --prepared ./prepared --out ./out
```

Build one with `cargo build --release --package ds-solar-cli`. Without it the
domain reports itself unavailable with that remedy, and `ds doctor` shows it:

```
$ ds doctor
15 command(s) available, 4 blocked

solar.engine
  `ds-solar` was not found
  → set DS_SOLAR_BIN to a built ds-solar (cargo build --release --package ds-solar-cli)
```

## Identity

`ds-solar` publishes **no** source-SHA identity. It has no `build-info`
subcommand; clap's `--version` is the only identity affordance, and it prints
a package version only.

`ds solar engine` says so in its output description rather than inventing a
richer identity the engine does not attest to. A solar result therefore cannot
be bound to an exact commit the way a reporter artifact can — worth knowing
before treating one as a record.

## Bounds

| Command | Timeout | Why |
|---|---|---|
| `engine`, `verify-weather` | 20 s | discovery answers immediately or something is wrong |
| `prepare` | 30 min | weather resolution across many cities |
| `run` | 4 h | pure compute on a large city set |

`--out` is required. A completed run writes each city's deterministic parity
draft and product APD, plus `portfolio-result.json` and
`portfolio-draft-fr.md`. `ds solar run` returns the batch identity and a
bounded artifact inventory; document bodies remain in the output directory.

## Related

- `ds-solar/apps/ds-solar-cli/src/main.rs` — the engine's own contract
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)
