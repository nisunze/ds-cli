# `ds design` — reference

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

## The governed groups, and why they are a family of their own

`city` and `phasing` are two reserved tag definitions held to a fixed shape —
single-valued, LV transformers only — and given a batch. The generic
`ds design tag set` refuses them and points at `ds design group`, so a governed
group has exactly one write door.

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

**`phasing` is not finished when its tag lands.** Its canonical home is
`AlignmentRow.delivery_phase` in the DS Grid model. `ds` holds no model session,
reports no receipt, and therefore gets `partial` back with every named
transformer listed as outstanding. That is the true state — nobody has written
the model — not a degradation to work around. Finish those in the application,
which resolves the alignment and writes it.

### `ds design group export` — the document a report pins

A per-city report is grouped by the tag group named exactly `city`. `export`
publishes that authority as the read-only `ds-report.design-tags/v1` document,
for an explicitly named transformer set: the project's active group
vocabularies (`phasing` and ordinary groups carried past untouched) and the
values those transformers carry.

Two things about it are load-bearing:

- **Write `.data.document` verbatim.** The `sha256` beside it is over those
  exact bytes, and it is what a report request pins. Parsing and
  re-serializing produces different bytes for the same facts, and the pin
  stops matching.
- **It refuses rather than guesses.** No group named exactly `city`, or two of
  them, and the export fails. Neither is repairable: picking one would decide a
  published per-city total on a coin flip.

Values the closed document shape cannot carry — one under an archived
definition, or a cleared assignment — come back in `excluded` with the reason,
so nothing is dropped in silence.

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

Two of those bounds differ on purpose. A governed group batch takes at most
200 transformers, because 200 is one Firestore transaction's write budget on
the server. `ds design group export` takes 2,000, because it is a read whose
unit is a whole project: a live project already carries 202 transformers, and
splitting one export would produce two documents with two digests that a report
pins separately and will not join.

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
| `invalid_design_anchor` | the anchor names a reserved document or a kind that does not exist |
| `attachment_too_large` | the file exceeds the desktop's bounded path reader |
| `invalid_value_list` | a comma-separated flag was given but carries no values |
| `too_many_values` | a list flag carries more entries than the record accepts |
| `missing_comment_target` | neither `--thread` nor a complete `--kind`/`--object`/`--title` |
| `unknown_tag_group` | `--group` named something other than `city` or `phasing` |
| `design_plan_stale` | the project moved after the plan was previewed; preview again |

The pairing refusals (`desktop_not_paired`, `desktop_ambiguous`,
`desktop_unreachable`, `pairing_rejected`, `desktop_signed_out`) are the shared
set every bridge domain uses; `ds map --help` documents them once.
