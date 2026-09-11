# `ds design` — reference

The [offline project workspace](design-project.md) owns local transformer
snapshots, version-fenced edits, native background processing, report/PDF
production and pending publication without a paired application.

Tier-4 reference. `ds design <command> --help` is the contract; this document
is the part that does not belong in any command's help because it is true of
all of them.

Governed collaboration serves
`ds-brain/docs/contracts/design-collaboration-roadmap.md`. Offline Fast LV
processing serves the ds-network native batch contract directly.

## Offline Fast LV processing

`ds design lv project-export` is the authenticated, mapless handoff from one
governed transformer snapshot into the local file contract below. It restores
the Firebase user for `--lane stable|canary`, uses only that user's
audience-fenced selected project, and performs the fixed
`get_transformers_data fields=context` call for one exact transformer. The
gateway rechecks membership. The command refuses legacy context unless the
server supplies both `metadata.version` and `metadata.content_digest`, then
asks ds-network to encode the returned layers as one validated request at an
absent `--out` path.

The context projection does not contain the project's process-settings model
or network-config sheets. The request therefore carries ds-network's explicit
owner defaults and an empty `config_dfs`. Its receipt says
`process_settings=ds-network-owner-defaults` and
`project_config=not-included`; this is a truthful baseline handoff, not the
configured Desktop preset. There is no `--project`, Desktop descriptor,
arbitrary request field, browser store, or processing-lane argument.

```bash
ds auth login --email operator@example.com
ds auth project use --project <exact-id>
ds design lv project-export --transformer T-1042 \
  --out ./T-1042.fast-lv.json --output json
ds design lv process --input ./T-1042.fast-lv.json \
  --out ./T-1042.fast-lv.result.json --output json
```

`ds design lv process` is the mapless, signed-out native route to the same
Rust engineering kernel used below ds-web's Fast WASM adapter. Its input is one
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

## Headless feature selection

`ds design features select` is the first native, map-independent design read.
It restores the Firebase user for `--lane stable|canary`, loads only the saved
project context fenced to that UID, canonical email, lane, and credential
audience, then performs the fixed `get_transformers_data` context projection
for one explicit transformer. The gateway remains membership authority. There
is no `--project`, Desktop descriptor, arbitrary URL, body, header, or
processing-lane argument.

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
ds auth login --email operator@example.com
ds auth project use --project <exact-id>
ds design features select --transformer T-1042 --layer lv_lines \
  --where drafting_status= --sample 5 --ids 100 --output json
