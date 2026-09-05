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
ds solar report bundle --run-id <id> --city <context> \
  --variant apd|draft|network|plant|financial --out <prompt-bundle.zip>
ds solar final import --run-id <id> --city <context> --file <final.md> --yes
ds solar final submit --run-id <id> --city <context> --yes
ds solar sync status --run-id <id>
ds solar portfolio list
ds solar portfolio create --name "Northern portfolio" --city city_a --city city_b --yes
ds solar portfolio update --portfolio <id> --membership-revision sha256:<digest> --city city_a --city city_c --yes
ds solar portfolio delete --portfolio <id> --membership-revision sha256:<digest> --yes
ds solar run start --portfolio <id> --membership-revision <sha256:digest> \
  --graph-strategy first|round-robin|city:<context>
ds solar portfolio read --run-id <id> --path <field> ...
ds solar portfolio analysis --portfolio <id>
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
used by the Pipeline page, but it is not just an id alias. The caller freezes
the ordered membership and chooses the legacy-compatible representative graph
strategy:

```json
{
  "portfolio": "portfolio-id",
  "membership_revision": "sha256:<digest returned by portfolio list>",
  "graph_strategy": "city:rw-kigali",
  "concurrency": 4
}
```

`--membership-revision` is the exact lowercase SHA-256 revision returned with
the selected portfolio row. The desktop recomputes it from the current ordered
membership and refuses if the portfolio changed after listing.
`--graph-strategy` is required and accepts `first`, `round-robin`, or
`city:<exact-member-id>`. The CLI sends those as `first`, `round_robin`, or the
exact `city:<id>` binding; the desktop refuses a named city outside the frozen
membership. The membership revision and graph strategy are portfolio-only and
are rejected with `--city`.

Currency, project lifetime and discount rate are governed prepared-city facts.
The native portfolio calculation derives and validates them across every
sealed member instead of accepting operator overrides at launch. Language and
report intents are document-generation choices and therefore do not belong to
this calculation command. Until a separate digest-bound portfolio reporting
operation is published, the paired application may use bounded internal
document defaults for compatibility; the CLI neither exposes nor claims those
defaults as calculation inputs.

It returns a run receipt rather than waiting for calculation. The remaining
commands call the paired lifecycle operations `solar.run.progress`,
`solar.run.result`, `solar.run.cancel`, and `solar.result.read`. Result reads
use `--city` as the user-facing spelling for the operation's `context` field;
repeated `--path` values are semantic result fields, never filesystem paths.
`solar results read` is the full-dashboard counterpart: it reads one named
section from canonical `report_input.json` through the same verified
ProjectResultReceipt store used by Site, Plant, BOQ, Finance and unified views.

`solar final import` accepts the explicit externally interpreted Markdown path.
The native shell performs the bounded UTF-8 read, lints it against that run's
authoring draft, and commits those exact bytes into the selected run/city final
slot. It calls no model and performs no
PDF/DOCX conversion. Import is
local review state and does not publish. `solar final submit` is the separate,
explicit operation that queues that exact imported final. DS GridDesign does
not call a model. `solar sync status` exposes the Sync Center's durable
publication states without starting or retrying work. `solar portfolio list`
returns exact ordered membership plus its revision and refreshes the shared
offline cache when connected.

`solar run result` includes the committed city document inventory and the
root-level portfolio inventory. A portfolio's governed publication is a handoff
the application performs after its local commit, so a run can seal and commit
its aggregate and still fail to queue that intent. The run stays `succeeded`
and the receipt carries `publication` with the state, the application's reason
and the remedy. An intent that never reached the outbox has no Sync Center row,
so `solar sync status` cannot report it and this receipt is where it is read.
No `publication` means the application stated nothing about one, which is what
every receipt written before it recorded the fact looks like.

`solar report export` pages one exact `apd`/`draft`, `network`, `plant`, or
`financial` Markdown document through the named `solar.document.read` bridge
operation. `solar report bundle` is the portable authoring path: it pages a ZIP
through `solar.report.bundle.read` containing the exact canonical Markdown, a
presentation-only Markdown copy with local links, every referenced verified
figure, `media-manifest.json`, and a README. The export refuses rather than produce a bundle with a
missing image. Each narration block is self-contained: its own `CONTEXT_DATA`
repeats the facts, table rows and trend series it may summarize. Images are
never evidence and their signed URLs never enter the canonical document. Edit
the canonical Markdown only, then import that externally reviewed Markdown
with `solar final import`.

