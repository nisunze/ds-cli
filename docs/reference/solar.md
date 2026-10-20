# `ds solar` — reference

Tier-4 reference. `ds solar <command> --help` is the executable contract.

## Native Core application

`solar application schema` lists the native operations. Select an operation
to discover its closed request schema, then execute that request with
`solar application --project <exact-project> --lane <lane> --request <file>`.
This path needs authenticated native DS Core, with no paired Desktop,
browser, Svelte or WASM. Core captures the authorized project and keeps
its workspace authority and jobs separate from other projects and accounts.
Workspace selection uses a current read, an approved-handle set or an explicit
default choice with generation comparison. Automated delivery ends at prompting
drafts; composing a final requires close human supervision.

Add `--out <absolute-new-json-file>` for large application receipts. The output
parent must already be private and have no linked ancestors; Core never replaces
an existing file. The file contains the exact complete
`ds-solar.application-receipt/v1` JSON. Stdout returns only
`ds-solar.application-artifact/v1` with `out`, `byte_count`, `sha256`,
`project_id`, `root` and `lane`. Consumers verify those facts and the file bytes
before adopting its value. The file response limit is 257 MiB: the existing
64 MiB artifact limit expanded to a compact JSON byte array, plus 1 MiB for its
envelope. Without `--out`, the existing 32 MiB response limit and JSON output
remain unchanged; the general process stdout limit is not increased.

Destination admission happens before the Core request. Typed Core refusals pass
through unchanged and create no result. If an action completed but file output
failed, `server_refused` explicitly reports completion, operation, scope, exact
response size/digest and a bounded original completion value where available.
Inspect those details before retrying; output failure does not undo or cancel
completed work. This additive option retains application contract 1.

The current native workspace selection belongs to Core's existing protected
SQLite Store, not browser preferences. Discover `workspace_selection_read`;
it returns null for absence or `{workspace_handle, dir, origin, generation}`
after validating the exact current-host approved receipt. Select an existing
approved handle with `workspace_selection_set`, or explicitly choose the owning
default with `workspace_selection_default`. Both take `expected_generation`:
null creates only when absent; a returned generation is required to replace it.
Set never accepts a frontend directory as authority. Core verifies account,
lane, project, the actual directory and late result identity. Rows are bounded
to 8 KiB and 64 project selections per owner; they contain no city/results data.

An invalid or missing selected receipt refuses and preserves the row; it never
falls back to a default, reapproval or old browser cache. Existing approved
custom directories and all inputs, pending work and results files stay intact.
Matching Core and native Desktop/client consumers are required for these three
closed operations; older unknown-operation refusals remain explicit. Independent
Solar engine identities and calculation behavior are unchanged. The operation
schemas come only from the same owning Rust request declaration.

The native owner supports workspace approval, prepared-input import with
digest-pinned replacement, preparation, readiness, annual generation profiles,
city calculation, progress and cancellation, explicit portfolio calculation,
closed artifact inventories and reads, and reviewed final Markdown import.
Use the project lifecycle for city input editing and publication, and the
portfolio catalog commands for governed membership edits. Discover each exact
contract before constructing a request; the schemas derive from the Rust DTOs.

A launch receipt is not a completed run. Poll `calculate_progress` until it
settles. Completed receipts survive a Server restart. Work interrupted before
settlement is reported explicitly; use a new run ID to retry after inspecting
any committed artifacts. Closed run IDs cannot silently overwrite earlier work.
Publication remains a separate operation and requires its online receipt.

Prompting drafts are the delivery endpoint for automated Solar work. A final
document is deliberately composed under close human supervision; calculation,
draft export and publication never imply approval to compose or submit one.

Discover `portfolio_start` for a background portfolio calculation, then
`portfolio_progress` with its returned job ID. Queued and running receipts are
unfinished; `ready` includes the exact closed result and batch identities.
`portfolio_cancel` requests cooperative cancellation without claiming that a
committed result was removed. Jobs are fenced to the captured project. A
restart preserves terminal receipts and reports unfinished workers as
`interrupted`, retaining workspace, source run and portfolio run identities.
Inspect that run before retrying the same exact intent: existing closed bytes
are verified and preserved. Each context admits 32 active workers and retains
at most 64 private journals; terminal journal expiration is explicit.

