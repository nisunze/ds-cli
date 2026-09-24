# `ds design` — reference

The [offline project workspace](design-project.md) owns local transformer
snapshots, version-fenced edits, native background processing, report/PDF
production and pending publication without a paired application.

Tier-4 reference. `ds design <command> --help` is the contract; this document
is the part that does not belong in any command's help because it is true of
all of them.

Governed collaboration serves
`ds-brain/docs/contracts/design-collaboration-roadmap.md`. Offline LV
processing serves the ds-network native batch contract directly.

## Design intake

`ds design intake upload` runs the same Rust-owned upload job as Transformer
Status without an open map or paired Desktop. Repeat `--file` for independent
`.gpkg`, `.zip`, or `.geojson` sources and choose `--mode lv-drafting`,
`sketch-lv`, or `lv-process`. For `lv-process`, `--settings` accepts a bounded
JSON object containing boolean and numeric process settings.

The command freezes the lane's selected project for the job, verifies each
server-issued resumable target against that project, completes the upload
phase before it starts one-file process jobs, and returns one result per file.

```bash
ds design intake upload --file ./T001.zip --mode lv-process \
  --settings ./process-settings.json --yes --output json
```

## Offline LV processing

`ds design lv project-export` is the authenticated, mapless handoff from one
governed transformer snapshot into the local file contract below. It restores
the Firebase user for `--lane stable|canary`, uses only that user's
project named by `--project` (the saved selection is never read), and performs the fixed
`get_transformers_data fields=full` call for one exact transformer. The
gateway rechecks membership. The command refuses legacy context unless the
server supplies both `metadata.version` and `metadata.content_digest`, then
asks ds-network to encode the returned layers as one validated request at an
absent `--out` path.

Use `--project-config` to hydrate current network-config sheets under the same
native identity and project as the transformer. Without that flag `config_dfs`
is empty. Both modes retain explicit owner-default process settings; resolve
the intended process preset before running. The receipt names configuration
inclusion and its SHA-256. There is no `--project`, Desktop descriptor,
arbitrary request field or browser store.

```bash
ds account connect
ds design lv project-export --project <id> --transformer T-1042 \
  --out ./T-1042.fast-lv.json --output json
ds design lv process --input ./T-1042.fast-lv.json \
  --out ./T-1042.fast-lv.result.json --output json
```

`ds design lv process` is the mapless, signed-out native route to the same
Rust engineering kernel used below ds-web's WASM adapter. Its input is one
closed `ds.fast-lv.request/v1` file:

```json
{
  "schema": "ds.fast-lv.request/v1",
  "jobs": [{
    "transformer_name": "Kigali_T1",
    "gdfs": {
      "tr": { "type": "FeatureCollection", "features": [] },
      "lv_lines": { "type": "FeatureCollection", "features": [] },
      "customers": { "type": "FeatureCollection", "features": [] }
    },
    "settings": {},
    "config_dfs": {}
  }]
}
```

The schema has no project id, credential, mutable session, browser table
address, or operation name. It cannot impersonate a project or dispatch a
different engine action. A batch holds 1–32 uniquely named transformers, at
most 64 layers and 64 config sheets per transformer, at most 100,000 input
features in total, and at most 64 MiB of source bytes. Names are at most 120
bytes. Unknown envelope/job fields and unknown process settings are refused.

Independent transformer jobs use native Rayon's process-wide pool and results
retain input order. The complete `ds.fast-lv.result/v1` document is written to
an absent `--out` path; it is never truncated or overwritten. Terminal output
is only a bounded count/digest receipt. This command neither reads nor updates
Desktop, map, IndexedDB, project, or saved transformer state. Importing or
saving the result is a separate governed operation.

`ds design lv project-save` completes that handoff through the normal project
save authority. It accepts the successful export/process JSON receipts and
their exact configured input/result files, validates the selected job through
ds-network, checks fresh source and configuration fences, and saves with server
version comparison. A fresh read verifies each published digest and version.
Keep the operation ID and exact files for retries; re-export and reprocess when
the source or configuration changes. Reports are a subsequent
`report.project.export` operation, not a side effect of network processing.

## Headless feature selection

`ds design features select` is the first native, map-independent design read.
It restores the Firebase user for `--lane stable|canary`, loads only the saved
project context fenced to that UID, canonical email, lane, and credential
audience, then performs the fixed `get_transformers_data` context projection
for one explicit transformer. The gateway remains membership authority. There
is no `--project`, Desktop descriptor, arbitrary URL, body or header.

The returned layers go directly to `ds-geo`'s bounded deterministic selector.
`--layer`, `--where`, `--bbox`, `--id`, `--sample`, and `--ids` retain the
legacy `map design select` spelling, but this command does not open a map.
Counts cover the complete accepted selection; IDs and samples are explicit
bounded projections. `source.version` and `source.content_digest` are returned
only when the server supplied them. `source.state` says `fenced` only when both
exist; otherwise it says `legacy`, and the CLI never fabricates a digest.

Each sampled row keeps `properties` and separately projects the Feature's
authoritative top-level GeoJSON `geometry` in WGS84. Point, LineString, Polygon,
and other GeoJSON values are copied exactly; geometry is neither rebuilt from
properties nor reprojected. Legacy `properties.geometry`, `properties.x`, and
`properties.y` are not fallbacks and their CRS is undeclared unless source
metadata explicitly says otherwise.

`geometryState` is always `included` or `omitted`. An omission names
`geometryOmissionReason` as `missing`, `null`, or `oversize`. One geometry is
included only when its compact JSON representation is at most 64 KiB. The
top-level receipt fields `sample_with_geometry`, `sample_without_geometry`, and
`sample_geometry_oversize_omissions` account for the bounded sample. The
selector's top-level WGS84 `--bbox` remains an extent-overlap filter; no source
Feature bbox is copied or synthesized.

```bash
ds account connect
ds design features select --transformer T-1042 --layer lv_lines \
  --where drafting_status= --sample 5 --ids 100 --output json
```

The older `ds map design select` contract is unchanged for callers already
using a paired application.

## Transformer status rows, without a browser

`ds design status` is the read every other headless Design answer is built
from. It restores the native user for `--lane stable|canary` and reads the
project `--project` names, through the fixed governed status call. There is no
Desktop descriptor, no URL, body or action override, and no fallback: if the
native path cannot answer, the command refuses in words rather than reaching
for the application.

**`--project` is required** (2026-09-18, breaking). This read used to follow
the machine's saved selection, so the same command answered differently on two
terminals and an agent repeating it could not know which project it had asked
about. Omitting it now refuses with `missing_input`; a blank or padded value
refuses `project_required`; anything that is not one path segment refuses
`context_corrupt`. The saved selection is neither read nor changed — use
`ds auth project list` to find the exact id.

```bash
ds design status --project <id> --output json
ds design status --project <id> --transformer TX-1 --transformer TX-2 --output json
ds design status --project <id> --findings --output json
```

