# `ds report` — reference

Tier-4 reference. `ds report <command> --help` is the contract.

## Why this domain calls a binary instead of linking a crate

`ds-network-reporter` publishes exactly one surface an agent host may call,
and wrote down why. From `src/bin/ds-report.rs`'s own header:

> one named subcommand per call — never a caller-supplied argv … a typed
> request file — not flags built from model output … a machine-readable
> result document — never parsed stdout prose.

That is a deliberate ownership boundary, not an accident of packaging. So
`ds report` builds a typed request, names one subcommand, and reads the
document that comes back. It links none of the reporter's library and
reimplements none of it.

Contrast `ds dsgrid`, which *links* `ds-grid-model` and `ds-grid-exchange`
directly — those are pure libraries with a clean boundary and no such
contract. Both routes are legitimate; which one applies is decided by the
owning workspace, not by convenience.

## Two engine rules that shape every command here

**The result file must not already exist, and there is no `--force`.** The
reporter refuses before doing any work. Its reason: a caller that finds a
stale document where its answer should be cannot tell the difference between
this run and the last one.

`ds` honours this rather than working around it. When you pass `--result`,
that path is yours — checked, used, and never removed by `ds`. When you do
not, `ds` writes to a scratch file it owns, reads it, and deletes it.

**A failed task still writes its document, then exits non-zero.** The blockers
are *in the file*; the exit status is only the coarse signal. This is right
for the engine — an exit code cannot carry a list of blockers — but it leaves
a direct caller holding a number and a path.

So `ds report export` reads the document in both outcomes:

| Engine outcome | `ds` result |
|---|---|
| exit 0, status `completed` | success; the document is `data` |
| exit 0, status `partial` | success; the document is `data`, blockers included |
| exit 1, document written | `export_blocked`, with `detail.blockers` |
| exit 1, no document | `engine_refused`, with `detail.engine` |

A caller never has to know the convention.

## Discovering the request contract

The engine publishes a full JSON Schema per task. That document is tens of
kilobytes, so `ds` tiers it the same way it tiers its own help:

```bash
ds report tasks                                    # the index
ds report tasks --task export_transformer_report   # one full schema
```

The schemas are never copied into this repository. They are read from the
engine installed on this machine, at the version actually installed, so they
cannot be stale.

## Flags versus `--request`

`ds report export` offers named flags for the common path *and* a
`--request <path>` passthrough for the engine's complete typed request. The
two are mutually exclusive — passing both is `conflicting_inputs`, because
silently ignoring one set would be worse than refusing.

The flag names are a hand copy of the engine's schema field names. A hand copy
nobody checks drifts silently, so `crates/ds/tests/engine_parity.rs` fetches
`ds-report task-schemas` from the installed engine and asserts:

- every **required** field of every task is reachable from a declared flag;
- every declared flag corresponds to a real engine request property.

The second direction matters as much as the first: a flag writing a field the
engine ignores looks like it worked.

One deliberate asymmetry: the engine's `transformer` (singular, one report)
and `transformers` (plural, combined) are both reached through a repeated
`--transformer`, so a caller does not have to know which task pluralizes.

For combined export, repeat `--transformer-document` in the same order; `ds`
builds the engine's required `{transformer, layers}` pairs. There is no
reporter-side `all`: a paired desktop/cache command must first resolve the
selection, refresh missing or stale IndexedDB rooms, and pass the exact local
documents. The reporter performs no download.

## Background project reports

`ds report project scope|combined|archives` is the other door of this
domain: no local engine, no map, no Desktop. The commands restore the native
user for `--lane stable|canary`, use the required `--project <exact-id>` on
each request, and call the governed report service's fixed
contract. ds-brain owns everything that follows — it resolves the exact scope
(every active saved transformer, or the `--transformer` names given), reuses
fresh individual report artifacts, regenerates missing or stale ones with the
cloud reporter, composes the overall and optional per-district combined sets,
streams one ZIP with its manifest and writes a registry row. Retired
transformers (`ds design transformer retire`) are never in scope. The reserved
computed identities — `collisions`, `combined_transformer` and its aliases
(`all_transformers`, `combined_transformers`) — are report output, never
participants: `scope` and `combined` refuse them locally with
`reserved_transformer_identity` before any credential is restored.

```bash
ds report project scope --project <exact-id> --output json                      # the plan
ds report project combined --project <exact-id> --file-level sector --yes      # publish
ds report project archives --project <exact-id> --output json                   # the registry
```

`ds report project compute` is the cloud twin of `ds report project export`:
the same individual transformer report, computed by the cloud reporter and
published to the project by the governed service (what the application's
"Export reports" button asks; ds-brain action `export_reports_only`).
`--transformer` names the scope; omitted, every active saved transformer is
named from the same inventory `scope` reads, because the route takes exact
names. The service skips a transformer whose report is fresh,
refuses one another editor holds a lease on (`auth_input_invalid`, HTTP 423,
the holder named), and answers per transformer. A run with any errored row
exits non-zero as `report_compute_partial`, with the whole receipt in
`detail`, so a script that asked for N reports never reads N-1 as success.

```bash
ds report project compute --transformer akagerero --lane canary --yes --output json
ds design status --project it_rwanda --transformer akagerero --lane canary --output json   # the report the cloud stamped
```

