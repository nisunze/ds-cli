# Native print preview artifacts

Rendering and artifact ownership belong to the headless Rust report host.
Disk artifacts are the only printing preview. Inline SVG and base64 page bytes
must not expose a parallel preview route. Draft layout presentation remains an
editor view; it does not establish that a page was rendered.
CLI previews return ordered page manifests and local paths, never inline SVG
page bytes. Desktop consumes the same native manifest and reads the selected
file for display. Transport limits remain bounded independently of page size;
paper size and rendering quality are preserved.

Managed SVG previews live under the disk-backed native account/project room,
isolated by account, project, lane and credential audience. Every manifest pins
the full native result metadata, layout digest, source revision, engine identity,
SVG digest and byte length. Publication is atomic and creates a private complete
directory. Retention is explicitly 24 hours (the default) or seven days from
first publication; exact reuse does not extend expiry. A consuming read refuses
expired, foreign, changed or missing bytes. Consuming writes run a bounded native
prune of validated expired results, including superseded result keys. Expiration
removes only an admitted result's two owned files under its lock; unexpected
files refuse cleanup before either owned file is deleted. Other jobs and foreign
files remain intact. An explicit SVG output is
a verified new copy and never overwrites a source or another output.

Repeat consumers reuse the existing exact native render result and disk bytes;
neither artifact reads nor copies render, fetch project inputs or check remote
freshness. Input/authority admission precedes reuse and remains required on
every host. A missing required input is an explicit acquisition need.

Multipage MV delivery retains native order and per-page station metadata. Its
consumer must be able to read the first ready page and request later pages from
disk without transferring the entire booklet inline. A single bounded sample
preview is one page; it is not proof that a full atlas was generated.
The shared [native MV job contract](../printing/mv-preview-jobs.md) admits full
jobs, intentional status/page consumers, bounded concurrency and safe expiry.

The MV renderer's optional native `preview_progress` target publishes complete
decorated SVG pages and a closed `ds.mv-preview-progress/v1` disk journal while
later sheets are still drawing. Each ready page retains its original source
sheet number, alignment, station window, byte count and digest. Progress names
completed and total sheets, stage and a monotonic disk revision; manifests and
events contain no SVG bytes. `DS_PRINT_PREVIEW` events announce only the exact
input digest and disk revision to an intentional native consumer. Closing its
pipe does not stop rendering or discard ready files. No polling or project
acquisition is attached to event production. Ordinary publication still commits
its complete directory separately and atomically, preserving page order and
the same SVG content, finalized PDF and raster hashes.