Omit `--transformer` for every transformer document in the project; repeat it
for exact names. A named read answers only the names that have a document, so
a shorter list than the request is an answer and not a refusal — use
`ds design transformer inventory` when the question is which names exist. A
project with no transformer document at all, and a named read that matches
none, both answer zero rows for the same reason.

**The rows are what the service sent.** `ds` does not reshape, rename,
normalise or complete them: `process_metadata`, `report_metadata`,
`draft_metadata`, `sketch_metadata`, `layers`, `uploads`, `report_artifacts`
and `retry_capabilities` appear exactly as the project stores them, and a
member the document does not carry is absent rather than defaulted. Fields the
paired application computes for its own display exist only there and are never
synthesised here. What each member means belongs to the reader, so a consumer
sees one shape whether it runs in `ds` or in the application.

### What is wrong with each of them

Every row also carries `health`: one worst-wins `severity`
(`ok|stale|processing|warning|error`) and the ordered `findings` behind it —
each with its phase, its backend code, an i18n `label_key`, the backend's own
bounded message, how many features it counts, how many the backend says it
affects, and, where the offending features are addressable, a `locatable`
directive naming the layer and the attribute filter that isolates them. The
answer also carries `summary`: how many transformers are processing, carry
warnings, and carry errors — a row with both is counted in both, which is how
a refresh has always reported it.

`ds-command-kernel::design_health` decides all of it, and the application
renders the same answer, so an agent reading `ds` and an operator reading the
Status page can no longer be told different things. A finding's `source` is
`document` for what the transformer's own record says — what the severity
badge counts — and `run` for what the last run's response said, which travels
beside it and never moves a verdict.

`--findings` answers the same findings as rows of their own, one per finding
across the whole project, each naming its `transformer`. That is the project's
issue list: what `jq '.data.findings[] | select(.severity=="error")'` prints is
what the Status page's error table shows.

```bash
ds design status --project <id> --findings --output json | jq '.data.summary'
ds design status --project <id> --findings --output json | jq -r '.data.findings[] | "\(.transformer) \(.severity) \(.code)"'
```

Two different bounds apply, and they are different on purpose. A request names
at most 500 transformers — the same bound retirement and Combined Reports
use. An answer carries at most 2,000 rows, which is the bound on one project's
own transformer collection: an unnamed read asks for the whole project, so it
can legitimately answer more rows than any single request could have named.
Above either bound, or past the fixed response size cap, the answer is refused
as `auth_response_unreadable` rather than truncated.

## The project's whole Dashboard, without a browser

`ds design dashboard` folds those same rows once into the model the
application's Design wall renders — the project's progress story, read from a
terminal. Same credential, same fixed status call, same refusals, and the same
REQUIRED `--project`: no Desktop descriptor, no saved selection, no fallback
to a browser. `--tz-offset-minutes` names the reading day the momentum
timeline buckets by, and is echoed in the receipt.

```bash
ds design dashboard --project <id> --output json
ds design dashboard --project <id> --output json | jq '.data.dashboard.health'
```

There is no `--transformer`: every percentage here is measured against the
whole fleet, so a dashboard over a subset would be a different question. There
is one way to process a transformer, so there is no lane to read the project
through either: the phase summaries cover Process, Report and Combined, and a
finding on a Draft/Sketch phase older documents still carry never reaches the
attention pile.

`.data.dashboard` carries, in one object:

| Member | What it answers |
|---|---|
| `total`, `designed_count`, `report_ready_count`, `combined_count` | how far the fleet has come. A transformer is *designed* through ANY compute phase — sketch, draft or process — and a green Process stamp whose saved output layers are missing is not designed at all |
| `pipeline` | the same four counts as stages with `pct_of_total`. Deliberately NOT a funnel: the phases are independent over one record, so a later stage can out-count an earlier one |
| `momentum` | designs per local day (`points`, `key` is `YYYY-MM-DD`), the 7-day split (`last7`, `prev7`, `trend`), `busiest`, `active_days`, `avg_per_active_day`. Days are bucketed at the host's UTC offset; `ds` uses this machine's |
| `crew`, `errors_by_user` | who designed what, and who ran the phases that failed. A bucket with no attributable account carries `unknown: true` and an empty name — the reader names it |
| `districts`, `district_count`, `sector_count` | where the work is. The leftovers chip carries `unassigned: true` and an empty label, for the same reason |
| `phase_summaries` | per phase slot (`process`, `report`, `combined`), how many rows sit in each status |
| `governance` | how many rows are locked and how many are open |
| `attention` | the pile, ranked red → amber → blue then by name. Each note is that row's `design_health` verdict: the same finding the register shows, with the backend's own sanitised `message` |
| `health` | `score` (clean rows over total), its band as `label_key`, and the red/amber/blue counts |
| `recent`, `latest` | the last thing that happened, per row and overall, through the same `design_status_row` ladder `ds design status` reports |
| `facts` | the wall's fun facts, in order, as a key with its parameters |

**Labels are i18n keys, not sentences.** `label_key`, `detail_key` and a
fact's `key` name an entry in the application's catalogue; the only English
that can appear is backend text `design_health` already bounded and sanitised,
in `message`. Timestamps are epoch millis. A reader that wants sentences
resolves the keys; the text renderer here spells only the few it prints.

**A headless client holds no browser rooms and no live process diagnostics.**
The application folds in the verdicts of runs that have just finished and are
not in the documents yet; `ds` has none, so `diagnostics` is empty and the
model is exactly what the project's documents say. Nothing this machine holds
unsaved can appear here, because a headless client holds nothing unsaved.

## Design Activities across projects

`ds design activities` answers the one Design question that spans projects:
*who has been designing, where, and what changed.* It is two commands — one
that takes photographs, one that reads them.

### What the answer is actually made of

There is **no design event log**. ds-brain audits membership, roles, lifecycle
and assets; it audits no design work, and `/api/v1/user-activity` is a stub.
What a status row carries is the LATEST stamp per phase per transformer. So
this family retains successive captures and diffs them, and those captures are
the entire history.

Read this before you read anything the commands print:

- **Nothing between two captures is visible.** If a transformer was drafted and
  redrafted between Monday's sweep and Tuesday's, Tuesday shows one stamp.
  Nothing is interpolated, inferred or synthesised to fill the gap; a gap is
  rendered as a gap.
- **No device, no installation.** Status rows carry none. If someone used
  another account's credentials, every row they touched reads as that account.
  Device evidence lives in Desktop installations (`app_installs`) and
  `ds auth device list` — not here.
- **Actions that never stamp a row are invisible**: reads, downloads, exports.
- **Only projects this account is a member of.** ds-brain fences `/report` by
  membership. A project that refuses the read is recorded as a refusal, never
  as an absence.
Every reply carries that list as `not_claimed`, because the reader who most
needs it is the one who did not go looking for it.

### Taking a capture

```bash
ds design activities sweep --limit 3 --yes --output json
ds design activities sweep --project <id> --yes --output json
ds design activities sweep --bucket all --limit 10 --max-age 60 --yes --output json
```