Use `prepare_captured` on Server to select city IDs without constructing snapshots.
The captured native authority obtains their governed inputs and references inside
the worker. Poll `prepare_progress`; completed preparation survives restart and
unfinished work returns `preparation_interrupted` with committed city states
preserved. `prepare_cancel` cooperatively stops further acquisition and input
commit. Discover `prepare` separately for authored snapshots and manual composition.

Final import creates an independent immutable closure, pinning the original
calculation batch, source APD draft and report input. It never replaces the
calculation manifest or sync intent. Discover `final_import` and provide an
explicit Markdown source path on Server. Discover `final_inspect` to recover
the committed final and its source and final batch identities after restart.
Artifact reads and verified exports reopen that closure and check its lineage.
An existing final cannot be overwritten; use a new source run for replacement.

Discover the native `report_bundle` operation for city APD, network, plant or
financial drafts and reviewed APD finals. It pins the source batch id and digest,
preserves exact canonical Markdown and emits an independent rendering copy with
all images. Declared plots are verified against their closed manifest; governed
map references are downloaded through the captured project's authenticated
snapshot producer and checked for exact generation, size, digest and image type.
A missing image refuses the whole package. Provide a new absolute destination
under an existing private directory; export never overwrites earlier output.
The package is local and does not imply publication.

Discover `portfolio_report_bundle` to package a declared portfolio draft or
reviewed final with its aggregate charts, pinned member charts and governed
maps. It requires both portfolio and city-source batch pins. Portfolio final
import and inspection use `portfolio_final_import` and `portfolio_final_inspect`;
each final has an independent immutable closure tied to its source draft,
portfolio input and result.

Discover `report_finish` for headless DOCX/PDF production from an imported
reviewed city or portfolio final. The Rust owner verifies its complete image
package, checks canonical source bytes, creates a separate presentation copy
without the authoring guide, and delegates fixed local tool effects. Pandoc
produces DOCX; LibreOffice converts that DOCX to PDF in an isolated headless
profile. The source and calculation closures remain unchanged. Destinations
must be new files under a private directory. A finished local document remains
`publication: not_requested`.

`portfolio_publish` observes publication of one selected fresh native portfolio
batch. Native completion seals its result, drafts and chart bytes into the common
sync store; an offline run retains those bytes for reconnect. The observation
cannot adopt a historical directory or enqueue old results. The legacy standalone
portfolio publisher is removed: recompute through `portfolio_start` or
`portfolio_calculate`. Current governed membership is fenced by common admission.
Require a published receipt before claiming online delivery; queued output is
still local.

Use `solar portfolio published read` with an explicit project, portfolio and
source run to inspect the current published aggregate without Desktop. Native
authority verifies generation-pinned bytes, the artifact head and current
membership before returning a bounded semantic projection. A different source
run or changed membership refuses instead of substituting a newer analysis.

The paired Solar commands were retired on 2026-10-20 with the Desktop Solar
window executor; the Desktop page is a JSON window over `solar.application`.
Standalone `results read` and dashboard composition read sealed native sources
without pairing or sign-in; the supplied project is attribution and grants no
cloud authority.

## Headless artifact route

`ds solar input capture --project <id> --city <canonical-id> --out <fresh-file> [--lane
stable|canary]` is the authenticated headless intake route. It restores the
native user and captures explicit project, account, lane and credential audience, derives
`eds_project/<project>/eds_solar` internally, and makes exactly the fixed
`desktop_snapshot` request for that city. It rechecks authority after the read, leaves saved selection unchanged and accepts no root, endpoint,
token, receipt or generic request override.

The bounded response enters the linked engine-free `ds-solar-project::intake`
owner without normalization of the captured Go bytes. Short-lived media
download URLs exist only in protected memory: they are never written to an
intermediate snapshot file, argv, stdout or the resulting governed intake.
The owner verifies the original fingerprint and exact project/city before
writing; no retired application executable is probed or launched.

