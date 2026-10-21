# Native MV preview jobs

`ds-report-host::mv_job` owns a disk job under the existing authenticated
account/project preview room. `start` admits bounded held scene and plan bytes,
resolves the template through `printing::mv_preview`, captures the current
Reporter identity and retains complete private inputs before launching the
fixed `render-grid-plan-profile` task. The exact key includes source bytes,
model revision, layout, governed styles and pens, issue configuration,
coordinate captures, engine identity and retention. Principal, project, lane
and credential audience scope every retained locator. Concurrent exact starts
join one job; a changed key starts separately.

Every consumer receives `ds.mv-preview-job/v1` metadata, never inline SVG or
base64. Ordered ready-page metadata records station windows, source sheet
numbers, digests and sizes. `progress` records completed/total/stage/revision.
Selecting a completed page returns its `ds.print-preview-artifact/v1` receipt;
selecting a pending page returns no page and preserves progress. Status and
unselected pages do not read SVG bytes, invoke an engine, acquire project data
or check remote freshness. An actual file consumer verifies its exact bytes
through the shared native artifact reader.

The Reporter publishes each complete, fully decorated SVG and its bounded
`ds.mv-preview-progress/v1` manifest atomically before rendering the next page.
Small `ds.mv-preview-progress-event/v1` stdout events announce native disk
revisions; closing a consumer pipe does not cancel rendering. Final booklet
delivery keeps its separate atomic publication. A native refusal before
pagination leaves `ds.mv-preview-failure/v1`; consumers can therefore display
failure even when no page plan exists.

Retention defaults to 24 hours; seven days is explicitly admitted. A same-key
consumer keeps the original expiry. The room admits two active jobs and at
most sixteen retained keys. Explicit consuming starts perform a bounded
expiry sweep; native `prune_expired` also supports explicit cleanup. The
producer holds an OS lock for its entire task, including early validation.
Cleanup skips active jobs and validates every owned filename, source/request
digest, page digest and final output inventory before deleting any file.
Foreign files or crossed manifests preserve the whole job. Cleanup never
recursively deletes unknown paths, polls, or contacts an API.