A sweep lists the projects this account can reach, asks the kernel which are
worth re-capturing and in which order, then walks that order **one project at a
time**, pausing `--pause-ms` (2 000 by default) between them. That etiquette is
not politeness: ds-brain runs ONE full status scan per instance at a time, so a
parallel sweep would simply queue behind itself and make everyone else's Design
page wait. `--limit` defaults to 25 for the same reason; raise it deliberately.

Per project the sweep reads the rows once, folds them into a ledger and into
the project's own Dashboard model through the shared kernel, and retains one
envelope. A project that refuses is a row with `outcome: "refused"`, its code
and its reason — the sweep never aborts. A project the `--limit` did not reach
appears in `plan.skip` with `reason: "limit"` rather than vanishing.

`--yes` is required: a capture is a durable artifact of record, and every one
of them spends a shared scan slot.

**Nothing is scheduled.** There is no timer, unit file, cron entry or
background refresh in this family, and there is not meant to be one. A capture
exists because a person or an agent asked for it.

### Where a capture lives

`--state-dir` (absolute) or, by default, `$XDG_STATE_HOME/ds` — falling back to
`$HOME/.local/state/ds`. Never `DS_CONFIG_HOME`: that is the credentials
namespace and a cache of governed reads does not belong in it. Under the root
the shared kernel owns the path:

```
<state-dir>/design-activities/<lane>/<account-digest>/<ds_project>/<13-digit-ms>.json
```

The account segment is the first 32 hex characters of
`sha256(uid + "\n" + credential-audience-digest)`, so two accounts on one
machine never read each other's captures and no path ever names a person. Two
lanes are two stores: a canary capture never answers for stable. Files are
written owner-only, staged and renamed, so a killed sweep leaves no half
capture. Retention keeps the newest 30 captures whole plus the newest capture
of each UTC day within 90 days, and the newest is never dropped.

### Reading them

```bash
ds design activities read --output json
ds design activities read --user someone@example.com --output json
ds design activities read --bucket all --limit 200 --output json
ds design activities read --since 1758000000000 --output json
```

`read` opens no socket. It restores the account from protected state, selects
the newest capture of each project and the one before it, and makes ONE kernel
fold call. `--since` moves the comparison point: the capture diffed against is
the newest one taken at or before that instant, so the answer covers everything
from then to now instead of only the last step. `--user` narrows `users`,
`changes`, `timeline` and `anomalies` to one actor and echoes
`filtered_by_user`; the totals deliberately stay project-wide, because "four of
the project's ninety" is the sentence worth reading. `sources` names the two
captures used per project, and `store_empty` is the refusal when nothing has
been swept yet.

A capture the kernel will not admit — a file half written, left by an older
kernel, or edited since — does not abort the read. It is named in
`unreadable` with the project, the capture time and the kernel's own reason,
and the project's other capture still answers; losing twenty-nine honest
projects to one damaged file is exactly the silence this command exists to
avoid. When nothing admissible is left at all the refusal is
`snapshot_invalid` naming each one, not `store_empty`, because "sweep again"
is the wrong remedy for a file that is already there.

The answer's `anomalies` are honest ones only: an actor with no name at all
(`unknown_actor`), and a capture that is now old (`stale_capture`). Nothing here accuses anyone of anything; it reports what two
photographs said.

## Local transformer rooms for background work

`ds design transformer download` was retired on 2026-09-20. It warmed the
paired application's private room cache (its browser store) for a report that
the native report path no longer needs: `ds report project …` reads rooms
from the service under the native credential and publishes the result as a
project artefact. Nothing headless has a window cache to warm, and a second
cache the CLI would own was refused deliberately (contract
`dsgrid-authority/01-server-required.md`, decision 14). The same decision
retired `ds design sync status|cancel|resume`, which inspected the window's
own reconciliation queue; the kernel sync store the Server and the desktop
share is the only queue, and `ds report outbox status|drain` is its surface.

## Reversible transformer retirement

`ds design transformer inventory|retire|restore` is the map-independent
lifecycle of a project's transformer documents. Like `features select`, the
family restores the native user for `--lane stable|canary` and acts on the
project `--project` names (the saved selection is never read), through the
fixed governed report contract. There is no Desktop descriptor, URL, body or
action override.

**Retirement is not deletion.** Deleting a transformer (`ds map design delete`,
paired) archives and destroys its artifacts. Retiring it flips the one
soft-delete bit every consumer already honours and records who, when and why:
the transformer leaves Transformer Status, every listing, combined and
Combined Reports, design tile runs and layer counts, while its document,
layers, versions, attachments, uploads and cached artifacts stay exactly where
they are. `restore` clears the bit and keeps the record as history. ds-brain
decides per name — membership and project lifecycle, `design.edit` or
`transformer.delete`, the edit lease, and creator ownership for non-admins —
and answers every name in order; one refusal never cancels the others.

`inventory` is the plan. Without names it lists every transformer document
with its lifecycle state; with names it answers exactly those:

| State | Meaning | Next |
|---|---|---|
| `active` | live; consumers include it | `retire` |
| `retired` | tombstoned with a retirement record | `restore` |
| `deleted` | tombstoned by another path, no record | not restorable here |
| `missing` | no such document | check the name |

```bash
ds design transformer inventory --project <exact-id> --transformer TX-1 --transformer TX-2 --output json
ds design transformer retire --project <exact-id> --transformer TX-1 --reason "superseded by the 2026 survey" --yes
ds design transformer restore --project <exact-id> --transformer TX-1 --yes
```

The receipt of a write names each transformer with `applied` and a timestamp,
or a closed `refusal`: `not_found`, `already_retired`, `not_retired`,
`no_retirement_record`, `special_document` (`mv_data`, the combined row and
collision docs are never retired), `governance_locked`, `not_owner`, `failed`.
Contract: ds-brain `docs/contracts/transformer-retirement.md`.

## Where design collaboration is

Governed collaboration is not on disk and is not reachable with a credential
this process holds. The offline LV file contract above is deliberately
separate from that authority boundary.

Saved selections, attachments, tags and comment threads are governed project
state behind ds-brain, which is the only gateway and the only authority: it
decides who may write, it arbitrates two people editing the same record in the
same second, and it refuses a write authored against a version that has since
moved. Attachment commands run headlessly under native authorization for the explicit
`--project`. LV revision bindings use governance `vN`; MV bindings use the exact
content `source_revision`, never the governance ordinal.

Since 2026-09-20 tags, groups, consumer grouping, comments, known columns and
material propagation run headless too: each command is one closed kernel door
(`ds_client_core::design_annotations`, `known_columns`, and the
material-propagation report action) that `ds auth` runs under the restored
native user against the project named by `--project` (the saved selection
is never read) for `--lane stable|canary`. `ds` sends a request and receives an outcome. It never holds
a window, never receives a credential it did not mint, and answers the same
on the Server and on the desktop. A caller who learned `--desktop-descriptor`
from an older release is refused by name, `requires_window_retired`, before
any credential is consulted.

