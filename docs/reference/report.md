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

`ds report project scope|compounded|archives` is the other door of this
domain: no local engine, no map, no Desktop. The commands restore the native
user for `--lane stable|canary`, load only its audience-fenced selected project
(`ds auth project use`), and call the governed report service's fixed
contract. ds-brain owns everything that follows — it resolves the exact scope
(every active saved transformer, or the `--transformer` names given), reuses
fresh individual report artifacts, regenerates missing or stale ones with the
cloud reporter, composes the overall and optional per-district combined sets,
streams one ZIP with its manifest and writes a registry row. Retired
transformers (`ds design transformer retire`) are never in scope. The reserved
computed identities — `collisions`, `combined_transformer` and its aliases
(`all_transformers`, `combined_transformers`) — are report output, never
participants: `scope` and `compounded` refuse them locally with
`reserved_transformer_identity` before any credential is restored.

```bash
ds report project scope --output json                      # the plan: who participates, who is excluded and why
ds report project compounded --file-level sector --yes     # publish; blocks until the service answers (≤ 10 min)
ds report project archives --output json                   # the registry, newest first: achieved foldering and short-lived signed downloads
```

`compounded` is `artifact_write` and needs `--yes`: it publishes a durable
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
ds report project settings --output json                                  # what this project produces, and whether it can
ds report project outputs set --selection outputs.json --yes              # save a selection authored as a document
```

`settings` reads the selected project's fresh configuration and hands its
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

A compounded archive consumes the project's applied `report_archive` consumer
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

## Compounded desktop reports

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

`report project compounded` is the contrast: `artifact_write`, because the ZIP
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

`ds report layout save --scope <global|project> --request <file>` takes a `save`, `create` or `update` request carrying `layout` and, for the latter two shapes, `expected_revision`. All three publish the same way: the kernel's setup lifecycle plan (`printing::lifecycle`) decides whether a publish is a create (no revision) or an update (an exact one) — the same decision the Printing setup page takes — so `ds` never sends the compatibility `save` action itself, and the word in the request never overrides the facts. `copy` and `delete` are different transactions and keep their own commands; `save` refuses them by name.

`ds report layout schema` documents all four transactions, `save` included, so a request derived from discovery is one a command accepts. A layout the deployed validator refuses comes back as `print_layout_invalid` naming the offending field, and a validator that cannot answer at all as `print_validator_unavailable` — never as an authentication failure.

## Printable inventory and report plans

`ds report transformers --limit 100 --output json` reads the native user's
selected project and asks `printing::inventory` which rows are printable.
Reserved aggregate, analysis and project-document identities are excluded even
when raw status rows omit their kind. The result reports `more.omitted`.
`cached` and `dirty` are **null**, with `local_rooms_known: false`, because this
native read does not inspect browser rooms. `ds desktop printing transformers`
remains the browser-room IO adapter and asks the same kernel with those facts.
Neither command opens or switches the map.

`ds report plan --action export --request export.json --output json` evaluates
an export request without running it. The file contains `project`,
`transformer`, optional `transformers`, `force`, `report_type`, `selection`,
and the host's `active_project`. A combined or compounded overview requires
that last fact to match `project`; an individual transformer does not.
The GUI asks again after preparation to detect a project switch during IO.
The input is a fact document, not an authorization grant.

`ds report plan --action batch-outcome --request batch.json --output json`
accepts `{"results":[{"transformer":"tx_a","status":"ok"}]}` and returns the
ordered results and `failed` count. Both actions accept at most 4 MiB and refuse
a `command` key inside the document.

Preview admission is available through `ds report layout edit --request
preview.json --output json`, using the kernel's `render_plan` operation:
`{"op":"render_plan","layout":{...},"available_style_refs":[],"layer_count":1,
"text":{"title":"Preview"},"date":"2026-09-11"}`. It returns `ready`, refusal
keys, required print-style references and text. This evaluates admission only;
PDF rendering still uses the existing renderer.

## Headless transformer reports

`ds report project export` produces individual transformer reports — prints
included — with no browser, no room cache and no Desktop. It is the door a
Linux operator calls. Decisions and fingerprints come from
`ds-command-kernel::report_export`; the shared native IO host is
`ds-web/crates/ds-report-host`, and the engine is the installed `ds-report`.
Desktop now uses the kernel's fingerprints, while its existing sidecar and
publication pipeline still perform the desktop IO. Full IO unification is
pending.

```bash
ds report project export --out-dir ./reports --output json                        # every active saved transformer
ds report project export --transformer tx_a --out-dir ./reports --concurrency 2  # named scope, two engines at once
```

The inputs are the governed ones, fetched under the native user for its
audience-fenced selected project: the Network Reporter input receipt ds-brain
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
(`report_batch_failed`). Nothing here publishes: the receipts carry what a
publication needs, but no publication row is enqueued by this command
(`publication_enqueued: false`). Configuration and room receipts must keep
the same principal, audience, lane and project throughout acquisition. A
saved content digest mismatch refuses before computation. A completed report
is promoted as one directory after every artifact and its receipt are flushed;
interrupted private delivery scratch never becomes a partial final report.

What is not headless yet: print context layers (survey, local, catalog,
buildings, contours, project MV) are prepared by the desktop from its caches,
so a server run renders named prints without them and the engine's print
warnings say so; a media scope grant is not minted here, so a room carrying
photos is refused by the engine with a typed blocker.
