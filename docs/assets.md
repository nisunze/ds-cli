# Project assets authority

`assets::evaluate` and `assets::evaluate_with_bytes` own the Project Assets
model of ds-web `docs/project-assets-contract.md`: the closed `kind`, `status`,
`sensitivity`, `format` and `source` vocabularies, format recognition from
magic bytes, classification with folder-default inheritance and the
never-loosen rule, the ZIP central-directory walk with exact-member
extraction into memory, bounded `PreviewDoc` decoding, and the deterministic
system-folder tree projection with `query` / `link` filters. Native `ds`, the
desktop and the browser call the same two functions; the WASM exports are
`assetsCommand(input)` and the two-slice `assetsPreview(bytes, request)`.

## What hosts own

Hosts own byte acquisition (signed reads, uploads, drag-drop), the catalogue
(ds-brain: `asset_folders`, asset rows, capabilities, audit), PDF rendering
(`pdf.js`, decision D8 — the kernel recognises and bounds pdf and decodes
nothing of it), geo feature decoding (`ds-io` in ds-network, decision D20 —
the kernel plans, resolves shapefile companions and bounds what comes back),
and worker dispatch: any input above the 1 MiB main-thread threshold (1 MiB) runs
through the compute worker pool, never the main-thread kernel singleton.
This module owns no storage, no network IO, no filesystem and no browser API,
and no preview references remote content as fetchable. Extraction to disk
(`--extract <dir>`, decision D11) is a host act with a receipt; the kernel
only ever inflates one member into memory.

Two host rules the kernel cannot check for them:

- `classify` must be handed the **destination** folder document when an
  override moves an asset (the kernel refuses a folder that is neither the
  asset's own nor the override's destination, and one from another project).
- Every `sys:` row's bytes come from its source's existing signed read
  (`origin.source` + `origin.ref` say which); the kernel never carries a URL,
  and a URL-valued source field is reduced to its path before it becomes an
  `origin.ref` (§12.11): query string and fragment go, and so do an
  `http(s)://` scheme and host (`https://storage…/reports/x.pdf?sig` →
  `/reports/x.pdf`); a `gs://` locator is a storage identity and stays whole.

## Actions

`assetsCommand` — one JSON request, `schema: "ds.assets.request/v1"`, at most
`MAX_REQUEST_BYTES` (1 MiB), tagged by `action`:

| action | request | result |
|---|---|---|
| `recognise` | `name?`, `bytes_len`, `head_b64` (first `RECOGNITION_HEAD_BYTES` = 64 KiB recommended; `bytes_len` must be at least the head's length) | `format`, `kind`, `previewable`, `reason` |
| `classify` | `asset`, `folder?`, `override? {kind?, status?, sensitivity?, owner?, folder?, reason?}`, `caller_capabilities[]` | `asset`, `changed[]` (field names in document order), `audit_required` |
| `tree` | `sources{<name>: [rows]}` over the 19 names of `TREE_SOURCE_NAMES`, `folders[]`, `assets[]`, `query?`, `link?`, `folder?`, `depth?`, `kind?` | `ds.assets.tree/v1` plus the not-loaded markers below |

`assetsPreview` — the document bytes (at most `MAX_PREVIEW_INPUT_BYTES`,
32 MiB) beside one JSON request with the same schema:

| action | request | result |
|---|---|---|
| `container_walk` | `format?` — any ZIP-based format (`zip`, `kmz`, `dsgrid`, `docx`, `xlsx`, `odt`, `ods`; default `zip`); `7z` / `tar.gz` are reported as packs and not walked, anything else "is not a container" | `ds.assets.container/v1` carrying the declared `format` |
| `extract_member` | `member`, `max_bytes?` (capped at 32 MiB) | `member`, `bytes`, `digest` (`sha256:<hex>`), `bytes_b64` (standard alphabet, padded) |
| `preview` | `format`, `member?` (a path inside the bytes), `sheet?` (a worksheet name, xlsx only), `pages?` (≤ 5), `rows?` (≤ 200), `name?` | `ds.assets.preview_doc/v1` |

PDF preview checks the signature and existing byte bounds, then returns a
metadata block with MIME type, byte count and `pdf.js` as the renderer. The
host holds and renders the bytes; the document contains no PDF content or
guessed page count.

### Native embedded PDF text host

`crates/ds-assets-pdf-host` is a separate native host, outside the portable
workspace and WASM artifact. `extract(bytes, PageRange { first, last })`
accepts governed bytes, never a path, URL, credential, executable, password,
environment or argv. CLI signed-read authorization and digest verification
remain unchanged. Default metadata preview and PDF.js rendering remain unchanged.

The opt-in returns `ds.assets.pdf_text/v1`, with actual `total_pages`, inclusive
`first_page`/`last_page`, and `pages: [{page,text,text_bytes}]`. Page numbers
are 1-based; no partial range is returned. `total_text_bytes` counts UTF-8
text bytes; `truncated` is always false on success. The CLI adds exact asset
identity, source digest and source byte count. Empty text establishes only
that the page has no extractable embedded text. No OCR, remote references,
annotations, external content or visual layout reconstruction are processed.

Linux admits only `/usr/bin/prlimit`, `/usr/bin/pdfinfo` and
`/usr/bin/pdftotext`, installed by `util-linux` and `poppler-utils`. The host
uses a private temporary directory and a 0600 fixed input filename, removes
them after success or refusal, clears the inherited environment, and supplies
fixed locale/private HOME/TMPDIR. There is no shell, PATH lookup or fallback
decoder. Other hosts/missing helpers refuse `pdf_text_unavailable`.

| Resource | Hard bound |
|---|---|
| Governed input | 32 MiB and PDF signature |
| Actual document pages | 10000 |
| Requested inclusive range | 1–5 actual pages |
| Decoder address space / CPU | 256 MiB / 5 seconds per subprocess, enforced by prlimit before exec |
| Decoder operation deadline | 8 seconds shared by metadata and all selected pages |
| Extracted UTF-8 text | 64 KiB per page (including decoder framing), 256 KiB total |
| Metadata / diagnostics | 32 KiB / 16 KiB |
| Decoder file size / descriptors / core | 256 KiB / 32 / disabled |

The private closed decoder choice builds fixed arguments: metadata inspection,
or text extraction for one validated numeric page. Pipe readers stop at their
bounds, the host kills and reaps a timed-out/over-output child, and returns a
refusal rather than truncated text. Poppler warnings/nonzero exits refuse
malformed or unsupported content; diagnostics and private paths are not exposed.
Encrypted PDFs, including password-free encrypted documents, are refused.
Invalid UTF-8 or replacement characters refuse unsupported text.

Process separation is intentional: the existing Rust reader's stream limits
do not bound cumulative object allocation or CMap range expansion and its
font fallback may silently guess characters. Poppler runs under OS limits;
no reader/parser or process dependency enters the portable kernel. Dependency
installation belongs to the operator; there is no automatic install or remote
build. The CLI's process-boundary gate explicitly admits this one native owner.

Every result carries `action`; documents carry their own schema and the
others carry `ds.assets.request/v1`. Request structs deny unknown fields;
source rows handed to `tree` are foreign documents and tolerate them.

`member` and `sheet` are different words on purpose: `member` is always
extracted from the container first (bounded by the member format's own cap,
declared size and inflated size both), and the decoder then receives the
member's bytes; `sheet` is read by the workbook decoder only. So "sheet
`Data` of `Lot3/boq.xlsx` inside this zip" is one request. The one exception
is `shp`, whose planner receives the whole container because a shapefile is
its companions too.

Order of checks in `evaluate_with_bytes`: the request bound, the input bound,
then the per-format cap of §4.2 (`format_cap_bytes`) — before any decoder
sees a byte. A `recognise` head longer than `bytes_len` is refused.

A zip member's declared size is a claim, never an allocation: extraction
reads through a bounded `take` into a buffer that grows with what actually
inflates, so a header that lies (0 bytes declared, 4 MiB inflated) is refused
at the bound plus one byte, and a header that overstates is refused before a
byte is read. Member names are reported exactly as the archive spells them
(`../../evil.txt`, `/abs/root.txt`, `C:\win\x.txt`) and are only ever looked
up by that exact name inside the bytes; nothing is a path on a disk. OOXML
relationship targets are resolved segment by segment against the package
(`word/` or `xl/`, absolute targets from the root); a target that climbs past
the root names nothing — a docx image with such a target is shown without
bytes and the target is not echoed, and an xlsx worksheet falls back to its
positional part name.

## Schemas

`ds.assets.asset/v1` and `ds.assets.folder/v1` are the §2.1 / §2.2 documents
(`schema` defaults when absent and is validated when present; `origin` is
only ever present on projected rows and is never written to Firestore, D16).

`ds.assets.container/v1` lists members with `path`, `bytes`, `kind`,
`format`, `companions` and `nested`, plus `walked`, `truncated` and
`member_count_total`. Members are classified from their names (a listing
reads the central directory, not the members); a member whose format is a
pack is `nested: true` and is not walked — the one nested level
is walked by the host chaining
`extract_member` then `container_walk`. Shapefile sidecars (`.dbf`, `.shx`,
`.prj`, `.cpg`, case-insensitive stem match) fold into their `.shp` row's
`companions` and lose their own rows; a `.shp` without both `.dbf` and `.shx`
keeps an **empty** `companions` list, which is the wire form of "incomplete
shapefile" and is what the geo planner refuses by name.

`ds.assets.preview_doc/v1` carries `format`, `kind`, `bounded`, `truncated`,
`notes[]` and `blocks[]` tagged by `type`:

| block | fields | who emits it |
|---|---|---|
| `heading` | `level`, `text` | md, docx |
| `paragraph` | `text` | md, txt, docx |
| `list` | `items[]`, `ordered` | md, docx |
| `grid` | `title?`, `columns[]`, `rows[][]`, `rows_total`, `rows_shown`, `columns_total` | csv, tsv, xlsx (columns are `A`…`AN`, every row stays a row), docx tables |
| `image` | `index`, `mime`, `width`, `height`, `bytes_b64?` | image (dimensions only, bytes never carried — the host holds them), docx inline images (bytes carried ≤ 1 MiB each, 20 max) |
| `mail` | `headers[]`, `text`, `attachments[{name, mime, bytes}]` | eml |
| `geo` | `decoder`, `member?`, `companions[]`, `feature_count?`, `bbox?`, `refused?` | geojson, shp, kml, kmz, gpkg |
| `meta` | `entries[]` | xlsx sheet tabs (`sheet 1` … in workbook order) |

`ds.assets.tree/v1` nests `folders[]` with `kind`, `path`, `name`, `counts`
(own assets and direct children, after filtering — and `counts.folders` stays
the folder's own when `depth` hides its children; the cut is told by
`truncated`), `children`, `assets`, and carries `total_assets`,
`total_folders`, `truncated`. `depth: 0` answers no folders and `truncated:
true`; a `folder` path nothing declared or projected answers no folders and
no error. `query` is a literal case-insensitive substring — `(`, `.*`, `[1]`
and `^` are characters, not a pattern — and a blank query filters nothing. On the wire the tree
also carries the §2.5 *not loaded* markers the shared types have no field
for: `not_loaded_sources[]` on the response, and on a system root
`not_loaded: true` when every source feeding it was absent or
`not_loaded_sources[]` when only some were. An absent source is never
rendered as an empty root; a loaded-and-empty source is.

## Bounds

Every bound is a `pub const MAX_*` in `src/assets/mod.rs` citing its contract
section, and a bound is a refusal naming the bound and the actual number
("preview input is 34,603,008 bytes; the pdf bound is 32 MiB"). Where a
document is legitimately cut at a display bound the result says so through
`truncated` and its totals; nothing is trimmed silently.

| bound | value | applies to |
|---|---|---|
| `MAX_REQUEST_BYTES` | 1 MiB | the JSON request of both entry points |
| `MAX_PREVIEW_INPUT_BYTES` | 32 MiB | the bytes of `evaluate_with_bytes` |
| `MAX_PDF_BYTES` | 32 MiB | pdf (bounded here, rendered by pdf.js) |
| `MAX_DOCUMENT_BYTES` | 8 MiB | docx (doc, odt, rtf share the row, never decoded) |
| `MAX_WORKBOOK_BYTES` | 8 MiB | xlsx (xls, ods share the row, never decoded) |
| `MAX_DELIMITED_BYTES` | 8 MiB | csv, tsv |
| `MAX_MAIL_BYTES` / `MAX_MAIL_BODY_BYTES` / `MAX_MAIL_PARTS` | 4 MiB / 200,000 bytes / 1,000 | eml (msg shares the input row, decoded in slice 2); parts past the bound are counted into a note, never walked |
| `MAX_TEXT_BYTES` | 1 MiB | md, txt |
| `MAX_IMAGE_BYTES` / `MAX_THUMBNAIL_PX` | 12 MiB / 2048 px | jpg, png, webp, tif (thumbnail edge is a host rule published here) |
| `MAX_GEO_BYTES` / `MAX_GEO_FEATURES` | 20 MiB / 20,000 | geojson, kml, kmz, gpkg, shp |
| `MAX_GRID_ROWS` × `MAX_GRID_COLUMNS` | 200 × 40 | every grid block |
| `MAX_DOC_BLOCKS` | 200 | md, txt, docx |
| `MAX_INLINE_IMAGE_BYTES` × `MAX_INLINE_IMAGES` | 1 MiB × 20 | docx inline images |
| `MAX_PDF_EAGER_PAGES` | 5 | a host rule for pdf.js |
| `MAX_CONTAINER_MEMBERS` / `MAX_CONTAINER_DEPTH` / nested archive levels | 5,000 / 8 / 1 | `container_walk` (`truncated: true`, `member_count_total` keeps counting) |
| `MAX_TREE_SOURCE_ROWS` / `MAX_QUERY_CHARS` | 20,000 / 200 | each `tree` source list, `folders`, `assets` / `query` |
| the 1 MiB main-thread threshold | 1 MiB | host dispatch: above it, the worker pool |
| `RECOGNITION_HEAD_BYTES` | 64 KiB | the head a host should send with `recognise` |

Decoders carry private scan bounds of their own (shared strings, worksheet
scan, relationship rows, XML items and characters, multipart depth, a
500-character cut per grid cell in csv/tsv and xlsx, 2,000 in a docx table
cell) so a hostile package is refused with a number rather than scanned to
exhaustion; each is a `const` at the top of its file. Remote-reference
stripping (md, txt, eml) is one scan per scheme, so a note that is nothing
but a hundred thousand links costs milliseconds, not minutes. XML entities
declared in a DOCTYPE resolve to nothing (only character references and the
five predefined entities are read), the parsers are flat state machines with
no stack to exhaust, and a truncated archive, image header, JSON document or
header block is a sentence, never a panic.

## Recognition

Magic bytes decide first and the name only breaks ties between formats that
share a container: a ZIP is `zip` unless the name says docx / xlsx / odt /
ods / kmz / dsgrid (an ODF package's stored `mimetype` member is byte
evidence and beats the name), an OLE compound file is `msg` unless the name
says doc / xls, and a SQLite file is `gpkg` only with the `GPKG` application
id. `.bak`, `.backup`, `.old`, `.tmp`, `.part`, `.download` are wrappers that
name no format and the reason says so. A name that contradicts the bytes
never wins: a `.xlsx` whose bytes are binary non-ZIP is `unknown` ("the name
says .xlsx but the bytes are not a ZIP archive"), one whose bytes are legible
text is `txt` with the conflict named — `unknown` is reserved for bytes that
match no signature and are not text. Text is sniffed as GeoJSON, KML, RFC
5322 headers (`eml`), quote-aware delimited text (`csv` / `tsv`), then `md`
by name or `txt`. An empty head with a name answers the name's format with
"no bytes were read; the name's .shp suffix decided (a container listing
reads names, not members)" — the contract the walk relies on, pinned by
`assets_zip_member_names_are_read_not_guessed`. An empty file is `unknown`.

`previewable` is the closed §4.1 table: pdf (host), docx, xlsx, csv, tsv,
eml, md, txt, geojson, shp, kml, kmz and the four image formats. `msg`
(slice 2), `gpkg` (no SQLite reader in WASM), `odt` / `ods` (slice 1 decodes
OOXML only), doc / xls / rtf, dsgrid, the packs and `unknown` are not, and
`reason` says why in the same sentence `preview` would refuse with.

## Classification

Three rules (`classify.rs`): the kernel infers `kind` from the format and
`status` from the folder's `default_status` only while `classified_by` is
`kernel`; a default may only close (floor `internal`, D4; a folder's
`default_sensitivity` is applied with `stricter`, so even an `open` folder
cannot open an asset and inference never reaches `open`); loosening needs
`assets.classify` plus the capability that reads the class being loosened
*from* (`capability_for`: `assets.read`, `assets.read.commercial`,
`assets.read.confidential`), taken after folder inheritance so a
confidential folder cannot be escaped by a looser incoming row. Every
override field needs `assets.classify` and is refused by field name; an
override always sets `classified_by: user`.

Decisions taken at integration, each a one-line change if the owner rules
otherwise:

- a folder's `default_sensitivity` is applied to user-classified rows too
  (it can only tighten); kind / status inference and the internal floor are
  skipped for them;
- `audit_required` is `override.is_some() || !changed.is_empty()` — every
  classification change is audited (§6.4), not only sensitivity;
- `override.reason` is carried, never required;
- `owner.kind` is a free string (§2.3 names "person or team" without a
  closed vocabulary); only `kind` and `id` must be non-empty.

## The system-folder tree

`tree.rs` projects the source listings of §2.5 into the eleven system roots
(`Transformers`, `MV models`, `Survey`, `Project data`, `My data`, `Project
work`, `Reports`, `Solar`, `Local data`, `Prints`, `Unclassified`), merges
the declared user folders and catalogued assets, then filters by `query`
(case-insensitive substring over name, path, kind, format, status and
owner display), `link`, `folder`, `depth` and `kind`. It reads nothing and
caches nothing; the "one read per source per refresh" rule is the host's.

The `sys:` id convention names the source so `read` / `preview` can call the
source's own read action: `sys:design_attachment:<attachment_id>:<revision_id>`,
`sys:pm_attachment:<id>`, `sys:grid_revision:<model_id>:<revision_id>`,
`sys:report_file:<transformer>:<sha256|filename>`, `sys:compounded:<stem>`,
`sys:upload:<transformer>:<blob_path>`, `sys:transformer_design:<name>`,
`sys:transformer_version:<name>:<version_id>`,
`sys:grid_export:<model_id>:<export_id>:<output_id>`,
`sys:survey_media:<form>:<entry_id>:<field>`, `sys:project_data:<upload_id>`,
`sys:user_data:<upload_id>`, `sys:compute_artifact:<work_id>:<output_id>`,
`sys:solar_result:<city>`, `sys:solar_artifact:<city>:<name>`,
`sys:dataset_room:<dataset>`, `sys:national_dataset:<dataset>`,
`sys:report_room:<transformer>`, `sys:local_print:<locator>`,
`sys:print_setup:<layout_id>`, `sys:unclassified:<source>:<ref>`. Every
projected row is `sensitivity: internal`, `classified_by: kernel`, carries
`origin: {source, ref}`, and takes its status from the source semantics of
§2.5. Reserved transformer documents (`mv_data`, `combined_transformer` and
its aliases, `collisions*`, matched after ds-brain's canonicalisation so
`MV_DATA` and `Combined Transformer` count) are never folders; a row no rule
can place lands under `Unclassified/<source>/…`, never dropped, and a
malformed row lands there too. Rows are decoded leniently (no
`deny_unknown_fields`, a surprising type reads as absent), so nothing here
panics on a foreign document. One `sys:` id is one asset: a row a source
hands over twice (an overlapping page, a retry) is placed once, so a folder
count never doubles. A source that reports no byte count says nothing about
emptiness, so a size-less `report.xlsx` is still `sheet` / `xlsx` from its
name (0 means "not reported", not "empty"). A design attachment whose
transformer is not in the `transformers` source still lands under
`Transformers/<name>/attachments/…` — the object key alone names it.

## The ds-io geo plan (D20)

The kernel does not link `ds-io`. For `geojson` it parses, counts features
and computes the bbox itself (`decoder: "kernel"`); above
`MAX_GEO_FEATURES` the block is refused with both numbers and the honest
alternative ("promote it as a layer"). For `shp`, `kml` and `kmz` the block
says `decoder: "ds-io"` with the member the host should hand its existing
decoder (`shp_zip_bytes_to_layer`, `kml_text_to_layer`, `kmz_bytes_to_layer`
in ds-network, pooled as `ds-network-wasm` ops) and, for a shapefile, the
companions resolved from the container's central directory; a shapefile
without `.dbf` / `.shx` is refused by name, and the `.shp`'s own declared
size is checked against `MAX_GEO_BYTES`. `gpkg` answers a `geo` block whose
`refused` names the missing SQLite reader. Malformed bytes are an `Err`;
a bound is a refused block, so the deck shows a card that says why. No geo
block ever carries a URL, and nothing is fetched.

## Files

`src/assets/mod.rs` (schemas, bounds, vocabularies, documents, the two
request grammars, `evaluate`, `evaluate_with_bytes`), `format.rs`,
`classify.rs`, `container.rs`, `tree.rs`, and
`preview/{mod,text,tabular,xlsx,docx,eml,image,geo}.rs`. Every file keeps its
tests at the bottom under the dual native / `wasm-pack test --node` attribute;
fixtures are built in memory (`zip::write::ZipWriter`, hand-written XML, CSV
and EML), never binary files. `src/lib.rs` exports `assetsCommand` and
`assetsPreview` and holds the native / browser parity cases in `wasm_tests`.

### Project assets and device helpers in a web host

Published compute heads, uploaded project data, custom map assets and print
setups belong to the explicit project and remain discoverable on the web.
Dataset rooms are a device cache of reproducible inputs, not an uploaded
project document. Local print artifacts are files retained by that device.
They may be projected alongside project assets on a native host, but their
absence on the web does not mean the project preview is incomplete.

The host checks structural I/O capabilities before calling a device inventory.
An unavailable inventory remains absent (never an invented empty list) and is
recorded separately from a failed project-source read. Authentication, offline
reads and genuine project-service failures retain their existing refusals.

The shared `compute_artifact_inventory::decode_head` validator owns safe
published-head decoding for native and WASM consumers. Solar operations are
`calculate-<city>`, `report-<city>` and `portfolio-<portfolio>`; each governed
identity has its own 128-byte bound. Portfolio identities are not city ids.
Head identity, positive revision, RFC3339 time, SHA256 input fingerprint and
bounded nonempty output declarations must all validate. Additive server
metadata does not change the safe projection. A malformed cloud head remains
a refused project read; it is not reclassified as an optional local helper.