**Saved selections** `ds design selection list|read|save|archive|assign`
reach ds-brain the same way. Nothing about the authority changed: ds-brain
still evaluates membership, still owns the version, and still refuses a write
authored against a version that has moved. Two rules follow, and both are
enforced rather than documented: a version is READ by a read this process
performed, never asserted from a flag; and the member digest `assign` echoes
is the one `read` returned, never recomputed here. A selection drawn by lasso
on a rendered map is a different thing and stays with the paired application
as `ds map design select`.

Version and attachment commands require explicit `--project` and native
authorization independently of Desktop and the Web active project; since
2026-09-18 `ds design status` and `ds design dashboard` do too. Tag, group,
comment, known-columns and materials commands, saved selections, the headless
feature reads and LV export use the audience-fenced selected context
(`ds auth project use`).

Idempotency keys for the writes that carry one (a comment post, a thread
promotion, a group or consumer-grouping apply) are minted here from the
device's own random source, never from a window, so a retried write is the
same write to the service.

## Why this is not `ds map`

No command here needs a map instance, an edit session, or an open design room:
local LV processing consumes an explicit file; a selection is a list of stable
identities; an attachment is bytes with a media type; a tag is a value from the
project's own vocabulary. `ds map` owns local map state; this domain owns none.

## The shape of a session

```bash
ds design selection list                                  # what is saved
ds design selection read --selection sel-week-32          # who is in it, right now
ds design selection assign --selection sel-week-32 \
  --title "Review LV designs" --owner nixon@example.com --yes

ds design attachment list --project <project-id> --kind mv_model --object mv_line_a
ds design attachment publish --project <project-id> --kind mv_model --object mv_line_a \
  --path ./MV_LINE_A.bak --version rev_2 --yes

ds design tag list --project <id> --kind lv_transformer --object kigali_a
ds design tag set --project <id> --kind lv_transformer --object kigali_a \
  --definition transformer_scope --values additional_scope --yes

# Typed definitions and values retain their numeric/text identity.
ds design tag define --project <id> --definition completion --name "Completion percent" \
  --value-type number --min 0 --max 100 --yes
ds design tag set --project <id> --kind lv_transformer --object kigali_a \
  --definition completion --number 82.5 --yes

# `know_columns` is the exact external property authority.
ds design known-columns list --project <id>
ds design known-columns set --project <id> --layer mv_lines --field tag_city \
  --visibility published --yes
ds design known-columns set --project <id> --layer mv_lines --field tag_internal_review \
  --visibility hidden --yes

# Project-wide typed filters never require an open map.
ds design tag query --project <id> --choice city:any_of:huye,kigali --output json
ds design tag query --project <id> --choice phasing:equals:phase-1 \
  --number completion:gte:80 --output json

ds design group list --project <id> --transformers kigali_a,kigali_b            # allowed values
ds design group preview --project <id> --group city --transformers kigali_a,kigali_b \
  --value kigali --output json                                   # plan + digest
ds design group apply --project <id> --group city --transformers kigali_a,kigali_b \
  --value kigali --digest <plan-digest> --yes
ds design group export --project <id> --transformers kigali_a,kigali_b \
  --output json | jq -r .data.document > tags.json               # for a report

ds design comment list --project <id> --kind lv_transformer --object kigali_a
ds design comment post --project <id> --thread thread-clearance --body "Agreed, re-spot it." --yes
```

## Batch-editing eligible tag definitions

`ds design group list` discovers active, single-valued choice definitions that
apply to LV transformers. Any returned definition id may use the bounded batch;
City and Phase are ordinary project-authored examples, not reserved branches.
The generic `ds design tag set` remains valid for one-object edits.

**A value is matched, never corrected.** Definition save trims outer whitespace
and otherwise preserves authored choice bytes and case. Two vocabulary values
that differ only by case are refused rather than collapsed. Assignments and
groups either match an allowed value byte for byte or are refused. `Phase 1` is
not `phase 1`; a `value_case_mismatch` refusal names the stored spelling. Read
`allowed` from `ds design group list` rather than guessing. Successful tag and
group write receipts echo the exact stored values.

**Preview is not optional.** `preview` returns one explicit outcome per named
transformer plus a `digest`, and `apply`/`unassign` must echo that digest back.
The server recomputes it, so a batch approved against one state cannot land
against another. `ds` carries the digest and never mints one. `preview` writes
nothing, so it keeps working on a project that accepts no changes.

**Model evidence is explicit.** If a returned plan carries model state, report
it and its outstanding rows exactly. The CLI never infers a model requirement
from a definition id or fabricates a receipt.

### `ds design group export` — the explicit projection a report pins

`export` publishes the read-only `ds-report.design-tags/v2` document for an
explicitly named transformer set and an explicit ordered `--definition-ids`
selection. Omitting the selection deliberately requests one untagged group; it
never means “find the city tag.”

Two things about it are load-bearing:

- **Write `.data.document` verbatim.** The `sha256` beside it is over those
  exact bytes, and it is what a report request pins. Parsing and
  re-serializing produces different bytes for the same facts, and the pin
  stops matching.
- **It refuses rather than guesses.** Missing, archived, duplicate, or
  inapplicable selected definitions fail by id. A similarly named definition
  is never substituted.

Values the closed document shape cannot carry — one under an archived
definition, or a cleared assignment — come back in `excluded` with the reason,
so nothing is dropped in silence.

## Feeder cable limits

`ds design categories read --project <id> --kind customer` or `--kind meter` reads fresh
canonical names, aliases and catalog metadata with bounded pagination.
Use the `ds-dirty-categories` skill when a report flags unknown values:
missing or newly introduced categories generally require a seed correction;
code should change only when it fails to honor a valid seed.

`ds design meter-types ensure --project <id> --name Readyboard --yes` preserves Readyboard
as its own meter category alongside Single Phase and Three Phase.
`ds design customer-categories alias --project <id> --alias Productive --category Commercial --yes`
seeds an explicit source-label mapping while preserving Commercial's demand
settings. Both commands use the project named by `--project`, retain unrelated
catalog rows, and verify fresh saved configuration without Desktop.

### What the catalog does to a report, and how to repair it

Reporting validates every grouped customer and meter description against the
catalog's canonical names, and a description that is not among them is rewritten
to a fallback rather than left out. The row totals a report prints therefore
depend on three catalog facts the rows themselves do not show, and
`ds design categories read` returns all three under `hazards`:

- `fallback` — the category unrecognised values are counted as, and
  `decided_by`: `project_setting` when `default_category` is stored, otherwise
  `catalog_order`, meaning nothing chose it but the seeding order.
- `ambiguous_aliases` — source labels two canonical categories both claim.
  A contested label is dropped from the alias map entirely, so the customers
  spelled that way stop resolving and land in the fallback.
- `unnamed_rows` and `scopes` — rows with no canonical name, and the
  client/country mixture a merged seed leaves in a single-country project.