`solar.input.capture` contract **3** replaces contract 2's process handoff.
CLI/MCP consumers must admit contract 3 and the `bounded-owner-intake` handoff
value; output remains `ds-solar.governed-city-intake/v1`. Existing verified
intakes retain explicit read compatibility. Missing, malformed, crossed or
expired authority refuses before output publication; no old executable fallback
or project retargeting is permitted.

The owner creates the output with create-new semantics. `ds` then opens that
exact regular file without following symlinks, performs a bounded read, and
verifies its closed v1 envelope against the explicit project, city, derived
root, snapshot digest, input fingerprint, receipt authority, expiry and source
counts before reporting success. The command returns only the intake digest,
bounded provenance and a SHA-256 of the receipt id. The raw actor-bound receipt
id remains solely inside the deliberate authority-bearing intake.

`ds solar input prepare --intake <file> --cache <dir> --out <fresh-dir>`
continues that handoff without a Desktop session. It calls the linked engine-free
`ds-solar-project::preparation` owner against an existing verified
`ds-solar` reference cache. There is deliberately no provider URL, weather
token, API key, project/root override, fixture mode, overwrite, or generic
engine argument. A missing exact weather/reference bundle is therefore a
refusal, not an implicit network request.

The command accepts one private, bounded, non-symlinked governed intake and a
real local cache directory. It creates `--out` itself with private permissions
on Unix and writes the unchanged `<city>.prepared.json` and
`<city>.prepared-publication-claim.json` pair plus the validated
`<city>.server-submission.json` envelope. All three artifact roles, paths, byte
counts and SHA-256 digests remain in the receipt, with `network: false` and
`cache_mode: verified-local-only`. Contract 2 replaces `engine` with
`preparation_owner`: `name: ds-solar-project`, `source_sha` (null for an unstamped
development build), and `contracts_build_manifest_sha256`. This identifies the
linked preparation source and contracts, not a selected calculation addon.
Descriptors and success/error envelopes advertise contract 2 so consumers can
refuse an unsupported receipt version before adoption.
The actor-bound claim and cache contents remain on disk. The prepared directory
can then be passed unchanged to `ds solar run --prepared`.

A cache miss remains a refusal from this local operation. Populate the exact
site/equipment reference cache through an explicitly authorized acquisition or
sealed-reference import operation, then retry. This command never contacts a
provider or substitutes development fixture data.

`ds solar run --prepared <dir> --out <dir>` contract **3** captures one
standalone Solar addon from `--lane stable|canary` (stable by default), without
reading account or project state. A missing or corrupt managed selection refuses;
there is no packaged `ds-solar` or PATH fallback. `DS_SOLAR_ENGINE_BIN` remains an
explicit development selection. All selected inputs pass the existing local
readiness checks before execution; publication claims are not consumed.

Contract 3 also adopts the existing fixed-task admission: at most 512 cities,
256 UTF-8 bytes per project/root/city/run identity, 64 MiB per prepared JSON file
and 512 MiB for the exact serialized request. Request admission runs before
engine selection and returns `prepared_batch_invalid`; local unreadable,
oversized or unready prepared files retain `engine_refused`. Closed task output
is bounded to 64 MiB per file and 4 GiB total. These explicit bounds replace the
legacy batch receipt's 512-byte identity and 128 MiB artifact allowance; formerly
oversized inputs/outputs now refuse without truncation or engine fallback.
Output bounds are verified after calculation, before local commitment. The
512-city transport limit does not widen the calculation owner's existing
200-city admission; its typed refusal remains authoritative.

The fixed `calculate-batch` owner runs offline with the existing concurrency
(default 10), chart switch and document defaults. The four-hour deadline includes
local admission and capture; only its remainder reaches the bounded worker wait
and calculation. Retained managed executable checks run before and after the
native collector, including a failed task. Calculation never acquires references.

The local owner verifies and commits the exact closed city artifacts, batch,
sync intent, failures and raw progress events without replacing existing output.
Task-only daily frames are not added to this legacy local layout. Failed cities
retain successful local artifacts before `engine_refused`; completion details
identify the committed scope. A publication collision retains its verified
candidate. Inspect these paths before explicitly retrying.

