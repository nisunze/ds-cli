# Survey control plane

`ds survey` runs natively without Desktop. Form Factory schemas, project form
bindings/settings, reusable templates and project creation use fixed typed
`ds-client-core` routes. Global schemas/templates use restored user authority;
project commands also require the selected project from `ds auth project use`.
An explicit `--project` must match that selection. `--lane` selects stable or
canary; no survey command accepts a Desktop descriptor. Server operations still
require service access and current permissions. Profile catalog schema v16 must
advertise the exact survey control routes; older catalogs fail closed.

Offline capture uses a native SQLite workspace:

```sh
ds survey workspace init --workspace ./survey-local --snapshot ./resolved-forms.json
ds survey workspace collect --workspace ./survey-local --form poles \
  --document ./point.json --doc-id source-123 --created-at 2026-09-05T10:00:00Z
ds survey workspace list --workspace ./survey-local --limit 20
# Once service access is available, explicitly publish a bounded batch:
ds survey workspace sync --workspace ./survey-local --limit 10 --yes
```

For a completely disconnected demonstration from this repository, use
`examples/survey/offline-snapshot.json` as the snapshot and
`examples/survey/entry.json` as the document. They define a synthetic local
project for exercising capture; replace them with an actual resolved project
snapshot before preparing a real migration.

`init`, `collect`, and `list` never contact the network or require sign-in.
The snapshot is `{ "project_id": "…", "forms": […] }` with full resolved enabled
forms, field definitions and entry document schemas. A collection document has
`data` and optional canonical `geometry`, `connectivity`, `detailed_location`,
and `context_key`. Unknown or hidden fields are refused without insertion;
source ids and capture times are retained for migration. The SQLite row and
stable replay key commit together. Reopening does not invent a new replay key.

When online, `survey workspace prepare --workspace ./survey-local` obtains the
selected project's full form snapshot instead of reading a supplied file.
Sync binds the workspace to the restored principal, project, lane and audience
before sending any entry. It stops on the first error and retains pending rows
for exact idempotent retry. A committed receipt confirms Firestore acceptance,
not BigQuery mirror visibility. Cached form metadata never grants authority.
No GCP access is needed for local preparation and capture; no sync was exercised
against a live service in the offline implementation tests.

`survey query` is the selected-project aggregate data plane. It restores the
same native identity, releases the audience-fenced project-context lease before
network work, and calls only `POST /api/v1/survey/query`. The backend refreshes
the project Survey mirror and rechecks project, form, paired/per-form, and
view-blocked authority before executing the question. The CLI returns at most
200 aggregate rows and never exposes raw entries, media, tokens, a project
override, or an arbitrary request body. The governed backend rejects questions
whose dry-run estimate exceeds the 256 MiB billed-byte ceiling.

The aggregate grammar is deliberately closed:

```text
ds survey query --form lv_poles_survey --metric count \
  --group-by created_by \
  --filter '{"field":"created_by","op":"eq","value":"operator@example.com"}' \
  --order desc --limit 50 --output json
```

`--metric` is `count` or `count_distinct`; only `count_distinct` requires and
permits `--distinct-field`. Repeat `--group-by` at most twice. Repeat
`--filter` at most eight times, once per JSON object—there is no whole-request
JSON flag. Operators are `eq`, `neq`, `gte`, `lte`, `in`, `between`,
`is_null`, and `not_null`, with exact operator-specific fields. `in` accepts 1
through 20 string values. The public `created_by` field remains legal for
filtering, grouping, and distinct counts; the server still applies any
view-blocked creator restriction independently. Defaults mirror the backend:
descending order and 50 rows; `--limit` may explicitly raise the bound to 200.
The response does not present `bytes_processed` as billing evidence because the
current service cannot recover iterator statistics after execution.

`survey entries select` is the bounded spatial data plane for headless
selection semantics. It accepts only an exact governed `--form`, one WGS84
`--bbox '<west,south,east,north>'`, a `--limit` from 1 through 500 (default
100), and the stable/canary lane. All four coordinates, their legal longitude
and latitude ranges, their west/east and south/north ordering, the form slug,
and the limit are validated before profile discovery, auth restoration,
project-context access, or network work. The command then releases the
selected-project lease before calling only
`POST /api/v1/survey/entries/select`.

```text
ds survey entries select --form lv_poles_survey \
  --bbox '29.70,-2.05,29.80,-1.95' --limit 100 --output json
```