Seeding alone cannot repair a catalog that is already wrong, so four verbs
order, rename and remove what is already there. None of them creates a row;
`ensure` and `alias` remain the only way to add one.

`ds design customer-categories retire --project <id> --name Pauvre --yes` drops one canonical
category — a second client's vocabulary, a duplicate — so reporting stops
validating against it. It refuses while that category is the fallback only
because it is first in the catalog; store the choice first with
`ds design config set --project <id> --sheet project_settings --parameter default_category
--value "<category>" --yes`, then retire.

`ds design customer-categories retire-unnamed --project <id> --yes` drops the blank rows a
seeding pass left behind. They group nothing and validate nothing, but they are
still listed wherever the catalog is offered.

`ds design customer-categories rename --project <id> --from Education_I --to "Primary School"
--yes` changes the name reports total under, keeping the demand settings and
every source label — including the old name, which stored customer records
still carry. It refuses a name another category already claims.

`ds design customer-categories unbind --project <id> --alias "Ecole Primaire" --category School
--yes` resolves a contested label by naming the category that loses it; follow
it with `alias` to bind the label to the intended owner.

`ds design meter-types default --project <id> --name "Three Phase" --yes` moves a meter type to
the front of the catalog. Reporting has no governed setting for the phase-type
fallback — it reads the first named row — so until it has one, this is how a
project states that choice on purpose.

`ds design feeder-limits read` reads the named project's feeder
bounds, LV cable bounds and transformer cable catalog from fresh configuration,
without Desktop. Optional `--out` retains this configuration at a new JSON path.
`ds design feeder-limits set --project <id> --minimum 25 --maximum 95 --yes` updates only
`minimum_feeder_cable_size` and `max_feeder_cable_size` (mm²), preserves other
project settings, and verifies the saved values with a fresh read. The report
matches transformer kVA → outgoing LV bundle → feeder catalog designation,
then applies these limits within that bundle's available feeder choices.
For example, a 70 mm² ABC bundle can use a 70 or 95 mm² feeder according to
transformer size. This does not assign all transformer current to one feeder
or resize design geometry. The minimum LV cable also uses its catalog match,
not automatically the minimum feeder: 50 mm² ABC maps to a 35 mm² feeder even
when the feeder minimum is 25 mm².

## Internal properties versus external publication

`tag_<definition_id>` is an ordinary property in Properties and Attribute
Table. The model preserves it whether or not an external consumer needs it.
`ds design known-columns set` edits the project's `know_columns` authority one
layer/field pair at a time; it does not edit or clear any feature value.

`published` allows that property on later report, GIS and design-tile
materializations. `hidden` removes the permission. An unlisted tag remains
internal by default. `set` reads the policy revision first, and ds-brain
commits the one-field change, derived-output invalidation and audit row
together. If another editor moves the revision, the write is refused and must
be retried from a fresh `known-columns list`.

The route is `GET|PATCH /config/{project}/known-columns` on ds-brain. On
2026-09-20 neither lane's API Gateway publishes it (`ds-apis-tf`
`api_ds_system.tf` publishes `/config/{eds_project_id}` GET only), so both
commands answer `design_route_unavailable` there, with the publication as the
remedy; the browser's known-columns feature is equally unreachable through
the gateway and only works on a local dev stack. This is a finding, not a
CLI defect.

## Typed tag definitions and Transformer Status queries

Definitions are not all vocabularies. `choice` owns an ordered `--values`
list and may use a radio, dropdown or multiselect according to cardinality.
`text`, `integer` and `number` are single-valued; they use `--text`,
`--integer` or `--number` when assigned, and constraints such as `--min`,
`--max` or `--max-length` when defined. The typed assignment is carried to the
owner as `typed_values`; the legacy string projection remains in read results
for old choice callers and report compatibility.

`ds design tag query` is the bounded, mapless Transformer Status filter. Each
repeated predicate names its type in the flag rather than asking the server to
infer it:

```bash
--presence inspection:exists
--choice city:any_of:huye,kigali
--text survey_note:contains:access
--integer revision:gte:3
--number completion:gte:80
```

**A read matches the same way a write does.** A `--choice` predicate is matched
against the definition's stored vocabulary byte for byte, and a value that is
not in it is refused: `tag_value_case_mismatch` names the stored spelling when
only case differs, `tag_value_not_in_vocabulary` lists the allowed values when
the value was never authored. `--choice phasing:equals:initial` against a
vocabulary storing `Initial` therefore refuses instead of reporting zero
transformers — and `not_equals` refuses instead of reporting all of them.
Definitions the project does not carry stay the server's refusal to make.

### Which control a definition may use

One matrix, and it is the shared kernel's — the same answer the application's
tag editor offers controls from and the same one the governed owner admits by:

| Value type | Cardinality | Controls, default first |
|---|---|---|
| `choice` | `single` | `dropdown`, `radio` |
| `choice` | `multiple` | `multiselect` |
| `text` | `single` | `text` |
| `integer`, `number` | `single` | `number` |

Nothing else is admissible. `text`, `integer` and `number` are single-valued,
so `--cardinality multiple` with one of them is refused rather than sent. A
`choice` carries 1 to 100 values and no numeric or length constraint — its
vocabulary IS its bound; a `text` definition carries `--max-length` (1 to 500,
defaulting to 500) and no numeric bounds; `integer` and `number` carry `--min`
and `--max` and no length, and an `integer` bound must itself be whole. Every
one of these is refused here, before the request leaves, under
`invalid_tag_input` with the offending member named in `error.details.field`.

Until 2026-09-11 this door checked `--input-control` against a flat list of the
five spellings and enforced no compatibility at all, so a combination like
`--value-type text --input-control number` was composed, sent and refused by
the owner after a round trip. That is now a local refusal.

## This project's collisions

```bash
ds design collisions --output json
```

A collision means two or more transformers claim overlapping ground or a
clashing identity, and a Combined Report cannot be produced while one
stands — so this is the read to make before `ds report project combined`
fails.

It reads the project-wide collisions document the report owner writes. The
count is taken in a fixed precedence: the document's own layer count, then the
terminal detection outcome's pair count, then that outcome's compatibility
twin. The order matters because a run that finds NOTHING deliberately writes no
layer, so the outcome is an equal-authority source and not a fallback — a
project with zero collisions reports `0`, not "unknown". A malformed count
stays `null`: nothing here turns text into a number the owner never wrote.

`checked` says whether the project has ever been checked at all, which is a
different state from a check that found nothing. `state` is the key an operator
reads the answer under, `pairs` is the count when one is recorded, and
`transformers` is how many ordinary transformers a detection run would cover.

This command starts nothing and writes nothing. Detection itself is a separate
governed action.

Use `--match all` (the default) or `--match any`. One call accepts at most 20
predicates and scans at most 2,000 current LV transformers. `--limit` is not a
page: if the complete match set is larger, the server refuses and asks for a
larger explicit bound rather than returning a selection that only looks
complete. A saved selection can then pin the returned object ids.

## The three rules the whole domain rests on

### 1. A read never substitutes