The receipt retains the existing fields, bounded 16 MiB batch manifest and at
most 500 artifact descriptors with `more.artifacts_omitted`. Its `engine` field
now contains the actual typed engine identity, replacing stdout summary lines.
`engine_path`, `lane`, `network: false`, `request_sha256` and
`completion_build_manifest_sha256` identify the captured task. Descriptors and
success/error envelopes advertise contract 3. All artifact bytes are verified.
This command does not invoke the owner binary's separate portfolio subcommand
and does not produce or claim a portfolio result. Use the native
`portfolio_start` operation through `solar.application` when a portfolio
deliverable is required.

```bash
# Headless explicit-project capture, cache-hit preparation and artifact run
ds solar input capture --project <exact-id> --city rw-kigali --out ./rw-kigali.intake.json --output json
ds solar input prepare --intake ./rw-kigali.intake.json \
  --cache ./solar-reference-cache --out ./prepared --output json
ds solar run --prepared ./prepared --out ./out --output json
```

## Authority and typed operations

| CLI command | operation | effect | result |
|---|---|---|---|
| `solar input capture` | fixed native `desktop_snapshot` + governed owner stdin | local file write | verified governed city intake |
| `solar input prepare` | linked cache-only preparation owner | local file write | prepared input, exact publication claim and Server submission (contract 2) |
| `solar result compare` | fixed native `compare` | read only | sealed-result equality and bounded provenance |
| `solar results read` | `solar.results.read` | read only | headless verified sealed-source section projection (contract 2) |
| `solar dashboard compose` | `solar.dashboard.compose` | local file write | headless Site/Plant/Finance/BOQ Rust projection; HTML cards and BOQ tables; Plant JSON chart options |
| `solar portfolio list` | `solar.portfolio.list` | read only | governed ids, membership revisions and ordered cities |
| `solar portfolio analysis` | `solar.portfolio.analysis` | read only | one portfolio's saved-analysis state, identity and verbatim error |

## Bounds

| Command | Timeout | Why |
|---|---:|---|
| `solar input capture` | 120 s request + 5 min owner intake | bounded explicit-project capture and create-new sealing |
| `solar input prepare` | synchronous local preparation | intake bounded to 32 MiB; each output artifact bounded to 128 MiB; no process or provider |
| headless `solar run` | 4 h | offline compute over a caller-supplied prepared batch |
| `solar engine` | 20 s | external engine discovery |
| `solar verify-weather`, `solar result compare` | synchronous local validation | each regular input file is bounded to 64 MiB; no engine process |

## Engine identity

`ds solar engine`, `solar calculate-prepared` and `solar run --prepared` use
the standalone `solar` addon selection described by their native lane contract. `solar input capture`
and `solar input prepare` call their linked engine-free project owners. Neither
uses addon selection, `DS_SOLAR_BIN`, a sibling executable or PATH.
The local Solar commands no longer consume the legacy bundled `ds-solar`.

`solar verify-weather` and `solar result compare` call the pure
`ds-solar-contracts` owner directly. They require neither a Solar executable nor
an addon selection, account, project or Desktop. Each input remains a bounded
64 MiB regular JSON file. Invalid schemas, altered digests and oversized files
retain `engine_refused` with bounded owner detail; missing paths retain their
specific missing-file refusal. Weather success retains the existing `dataset`,
`verified` and `engine` fields, with the same validated weather report text.

`solar build-info` publishes the immutable `ds.engine-build/v1` identity:
engine name and version, exact source SHA, Cargo.lock digest, features, target,
profile, supported schemas and a canonical manifest digest. `ds solar engine`
returns the selected addon's identity with its resolved executable path; the
addon has its own source revision, independently of the consuming `ds` release.

`ds solar result compare` validates both complete `ds-solar.result/v1`
documents through the same pure owner used by the legacy tool, including
recomputing each canonical digest, then returns the closed `ds-solar.result-comparison/v1` equality receipt with
project/city, input, weather and engine provenance. A valid difference is data (`equal: false`), not a
failure. The operation reads local artifacts only; it neither proves current
project membership nor authorizes publication or mutation.