The backend refreshes the project Survey mirror, reapplies project, paired,
per-form, and view-blocked creator authority, and returns rows ordered by
`doc_id`. Each row contains only `doc_id`, GeoJSON `geometry`, `created_by`,
and `firestore_updated_at`. The CLI verifies the echoed project, form, and
bounding box, row ordering and bounds, geometry shape, explicit consistency,
and the server-issued `selection_digest` before returning anything. The
consistency is live mutable `survey_mirror` data after `ensure_synced_all`; the
selection is not a datastore snapshot and its digest is not a revision token.

`truncated: true` and `complete: false` mean the returned rows are not a
complete selection. Narrow `--bbox` and run a new selection. There is no
cursor, pagination contract, include-deleted mode, arbitrary field projection,
media expansion, or mutable apply path. The command also accepts no project,
URL, method, body, token, WKT, GeoJSON request, caller-authority, force, or
Desktop override.

The shared native core validates the backend error envelope and exact
HTTP-status/code pair before exposing one closed service enum. Backend error
messages, details, and response bytes do not cross into ds-cli. The command
maps that enum as follows:

| Service meaning | CLI code | Operator action |
| --- | --- | --- |
| Query budget exceeded | `survey_entries_too_expensive` | Narrow `--bbox`. |
| Response bound exceeded | `survey_entries_too_large` | Narrow `--bbox` or lower `--limit`. |
| Mirror synchronization failed | `survey_entries_sync_failed` | Retry; report repeated sync failures. |
| Mirror data is unsafe to represent | `survey_entries_mirror_invalid` | Repair or update the governed mirror; an unchanged retry is not a remedy. |
| Bounded request rejected | `survey_entries_invalid` | Recheck the exact form, bbox, and limit. |
| Route unavailable | `survey_entries_unavailable` | Retry later. |
| Service failed temporarily | `survey_entries_failed` | Retry; report repeated failures. |
| Project or form scope unavailable | `survey_entries_scope_not_found` | Verify the selected project and an exact available form; the response does not reveal which scope was absent. |

If an older or unrecognized response has no typed service code, ds-cli retains
the coarse status-class fallback without inspecting response bodies.

`survey entries changes` is the bounded incremental mirror data plane. It
accepts only an exact governed `--form`, a required RFC3339
`--updated-after`, a `--limit` from 1 through 500 (default 100), an optional
opaque `--cursor` no larger than 4096 bytes and containing no whitespace, and
the stable/canary lane. The CLI validates that entire grammar before profile
discovery, identity restoration, or selected-project access. It releases the
selected-project lease before one fixed
`POST /api/v1/survey/entries/changes` call and never auto-paginates.

```text
ds survey entries changes --form lv_poles_survey \
  --updated-after 2026-08-30T00:00:00Z --limit 100 --output json
```

The inclusive lower clock may safely replay exact-boundary evidence. Each row
contains exactly `doc_id`, optional GeoJSON `geometry`, `created_by`,
`is_deleted`, and `firestore_updated_at`. Automation must therefore apply or
deduplicate idempotently by `doc_id` plus `firestore_updated_at`; a tombstone
(`is_deleted: true`) removes the corresponding local live row. This is a
coalesced current-state `survey_mirror` delta, not Firestore snapshot or
mutation history, so hard-deleted documents cannot be inferred.

`has_more: true` and `complete: false` mean the checkpoint must not advance.
Call the command again with the identical `updated_after` and effective
`limit`, plus the exact returned `next_cursor`. Only a response with
`complete: true` may promote its `upper_fence` to the next checkpoint. If a
cursor's fence expires, discard that incomplete cursor and restart from the
last **previously completed** checkpoint—never from the expired feed's
`upper_fence`. There is no auto-loop, project override, arbitrary transport,
field/media projection, deletion filter, force flag, caller authority, or
Desktop fallback.

The changes feed preserves the core's closed service meanings without parsing
backend response text:

| Service meaning | CLI code | Operator action |
| --- | --- | --- |
| Request rejected | `survey_entries_changes_invalid` | Recheck form, lower clock, limit, and cursor. |
| Cursor rejected | `survey_entries_changes_cursor_invalid` | Reuse the exact cursor with identical lower clock and limit, or restart from the last completed checkpoint. |
| Immutable fence expired | `survey_entries_changes_fence_expired` | Discard the incomplete cursor and restart from the last previously completed checkpoint, never its upper fence. |
| Query budget exceeded | `survey_entries_changes_too_expensive` | Keep the last completed checkpoint unchanged; repair partitioning/indexing or raise the governed backend budget, then restart there. |
| Response bound exceeded | `survey_entries_changes_too_large` | Lower the limit and restart from the last completed checkpoint. |
| Mirror data is unsafe | `survey_entries_changes_mirror_invalid` | Repair the governed mirror; retry alone cannot repair it. |
| Immutable table version temporarily unavailable | `survey_entries_changes_snapshot_unavailable` | Retry the identical page with the same cursor. |
| Route or durable cursor signing is unconfigured | `survey_entries_changes_unavailable` | Configure the governed deployment and durable changes cursor signing key, then restart from the last completed checkpoint. |
| Mirror synchronization failed | `survey_entries_changes_sync_failed` | Retry the identical page; report repeated failures. |
| Service failed temporarily | `survey_entries_changes_failed` | Retry the identical page; report repeated failures. |
| Project or form scope unavailable | `survey_entries_scope_not_found` | Verify the selected project and exact form; the response does not reveal which scope was absent. |