The bundled `ds-solar-final-authoring` skill owns the optional finishing step.
After import lint passes, it may project bundle-local image links into a
separate rendering copy and invoke installed Pandoc, LibreOffice, Microsoft
Office, or a bounded bridge script. Those tools do not become Solar compute
authority: the exact reviewed Markdown remains canonical, and converted bytes
are uploaded only through an explicitly discovered `ds` report-attachment
surface.

`solar portfolio read` pages and
verifies the sealed aggregate result JSON, then returns one bounded semantic
projection. Repeated `--path` values descend through at most eight object keys;
large arrays and strings are edge-sampled and reported with `complete: false`.
Every projection retains the v2 or v3 schema, bounded engine identity,
portfolio name/id, membership revision, ordered city ids,
run/input/result/content digests, native name, batch id/digest, currency,
horizon, and rate. A v2 result retains its one representative city. A v3
result additionally exposes bounded graph-strategy provenance: fixed strategies
retain that city, while round-robin truthfully returns a null representative
and the sealed city id for each available or unavailable graph. It never
reconstructs the portfolio from city results.

`solar portfolio analysis` answers a different question and is addressed by
portfolio id alone: does this governed portfolio have a saved analysis at all?
`solar portfolio list` reports membership only and `solar portfolio read`
requires a completed run id, so neither could say. It returns the SAME typed
projection the application's own Pipeline panel renders — portfolio identity,
membership revision, ordered members, a `saved_analysis` state of `ready`,
`failed` or `none`, the analysis identity when one exists, and the governed
refusal verbatim when the read failed. `none` is a state, not an error. It
calculates nothing, selects no run, and does not read a sealed artifact.
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

`ds solar input capture --city <canonical-id> --out <fresh-file> [--lane
stable|canary]` is the authenticated headless intake route. It restores the
native user and the audience-fenced selected project, derives
`eds_project/<project>/eds_solar` internally, and makes exactly the fixed
`desktop_snapshot` request for that city. There is no project, root, endpoint,
token, receipt or generic request override.

The bounded response is streamed directly to the pinned `ds-solar intake
--snapshot - --out <fresh-file>` owner contract. Short-lived media download
URLs therefore exist only in protected memory and the stdin pipe: they are
never written to an intermediate snapshot file, argv, stdout or the resulting
governed intake. Before the authenticated read, `ds` requires the installed
owner's machine-readable build identity to advertise
`ds-solar.governed-city-intake/v1`; a mismatched package refuses safely rather
than attempting an unversioned handoff.

The owner creates the output with create-new semantics. `ds` then opens that
exact regular file without following symlinks, performs a bounded read, and
verifies its closed v1 envelope against the selected project, city, derived
root, snapshot digest, input fingerprint, receipt authority, expiry and source
counts before reporting success. The command returns only the intake digest,
bounded provenance and a SHA-256 of the receipt id. The raw actor-bound receipt
id remains solely inside the deliberate authority-bearing intake.

`ds solar input prepare --intake <file> --cache <dir> --out <fresh-dir>`
continues that handoff without a Desktop session. It calls only the fixed
native governed-intake preparation operation, against an existing verified
`ds-solar` reference cache. There is deliberately no provider URL, weather
token, API key, project/root override, fixture mode, overwrite, or generic
engine argument. A missing exact weather/reference bundle is therefore a
refusal, not an implicit network request.

The command accepts one private, bounded, non-symlinked governed intake and a
real local cache directory. It creates `--out` itself with private permissions
on Unix, then requires the owner to emit exactly one matching
`<city>.prepared.json` and
`<city>.prepared-publication-claim.json` pair. Its receipt carries only paths,
sizes, SHA-256 digests and immutable engine identity; the actor-bound claim and
cache contents remain on disk. The prepared directory can then be passed
unchanged to `ds solar run --prepared`.