## Related

- `crates/ds-cli-solar/src/input_capture.rs` — explicit-project governed intake capture
- `crates/ds-cli-solar/src/input_prepare.rs` — cache-only governed intake preparation
- `crates/ds-cli-solar/src/workflow.rs` — headless result-section, portfolio list and analysis reads
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

`project sync --project <exact-project> --lane stable --background --yes` launches the fixed native
input publisher. It binds to the explicit workspace project, native principal and audience;
saved selection is unchanged and detached workers retain that project;
queues survive failure. Omit `--background` to drain once or use `--watch` to
watch for new work. The worker exits after 12 hours or a permanent refusal.
A reviewed city conflict can be rebased with `project sync rebase --sequence N
--expected-cloud FINGERPRINT`; this does not publish until `sync --yes`.

Input publication needs the server's `project_commit` route. Shared result
publication comes from a fresh native application run with an admitted release
build; standalone local drafts remain usable offline. Governed portfolio
membership is separate from this explicit local workspace.

### City ids and local reference data

Discover an explicit project's city ids with `ds solar cities --project <id>`; use those exact ids for capture and preparation.

Discover `solar.reference.plan` and `solar.reference.import` for local preparation
after capturing and seeding a workspace. These controls link the native local
project owner and need no retired `ds-solar` application executable. The native
plan exports the complete
selected-city request and its input digest; an optional external reference
producer seals the four files. Import preserves the explicit local project and
expected city digest, verifies bytes/site/equipment, and commits/readbacks the
existing native cache. Retained identical bytes reuse the same generation. No
API, Python launch or cloud publication occurs in these controls. Local project
attribution grants no cloud authority. The retired remote acquisition leaf is
removed; there is no implicit provider fallback.
The compact `solar-input` MCP profile keeps both local reference controls within
its existing bound. Result inspection remains in `solar-dashboard` and the CLI;
it is separate from the input profile.

## Shared network forms

Solar consumes the already-classified project table and maps through Assets.
City sources resolve by exact tag definition/value; transformer maps resolve by
transformer identity. Discover `solar.network.resolve` and `solar.network.save`
for the live form contract. Missing sources seed manual entry, never a geographic
refusal. Only source references and operator overrides are saved and synced.

The owning [shared-network contract](../../../ds-solar/docs/contracts/shared-network-assets.md)
describes inheritance, source refresh, legacy adoption and calculation storage.
A standalone project draft is local-only; it does not enqueue a shared result.

`solar project clean --workspace DIR --project PROJECT --yes` clears local
computed runs and their publication intents. Authored inputs, parameters, copied
maps and reference cache remain. Active computation refuses cleanup. Recompute
from the engine and current inputs; fresh native runs retain outputs offline and
publish on reconnect. The same owner JSON control is `project_clean`.

## Sealed portfolio dashboards

`ds solar dashboard compose --source <closed-portfolio-dir> --project <id>
--run-id <portfolio-run> --section portfolio_finance --portfolio <id>
--membership-revision <sha256:digest> --out <fresh-dir>` verifies the exact
immutable portfolio closure before presentation. City and scenario selectors
are refused for this section. Its cards, tables and chart options use the same
Rust owner as browser WASM. The standalone HTML includes script-free SVG and
exact chart data. Negative IRR values remain negative, unavailable values stay
unavailable, and DSCR observations align with their actual years. No desktop,
browser, network or TypeScript is needed. This local export is not proof of
online publication; external plot files are not included.

`portfolio_site`, `portfolio_plant` and `portfolio_boq` use the same sealed
portfolio selectors. Rust selects the calculator-owned tables, columns, totals,
assumptions, representative graph cities and exact unavailable graph slots.
The server produces JSON and standalone HTML; browser views consume that same
projection through WASM. BOQ amounts and unit prices retain authoritative zero
values and take precedence over older per-city aliases. V3's ordered graph rows
are projected into section groups by Rust. Native graph reads pin source batch,
city, role, filename, size and digest; fetched PNG bytes are verified in Rust
before the browser creates an object URL.