Malformed, unknown, contradictory, or oversized service envelopes retain a
coarse status-class refusal. The CLI never parses backend response bodies or
promotes an unrecognized message into a typed service meaning.

`survey entries create` is the governed single-entry write path. It requires
explicit `--yes` confirmation and accepts only a form slug, new document id,
opaque idempotency key, RFC3339 device creation time, one closed local JSON
document, an optional context ancestor chain, and stable/canary lane. Project
identity comes only from the restored user's audience-fenced selection. The
CLI validates the complete local grammar before profile discovery or auth,
releases the selected-project lease before the request, and calls only the
create-bound `POST /api/v1/entries/mutate` native-core contract. It never
retries or falls back automatically.

```text
ds survey entries create --form lv_poles_survey --doc-id pole-104 \
  --idempotency-key '<opaque-key>' --created-at 2026-08-30T12:00:00Z \
  --document ./pole-104.json --yes --output json
```

The document must be a regular non-symlink file no larger than 900 KiB. Its
root is closed: required `data` must be an object; optional `geometry`,
`connectivity`, and `detailed_location` must be non-null, with the latter two
also objects. Unknown root keys are refused. The shared core owns form and
document identity, context, canonical timestamp, GeoJSON, finite-number, and
exact serialized payload validation. No project id, raw URL, method, request
body, token, origin, operation, retry, force, caller authority, or Desktop
descriptor is accepted.

Success returns a receipt only: Firestore is `committed`, while the BigQuery
mirror remains `unconfirmed`. The output never contains request data or the
idempotency key. A later governed selection or changes read establishes mirror
visibility; the create receipt itself does not. A manual retry after an
ambiguous service failure must reuse the exact document and idempotency key.

`survey entries import` is the separate bounded migration path. It leaves the
single-entry command unchanged, requires explicit `--yes`, and accepts one
immutable NDJSON source, checkpoint path, receipt path, fixed form, and lane.
It validates the complete source twice before profile discovery, auth, local
state creation, or network work; restores one native session; freezes the
selected project and form; then invokes the same governed create contract
sequentially. There is no concurrency, automatic retry, source-format parser,
project override, per-row form, or transport fallback.

```text
ds survey entries import --form lv_poles_survey \
  --file ./survey123.ndjson \
  --checkpoint ./survey123.checkpoint.json \
  --receipt ./survey123.receipt.ndjson --yes --output json
```

Each line is one closed canonical object. The four optional buckets, when
present, must be non-null. `connectivity` and `detailed_location` must be
objects, and `geometry` must satisfy the shared GeoJSON contract.

```json
{"doc_id":"pole-104","idempotency_key":"<opaque-key>","data":{},"metadata":{"created_at":"2026-08-30T12:00:00Z"},"context_key":"parent_form:parent-id","geometry":{"type":"Point","coordinates":[30.1,-1.9]},"connectivity":{},"detailed_location":{}}
```

Only `metadata.created_at` is caller-provided metadata. The selected
`project_id`, authenticated `created_by`, operation `create`, origin `unknown`,
audit fields, and Firestore replication clock remain authority-owned. The
current create contract has no safe source-provenance field, so imports do not
invent or persist one.

The source is bounded to 8 GiB, 100,000 rows, and 1 MiB per physical line.
Duplicate idempotency keys or duplicate canonical context/document identities
are refused before auth. Context keys containing percent encoding are refused
in this first version because the shared core does not expose an unambiguous
canonical identity projection for alias detection.

The append-and-sync receipt records only the row number, source-row digest,
form, document-identity digest, terminal code or verified commit clock/version.
The atomic checkpoint and machine summary contain no payload, field names, coordinates,
idempotency material, token, or email. A terminal receipt is synced before the
checkpoint advances. A crash before that append safely replays the exact
idempotent create; a complete receipt event one row ahead of the checkpoint is
reconciled without a network call; a partial receipt tail is removed before
the exact row is replayed, but only after the unchanged receipt, principal,
audience, selected project, form, and both state paths are rebound. Pre-auth
inspection never truncates a receipt. Other incomplete or contradictory state
refuses.
A non-link sidecar lock derived from the canonical receipt gives one process
exclusive ownership of every writer to that receipt while allowing imports
with distinct receipts to run together. Checkpoint and receipt manifests bind
both canonical state paths.
`--on-error continue` advances only past the exact row-local `invalid`,
`idempotency conflict`, and `already exists` create outcomes. Permission,
disabled/read-only scope, missing-scope, coarse refusal, uncertain, and
retryable outcomes all pause without advancing because they may affect every
remaining row.

