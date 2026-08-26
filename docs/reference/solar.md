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
ds solar report export --run-id <id> --city <context> \
  --variant apd|draft|network|plant|financial --out <file>
ds solar final import --run-id <id> --city <context> --file <final.md> --yes
ds solar final submit --run-id <id> --city <context> --yes
ds solar sync status --run-id <id>
ds solar portfolio list
ds solar run start --portfolio <id> --membership-revision <sha256:digest> \
  --currency <ISO> --project-years <n> \
  --discount-rate <rate> --representative-city <context> --language fr|en \
  --report apd|network|plant|financial ...
ds solar portfolio read --run-id <id> --path <field> ...
ds solar portfolio export --run-id <id> \
  --artifact result|apd|network|plant|financial --out <file>
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
contexts or one portfolio id (never both). City launches carry only their
contexts and the shared execution controls:

```json
{
  "contexts": ["<context>"],
  "render_charts": false,
  "concurrency": 2,
  "serial": true
}
```

Portfolio launch resolves the same exact refreshed/cache-retained membership
used by the Pipeline page, but it is not just an id alias. The caller must
freeze every portfolio assumption, choose a representative member and request
at least one report intent:

```json
{
  "portfolio": "portfolio-id",
  "membership_revision": "sha256:<digest returned by portfolio list>",
  "currency": "XAF",
  "project_years": 25,
  "discount_rate": 0.08,
  "representative_city": "rw-kigali",
  "language": "fr",
  "report_intents": ["apd", "financial"],
  "concurrency": 4
}
```

`--membership-revision` is the exact lowercase SHA-256 revision returned with
the selected portfolio row. The desktop recomputes it from the current ordered
membership and refuses if the portfolio changed after listing. `--currency` is
exactly three uppercase ASCII letters, `--project-years` is
from 1 through 100, and `--discount-rate` is finite and lies in `[0, 1)`.
`--language` accepts only `fr` or `en`; repeat `--report` with one or more of
`apd`, `network`, `plant`, and `financial`. These seven portfolio-only inputs are
all required with `--portfolio` and are rejected with `--city`. The CLI sends
their values unchanged except for parsing the bounded numeric fields; it does
not infer XAF, 25 years, a discount rate, a representative city, a language or
a default report set.

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
selected run/city final slot, and optionally renders DOCX with Pandoc. Import is
local review state and does not publish. `solar final submit` is the separate,
explicit operation that queues that exact imported final. DS GridDesign does
not call a model. `solar sync status` exposes the Sync Center's durable
publication states without starting or retrying work. `solar portfolio list`
returns exact ordered membership plus its revision and refreshes the shared
offline cache when connected.

`solar run result` includes the committed city document inventory and the
root-level portfolio inventory. `solar report export` pages one exact
`apd`/`draft`, `network`, `plant`, or `financial` Markdown document through the
named `solar.document.read` bridge operation. `solar portfolio read` pages and
verifies the sealed aggregate result JSON, then returns one bounded semantic
projection. Repeated `--path` values descend through at most eight object keys;
large arrays and strings are edge-sampled and reported with `complete: false`.
Every projection retains the v2 schema, bounded engine identity, portfolio
name/id, membership revision, ordered city ids, run/input/result/content
digests, native name, batch id/digest, currency, horizon, rate, and
representative city. It never reconstructs the portfolio from city results.
`solar portfolio export` pages exactly one of the native `result`, `apd`,
`network`, `plant`, or `financial` artifacts through the same
`solar.portfolio.read` operation. The selected name is sent unchanged: `result`
is the sealed aggregate JSON and the other four names are the explicitly
requested Markdown report intents. Every page repeats the same native name,
content digest and batch identity; the CLI verifies those values and the
assembled SHA-256 before creating `--out`. No arbitrary artifact name or legacy
draft selector is accepted. Both export commands use `create_new`: they never
overwrite an existing file, and neither receives the native workspace path.

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
offline adapter over the external `ds-solar run` process contract. It accepts
an already prepared city-artifact directory, performs no intake or network
call, and writes only that city batch under `--out`.

The resulting `batch.json` inventory can contain committed city results,
reports, optional Word documents and charts. The CLI reads at most 16 MiB,
requires the native v1 schema, run/city identity and declared artifact shape,
recomputes the manifest's batch digest and digest-derived id, and returns at
most 500 artifact descriptors with an explicit `more.artifacts_omitted` count.
It does not re-read each artifact file to verify its declared content digest.
This command does not invoke the owner binary's separate portfolio subcommand
and does not produce or claim a portfolio result. Use the paired governed
portfolio lifecycle above when a portfolio deliverable is required.

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
| `solar portfolio list` | `solar.portfolio.list` | read only | governed ids, membership revisions and ordered cities |
| `solar portfolio read` | `solar.portfolio.read` | read only | bounded projection of one sealed aggregate result |
| `solar final import` | `solar.final.import` | artifact write | committed interpreted final in local review state |
| `solar final submit` | `solar.final.submit` | artifact write | explicit publication enqueue for that imported final |
| `solar report export` | `solar.document.read` | local file write | one exact APD/draft/network/plant/financial Markdown file |
| `solar portfolio export` | `solar.portfolio.read` | local file write | one exact result/APD/network/plant/financial artifact |

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
| final submit | 30 s | exact imported-final lookup and publication enqueue |
| headless `solar run` | 4 h | offline compute over a caller-supplied prepared batch |
| `solar engine`, `solar verify-weather` | 20 s | external engine discovery or verification |

`--concurrency` on `solar run start` is constrained to 1 through 32. This is a
bounded native worker pool, not one WASM instance per city. `--serial`
asks the desktop to calculate strictly serially. `--no-charts` maps to
`render_charts: false`; charts are otherwise left to the desktop's product
default. Those three execution controls apply to both city and portfolio
launches; the assumption and report flags apply only to portfolio launches.

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
- `crates/ds-cli-solar/src/exports.rs` — paired city-report and exact portfolio-artifact exporter
- `crates/ds-cli-solar/src/workflow.rs` — canonical result, sync, portfolio and final import/submit operations
- `crates/ds-cli-solar/src/run.rs` — headless artifact adapter
- `skills/ds-solar-workflow/` — single-city and explicit city-batch lifecycle guidance
- `skills/ds-solar-portfolio/` — exact governed portfolio lifecycle guidance
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)