This closes cache-hit preparation, not fresh-server reference acquisition.
Until DS exposes a governed reference-cache acquisition contract, populate the
exact cache through the reviewed paired product route. Do not compensate with
raw `ds-solar` provider URLs or credentials. That remaining gap is explicit so
a Linux host is never described as end-to-end headless when it lacks reference
data.

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

# Headless selected-project capture, cache-hit preparation and artifact run
ds solar input capture --city rw-kigali --out ./rw-kigali.intake.json --output json
ds solar input prepare --intake ./rw-kigali.intake.json \
  --cache ./solar-reference-cache --out ./prepared --output json
ds solar run --prepared ./prepared --out ./out --output json
```

The product `prepare` command does not write a caller-addressable `--prepared`
directory. That would cross the desktop cache boundary and invite two cache
protocols. Use the paired lifecycle for product city contexts; use the
`ds-solar` artifact contract when an offline batch already has prepared bytes.

## Governed project seeding

Seeding is what makes a project's Solar cities exist before any preparation or
run. It COPIES authored city inputs from a governed seed source root into the
project's Solar root, and it is the same governed copy `create_template`
already performs — not a second seed model and not a duplicate city catalog.
`ds solar prepare` is a different thing entirely: it caches and seals inputs
for cities the project already has.

```text
ds solar seed preview [--source <root>] [--city <id> ...]
ds solar seed apply --seed-digest <64-hex> [--source <root>] [--city <id> ...] --yes
```

ds-brain owns every decision. Its
`docs/contracts/solar-project-seeding.md` is the authority, and it names
exactly two actions on the existing `POST /api/v1/solar` door: `seed_preview`
(read only) and `seed_apply` (digest bound). There is one parity boundary —
the ds-web seeding card and this CLI — and `ds mcp serve` is not a third
consumer, because it transports these same registered commands.

**Propose, then confirm.** `preview` writes nothing; its plan carries the
server's own `mutated: false`. `apply` takes `--seed-digest` from that plan and
echoes it. `ds` never derives, recomputes or repairs that digest: it exists to
prove the set being written is the set someone looked at. If either end moved,
ds-brain refuses with `solar_seed_digest_mismatch` and the remedy is to preview
again and re-confirm — never to retry with a fresh digest the operator never
saw.

**The plan is returned verbatim.** `changed`, `missing` and `warnings` are the
rows a human would have acted on, so nothing summarizes them away. `changed`
in particular is how seeding protects authored project data: it never
overwrites, so a diverged destination is reported and left alone.
`root_digest` vs `destination_root_digest` says which half moved, and no
client re-derives it.

**The city root is a listed document.** Each city's `documents[]` begins with
its root row — `kind: "root"`, an empty `subcollection`, `doc_id` equal to the
city id — because the apply writes it like any other document. `ds` refuses a
plan whose `document_count` does not equal what the creatable cities list, and
an apply receipt that claims every creatable city committed but wrote a
different number. Both are `desktop_contract_mismatch`: a plan that omitted the
document deciding whether a city exists would promise fewer documents than the
apply then wrote.

**What `ds` sends is only the selection.** The destination is the paired
session's selected project, composed by the application exactly as the card
does; there is no project or root argument, because a project id is not proof
of anything. ds-brain decodes the body with `DisallowUnknownFields` and reads
an absent `--source` as its governed catalog and an absent `--city` list as
every live source city, so an unset optional is omitted rather than sent as an
empty value.

Network assets are reported and never seeded: a finalized media reference is
pinned to its own city's storage prefix and receipt, so a copied one would fail
verification at the seeded city's first calculation. The plan carries
`network_assets_are_not_seeded`, and the seeded city needs its network maps
uploaded through the normal upload/finalize path.

ds-brain's own refusal codes survive the trip under the same names in
snake_case, with `detail.server_code` carrying the server's spelling verbatim:
`solar_seed_project_root_required`, `solar_seed_source_invalid`,
`solar_seed_component_disabled`, `solar_seed_digest_required`,
`solar_seed_digest_mismatch` and `solar_seed_bounded`. `ds` also holds
ds-brain's 64-city request bound locally, so an over-large selection is refused
once with that same code rather than after a round trip.

**One door, both surfaces.** DS GridDesign answers `solar.seed.preview` and
`solar.seed.apply` from `src/lib/desktop/cli-solar-seed.ts`, which calls the
same `$lib/api/solar-seed` client the Project Control seeding card uses. So the
CLI and the card send one governed request through one declared operation and
read one refusal vocabulary; `crates/ds/tests/bridge_parity.rs` holds that, and
`ds mcp serve` inherits both commands by their being registered rather than by
any MCP-specific implementation. An older DS GridDesign build that predates the
door still refuses with `desktop_operation_unsupported` and the remedy to
update it.

## Authority and typed operations

| CLI command | paired operation | effect | result |
|---|---|---|---|
| `solar input capture` | fixed native `desktop_snapshot` + governed owner stdin | local file write | verified governed city intake |
| `solar input prepare` | fixed native cache-only `prepare` | local file write | private prepared input and publication-claim pair |
| `solar seed preview` | `solar.seed.preview` | read only | ds-brain's SolarSeedPlan, verbatim |
| `solar seed apply` | `solar.seed.apply` | global write | ds-brain's SolarSeedApplyResult, verbatim |
| `solar prepare` | `solar.prepare` | local file write | completed preparation receipt |
| `solar run start` | `solar.run.start` | local file write | launch receipt with a run id that outlives the session; the compute itself is owned by the paired application and ends with it |
| `solar run progress` | `solar.run.progress` | read only | bounded progress receipt |
| `solar run result` | `solar.run.result` | read only | bounded public result receipt, with any unqueued governed publication |
| `solar run cancel` | `solar.run.cancel` | local UI | cancellation receipt |
| `solar result read` | `solar.result.read` | read only | bounded city result projection |
| `solar result compare` | fixed native `compare` | read only | sealed-result equality and bounded provenance |
| `solar results read` | `solar.results.read` | read only | bounded canonical dashboard-section projection |
| `solar sync status` | `solar.sync.status` | read only | durable publication rows and state counts |
| `solar portfolio list` | `solar.portfolio.list` | read only | governed ids, membership revisions and ordered cities |
| `solar portfolio read` | `solar.portfolio.read` | read only | bounded projection of one sealed aggregate result |
| `solar portfolio analysis` | `solar.portfolio.analysis` | read only | one portfolio's saved-analysis state, identity and verbatim error |
| `solar final import` | `solar.final.import` | artifact write | committed interpreted final in local review state |
| `solar final submit` | `solar.final.submit` | artifact write | explicit publication enqueue for that imported final |
| `solar report export` | `solar.document.read` | local file write | one exact APD/draft/network/plant/financial Markdown file |
| `solar report bundle` | `solar.report.bundle.read` | local file write | canonical prompting Markdown, rendering-only peer, verified media and boundary README in one ZIP |
| `solar portfolio export` | `solar.portfolio.read` | local file write | one exact result/APD/network/plant/financial artifact |

The operation name is fixed in source for each command. It is never an
argument, so possession of the pairing descriptor authorizes only this narrow
set of product actions and never a generic desktop RPC.

## Bounds

| Command | Timeout | Why |
|---|---:|---|
| `solar input capture` | 120 s request + 5 min owner intake | bounded selected-project capture and create-new sealing |
| `solar input prepare` | 30 min | cache-only native preparation of one governed intake |
| `solar seed preview` / `apply` | 60 s | one ds-brain round trip; the card allows the same |
| `solar prepare` | 30 min | cache capture or authenticated refresh across selected cities |
| `solar run start` | 30 s | creates a local run receipt; compute continues inside the paired application for as long as it runs — closing DS GridDesign ends the run and later reads settle it as abandoned |
| lifecycle reads / cancel | 30 s | bounded local bridge replies |
| results/sync/portfolio reads | 30 s | bounded local receipt/cache projections |
| report bundle | 10 min | downloads every referenced verified image, assembles the portable ZIP, then pages it without exposing credentials |
| final import | 10 min | bounded source validation and exact-byte local import |
| final submit | 30 s | exact imported-final lookup and publication enqueue |
| headless `solar run` | 4 h | offline compute over a caller-supplied prepared batch |
| `solar engine`, `solar verify-weather`, `solar result compare` | 20 s | external engine discovery or bounded verification |

`--concurrency` on `solar run start` is constrained to 1 through 32. This is a
bounded native worker pool, not one WASM instance per city. `--serial`
asks the desktop to calculate strictly serially. `--no-charts` maps to
`render_charts: false`; charts are otherwise left to the desktop's product
default. Those three execution controls apply to both city and portfolio
launches; the assumption and report flags apply only to portfolio launches.

## Engine identity

`ds solar engine`, `solar input capture`, `solar input prepare`, the headless artifact runner,
`solar verify-weather` and `solar result compare`
resolve the `ds-solar` sibling packaged with `ds`. `DS_SOLAR_BIN` is an
explicit development override and wins when set. The paired product lifecycle
uses the same release-pinned Solar source linked into DS GridDesign rather than
starting the sidecar.

`ds-solar build-info` publishes the immutable `ds.engine-build/v1` identity:
engine name and version, exact source SHA, Cargo.lock digest, features, target,
profile, supported schemas and a canonical manifest digest. `ds solar engine`
returns that document with the resolved executable path, so headless results
can be tied to the same exact Solar source revision as its desktop or headless
ds release.

`ds solar result compare` validates both complete `ds-solar.result/v1`
documents in that native engine, including recomputing each canonical digest,
then returns the closed `ds-solar.result-comparison/v1` equality receipt with
project/city, input, weather and engine provenance. A valid difference is data (`equal: false`), not a
failure. The operation reads local artifacts only; it neither proves current
project membership nor authorizes publication or mutation.

## Related

- `crates/ds-cli-solar/src/seed.rs` — governed project seeding: preview and digest-bound apply
- `crates/ds-cli-solar/src/input_capture.rs` — selected-project governed intake capture
- `crates/ds-cli-solar/src/input_prepare.rs` — cache-only governed intake preparation
- `crates/ds-cli-solar/src/prepare.rs` — paired preparation adapter
- `crates/ds-cli-solar/src/paired_run.rs` — paired run lifecycle adapter
- `crates/ds-cli-solar/src/exports.rs` — paired city-report and exact portfolio-artifact exporter
- `crates/ds-cli-solar/src/workflow.rs` — canonical result, sync, portfolio and final import/submit operations
- `crates/ds-cli-solar/src/run.rs` — headless artifact adapter
- `crates/ds-cli-solar/src/compare.rs` — headless sealed-result comparison adapter
- `skills/ds-solar-workflow/` — single-city and explicit city-batch lifecycle guidance
- `skills/ds-solar-portfolio/` — exact governed portfolio lifecycle guidance
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)

## Offline project workspace

`ds solar project init --workspace DIR --project ID` creates local project
attribution without signing in. `project seed --input FILE` imports complete
local city inputs or previously captured intakes. `project city write --city ID
--snapshot FILE` creates a city from a complete snapshot; add `--expected DIGEST`
to replace an existing revision. `project city read --city ID --out FILE` exports
a snapshot for editing. Every command takes `--workspace DIR`.

`project run --cache DIR --run-id ID --city CITY --charts --draft apd` prepares,
calculates and produces drafts offline. Repeat `--city` and `--draft`; supported
drafts are apd, network, plant and financial. The cache must already contain the
exact reference data. `project result --run-id ID`, `project status` and
`project outbox` inspect local state without Desktop or authentication.

`project sync --lane stable --background --yes` launches the fixed native
uploader. It binds to the signed-in selected project, principal and audience;
queues survive failure. Omit `--background` to drain once or use `--watch` to
watch for new work. The worker exits after 12 hours or a permanent refusal.
A reviewed city conflict can be rebased with `project sync rebase --sequence N
--expected-cloud FINGERPRINT`; this does not publish until `sync --yes`.

Cloud publication needs the server's `project_commit` route and admitted Solar
release build. It never gates local drafts. Existing paired commands and
governed portfolio membership are separate from this explicit local workspace.
