# Survey control plane

`ds survey` manages the API-backed survey lifecycle without requiring an open
map. Existing control-plane commands retain their explicit project ids and
paired Desktop authority. `survey project-forms list` and `survey project-form
settings` are separate native reads: they restore `ds auth` identity and use
only the UID/email/lane/audience-
fenced project selected by `ds auth project use`. It has no project override or
Desktop descriptor, and ds-brain rechecks membership on the fixed
`POST /api/v1/project-forms` actions `activate` and `settings_editor`. The
settings command accepts one form slug but no project override; ds-brain
rechecks project-form admin authority before returning the backend-owned legal
settings vocabulary and optimistic revision.

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

Four related objects have separate lifecycles:

1. A **Form Factory form** is a global master schema. Use `survey forms list`,
   `survey form read`, `survey form types`, `survey form create`, `survey form
   update`, and `survey form lifecycle`.
2. A **project-form binding** enables a master form for one project and stores
   that project's settings. Use `survey project-forms list` for the selected
   native project's bounded summary and `survey project-form settings` for one
   selected-project editor. Use `survey project-forms read`, `survey
   project-form editor`, `survey project-forms plan`, and `survey project-forms
   apply` for the existing explicit-project Desktop workflow.
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

The native settings read intentionally does not replace the existing Desktop
editor command yet. It proves a selected-project, no-map authority path for
read and planning consumers; mutation parity and an explicit safe handoff must
land before the paired editor/plan/apply route can be retired.

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