```

The older `ds map design select` contract is unchanged for callers already
using a paired application.

## Transformer status rows, without a browser

`ds design status` is the read every other headless Design answer is built
from. Like `features select` it restores the native user for
`--lane stable|canary` and reads only that user's audience-fenced selected
project, through the fixed governed status call. There is no `--project`, no
Desktop descriptor, no URL, body or action override, and no fallback: if the
native path cannot answer, the command refuses in words rather than reaching
for the application.

```bash
ds design status --output json
ds design status --transformer TX-1 --transformer TX-2 --output json
ds design status --findings --output json
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
ds design status --findings --output json | jq '.data.summary'
ds design status --findings --output json | jq -r '.data.findings[] | "\(.transformer) \(.severity) \(.code)"'
```

Two different bounds apply, and they are different on purpose. A request names
at most 500 transformers — the same bound retirement and compounded reports
use. An answer carries at most 2,000 rows, which is the bound on one project's
own transformer collection: an unnamed read asks for the whole project, so it
can legitimately answer more rows than any single request could have named.
Above either bound, or past the fixed response size cap, the answer is refused
as `auth_response_unreadable` rather than truncated.

## The project's whole Dashboard, without a browser

`ds design dashboard` folds those same rows once into the model the
application's Design wall renders — the project's progress story, read from a
terminal. Same credential, same fixed status call, same refusals: no
`--project`, no Desktop descriptor, no fallback to a browser.

```bash
ds design dashboard --output json
ds design dashboard --fast --output json
ds design dashboard --output json | jq '.data.dashboard.health'
```

There is no `--transformer`: every percentage here is measured against the
whole fleet, so a dashboard over a subset would be a different question.
`--fast` reads the project the way the application's Fast lane does — no
Draft/Sketch phase summary, no legacy-phase attention note, and the Standard
lane named as the legacy it is.

`.data.dashboard` carries, in one object:

| Member | What it answers |
|---|---|
| `total`, `designed_count`, `process_lane_count`, `report_ready_count`, `combined_count` | how far the fleet has come. A transformer is *designed* through ANY compute phase — sketch, draft or process — and a green Process stamp whose saved output layers are missing is not designed at all |
| `pipeline` | the same four counts as stages with `pct_of_total`. Deliberately NOT a funnel: the phases are independent over one record, so a later stage can out-count an earlier one |
| `momentum` | designs per local day (`points`, `key` is `YYYY-MM-DD`), the 7-day split (`last7`, `prev7`, `trend`), `busiest`, `active_days`, `avg_per_active_day`. Days are bucketed at the host's UTC offset; `ds` uses this machine's |
| `crew`, `errors_by_user` | who designed what, and who ran the phases that failed. A bucket with no attributable account carries `unknown: true` and an empty name — the reader names it |
| `districts`, `district_count`, `sector_count` | where the work is. The leftovers chip carries `unassigned: true` and an empty label, for the same reason |
| `phase_summaries` | per phase slot, how many rows sit in each status |
| `lanes`, `fast_pct` | which lane ran the rows whose Process actually ran |
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

## Local transformer rooms for background work

`ds design transformer download` materializes saved transformer rooms into the
paired application's local cache without opening a map or entering any
transformer edit context. It is the preparation step for a local report when
the room is not already on this device. Omit `--transformer` for every active
ordinary transformer, or repeat the flag for an exact subset:

```bash
ds desktop status --output json
ds design transformer download --transformer TX-1 --transformer TX-2 --output json
ds design transformer download --output json
```

The paired application's visible project is authoritative because that
application owns the destination cache. This differs deliberately from the
CLI-selected headless project used by retirement and project-wide cloud
reports. The operation does not navigate, process, renumber, stage, save,
publish or version anything. A clean room already at the saved server version
is reused. `--force` refreshes clean rooms, but never overwrites a dirty local
room. The bounded receipt distinguishes downloaded, already-local,
dirty-preserved, failed and cancelled names and states `staged: false`,
`persisted: false`, and `context_changed: false`.

## Reversible transformer retirement

`ds design transformer inventory|retire|restore` is the map-independent
lifecycle of a project's transformer documents. Like `features select`, the
family restores the native user for `--lane stable|canary`, loads only its
audience-fenced selected project, and calls the fixed governed report contract.
There is no `--project`, Desktop descriptor, URL, body or action override.

**Retirement is not deletion.** Deleting a transformer (`ds map design delete`,
paired) archives and destroys its artifacts. Retiring it flips the one
soft-delete bit every consumer already honours and records who, when and why:
the transformer leaves Transformer Status, every listing, combined and
compounded reports, design tile runs and layer counts, while its document,
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
ds auth project use --project <exact-id>
ds design transformer inventory --transformer TX-1 --transformer TX-2 --output json
ds design transformer retire --transformer TX-1 --reason "superseded by the 2026 survey" --yes
ds design transformer restore --transformer TX-1 --yes
```

The receipt of a write names each transformer with `applied` and a timestamp,
or a closed `refusal`: `not_found`, `already_retired`, `not_retired`,
`no_retirement_record`, `special_document` (`mv_data`, the combined row and
collision docs are never retired), `governance_locked`, `not_owner`, `failed`.
Contract: ds-brain `docs/contracts/transformer-retirement.md`.

## Where design collaboration is