`ds design selection read` evaluates membership server-side and reports every
member as `present`, `changed` or `missing` — under the label it was saved
with. Nothing is swapped in for a member that has gone. Because a transformer
rename mints a NEW document identity and retires the old one, a renamed member
reads as missing under its old name. That is the honest answer, and it is the
one a person needs in order to go and find where it went.

### 2. What is assigned is pinned

`ds design selection assign` re-evaluates membership, refuses if it moved since
the read, and writes an immutable receipt carrying the selection's version, its
member digest and the exact transformer ids that resolved at that moment. The
task carries a link to the selection, never a copy of the transformer data — so
editing the selection afterwards cannot change what somebody was asked to do.

### 3. Nothing overwrites

An attachment revision is immutable: each one owns its own storage object, its
own server-verified SHA-256 and its own generation, so publishing a new `.bak`
for a later model version sits alongside the earlier one. A comment is
append-only; there is no `ds design comment edit`, because there is no such
server action. Retiring an attachment or archiving a selection is soft and
reversible, and `--restore` brings it back.

## Bounds, and how they are reported

Every list is bounded and every bound is reported. `--limit` is a page (1–200)
and the matched `total` always comes back, so a short page says so rather than
ending quietly. An attachment whose revisions exceed the server's page reports
`more: true` on that attachment rather than looking complete.

A comma-separated list flag is bounded locally as well as on the server, so an
over-long `--transformers` or `--values` is refused before a round trip that
would have been rejected anyway.

Two of those bounds differ on purpose, and the difference is not read versus
write. The 200 is one Firestore transaction's write budget on the server, so
`ds design group preview`, `apply` and `unassign` take at most 200
transformers. `ds design group list` takes 200 as well — a read has no writes
to budget, but ds-brain bounds a group listing by that same constant, so
carrying it locally refuses in one place instead of one round trip later.

`ds design group export` takes 2,000, because it is the read whose unit is a
whole project: a live project already carries 202 transformers, and splitting
one export would produce two documents with two digests that a report pins
separately and will not join. So a listing over its bound is refused with the
whole-project reads named — `ds design group export`, or `ds design tag query`
for the same project by predicate — rather than with "pass fewer values".

## Where `ds` deliberately stops

**Publishing a large attachment.** The native client reads the named path with
a 512 MiB bound and uploads its exact opaque bytes through the server-granted
Storage URL before finalization. Larger files are refused with a bounded-file
remedy; no revision is finalized from truncated bytes. Desktop pairing is not
required.

**Redacting a comment.** Redaction is a moderator's audited action that clears
text the server does not retain. It stays in the application, where the
moderator can read what they are about to remove before they remove it.

**Promoting a tag definition to a global template.** That is the one design
action that leaves the project boundary, and it carries its own capability. It
belongs to the governance surface, not to a headless command.

## Refusal codes

| Code | What it means |
|---|---|
| `design_not_permitted` | the signed-in user may read but not change these records |
| `design_version_conflict` | the record moved while the command was in flight; re-read and retry |
| `design_project_read_only` | the project is archived or expired and accepts no changes |
| `design_request_invalid` | ds-brain refused the request's own shape, or a bound it exceeded |
| `design_record_not_found` | the named definition, selection, attachment or thread does not exist |
| `design_service_failed` | the design collaboration service faulted; the request itself is sound |
| `backend_unreachable` | the Data Solutions API did not answer the request the command needs |
| `invalid_design_anchor` | the anchor names a reserved document or a kind that does not exist |
| `attachment_too_large` | the file exceeds the bounded path reader |
| `design_route_unavailable` | this lane's API Gateway does not publish the known-columns route |
| `requires_window_retired` | `--desktop-descriptor` was given to a command that runs headless now |
| `invalid_value_list` | a comma-separated flag was given but carries no values |
| `too_many_values` | a list flag carries more entries than the record accepts |
| `missing_comment_target` | neither `--thread` nor a complete `--kind`/`--object`/`--title` |
| `unknown_tag_group` | `--group` is missing or does not identify a batchable project definition |
| `design_plan_stale` | the project moved after the plan was previewed; preview again |
| `invalid_transformer_scope` | no transformer named, or a name is blank, repeated or over 200 characters, or over 500 named |
| `invalid_reason` | `--reason` is blank, untrimmed or over 512 characters |
| `design_plan_invalid` | the shared kernel refused the assembled preview: an unknown verb or format, or more rows than the register holds |

Those four are what `ds auth` types from the route's HTTP status, and the
class is what a caller acts on: an exceeded bound and an
unknown record are `invalid_input`, so the identical call can never succeed,
while a service fault and an unreachable API are `unavailable` and the same
call works once the service answers. A write that never answered may still
have been applied, so `backend_unreachable` asks for a re-read before a retry
rather than a blind repeat.

The headless `transformer` family adds the native-profile, headless-session
and `auth_*` codes `ds tile --help` documents once; `auth_rejected` there also
covers an archived or expired project and a missing capability.

No command in this domain pairs with a window any more, so none of the
pairing refusals (`desktop_not_paired`, `desktop_unreachable`, …) can be
answered here; `ds map --help` documents them for the commands that still
need a rendered map.

## Status row truth