`combined` is `artifact_write` and needs `--yes`: it publishes a durable
archive of record. Its receipt carries `status` (`success` or `partial`), the
archive `prefix` (the registry stem), cloud locators, individual artifact
coverage, the missing individuals with typed causes, bounded errors and
`registry_write_failed`. A receipt advertising an archive for zero individual
artifacts is refused as unreadable, as the application refuses it. The scope
rules, layout vocabulary and archive tree are ds-brain's
`docs/contracts/compounded-reports.md`; this is the same deliverable the paired
`ds map design batch report` requests through the application's session.

## The project's output policy

Which files a report produces, and where each one may be produced, is one
decision and `ds-command-kernel::report_formats` makes it. These two commands
are its headless door; the Settings page in the application is the other, and
both get the same answer because neither computes it.

```bash
ds report project settings --project <exact-id> --output json                 # output policy and readiness
ds report project outputs set --project <exact-id> --selection outputs.json --yes
```

`settings` reads the named project's fresh configuration and hands its
sheets to the kernel. The reply is the kernel's, unedited: `source` (the
project's own export row, or the report defaults when it has none), the stored
`setting` row exactly as saved, the `outputs` it resolves to with their formats
and file suffixes, the `papers` of each named printout, `ready`, any `issues`,
and a `refusal` naming a `code`, a `message_key` and the `mode` the settings
were read in. The message key is deliberate: the GUI resolves it in the
operator's language, `ds` prints the code and the kernel's own findings, and
neither composes a sentence of its own. A project stores its selection in
whatever shape it has ever stored one — a comma or semicolon string, a token
array, a truthy map, or the versioned `ds.design-output-selection/v1` document
— and all of them read here.

`outputs set` takes that versioned document (`ds report layout schema` returns
its schema under `output_selection`). It is validated against the kernel's
closed type *before* any credential is restored, so a malformed selection costs
no round trip. A selection naming a printing setup this project does not
hold is refused the same way, before the write: naming a global template is not
adopting it, and `print_setup_not_adopted` names `ds report layout copy` as the
way to adopt one. A selection that cannot execute is never saved as though it
could. The kernel then writes it into the project's settings sheet —
into whichever of the five export-row aliases the project already uses
(`design_export_format`, `design_export_formats`, `transformer_export_formats`,
`tr_export_formats`, `report_formats`, in any case and with hyphens), or a new
`design_export_format` row when it has none — and every other settings row is
preserved byte for byte. The patched sheet is saved through the same
`save_config` request the application sends, then read back fresh and verified;
a save whose read-back disagrees is reported as unreadable rather than as
success.

A selection's `execution` map is where each output may run: keys are an exact
output id (`pdf__detail`), an output class (`print`, `geospatial`, `tabular`),
or `*`, and the value is a subset of `["desktop","web"]`. An output no key
names may run on both. The map is saved as authored, and ds-brain admits an
export against it — the kernel decides what the policy says, the service
decides whether this request is allowed.

```json
{
  "schema": "ds.design-output-selection/v1",
  "prints": [{ "layout_id": "detail", "enabled": true, "formats": ["pdf", "png"] }],
  "geospatial": ["gpkg"],
  "tabular": ["xlsx"],
  "execution": { "pdf__detail": ["desktop"], "geospatial": ["web"] }
}
```

A Combined Report archive consumes the project's applied `report_archive` consumer
grouping: that plan, not this request, is the folder and section authority.
`ds design consumer-grouping read|preview|apply --purpose report_archive` is
where it is inspected, re-planned and applied, and a project without it is one
of the causes the service reports here as `auth_input_invalid`.

The receipt does not confirm the foldering. `--file-level sector|district` and
`--combine-per-group` are a request: when no administrative value resolves,
the requested layout silently collapses to `_unassigned` folders while the run
still reports `success`, which the archives registry row exposes as
`district_count: 0` and `ds` derives there as `layout_collapsed`.

**Both commands describe the layout in the report layer's own vocabulary.**
`archive_layout` carries the recorded spelling plus `level`, `level_key`,
`label_key` and `label_level_key` — the same keys the application renders, from
`ds-command-kernel::report`. Two spellings describe one choice: `file_level` is
current and `transformer_grouping` is the legacy twin older archives were
written with, and the kernel folds them (`district_sector` reads as a
sector-level archive; an unrecognised or absent level is the default,
transformer). Until 2026-09-11 `ds report project archives` ignored the legacy
spelling entirely and reported a foldered archive as having no layout at all,
which is how one archive came to be described two ways.

The registry's `download_url` is freshly signed by the service with about an
hour of validity, but has been observed arriving with seconds left, so a caller
must never assume a returned URL is still usable. `ds` reads each URL's own
expiry out of its signature (`Expires`, or `X-Goog-Date` plus
`X-Goog-Expires`) and reports it as `download_url_expires_at`,
`download_url_seconds_remaining` and `download_url_expired`; check that before
fetching, and list again for a fresh signature.

## When the Combined Report refuses

`ds report project combined` packages one archive out of the rooms' own
reports, so a room whose report is not current can only contribute an
out-of-date file. It used to be dropped from the archive without a word —
that is how one project's archive combined nothing and another's carried
June's files. The run now refuses instead, and the refusal names every room
it will not package plus the one command that fixes them:

| refusal | what the rooms are waiting for | the command it prints |
|---|---|---|
| `combined_inputs_publication_pending` | computed on a device, still in the publication queue | `ds report outbox drain` |
| `combined_inputs_not_current` | missing, stale, failed, or given up on by the queue | `ds report project export --transformer …` |
| `combined_inputs_empty` | current, and genuinely holding nothing | `ds report project scope` |
| `combined_no_inputs` | nothing in scope at all | `ds report project scope` |

A pending room is never answered with a re-export: exporting it again would
seal a second artifact beside the one already waiting, which is the hoarding
the queue exists to end. The room list is bounded, and the reply says how
many more rooms there are and whether the printed command reaches all of
them.

## Combined desktop reports

`ds report bundle --request <file>` invokes the reporter-owned
`export_compounded_report` task. The request lists transformer and combined
artifacts with their SHA-256 digests and safe archive paths, supplies the
manifest, and names a new output ZIP. The task streams local files, verifies
each digest, embeds `manifest.json`, and never contacts ds-brain or cloud
storage. Discover its exact schema with:

```bash
ds report tasks --task export_compounded_report
```

## Finding the engine

| Order | Location | Why |
|---|---|---|
| 1 | `DS_REPORT_BIN` | explicit beats inferred, always |
| 2 | a sibling of the running `ds` | the deployed case — the desktop installs both into the same directory |
| 3 | `PATH` | for a developer who put one there |

`PATH` is last on purpose. If it were first, a stale binary earlier in
someone's `PATH` would outrank the one shipped alongside the application, and
the resulting wrong answer would look like a correct one.

Availability is resolved with filesystem metadata only — it never runs the
binary, because `ds doctor` and domain help both call it.

## Effect classification

`report.export` declares `local_file_write`, not `artifact_write`, and that is
the reason it does **not** require `--yes`: it writes one file into a directory
the operator named on the command line, and publishes nothing of record.