Governed collaboration is not on disk and is not reachable with a credential
this process holds. The offline Fast LV file contract above is deliberately
separate from that authority boundary.

Saved selections, attachments, tags and comment threads are governed project
state behind ds-brain, which is the only gateway and the only authority: it
decides who may write, it arbitrates two people editing the same record in the
same second, and it refuses a write authored against a version that has since
moved. So every collaboration command here is one named semantic operation the
*paired application* performs under the session it already holds. `ds` sends a
request and receives an outcome. It never receives a credential, and it never
runs code inside the application — `docs/reference/desktop.status.md` has the
pairing argument in full.

There is no `--project` flag anywhere in this domain. Collaboration commands
use the project open in the paired application; the headless feature and LV
export commands use the exact audience-fenced context selected by
`ds auth project use`.

## Why this is not `ds map`

No command here needs a map instance, an edit session, or an open design room:
local Fast LV consumes an explicit file; a selection is a list of stable
identities; an attachment is bytes with a media type; a tag is a value from the
project's own vocabulary. `ds map` owns local map state; this domain owns none.

## The shape of a session

```bash
ds design selection list                                  # what is saved
ds design selection read --selection sel-week-32          # who is in it, right now
ds design selection assign --selection sel-week-32 \
  --title "Review LV designs" --owner nixon@example.com --yes

ds design attachment list --kind mv_model --object mv_line_a
ds design attachment publish --kind mv_model --object mv_line_a \
  --path ./MV_LINE_A.bak --version rev_2 --yes

ds design tag list --kind lv_transformer --object kigali_a
ds design tag set --kind lv_transformer --object kigali_a \
  --definition transformer_scope --values additional_scope --yes

# Typed definitions and values retain their numeric/text identity.
ds design tag define --definition completion --name "Completion percent" \
  --value-type number --min 0 --max 100 --yes
ds design tag set --kind lv_transformer --object kigali_a \
  --definition completion --number 82.5 --yes

# `know_columns` is the exact external property authority.
ds design known-columns list
ds design known-columns set --layer mv_lines --field tag_city \
  --visibility published --yes
ds design known-columns set --layer mv_lines --field tag_internal_review \
  --visibility hidden --yes

# Project-wide typed filters never require an open map.
ds design tag query --choice city:any_of:huye,kigali --output json
ds design tag query --choice phasing:equals:phase-1 \
  --number completion:gte:80 --output json

ds design group list --transformers kigali_a,kigali_b            # allowed values
ds design group preview --group city --transformers kigali_a,kigali_b \
  --value kigali --output json                                   # plan + digest
ds design group apply --group city --transformers kigali_a,kigali_b \
  --value kigali --digest <plan-digest> --yes
ds design group export --transformers kigali_a,kigali_b \
  --output json | jq -r .data.document > tags.json               # for a report

ds design comment list --kind lv_transformer --object kigali_a
ds design comment post --thread thread-clearance --body "Agreed, re-spot it." --yes
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

`ds design categories read --kind customer` or `--kind meter` reads fresh
canonical names, aliases and catalog metadata with bounded pagination.
Use the `ds-dirty-categories` skill when a report flags unknown values:
missing or newly introduced categories generally require a seed correction;
code should change only when it fails to honor a valid seed.

`ds design meter-types ensure --name Readyboard --yes` preserves Readyboard
as its own meter category alongside Single Phase and Three Phase.
`ds design customer-categories alias --alias Productive --category Commercial --yes`
seeds an explicit source-label mapping while preserving Commercial's demand
settings. Both commands use the native selected project, retain unrelated
catalog rows, and verify fresh saved configuration without Desktop.

`ds design feeder-limits read` reads the native selected project's feeder
bounds, LV cable bounds and transformer cable catalog from fresh configuration,
without Desktop. Optional `--out` retains this configuration at a new JSON path.
`ds design feeder-limits set --minimum 25 --maximum 95 --yes` updates only
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
internal by default. The paired application reads the policy revision before
writing, and ds-brain commits the one-field change, derived-output
invalidation and audit row together. If another editor moves the revision,
the write is refused and must be retried from a fresh `known-columns list`.

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

**Publishing a large attachment.** The paired desktop reads a named path
through a bounded reader, so `ds design attachment publish` refuses a file
larger than that bound with `attachment_too_large` and names the Attachments
dialog, which streams from the file picker. Truncating the file to a preview
and registering a revision against the wrong bytes would be worse than
refusing.

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
| `attachment_too_large` | the file exceeds the desktop's bounded path reader |
| `invalid_value_list` | a comma-separated flag was given but carries no values |
| `too_many_values` | a list flag carries more entries than the record accepts |
| `missing_comment_target` | neither `--thread` nor a complete `--kind`/`--object`/`--title` |
| `unknown_tag_group` | `--group` is missing or does not identify a batchable project definition |
| `design_plan_stale` | the project moved after the plan was previewed; preview again |
| `invalid_transformer_scope` | no transformer named, or a name is blank, repeated or over 200 characters, or over 500 named |
| `invalid_reason` | `--reason` is blank, untrimmed or over 512 characters |

Those four replace what `desktop_refused` used to carry for the collaboration
surfaces, and the class is what a caller acts on: an exceeded bound and an
unknown record are `invalid_input`, so the identical call can never succeed,
while a service fault and an unreachable API are `unavailable` and the same
call works once the service answers. A write that never answered may still
have been applied, so `backend_unreachable` asks for a re-read before a retry
rather than a blind repeat.

The headless `transformer` family adds the native-profile, headless-session
and `auth_*` codes `ds tile --help` documents once; `auth_rejected` there also
covers an archived or expired project and a missing capability.

The pairing refusals (`desktop_not_paired`, `desktop_ambiguous`,
`desktop_unreachable`, `pairing_rejected`, `desktop_signed_out`) are the shared
set every bridge domain uses; `ds map --help` documents them once.

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

`ds design status --search <text> --sort <key> [--desc] --filter <dimension=value>…`
answers the Transformers register's own question with no browser: which rows
the operator would see and in what order, decided by
`ds-command-kernel::design_status_query` — the same selector the register and
the transformer picker read. A quoted `--search` ("tx_12") is an exact name;
a bare one is a substring. `--sort` takes `name`, `district`, `tags`,
`legacy`, `process`, `report`, `combined`, `governance`, `user`, `updated` or
`version`; special rows (`combined_transformer`, `mv_data`, `collisions`,
`compounded_report`) lead in their fixed order, except that `mv_data` joins
an `updated` sort. `--filter` repeats: `sync=saved|unsaved`,
`legacy|process|report=<phase status>`, `combined=<state>`,
`governance=locked|draft`, `user=<account>`, `lane=standard|fast`,
`warning-type=<code>` and one `admin-<district|sector|cell|village>=<value>`
(`__admin_bounds_none__` selects rows with no bounds at that level). Any
secondary filter hides the special rows, as the register does. The answer's
`transformers` are the selection in order, `count` its size, and `query`
carries the selector, the sort and `options` — what each filter may offer
over this project (`admin`, `legacy`, `process`, `report`, `combined`,
`governance`, `user`, `warning_types` with counts, `sync`, `lane`). A
headless client holds no browser session, pins, tags or saved selection, so
those members of the selector are empty here; `ds` answers what the register
would show a fresh browser.

## Process settings, resolved headlessly

`ds design process settings --preset drafting|sketch [--lane standard|fast]
[--project-config config.json] [--operator toggles.json]
[--firestore-design-data]` answers what the LV process dialog sends for that
lane and preset, from the same kernel module (`process_settings`) over the
engine's own processor catalogue: `settings` (the preset applied to the
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

### Project Settings

`ds design config sheets` lists the fresh selected project's kernel model.
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
