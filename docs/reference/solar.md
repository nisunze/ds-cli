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
ds solar results read --run-id <id> --city <context> --section <name>
ds solar report export --run-id <id> --city <context> --variant draft --out <file>
ds solar final import --run-id <id> --city <context> --file <final.md> --yes
ds solar sync status --run-id <id>
ds solar portfolio list
ds solar run start --portfolio <id> ...
ds solar portfolio export --run-id <id> --artifact result --out <file>
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

`ds solar run start` calls exactly `solar.run.start` with either repeated city
contexts or one portfolio id (never both) and only the optional fields the
caller supplied:

```json
{
  "contexts": ["<context>"],
  "render_charts": false,
  "concurrency": 2,
  "serial": true
}
```

Portfolio launch resolves the same refreshed/cache-retained membership used by
the Pipeline page:

```json
{ "portfolio": "portfolio-id", "concurrency": 4 }
```

It returns a run receipt rather than waiting for calculation. The remaining
commands call the paired lifecycle operations `solar.run.progress`,
`solar.run.result`, `solar.run.cancel`, and `solar.result.read`. Result reads
use `--city` as the user-facing spelling for the operation's `context` field;
repeated `--path` values are semantic result fields, never filesystem paths.
`solar results read` is the full-dashboard counterpart: it reads one named
section from canonical `report_input.json` through the same verified
ProjectResultReceipt store used by Site, Plant, BOQ, Finance and unified views.

`solar final import` accepts the explicit operator/LLM-produced Markdown path,
but the native shell performs the bounded UTF-8 read, commits it into the
selected run/city final slot, optionally renders DOCX with Pandoc, and queues
the final report variant. DS GridDesign does not call a model. `solar sync
status` exposes the Sync Center's durable publication states without starting
or retrying work. `solar portfolio list` returns exact membership and refreshes
the shared offline cache when connected.

`solar run result` includes the committed city document inventory and the
root-level portfolio inventory. `solar report export` pages either the clean
APD or the frozen parity draft through the named `solar.document.read` bridge
operation. `solar portfolio export` pages the aggregate result JSON or draft
Markdown through `solar.portfolio.read`. Both commands create `--out` with
`create_new`: they never overwrite an existing file, and neither command ever
receives the native workspace path.

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

The closed `batch.json` inventory includes every committed city result, APD,
draft, optional Word document, chart, and the root-level `portfolio-result.json`
and `portfolio-draft-fr.md` when the engine produced them. The portfolio is a
first-class batch deliverable, not a client-side reconstruction.

This route is intentionally distinct from the paired product lifecycle:

```bash
# Product/local desktop lifecycle
ds solar prepare --city rw-kigali --output json
ds solar run start --city rw-kigali --output json

# Headless, caller-supplied prepared artifacts
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
| `solar results read` | `solar.results.read` | read only | bounded canonical dashboard-section projection |
| `solar sync status` | `solar.sync.status` | read only | durable publication rows and state counts |
| `solar portfolio list` | `solar.portfolio.list` | read only | governed ids and city membership |
| `solar final import` | `solar.final.import` | artifact write | committed interpreted final and queued publication |
| `solar report export` | `solar.document.read` | local file write | one new APD or parity-draft Markdown file |
| `solar portfolio export` | `solar.portfolio.read` | local file write | one new aggregate result JSON or draft Markdown file |

The operation name is fixed in source for each command. It is never an
argument, so possession of the pairing descriptor authorizes only this narrow
set of product actions and never a generic desktop RPC.

## Bounds

| Command | Timeout | Why |
|---|---:|---|
| `solar prepare` | 30 min | cache capture or authenticated refresh across selected cities |
| `solar run start` | 30 s | creates a local run receipt; compute continues as a job |
| lifecycle reads / cancel | 30 s | bounded local bridge replies |
| results/sync/portfolio reads | 30 s | bounded local receipt/cache projections |
| final import | 10 min | bounded source validation plus optional local Pandoc render |
| headless `solar run` | 4 h | offline compute over a caller-supplied prepared batch |
| `solar engine`, `solar verify-weather` | 20 s | external engine discovery or verification |

`--concurrency` on `solar run start` is constrained to 1 through 32. This is a
bounded native worker pool, not one WASM instance per city. `--serial`
asks the desktop to calculate strictly serially. `--no-charts` maps to
`render_charts: false`; charts are otherwise left to the desktop's product
default.

## Engine identity

`ds solar engine`, the headless artifact runner and `solar verify-weather`
resolve the `ds-solar` sibling packaged with `ds`. `DS_SOLAR_BIN` is an
explicit development override and wins when set. The paired product lifecycle
uses the same release-pinned Solar source linked into DS GridDesign rather than
starting the sidecar.

`ds-solar build-info` publishes the immutable `ds.engine-build/v1` identity:
engine name and version, exact source SHA, Cargo.lock digest, features, target,
profile, supported schemas and a canonical manifest digest. `ds solar engine`
returns that document with the resolved executable path, so headless results
can be tied to the same exact Solar source revision as the desktop release.

## Related

- `crates/ds-cli-solar/src/prepare.rs` — paired preparation adapter
- `crates/ds-cli-solar/src/paired_run.rs` — paired run lifecycle adapter
- `crates/ds-cli-solar/src/exports.rs` — paired APD/draft and portfolio exporter
- `crates/ds-cli-solar/src/workflow.rs` — canonical result, sync, portfolio and final-import operations
- `crates/ds-cli-solar/src/run.rs` — headless artifact adapter
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)