On Unix, import state also enforces owner and `0600`-equivalent file
permissions. The current Windows build can prove reparse points, file identity,
link count, and process exclusion, but cannot yet prove an owner-private DACL.
The import command therefore reports
`survey_entries_import_windows_state_unavailable` on Windows until a protected
state-root adapter is available; it does not claim durable-state privacy there.

Four related objects have separate lifecycles:

1. A **Form Factory form** is a global master schema. Use `survey forms list`,
   `survey form read`, `survey form types`, `survey form create`, `survey form
   update`, and `survey form lifecycle`.
2. A **project-form binding** enables a master form for one project and stores
   that project's settings. Use `survey project-forms list` for the selected
   native project's bounded summary and `survey project-form settings` for one
   selected-project editor. Use `survey project-forms read`, `survey
   project-form editor`, `survey project-forms plan`, and `survey project-forms
   apply` for explicit-project native planning and apply. The project must match the selected project.
3. A **project template** is a reusable snapshot containing project-form
   configuration. Use `survey templates list`, `survey template read`, `survey
   template create`, `survey template apply`, and `survey template lifecycle`.
4. A **project created from a template** is a new, independent project. Use
   `survey project create-from-template`. Applying a template instead modifies
   an existing project.

## Safe discovery order

For a new complex form, first read `ds survey form types --output json`, create
the master schema from a bounded JSON document, then read the target project's
bindings and the new form's backend-owned settings editor. Do not invent
network keys: the editor's `sections`, `field_state`, and `capabilities` are the
authoritative vocabulary for that form.

For example, a hypothetical water-network request can use node forms for
valves, reservoirs, or junctions and edge forms for pipes. The domain is only
an example—the same workflow applies to electrical, telecom, road, or other
survey networks. The LLM should discover the legal settings from the live
editor instead of assuming field names from the example.

## Project-form changes

`--changes` names a UTF-8 JSON file containing 1 to 32 unique rows:

```json
[
  {
    "form_slug": "water_junction",
    "enabled": true,
    "settings": {
      "is_network_element": true,
      "network_element_type": "node"
    },
    "expected_version": 3
  },
  {
    "form_slug": "water_pipe",
    "enabled": true
  }
]
```

A row containing `settings` must echo `expected_version` from the immediately
preceding `survey project-form editor` response. An enable-only row deliberately
has no settings revision and preserves existing settings. Always run `survey
project-forms plan` before the confirmed `survey project-forms apply`; apply
rechecks live editors and refuses stale or unknown settings.

Unavailable bindings are returned separately from resolved forms. A missing or
archived master may be cleaned without restoring it by planning and applying an
enable-only `false` row. Settings edits and re-enabling remain refused until
the master exists and is active again.

## Templates and projects

These two requests are intentionally different:

```text
ds survey template apply --project existing-water --template water-network --merge-strategy preserve --yes
ds survey project create-from-template --template water-network --project-name "New Water Survey" --yes
```

The first changes an existing project's project-form configuration. The second
creates a new project instance. Creating a template is different again: `survey
template create` snapshots a named source project's current configuration into
a reusable catalogue object.

## Complex-form lifecycle and refusals

Keep the master-form lifecycle separate from a project's binding lifecycle. A
safe complex-network sequence is: discover `form types`; create the master
schema; read it; update it with the returned version; publish it; use the
project-form editor to configure the explicit project binding; then unpublish,
archive or delete only with the exact dependency result the backend returns.
`survey form lifecycle` is the master transition door; `project-forms plan` /
`apply` are the project-settings door; neither creates a project template or a
new project instance.

Every transition can refuse. A stale `--expect-version` is a concurrency
refusal; archive/delete can refuse live bindings unless an operator explicitly
uses `--force`; a missing or archived master is returned as an unavailable
binding and permits only an enable-only `false` cleanup. Follow that refusal
through the same `ds survey` contract—do not open the map, reconstruct settings
from a cached form, or make template management conditional on an unavailable
form.

Commands under `ds map` remain reserved for operations that genuinely consume
map-owned local state, such as Working Area transfer or survey-data migration.
Form Factory, project-form settings, project templates, and project creation
are API control-plane operations and stay usable with no map open.