Every `ds design status` row also carries the shared kernel's truth for it
(`design_status_row`, the same module the Transformers register reads):
`view` (saved/unsaved and why, locality, lane, combined and report state,
version, presence, whose room is here), `phase_ownership` (which run owns the
record when Draft/Sketch and Process both stamped it; the superseded run's
retained error is a `legacy_phase_superseded` finding in `health`),
`latest_action` (a label key, when, who, and the rank that decided a tie),
`governance` (the label over ds-brain's verdict: open, locked, blocked,
unknown), `retry` (whether a Process retry would be refused, from what source,
and why — ds-brain's published verdict is authoritative when present), `kind`
(individual, aggregate, project_document, analysis) and `allows` (combinable,
versionable, reportable, deletable). A headless client holds no browser rooms,
so every row is remote and nothing is dirty here by construction.

## Status query — the register's selector, headless

`ds design status --project <id> --search <text> --sort <key> [--desc] --filter <dimension=value>…`
answers the Transformers register's own question with no browser: which rows
the operator would see and in what order, decided by
`ds-command-kernel::design_status_query` — the same selector the register and
the transformer picker read. A quoted `--search` ("tx_12") is an exact name;
a bare one is a substring. `--sort` takes `name`, `district`, `tags`,
`process`, `report`, `combined`, `governance`, `user`, `updated` or
`version`; special rows (`combined_transformer`, `mv_data`, `collisions`,
`compounded_report`) lead in their fixed order, except that `mv_data` joins
an `updated` sort. `--filter` repeats: `sync=saved|unsaved`,
`process|report=<phase status>`, `combined=<state>`,
`governance=locked|draft`, `user=<account>`, `warning-type=<code>` and one
`admin-<district|sector|cell|village>=<value>` (`__admin_bounds_none__`
selects rows with no bounds at that level). Any secondary filter hides the
special rows, as the register does. There is one way to process a
transformer, so there is no lane dimension and no legacy phase to sort or
filter by: a Draft/Sketch stamp an older document still carries is data
provenance the row reports (`phase_ownership`), never a selector. The answer's
`transformers` are the selection in order, `count` its size, and `query`
carries the selector, the sort and `options` — what each filter may offer
over this project (`admin`, `process`, `report`, `combined`, `governance`,
`user`, `warning_types` with counts, `sync`). A headless client holds no
browser session, pins, tags or saved selection, so those members of the
selector are empty here; `ds` answers what the register would show a fresh
browser.

## Previews: what a batch, a download or an overwrite would do

Five reads answer questions the register used to keep to itself. Each reads
the same `list_transformers_status` rows `ds design status` reads, folds them
once through the shared kernel, and performs nothing.

`ds design bulk plan --action <verb> [--transformer <name>…]
[--capability <name>…] [--combined-mirror]` previews one batch verb:
`add_to_combined`, `combined_and_export`, `generate_reports`, `retry_process`,
`save`, `delete` or `version`. The answer's `targets` are the rows the verb
would actually dispatch, each with `fresh` — true when the work is already
current, so running it would re-do finished work — and `fresh`/`stale` are
their counts, the same denominators the Status page draws beside its buttons.
`skipped` names every other row with a reason: `not_selected`,
`not_combinable`, `not_versionable`, `not_deletable`, `special_row`,
`retry_refused`, `already_saved`, `combined_mirror_disabled` or
`capability_missing`. A row carrying unsaved local work counts as stale for
every published-state verb, because whatever the server calls current is
current about something else. `--capability` is what this operator holds;
without the one a verb is gated on, `unavailable` refuses the whole verb by
name rather than silently returning nothing. Naming no transformer previews an
empty tick set, which is what the page shows before a tick. ds-brain re-reads
its own state before dispatching, so this is a preview, never the authority.

`ds design download plan [--transformer <name>…] [--format xlsx|shp|kmz|gpkg]…`
previews a download. `scope` is the rows in it — naming transformers narrows
it, naming none takes the project, and reserved rows are never in it, which
`scope_precedence` states. `urls` is every artifact those rows deliver, in
order; `filtered_urls` is the same list under the format flags. `summary`
counts them `fresh` / `stale` / `missing`, with `stale_with_files` and
`cached` for the rows whose artifacts are already listed. `placement` is the
rule worth knowing: when one artifact NAME appears twice for a row — a cloud
pointer and a copy already on disk — the LOCAL copy wins, whatever order they
arrived in, and among two copies of the same kind the later one wins. Each
entry names the row, the artifact, the copy it resolved to and `why`
(`only`, `local_wins`, `later_wins`). Each row's `source_files` is the normalized
draft/sketch/process input inventory and its `source_entries` gives safe ZIP
member names by action. The plan-level `source_entries` is the selected scope's
flat download list. `source_uploads` counts those inputs and how many have a
URL. The fetch itself stays with the caller.

`ds design version status --project <id> --kind <kind> --object <id>` reads
that object's exact server head. The LV `--transformer <name>` spelling remains
supported. For MV, source_revision identifies the immutable content, while
published_version is the governance ordinal; manifest_model_revision is the
native package's separate nonnegative lineage counter. Status never inspects
an unsaved room or guesses whether its local contents need publication.

`ds design conflict list` and `ds design conflict check --transformer <name>`
answer overwrite admissibility. `list` applies the kernel's detection rule —
a room this browser holds, dirty and server-known, whose save counter moved
past the base it was taken from — and `check` runs the ordered preflight,
naming the FIRST refusal rather than a bare no: `force_capability_missing`,
`overwrite_not_selected`, `no_conflict_recorded`, `review_missing`,
`local_copy_changed`, `local_base_version_changed`, `review_not_pinned`,
`conflict_base_version_mismatch`, `cloud_head_moved` — and, for the tick
alone, `review_not_finished`. `eligible` is whether the overwrite may be SENT;
`tick_admissible` whether the box may be TICKED. Both come from one
evaluation, so they cannot disagree.

`ds design presence status` reports the lease pass: which rooms should hold a
server lease, which should release, which waited, and `bounds` — the hold
refresh window, the draft interval, and the per-pass lock-call cap that the
hold and release loops SHARE, so a pass that spends the cap on holds defers
every release to the next one.

A conflict, a comparison and a lease are all facts about a working copy. A
headless client holds none, so `conflict list`, `conflict check` and
`presence status` stamp `room_state: "unknown"` and answer from what the rows
themselves carry — never a confident zero, and never an invented room. Run
them where the rooms are and the same kernel answers over real ones.

## Process settings, resolved headlessly

`ds design process settings --preset drafting|sketch
[--project-config config.json] [--operator toggles.json]
[--firestore-design-data]` answers what the LV process dialog sends for that
preset, from the same kernel module (`process_settings`) over the engine's
own processor catalogue: `settings` (the preset applied to the
catalogue defaults and the project's own `transformer_settings` rows, the
dependency collapse applied, the operator's toggles laid over), `visible_groups`
(what the dialog would show), `wire_settings` (what the run receives — hidden
keys forced or omitted, geometry property calculation pinned on, the Sketch
customer-connection contract stamped) and `dropped` (every key that left the
wire or was forced, with its reason). Without `--project-config` the catalogue
alone resolves; pass the project's configuration document to include its
tolerances and preset rows. The dependency collapse follows the engine's five
property-keep keys: `keep_flying_stay` alone never preserved computed
properties on the engine side, and no longer pretends to here.

## AutoProcess, planned without running it

`ds design autoprocess plan --changes edits.json [--now-ms 1789…]` answers the
four admission questions AutoProcess asks in the browser, from the same kernel
module (`autoprocess`). The document holds up to four sections and the answer
carries the ones it found:

- `mode` — `{process_active, auto_process_enabled}` → `auto` when the process
  action is available in this editing context and AutoProcess is on, else
  `manual`; every host presents the same process action.
- `trigger` — `{reason, changed_fields[], vocabulary{lockable_cells[],
  status_fields[]}}` → does this committed edit warrant re-running the LV
  network. The two attribute vocabularies are ds-network's and are passed in;
  the kernel owns only the rule that unions them with the sizing and topology
  extras (`plan_kva`, `ex_tr_size`, the connection identity fields).
- `differential_scope` — `{differential_enabled, is_mv_session,
  accumulator_bound, force_full, change_count, blocking_diagnostics?,
  mapping?}` → `feeders` or `full`, and which of the seven fallbacks answered.
  The two expensive host walks are optional: leave them out and the kernel
  either decides on a cheaper branch or answers `pending`, naming the input it
  now needs.
- `cadence` — `{enabled, running, queued, force, pending_edit_count,
  window_started_at_ms, cadence{min_pending_edits, max_pending_seconds},
  now_ms}` → `dispatch` / `wait` with `wait_ms` / `idle`, plus
  `waiting_not_executing`, which a host renders instead of a running spinner.

`now_ms` is an input, never a clock the kernel reads, so the same document
always plans the same way; `--now-ms` overrides the cadence section's own value.
The timer, the change accumulator, the GDF walk and the engine latch stay with
whoever runs AutoProcess — this plans, it never runs.

## The force gate, headless

`ds design force-gate check --action <action> [--force] [--targets n]
[--native-reports] [--no-force-capability] [--gesture id --confirm code]`
answers whether an expensive forced action needs the operator's confirmation
before it bypasses the freshness guards, from the same policy the Transformer
Status register and Project Control use (`force_gate`). One condition, where
those two surfaces used to disagree: the gate is live only when force is
requested AND `pipeline.force` is held, and the two report exports are exempt
when the installed compute generates them, because local regeneration spends
no shared resource.

With `--gesture` and a matching `--confirm`, the kernel mints a grant bound to
that gesture, that action and an expiry, which a headless run then presents as
`--force-grant`. The confirmation code is never in this executable's source or
in any client's: `ds` receives a token or nothing. This is cost friction, not
authorization — ds-brain re-checks `pipeline.force` on the call itself, and a
grant does not change what it will accept.

## Which store the project's design data comes from

`ds design data lane --project <id>` reads the named project's fresh Settings through the
native user client and asks the kernel which design-data path the project
declares: `firestore`, or the `mirrored` combined store. The explicit
`use_firestore_design_data` parameter wins; the legacy `design_data_source` and
`combined_source` spellings answer only when it is absent; a project that
declares nothing is Firestore. The answer names the `parameter` that decided
and the `reason_key`, so a surprising lane traces back to the row that set it.
ds-brain evaluates the same question on the grant side before it serves, which
is a fence rather than a second answer: this is what the client believes, and
it is now one belief rather than the browser's and the CLI's.

### Project Settings

`ds design config sheets --project <id>` lists the named project's fresh kernel model.
`read --sheet KEY` returns a page of rows (for rules, `--rule-set NAME` selects
one set). `--limit` is 1–100 and `--offset` pages; `more` counts omitted rows,
while `truncated` identifies shortened cell/metadata evidence. Use
`read --sheet KEY --out NEW_PATH` for complete, untruncated sheet JSON.

`diff --sheet KEY --file BASELINE.json` compares that file to the current
sheet. Object key order is immaterial; row order, missing values and whitespace
edits remain meaningful. `set --sheet project_settings --parameter NAME
--value TEXT --yes` changes one existing parameter using the Settings model's
control. `save --sheet KEY --file SHEET.json --yes` validates and saves a whole
sheet. `rule-set duplicate --sheet lv_poles_rules --source NAME --target NAME
--yes` copies compacted rows and metadata while preserving sibling sets.

All six commands use native selected-project authentication. Writes are
kernel-prepared, server-authorized and verified with fresh readback. A failed
readback never reports success; inspect current state before repeating an
uncertain write. The server's existing whole-sheet persistence is unchanged:
this family does not claim a cross-client editing lock.

## Download coverage

`design.download.plan` includes PDF/PNG prints, combined report records and
current relevant Combined Report bundles. Source `report_status` is independent
of `download_state`. `available_in_archive` names verified ZIP members and
the containing download URL; it never invents standalone member URLs.
`--format pdf` includes a ZIP when its indexed members contain matching PDFs.
Unknown archive coverage is explicit and does not justify regeneration.

### Governed design history

`design.version` names one explicit project and either an LV transformer or MV
project model. The transformer flag remains the LV compatibility spelling.
The server alone assigns governed `vN` identities; native Server and Web use
shared Rust request planning and response validation. Creation snapshots saved
server content, with an exact idempotency key, independently of an open map.
LV comparisons use immutable snapshots; MV comparisons describe pinned content
revision/digest/manifest lineage without claiming geometry comparison. Restore
is LV-only and pins the source head before its fenced transaction.

Paired map version creation/listing have been retired. Playback and comparison
rooms remain presentation adapters. Device-local snapshots are retained drafts,
not governed history or queued publication work.

`design.attachment` uses the native authenticated client and an explicit project,
without Desktop state. It lists opaque revisions, uploads through a server grant
and verified native transfer, finalizes immutable bytes, authorizes generation-
pinned downloads and performs fenced soft retirement. LV attachment versions
are assigned `vN`; MV attachment versions are exact content revision IDs. Read
live capabilities for exact flags, limits and refusal remedies.

For a prepared native Design workspace, `ds design project revisions` lists
retained content digests and `ds design project compare` compares those exact
revisions or its head. These are explicitly local, not server vN ordinals.

## Pinned context

A pinned transformer is READ-ONLY CONTEXT: dumb GeoJSON an operator glances at
while working on something else. It is not editable, not a selection target and
carries no session. `ds design pinned preview --project <id>` answers what one costs, on a
Server, with no browser, using the same kernel decision (`ds.pinned-context/v1`)
the map executes.

**What to fetch.** `--held` is a JSON array of what a machine already holds —
`[{name, layers: {class: count}, version, complete}]` — an inventory of counts,
never payloads. A room the machine holds is reused. A room missing ONE design
class asks for that class rather than the whole room again; the classes a
displayable room must carry are `--require`, defaulting to the transformer's
own `tr` anchor, whose other spellings (`transformer`, `transformers`,
`transformer_point`) resolve to the same class. A held copy is refetched WHOLE
only when the project's status register proves its head revision has moved: a
copy whose own revision is simply unknown is not stale on that basis. A room
already read whole (`complete: true`) that still lacks a required class does
not have one on the server either, and is never asked for again. `--force` is
the operator's Refresh: every pinned room, held or not. `--focus` is the
transformer being edited, which is never pinned context as well and is dropped
from the plan rather than fetched twice. Every context read asks for the
`context` projection, so the non-scalar property bags are dropped server side.

**What one layer is.** Every pinned room of the same design class folds into ONE
FeatureCollection per (source, class): fifty pinned transformers across six
classes are six map sources, not three hundred. Each merged feature carries
`pinned_transformer_name`, so a popup, an attribute-table row and a hide choice
can all still name the transformer it came from, and merged feature ids are
namespaced by transformer so two rooms sharing a document id stay two features.
`--hide <name>` is how a single pinned transformer is switched off: it is an
input to the fold, not a render filter, so its rows are simply not in the
payload.

**Which attributes survive.** The fields the class's resolved style document
actually reads, and nothing else. The document is the class's `_pinned` variant
where the project publishes one and the plain design document otherwise; the
field names are computed by walking it for every `get`/`has`, aliases included,
because the documents are backend-authored and arrive at runtime — a list in
code would be a guess that silently paints the layer wrong. The nested
`*_properties` bags, the topology helpers and the survey columns are dropped
before the payload reaches a renderer or a store.

The receipt is bounded: whole-room fetches are counted rather than listed,
`--plan-only` reads nothing from the project at all, and a per-transformer read
failure is reported beside the answer rather than ending it.
