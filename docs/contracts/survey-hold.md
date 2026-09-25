# Survey hold: the core's copy of a project's survey forms

The Server core (which is also the Desktop's core) holds one read-only copy of
each survey form it has read, per account and project. The CLI, MCP and the
Desktop window read that one copy, so "what is held here, under which filter,
refreshed when" has one answer on a machine.

It is a copy, not a workspace: nothing is edited in it and it has no outbox.
Entry creation and photo publication keep their own governed paths. The
runtime rule "do not recreate the removed SQLite-based native Survey
workspace" stands; the hold is plain files.

## Owners

- Decisions: `ds_command_kernel::survey::hold` — the filter and its
  fingerprint, the manifest, reuse | delta | replace, the merge of a verified
  stream, the local view, the status. The query plan and the stream grammar
  are `survey::read`'s (`plan`, `batch`, `finish`), reused, not repeated.
- Bytes: `ds_project_data::survey_hold` — files under the layer root, a room
  lock, whole-file writes renamed into place. The host injects the fetch; the
  crate never opens a socket.
- Hosts: `ds` commands and the Desktop shell call those two; neither
  re-derives a decision. The Desktop window reads its rows through the
  shell (`survey_hold_read`, `survey_hold_status`) and keeps only what it
  renders, in memory. A browser window has no core: it streams `query_entries` through the
  same read policy into memory for its session and keeps nothing.

## Layout

`<layer root>/survey-hold/v1/<sha256(schema, account, project)>/`

- `forms/<form>/rows.ndjson` — held live rows, one GeoJSON Feature per line,
  exactly as `query_entries` streamed them. Deleted entries are never held.
- `forms/<form>/manifest.json` — `ds.survey.hold/v1`: project, form, the
  canonical filter and its SHA-256, rows, cursor (newest
  `metadata.firestore_updated_at`), created/refreshed times, the last read
  (mode, received, upserts, deletes), and the rows file's SHA-256.
- `media/<object path>` — held photos at their object paths, thumbnails
  (`_thunder.jpeg`) beside them.

## Filter

The working-area filter in `query_entries` fields: `bbox`,
`filter_admin_boundary {code, name}`, `filter_boundary` (GeoJSON polygon),
`date_from`, `date_to`, `surveyors`. The empty filter is the whole form. A copy
answers a read under the same filter, or — held as the whole form — a read
narrowed only by a box, applied locally. Dates, surveyors and boundaries are
the backend's to apply and are never re-derived locally.

## Reads

`Refresh` is `auto` (reuse a covering copy younger than its max age, default
300 s; else refresh the changes since its cursor; else read the form), `delta`,
`full` or `local` (answer only from the copy; never reach the cloud). A delta
asks for tombstones so deletions leave the copy. A cut-off stream, rows after
the summary, or a summary that disagrees with the rows is refused and the copy
is left as it was. Every answer names its source: `held`, `delta` or
`replace`.
