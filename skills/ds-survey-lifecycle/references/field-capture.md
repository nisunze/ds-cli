# Prepare, collect, verify, publish

Choose the host the user actually intends. The DS Field/browser UI acquires GPS,
photos and operator input; native `survey.workspace.*` is a separate local
capture workflow, not remote control of an Android device. Do not claim Android
app-kill/reboot durability from kernel portability alone.

## Before the field visit

For a native workspace, `survey.workspace.prepare --workspace <directory>`
fetches the selected project's complete resolved forms while online.
Alternatively `survey.workspace.init --workspace <directory> --snapshot <file>`
uses an already supplied full resolved project/forms snapshot without sign-in or
network. Keep the project and form revisions. A partial field list or cached
schema is not evidence of permission to publish.

For browser/Field capture, verify the requested project, cached resolved forms,
needed Working Area/reference coverage and local media readiness in that host.
A downloaded map is not the same object as a prepared native workspace.
Do not invent a CLI device-readiness check if the installed surface has none.

## Capture and retain

`survey.workspace.collect --workspace <directory> --form <slug>
--document <file> --created-at <RFC3339>` validates and commits a local entry and
stable replay identity. Supply `--doc-id` only for a known stable source ID.
`survey.workspace.list --workspace <directory>` verifies local inventory/pending
state without network;
it returns summaries, not full field values. Retain the workspace after failure.

Browser capture similarly keeps entry/intent and photo bytes locally before
upload. Supported JPEG/PNG normalization and thumbnails run locally in WASM;
Flutter retains its server thumbnail path. A local pending entry is collected,
not yet published. Initial preparation and later sync still require connectivity.

## Publish only when requested

Use `survey.workspace.sync --workspace <directory> --limit <n> --yes` for the
user's authorized publication. It binds principal, project and lane and stops on
refusal. Verify each bounded batch and remaining pending count. Preserve the
same IDs/replay keys after interruption; never recreate the workspace to clear
an error. Commit receipts prove server acknowledgement, not mirror convergence.

For an already prepared single online document, `survey.entries.create` is the
separate governed create door. For migration of canonical NDJSON, use
`survey.entries.import` with its checkpoint and receipt paths. Keep the immutable
source and state files; resume them rather than minting new IDs. Do not present
that command as a CSV/Survey123 parser. `map.survey.migrate.plan/apply` is a
separate project-copy workflow: verify source and destination, plan first, and
apply only the authorized copy; it is not needed for analysis or routine capture.

Finish with local collected/pending and server-committed counts kept distinct,
workspace/delivery location, form revisions, and any mirror readback evidence.
