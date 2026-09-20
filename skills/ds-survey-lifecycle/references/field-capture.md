# Prepare browser capture and verify delivery

Offline Survey capture uses the browser application and its IndexedDB store.
The separate native CLI Survey workspace is retired; do not discover or suggest
its former prepare/init/collect/list/sync operations as a fallback.

## Before field work

Verify the selected project, cached resolved forms, required Working Area or
reference coverage, and local media readiness in the actual browser host.
Keep the relevant form revisions. Cached metadata does not grant publication
permission. Do not invent a CLI device-readiness operation.

## Capture and retain

Browser capture commits the entry and pending mutation together before painting
success. Photo bytes are retained locally before upload. Supported JPEG/PNG
normalization and thumbnails run locally in WASM; Flutter retains its existing
server thumbnail path. Local pending means collected, not yet published.
Initial preparation and later publication still require connectivity.

## Held photos and the one rotation

`survey.moments.list --project <id>` is what this machine holds: the Server's
survey-media store, read without a credential or a running Server. `waiting`
rows are rotations not yet published (the only copy); `synced` rows equal the
published head. `survey.moments.read` gives one photo's details and bundle
paths. Rotate with `survey.photo.rotate --path <original> --degrees 90|180|270`
(net clockwise; 270 is counter-clockwise): the result is held `waiting`, a
further turn turns the held bytes, a full circle discards the wait; publish
the exact held bytes with `survey.photo.publish --path <original> --yes`. A
thumbnail, URL or foreign path is refused before any byte moves. The browser
answers the same commands over its own photo cache; do not read IndexedDB
or the store files directly.

## Publication and migration

Follow the browser's supported synchronization workflow and verify pending,
blocked and committed work separately. Preserve identities and replay keys
through interruption. Server commit receipts do not prove mirror convergence;
governed readback supplies that evidence.

For an already prepared online document, use the existing governed
`survey.entries.create` operation. Canonical NDJSON migration uses
`survey.entries.import` with its retained checkpoint and receipt. Read the live
contract for the operation needed; import is not a CSV/Survey123 parser.
Project-to-project map migration is a separate explicit copy workflow.

If the user has files from the retired native workspace, preserve them. Removal
of the command does not delete or publish those files. Treat recovery as a
separate scoped task; do not mint replacement IDs or claim automatic migration.

Finish with captured/pending and server-committed counts kept distinct, the
actual capture host, form revisions and any mirror readback evidence.
