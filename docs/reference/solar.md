# `ds solar` — reference

Tier-4 reference. `ds solar <command> --help` is the executable contract.

## Product route: paired, cache-first, local compute

The product route is a closed lifecycle through the paired DS GridDesign
application:

```text
ds solar prepare --city <context> ...
ds solar run start --city <context> ...
ds solar run progress --run-id <id>
ds solar run result --run-id <id>
ds solar result read --run-id <id> --city <context>
ds solar run cancel --run-id <id>
```

`ds solar prepare` calls exactly `solar.prepare` with:

```json
{ "contexts": ["<context>"], "overwrite": true, "language": "fr" }
```

Only `contexts` is required; `overwrite` and `language` are omitted unless
requested. The paired application owns the selected project, captures the
complete cacheable city input first, and then owns its fresh/cache-hit decision.
That input includes the weather/PV reference material needed by the native Rust
Solar pipeline. A cache miss may cause the application to acquire source data;
the calculation and report generation remain local once preparation completes.

`ds solar run start` calls exactly `solar.run.start` with selected contexts and
only the optional fields the caller supplied:

```json
{
  "contexts": ["<context>"],
  "render_charts": false,
  "concurrency": 2,
  "serial": true
}
```

It returns a run receipt rather than waiting for calculation. The remaining
commands call the paired lifecycle operations `solar.run.progress`,
`solar.run.result`, `solar.run.cancel`, and `solar.result.read`. Result reads
use `--city` as the user-facing spelling for the operation's `context` field;
repeated `--path` values are semantic result fields, never filesystem paths.

This is deliberately not an IndexedDB protocol. The CLI never lists, reads or
scrapes browser storage, never accepts a cache directory or project root, and
never receives a JWT, source URL or raw cached input. The desktop validates
selection and freshness, performs any authenticated source access under its
existing identity, and returns a bounded receipt over the loopback pairing
bridge.

All product lifecycle commands require a paired, signed-in DS GridDesign
session (`authority: desktop_user`). If no session is running they refuse with
`desktop_not_paired`; if the desktop does not yet implement an operation they
refuse with `desktop_operation_unsupported`. Update both sides together rather
than falling back to an untyped request or a storage scrape.

## Headless artifact route

`ds solar run --prepared <dir> --out <dir>` remains a separate, reproducible
offline adapter over the external `ds-solar` process contract. It accepts an
already prepared artifact directory, performs no intake and no network call,
and writes its result artifacts under `--out`.

This route is intentionally distinct from the paired product lifecycle:

```bash
# Product/local desktop lifecycle
ds solar prepare --city rw-kigali --output json
ds solar run start --city rw-kigali --output json

# Headless, caller-supplied prepared artifacts
DS_SOLAR_BIN=/path/to/ds-solar \
  ds solar run --prepared ./prepared --out ./out --output json
```

The product `prepare` command does not write a caller-addressable `--prepared`
directory. That would cross the desktop cache boundary and invite two cache
protocols. Use the paired lifecycle for product city contexts; use the
`ds-solar` artifact contract when an offline batch already has prepared bytes.

## Authority and typed operations

| CLI command | paired operation | effect | result |
|---|---|---|---|
| `solar prepare` | `solar.prepare` | local file write | completed preparation receipt |
| `solar run start` | `solar.run.start` | local file write | durable run id / launch receipt |
| `solar run progress` | `solar.run.progress` | read only | bounded progress receipt |
| `solar run result` | `solar.run.result` | read only | bounded public result receipt |
| `solar run cancel` | `solar.run.cancel` | local UI | cancellation receipt |
| `solar result read` | `solar.result.read` | read only | bounded city result projection |

The operation name is fixed in source for each command. It is never an
argument, so possession of the pairing descriptor authorizes only this narrow
set of product actions and never a generic desktop RPC.

## Bounds

| Command | Timeout | Why |
|---|---:|---|
| `solar prepare` | 30 min | cache capture or authenticated refresh across selected cities |
| `solar run start` | 30 s | creates a local run receipt; compute continues as a job |
| lifecycle reads / cancel | 30 s | bounded local bridge replies |
| headless `solar run` | 4 h | offline compute over a caller-supplied prepared batch |
| `solar engine`, `solar verify-weather` | 20 s | external engine discovery or verification |

`--concurrency` on `solar run start` is constrained to 1 through 32. `--serial`
asks the desktop to calculate strictly serially. `--no-charts` maps to
`render_charts: false`; charts are otherwise left to the desktop's product
default.

## Engine identity

`ds solar engine`, the headless artifact runner and `solar verify-weather`
resolve `ds-solar` through `DS_SOLAR_BIN` or the executable path. The paired
product lifecycle does not require that external binary: it uses the native
Solar runtime linked into DS GridDesign. `ds-solar` publishes no source-SHA
identity, so a headless artifact result cannot claim an exact commit solely
from this CLI.

## Related

- `crates/ds-cli-solar/src/prepare.rs` — paired preparation adapter
- `crates/ds-cli-solar/src/paired_run.rs` — paired run lifecycle adapter
- `crates/ds-cli-solar/src/run.rs` — headless artifact adapter
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)
