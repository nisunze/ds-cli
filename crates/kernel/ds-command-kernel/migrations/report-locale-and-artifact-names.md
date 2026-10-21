# Report locale and artifact-name backfill

Do not run this migration during print-lv development. Default mode is dry run.
The kernel's migration planner consumes captured project configuration and
artifact metadata. It emits conditional changes; it does not read or write
Firestore or objects. A privileged migration host must use the same plan and
reject stale revisions, absent source hashes, duplicate targets and unknown
formats before applying it. Never probe a second filename in report readers.

Locale source: `eds_project/{project}/docs/network_config`, existing
`parameters.project_settings` (flattened `project_settings` in `/config`).
Add exactly one `report_locale` parameter with an explicitly reviewed locale.
Preserve existing locale values. Do not derive language from country or the
operator's UI language. Update the Network Template's corresponding setting
for subsequently created projects in the same reviewed migration.

Name source: artifact declarations / `report_metadata.cached_urls`, local
publication outbox names, combined-report metadata and archive member indexes.
Canonical target is `report_formats::report_filename(stem, format)`. PDF stems
use title case, exchange/raster identifiers stay exact, voltage-drop PDFs
retain the established human word-spacing rule. No PDF/PNG/SHP bytes change.
For each object: copy to the canonical key using the source generation and
SHA-256 preconditions; verify bytes; commit all metadata pointers and filename
fields under the captured document revision; only then retire the old key.
An existing different target is a refusal, including case-folding collisions.
Unchanged objects are skipped. ZIP member names and their manifest pointers
must be changed together by the archive owner, with a new ZIP digest.

Gateway integration outside this slice: consume the same kernel rule in
`network_reporter_freshness.go`, `compute_artifact_discovery.go` (`computeArtifactDownloadFileName`), and archive readers. Regenerate the
kernel contract cases rather than introducing another casing table. Readers
must use the canonical metadata after backfill; no old-name fallback.

Offline plan entry point (no mutation):

```sh
cargo run --example report_backfill < captured-snapshot.json > reviewed-plan.json
```

The planner and example have no apply mode, IO host, credentials or project
lookup. Application must be implemented by the authorized gateway migration host
using the listed conditional operations; never apply it by a direct ad hoc API
or storage loop from reporting. Both backfills are written, not run.

Audited gateway readers that must consume the kernel contract at integration:
`compute_artifact_discovery.go` (lazy status and downloads),
`compute_artifacts.go` (download response), `network_reporter_output_merge.go`
(output replacement), `network_reporter_status_projection.go` (legacy pointer
projection), `network_reporter_freshness.go` (suffix classification), and
archive member indexes. Existing suffix coverage is not a second filename lookup;
target-name changes still require the metadata/object backfill.