The test is where the bytes land and who else can see them, not how much work
produced them. A command earns `artifact_write` when what it leaves behind is a
durable record someone else will read as authoritative — `map design save`,
`library seed`, `solar final submit`. A `machine_write` command reaches further
still, changing this machine outside any workspace. Both are confirmation-gated
and `report export` is not, because undoing `report export` is deleting one
file at a path the caller chose. See
[`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)
for the full table.

`report bundle` sits in the same class as `export` for the same reason: it
assembles a ZIP at a path the caller names, from local documents whose digests
it verifies, and contacts nothing.

`report project combined` is the contrast: `artifact_write`, because the ZIP
it publishes lands in the project's cloud registry where every member reads it
as the delivery. `scope`, `settings` and `archives` are `local_auth_state` like
every headless read — they may rotate the native credential, and write nothing
else. `report project outputs set` is `global_write`: it changes saved project
settings every member's next export reads.

## Related

- `ds-network-reporter/src/bin/ds-report.rs` — the contract, in its own words
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)

## Printing layouts

`report.layout.new` returns an A3 page document; `report.layout.schema` describes
its physical units, allowed elements and closed edit intents. `report.layout.edit
--request edit.json` evaluates `{op:"edit",layout,element}` (or validate/remove)
through command-kernel. It never changes an open desktop canvas.

`report.layout.list --scope global` browses published samples. `report.layout.get
--scope global --id network-a3` returns a layout with its revision. Use
`--scope project` for the native selected project's independent customizations.
Both scope variants use the same protected native identity and accept no URL,
token or project override. `--lane canary|stable` selects its credential lane.

Use `report.layout.create --scope global|project --request create.json --yes`
with `{action:"create",layout}` for a new stable ID. Use
`report.layout.update` with `{action:"update",layout,expected_revision}` for an
existing setup, and `report.layout.delete --id ID --expected-revision REV` for
an exact deletion. `report.layout.save` remains a compatibility command for
older clients. Global writes require
`map.defaults.edit`; project writes require `printing.setup.edit`, membership
and an open project lifecycle. A conflict never becomes an unconditional save.

`report.layout.copy --request copy.json --yes` accepts one source and
destination, each scoped `global` or `project`. It pins the exact source
revision and creates with an empty destination revision or replaces an exact
destination revision. Project scope always means the held selected project.
This one transaction implements adoption, promotion and published duplication.

`report.layout.render --request render.json` calls the installed reporter's
fixed `render-print-layout` task with held GeoJSON. Discover that task's complete
schema with `report.tasks --task render_print_layout`. It writes SVG/PDF to a
fresh directory and returns paths and hashes. No network or desktop is required.
Published setup reads use Brain; a held layout/render file prints offline.

`ds mcp serve --exposure commands --profile printing` exposes all twelve layout
commands together. The grid profile keeps its existing report-export workflow.
Reference A3/A4 layouts and runnable synthetic-data proofs live in
`ds-network-reporter/examples/printing/`; importing them never publishes them.

Create a named portrait template through the same closed authoring evaluator:

```json
{"op":"new","id":"a0-survey-portrait","name":"Survey · A0 portrait","paper":"A0","orientation":"portrait"}
```

Pass that JSON to `report.layout.edit --request FILE`. Other paper presets are
A1–A5. `{"op":"rename","layout":{...},"name":"New name"}` preserves identity;
`{"op":"duplicate","layout":{...},"id":"new-id","name":"Copy"}` requires a
new identity. Shared saves still require an expected revision and explicit
confirmation. Rendering a held layout does not mutate its shared template.

## Context layers

`ds report layout context` answers what the Printing setup page's context
picker answers, from the same kernel module (`printing::context`), with no
browser: `--action defaults` (the default) lists the derived sources every
template may carry — project DS Grid MV, building footprints, elevation
contours — with their print styles and the catalogue layers excluded from a
default selection; `--action catalog --resources facts.json` takes the
catalogue as facts (`[{layer,label,country,ready,downloadable,unavailable}]`)
and returns one row per dataset saying whether it may be switched on and
whether a fresh template starts with it; `--action select --layout l.json
--resources facts.json` applies that default selection to a held layout;
`--action toggle --layout l.json --layer ID --enabled true|false` switches one
layer, seeding its print styles, refusing `printing_context_no_dataset` for a
catalogue layer that is neither held nor downloadable unless it is already
selected. `report.layout.edit` accepts the same `context_*` ops as raw requests.

## New elements and print styles

`ds report layout add --layout l.json --kind table|text|legend|logo|…`
appends the canonical new element of that kind — the frame, ink and type
size the Printing setup page gives one, a table bound to `lv_print_info`
with Description/Unit/Quantity, a legend carrying its binding from the start,
a logo (`--asset`) showing an asset the layout holds — and returns the layout
with the element's index; the id is minted as `<kind>-<n>` unless `--id` is
given. `ds report layout style-ref --catalogue styles.json` lists the governed
print styles a print layer may bind, from the layers snapshot's style facts
(`[{style_ref, style_target}]`); `--action set --layout l.json --layer ID
--style-ref REF` binds one (an empty reference unbinds), refusing
`printing_style_ref_ineligible` for a reference the catalogue does not offer.
`ds map print schema --section choices` advertises the DPI and paper a local
map print may ask for, with their defaults.

`ds report layout pens --layout l.json --documents styles.json` lists the
template's pens (`printing::pens`): one row per print layer bound to a governed
`_print` style, design layers first in engineering order, with the base
colour, size, opacity, visibility, category table and label read from the
resolved document given as a fact (`[{style_ref, document}]`), the
template's own override, and the effective values a delivery or preview
prints. `--action edit --layer ID --property size|color|opacity|visible|
label_visible|label_size_pt|label_color|label_halo_mm|label_halo_color --value
JSON` sets one bounded property of that layer's `style_overrides` entry
(`null` clears it; an override left empty disappears); `--action reset --layer
ID` removes the whole override. A layer without a bound style refuses
`printing_pen_unbound`. `report.layout.edit` accepts the same `print_pens`,
`pen_edit` and `set_style_override` ops as raw requests; the Printing setup
page's pen panel is these rows and edits.

## The editor's workflow

`ds report layout session` answers what the Printing setup page's flags
answered — may this layout be changed right now, is it dirty, what does Cancel
restore, what do Undo and Redo mean and how deep they go — from the same
kernel reducer (`printing::session`). With no arguments it returns the zero
state and the history cap. With `--state state.json --event event.json` it
returns the next state, the operations the host performs on the documents it
holds (`install_document`, `push_undo`, `pop_undo`, `pop_redo`,
`clear_history`, `snapshot_baseline`, `restore_baseline`, `drop_baseline`,
`clear_document`) and whether anything applied; a refusal names its code
(`printing_discard_unconfirmed`, `printing_not_editing`,
`printing_global_read_only`, `printing_no_document`,
`printing_nothing_to_undo`, `printing_nothing_to_redo`). Events:
`draft`, `load {id, revision, global, same_document}`, `begin_edit`,
`open_context`, `edit`, `undo`, `redo`, `discard`, `saved {id, revision}`,
`deleted`; `draft`, `load` and `discard` take `confirmed` once the operator
has agreed to drop unsaved edits. Documents never cross this boundary.

## Publishing a setup

`ds report layout save --scope <global|project> [--project <id>] --request <file>` (project scope names its project; the saved selection is never read) takes a `save`, `create` or `update` request carrying `layout` and, for the latter two shapes, `expected_revision`. All three publish the same way: the kernel's setup lifecycle plan (`printing::lifecycle`) decides whether a publish is a create (no revision) or an update (an exact one) — the same decision the Printing setup page takes — so `ds` never sends the compatibility `save` action itself, and the word in the request never overrides the facts. `copy` and `delete` are different transactions and keep their own commands; `save` refuses them by name.

`ds report layout schema` documents all four transactions, `save` included, so a request derived from discovery is one a command accepts. A layout the deployed validator refuses comes back as `print_layout_invalid` naming the offending field, and a validator that cannot answer at all as `print_validator_unavailable` — never as an authentication failure.

## Printable inventory and report plans

`ds report transformers --project <id> --limit 100 --output json` reads the
named project (the saved selection is never read) and asks `printing::inventory` which rows are printable.
Reserved aggregate, analysis and project-document identities are excluded even
when raw status rows omit their kind. The result reports `more.omitted`.
`cached` and `dirty` are **null**, with `local_rooms_known: false`, because this
native read does not inspect browser rooms. `ds desktop printing transformers`
remains the browser-room IO adapter and asks the same kernel with those facts.
Neither command opens or switches the map.

`ds report plan --action export --request export.json --output json` evaluates
an export request without running it. The file contains `project`,
`transformer`, optional `transformers`, `force`, `report_type`, `selection`,
and the host's `active_project`. A combined overview requires
that last fact to match `project`; an individual transformer does not.
The GUI asks again after preparation to detect a project switch during IO.
The input is a fact document, not an authorization grant.

`ds report plan --action batch-outcome --request batch.json --output json`
accepts `{"results":[{"transformer":"tx_a","status":"ok"}]}` and returns the
ordered results and `failed` count. Both actions accept at most 4 MiB and refuse
a `command` key inside the document.

`ds report plan --action activity --request activity.json --output json`
projects generation and publication independently from one explicit project's
host facts. Its request is `{"snapshot":{"project":"p1","now_ms":300000,
"rows":[{"name":"tx_a","phase":"generated","updated_at_ms":100000},
{"name":"tx_b","phase":"generating","updated_at_ms":110000}]}}`.
Transformer names use canonical lowercase letters, digits and underscores
(up to 121 bytes), not display titles. `project` is the exact project-id string.
The native sync-store artifact/receipt projections and requested output IDs may
also be supplied. The result reports generated/published counts, queue issues,
per-row regeneration reasons and a waiting transformer after no generation
progress. It does not run or authorize an export. See the [owning activity
contract](../../../ds-command-kernel/docs/contracts/report-activity.md).

Preview admission is available through `ds report layout edit --request
preview.json --output json`, using the kernel's `render_plan` operation:
`{"op":"render_plan","layout":{...},"available_style_refs":[],"layer_count":1,
"text":{"title":"Preview"},"date":"2026-09-11"}`. It returns `ready`, refusal
keys, required print-style references and text. This evaluates admission only;
PDF rendering still uses the existing renderer.

The same `report layout edit` surface exposes the parent-frame operations:
`set_fit_policy` accepts a layout plus optional `network_fit` and `scale_limits`
policies (explicit `null` removes either). `network_fit` names 1..16 required
engineering `source_layers` and 1..30 mm `clearance_mm`; the native renderer fits
their conservative stepped footprint around measured furniture using one
projection. `scale_limits` declares ordered `min_denominator` and
`max_denominator` within 10..100,000,000. The minimum caps enlargement of a tiny
design; the maximum refuses an incomplete fit. Existing layouts retain
rectangular fitting without these policies. Explicit cameras must satisfy scale
limits and refuse stepped fitting. The actual assessment's `map_fit` contains
mode, resolved denominator and occupied footprint. This edits a draft; use the
normal project-template save contract to apply it across transformer sheets.

`preview_context_preparation` accepts a draft layout and `online`; it returns
only that draft's selected contexts and whether missing inputs may be acquired.
Prepare these inputs before the read-only native preview. Contour intervals are
part of the retained context address, so different templates cannot reuse the
wrong interval set. Acquisition failures and offline omissions remain named.
`context_status` accepts the render's `omitted` rows (`layer`, `reason`) and
separates neutral `empty_layers` from `unavailable` sources with their reasons.

The parent-frame operations remain independent:
`sheet_frames` returns map space → drawn printable border → independent table
containers → local table origins; `table_container_rect` accepts `element_id`
and border-relative `rect_mm`, resolves the parent offset in Rust, and clears
sibling flow for explicitly positioned tables. `furniture_hit` accepts measured
panel rectangles and `point_mm`; gaps between stepped panels do not count as
table content. These decisions are shared with the UI through WASM. Rendering
returns the measured `frame_hierarchy`, `printable_boundary_mm`, and
`furniture_frames` in each page assessment, after flow and packing.

Two table treatments keep columns narrow without abbreviating by hand, both
decided in `ds-command-kernel::printing` and painted identically by every
renderer. `table.heading_mode` (`literal|auto_index|indexed|abbreviated`)
aliases titles with a full key above each panel. `table.value_key`
(`{mode: off|auto|columns, max_categories: 2..=26 (3), columns: [...]}`) prints
few-category values as 1-based indexes with one legend line per column under
each panel (`Category: 1 Commercial · 2 Residential`); absent, the customers
(house connection) schedule defaults to `auto` and every other table to `off`.
`auto` compacts a bound column whose values recur in at most `max_categories`
categories and are wider than their index, and yields to a frame with no room
for the legend; `columns` compacts exactly the named bound columns and refuses
one with more categories than allowed, naming the column and its count. The
row plan receives the height minus the legend. A per-transformer
`TableOverride` carries the same field.

`table_drafts` returns compact editor schematics in millimetres, using authored
font and row sizes. Approximate heading widths never expand to a maximum
container or replace Reporter's measured print geometry. Real label omissions
travel as scoped `print_label_omissions` warnings; the shared Rust validator
accepts them while rejecting mismatched run/output/layout identities.

## Headless transformer reports

`ds report project export` produces individual transformer reports — prints
included — with no browser, no room cache and no Desktop. It is the door a
Linux operator calls. Decisions and fingerprints come from
`ds-command-kernel::report_export`; the shared native IO host is
`ds-command-kernel/crates/ds-report-host`, and the engine is the installed `ds-report`.
Desktop now uses the kernel's fingerprints, while its existing sidecar and
publication pipeline still perform the desktop IO. Full IO unification is
pending.

```bash
ds report project export --project <exact-id> --out-dir ./reports --output json
ds report project export --project <exact-id> --transformer tx_a --out-dir ./reports --concurrency 2
ds report project publish --project <exact-id> --from ./reports --yes
```

The inputs are the governed ones, fetched under the native user for its
audience-fenced named project: the Network Reporter input receipt ds-brain
mints beside the fresh configuration (country, the exact settings sheets, the
reference snapshot) and each transformer's exact saved layers with their
revision. From them the kernel decides the formats (the project's output
policy, `ds report project settings`), the `input_base_fingerprint`
(`network_reporter/input-base/v2`) and the `room_content_sha256` (RFC 8785
over the room) — byte for byte what the desktop shell records — and the exact
typed request the engine reads. Named print outputs (`pdf__<layout>`) render
from the project's saved printing setups inside the same engine run; a Rwanda
project is stamped with the installed villages asset
(`--admin-bounds` names one explicitly).

For local template review, repeat `--print-layout ./layout.json` for existing
selected layout IDs. Each replacement keeps its paper identity; the kernel
first verifies the original server receipt, then derives a print-only receipt.
Every output run records `local_print_recipe` with the original sheets digest
and exact replacement layout digests. Neither project recipes nor engineering
rooms are changed. These files are proofs, not publishable project artifacts:
`--print-layout` and `--publish` are incompatible, even with a release engine.
This is the route for inspecting a new renderer before deployment.

For a live look at one draft, pass `--preview-layout ./draft.json` instead: any
layout id and any paper, saved or not. The kernel's preview recipe
(`report_export::preview`) puts the draft beside the project's setups in
memory and makes it the only output: one `svg__<id>` page per transformer in
scope. That page is the engine's own SVG — the one every delivered format is
derived from (PDF by conversion, PNG and JPEG by rasterisation) — with its
text flattened to outlines through the same bundled font, so any viewer draws
the sheet exactly as the delivery comes out and one preview serves every
format a template may later be delivered in. The template's pens
(`style_overrides`, edited through `report layout pens` or the `pen_edit` /
`set_style_override` ops of `report layout edit`) print exactly as they will
in the delivery, through the same engine, workbook schedules and context.
A preview is draft state, not an artifact: the delivery is the artifact. It
is executed once, in `ds-report-host` (`preview::execute`), for both doors —
this command and the desktop Printing setup, which obtains the same page
through the same command id via the desktop door and paints it over its
Canvas2D sheet. The engine runs over the room this command admits from the
service answer and the geographic holdings this machine already keeps: a
preview never seeds, never projects MV, and refuses `--seed` and
`--context-vectors` (`report_inputs_invalid`, decided before any
credential). A draft context layer this machine holds nothing for is a named
omission on the sheet, never a refusal.

What it writes, per transformer in scope: `<out-dir>/<transformer>/
<transformer>.<id>.svg` and `report-run.json` beside it (its
`local_print_recipe.preview` says it was a preview); there is no batch
receipt. `.data.preview` names `layout_id`, `layout_sha256` and the
`output_id` (`svg__<id>`); `.data.results[]` carries one row per page —
`page.path`, `filename`, `sha256`, `engine`, `elapsed_ms`, `warnings`,
`print_diagnostics` and `print_context` (`sha256`, `layers`, and `omitted`
as `{layer, reason}` rows keyed by the draft's context layer ids) — and
`.data.batch` counts the pages; `lane`, `project`, `scope`, `out_dir` and
`local_print_recipe` are this host's own facts. A refusal is classified as
the desktop door classifies it (`error.class`, `error.code`), with
`error.detail.message_key` and `params` when the kernel decided it. A
preview changes no recipe, room or output, `svg` is never a deliverable
selection, and the run cannot publish.

A selected `project_dsgrid_mv` context now reads the native project's complete
MV catalog and verifies each exact current head through `dsgrid.project`'s
existing authority. Model decoding, engineering projection and CRS conversion
use the native model owners. Preparation holds bounded projected geometry for
the batch; each sheet selects nearby features through the kernel and records
model revision/digest provenance. It changes no model, workspace or version.
Missing or unprojectable model data is named rather than silently dropped.

Named title blocks can use `{paper}`, `{sheet}`, and `{sheets}`. Sheet positions
are taken from the complete active project inventory in canonical name order,
including when only one transformer or a selected subset is exported. Keep
these bindings in the layout instead of literal “1 of 1” text.

The batch runs independent transformers under `min(scope, processors,
--concurrency|4)` resident engines; rooms are fetched one at a time. Each
completed transformer leaves `<out-dir>/<transformer>/` with its artifacts and
`report-run.json` (every artifact's SHA-256, size, content type and paper
identity, the engine identity, the fingerprint and digests, and the
publication identity — `state` is `pending` for a release engine and
`local_only` for a development build). `<out-dir>/report-batch.json` records
every result in the kernel's stable order; a transformer the engine refused is
one `error` row with its typed blockers, never the end of the batch. The
command exits non-zero only when no transformer completed
(`report_batch_failed`).

**A transformer that produced some of its formats is a result, not an error.**
One A0 that will not lay out never discards the A3, the KMZ, the shapefile and
the workbook beside it: those are finished work, and finished work has to be
able to leave the machine. Its row stays `ok`, its artifacts are placed,
verified and receipted like any other run's, `--publish` seals them, and every
format it did not get is named in `failed_formats` — the `output_id`, the
engine's `code` and bounded `message`, a `remedy`, and, for a print that did
not fit, the `layout` element and the knob that decides it (`panels`,
`overflow` or `row_mm`). `report-run.json` carries `status: "partial"` and the
same `failed_formats`; `report-batch.json` counts those runs in
`partial_formats` while still counting them as completed. `--output text`
prints such a run as a `partial` row — never a bare `ok` — with the format it
lost, the member that decides it and the remedy; past ten lost formats the
screen says how many more there are and leaves them to the receipts.
`export_blocked` is now only the case where the engine produced nothing at
all. Nothing here publishes: the receipts carry what a
publication needs, but no publication row is enqueued by this command unless
you explicitly pass `--publish`. That opt-in rechecks the native UID, lane,
credential audience, selected project and credential generation immediately
before every sealed batch commit. It writes only to
`<server-state-dir>/report-artifacts` (the default state directory is
`$XDG_STATE_HOME/ds/server/<lane>`, or `$HOME/.local/state/ds/server/<lane>`)
and reports `queued_for_server_sync`; it does not claim cloud publication.
For a custom Server `--state-dir`, pass the same absolute path as
`--server-state-dir`. The matching native Server sync pump transfers committed
sealed batches on its next run.
(`publication_enqueued: false`). Configuration and room receipts must keep
the same principal, audience, lane and project throughout acquisition. A
saved content digest mismatch refuses before computation. A completed report
is promoted as one directory after every artifact and its receipt are flushed;
interrupted private delivery scratch never becomes a partial final report.

Print context is headless. For every transformer the batch reads the context
layers its selected printing setups declare — the kernel's one decision over
the sealed sheets — from this machine's project rooms (`ds data project-cache
status`): catalogue subsets, building footprints and contours. `--seed`
acquires what is not held first, for the printed transformer's own cluster,
through the same doors `ds data project-cache seed` uses; so a job that always
passes `--seed` acquires on its first run and nothing afterwards. Without
`--seed` a print never reaches a provider: an unheld layer is omitted, named
in the row's `print_context.omitted` and the run receipt, and `context.notes`
says why. A survey or local-layer source has no headless holding and fails
that transformer's row (`print_context_unsupported`); the project's MV models
are omitted as desktop-only. Each `report-run.json` records
`print_context_sha256`, `print_context_layers` and `print_context_omitted`,
and a desktop given the same rooms stages byte-identical context. A media
scope grant is not minted here, so a room carrying photos is refused by the
engine with a typed blocker.

### Multipage drawing collections

Discover `report.bundle` and the reporter's `export_compounded_report` task. Its
optional `pdf_collections` names an output PDF member and ordered existing PDF
members to join. Pages retain their original map, dimensions, title block and
project-wide sheet number. Keep A0 and A3 in separate collections. Source hashes
are checked before the archive is published; a missing or invalid source refuses
the collection rather than silently dropping a drawing.

The governed `report.project.combined` workflow adds one collection per named
PDF layout to the overall archive and each requested grouping slice. Selected
sector exports keep the complete project's original drawing numbers.

The layout schema exposes measured sheet anchors, workbook schedule bindings,
explicit unequal `table.panel_rows`, and flow-connected composition groups.
Composition maximizes the full network's vertical fit. `min_font_scale` is 1 by
default; 0.7 permits up to 30% smaller schedule type. Panel profiles and allowed
font tiers are tested without dropping rows or fields; equal fits prefer larger
type. Fixed titles, scales and furniture belong in the map’s `fit_around` list.

### Native project MV overview

`report.plan-profile --project <id>` renders one revision-pinned DS Grid scene
and plan headlessly. Its A3 profile and plan order, structure labels, scales,
vertical exaggeration, span annotations, 6 m corridor and grid are authored
inputs. Plan and profile use the same horizontal station scale so every plan
structure sits directly below its profile ordinate. `--plan-scale`, when set,
must equal `--horizontal-scale`; angle breaks consume no station distance.
The trace uses rounded station targets and a clear edge inset. At H 1:1500,
it aims for 0+500, 1+000, and so on, then cuts at a nearby structure with
less local terrain relief. That structure closes one page and opens the next;
the profile trace is clipped at the shared station. A short last page is
balanced across the final pair. The manifest records each printed window.
Each default vertical label row holds one entity field: structure number,
structure type, then comments in the advanced format. `--ink monochrome` is
the default; `--ink reference_accents` applies
restrained green OPGW, plum phase and red structure pens inspired by the
approved CJIC 120 ACSR sheets while keeping geographic context grayscale.
`--sample-pages 5` selects representative original sheets and records their
source sheet numbers. With `--model-crs`, the manifest also gives each sheet's
WGS84 route bounds.

`report.project.map-inputs --project <id>` captures the named project's active LV
overview and complete promoted MV geometry through native fenced reads. Add
`--mv-model <absolute.dsgrid>` for one local draft. An authored layout selects
geographic context; `--seed` explicitly acquires missing coverage. For an MV
plan/profile sheet, pass `--focus-bounds west,south,east,north` from that sheet's
`plan_route_bounds_wgs84` manifest entry. The geographic context is then read
around that page's route rather than the full project's bounding rectangle.
Read its omissions before passing the emitted request to `report.layout.render`.
The request is a portable, editable print capture, not a model version.
After visual review, `map.design.attach-print --scope mv` uploads a PDF or PNG
through the existing report-artifact service to `mv_data`, without Desktop or
report computation. Repeat for each format and inspect `design.status` afterward.
Scale-dependent line pens are authored through `scale_weight` in the live layout
schema; legend samples use the identical resolved pens.

### Mechanical print feedback

Layout rendering returns `print_diagnostics`: checked/affected page counts,
bounded findings with element identities and millimetre rectangles, omitted
schedule rows and labels suppressed by collision/frame clipping. Measurements
come from the actual composition, including flowed and unequal-height panels.
Diagnostics are retained with cached SVGs and verified by the cache digest.
Named transformer exports carry these findings in artifact warnings.

These checks cover occupied furniture rectangles, not every cartographic or
engineering judgement. Decorative backgrounds and map frames intentionally
contain other elements and are excluded from furniture-pair collision checks.
No findings is not visual approval. Inspect representative sheets when needed,
not every page as a routine production gate.


`report layout edit` with `op: "sheet_status"` takes verified `assessments` and
scoped `warnings`. It returns `warnings` for sheet/data problems and
`label_notes` for map text omitted by collision, containment, spacing or frame
clipping. Notes retain layer, reason and count without implying missing data.
The shared Rust owner deduplicates repeated output assessments; UI displays its
messages without implementing a separate warning policy.

## The publication queue

A produced report is not finished when the engine stops: its verified bytes
are sealed into the Server's `report-artifacts` root and its row into the
lane's sync store (`store.sqlite` beside it) in one acknowledged step, and it
is published from there. **The sync store is the queue** (owner,
2026-09-20): a `held` row is queued, a `published` row is this machine's copy
of the room's head, a `conflict`/`refused` row lost and its bytes are freed
by the next pass. The directory of sealed batches is bytes, not an index; a
batch sealed by an earlier release with no row is adopted once (`adopted`
receipt). One shared runner drains the queue — the Server's pump, or
`ds report outbox drain` by hand.

```bash
ds report outbox status --output json          # every project queued on this machine
ds report outbox status --project <exact-id>   # one project's queued reports
ds report outbox drain --yes --output json     # publish what is queued, now
ds report outbox drain --project <exact-id> --yes --output json   # sync one project: publish and pull
```

A report publishes on the shared record's one door, `compute_artifacts`
(`open`, the bytes the record asks for, `finalize`), under the signed-in
account's `design.report` capability on the project; the engine build that
produced it travels as provenance and is never an admission question
(owner ruling 2026-09-18, one JWT). The record passes every verdict: a moved
source is a `conflict`, a refused declaration or capability is `refused`, and
both free the bytes in the same pass; anything without a verdict stays held
with its cause.

Naming `--project` on a drain is "Sync now" for that project: the pass runs
even when nothing is queued for it on this machine, reads the record, and
downloads every published head this machine lacks, each output verified by
its SHA-256 before it is placed. That is how a second machine receives what
the first published.

`status` reads the store read-only — no gateway session, no project
selection, no running Server; it needs only the native identity on this
machine to name the store's fence. It answers, for the machine and per
project: how many rows are queued, how many bytes, how long the oldest has
waited (`oldest_age_ms`), the rooms and the distinct hold `reasons` a receipt
left on them, how many published copies stay (`held_batches`), how many lost
rows still hold bytes (`reclaimable_batches`), and whether a live lease is
pumping the project (`pumped`, `leases[]`). `stuck` is the single answer to
"does anything here need me?": queued work older than an hour on a project no
live lease holds. `next` is the one command to run. `bytes_lock` reports the
artifact directory's own writer lock — a seal or a discard waits on it, and
one whose holder is proved gone is released by the next seal or discard
itself; it is not the queue's liveness.

`drain` runs one pass through the same shared runner the Server's background
pump uses. There is deliberately no second pump and no second queue. It is
safe to run twice: a publication carries its own client publish id, so a batch
already in the shared store is recognised instead of published again. A pass
never says nothing: per project it reports `summary` (uploads, downloads,
conflicts, refused, in_sync after the pass), `reclaimed` (batches and bytes
freed for rows that lost), `idle` (why nothing moved, when nothing did) and
the pass's `receipts`. An offline pass changes nothing and says so, and the
queue keeps its work.

## Removing printed artifacts

`ds report artifact remove` removes one exact published print reference through
the native project authority. Name the project with `--project` and supply its scope, transformer, filename, `gs://`
locator and SHA-256 from the current print receipt, then confirm with `--yes`.
A replaced print is refused. The operation retains stored bytes and other
outputs. Archive standalone custom maps through their Assets lifecycle.
