# `ds data`

`ds data` prepares a local file for analysis. It converts a source to
GeoParquet as a named step that happens *before* anything queries it — never
silently inside an import.

Inspection and conversion run entirely locally. Admin-bound attachment also
runs locally, but pairs with the desktop to resolve the active project's
digest-pinned Rwanda reference asset. None of these commands needs a map open.

Native elevation attachment pairs with Desktop for the governed Rwanda DEM and
native engine. It writes a new local GeoJSON; it does not upload the source or
result and does not need a signed-in project or an open map.

Point-cloud extraction is the complementary operation: it takes an explicit
area and creates points before attaching elevation. It is not a map workflow:
use an absolute local area file or a WGS84 bounding box, so the same commands
work through MCP with no map open.

```text
inspect → (choose the sheet, layer, or coordinate columns) → convert
```

## Native local vector processing

`data vector measure`, `buffer`, `sample` and `intersect` call the same compiled
Rust operations used by the map. They import GeoJSON, KML or Arrow files
and return JSON receipts with bounded previews. --out exports layer data as
GeoJSON or Arrow IPC. Read each
live descriptor for its mutually exclusive inputs and bounds. MCP inline
documents are strings containing JSON text, not object arguments; all four
tools are in the `datasets` profile or the broad `ds_data` chapter. Sampling
returns points along lines and intersection returns line-crossing points;
neither promises line densification or polygon overlay. Check `skipped`,
`more` and `note` for eligibility and completeness. An output file includes
all produced results, but cannot include source features excluded by a bound.
Use the shipped `ds-vector-tools` skill for this local workflow.

## `mv-lv-orphans`

`data mv-lv-orphans` joins MV lines to transformers within `--tolerance-m`,
offline and with no project. `--mv` takes GeoJSON lines (repeat per file, for
example the files `ds dsgrid project geojson` writes); `--transformers` takes
named GeoJSON points or the JSON `ds design status --output json` wrote, whose
individual rows carry `metadata.spatial.representative_point` (a row without
one is skipped as `no_location`; a point taken from the room extent rather than
a transformer layer says `location: extent_centre`). A line end within
`--junction-m` (default 1 m) of another MV line is a junction; every other end
is a tip. Tips and transformers farther than the tolerance from their partner
are orphans, and alignments none of whose tips reaches a transformer are listed
in `alignments_ending_at_no_transformer`. `--exclude-prefix` keeps transformers
such as fill-in placeholders as tip targets but never judges them. Distances
are geodesic metres; no projection or country is assumed. Counts are complete,
each list is bounded by `--limit` and reported in `more`, `skipped` counts every
dropped feature by reason, and `--out` writes every orphan as a point for
`ds map local register`.

## `admin-bounds attach`

Writes a new CSV, TSV, or GeoJSON elevation-point file carrying `province`,
`district`, `sector`, `cell`, `village`, and `code_village`. Geometry,
elevation values, and non-empty operator-supplied admin values are preserved.
CSV/TSV callers name longitude and latitude columns explicitly; GeoJSON uses
feature geometry. The source is never overwritten.

## `admin-bounds list` and `admin-bounds read`

These commands read the authenticated Rwanda administrative-boundary authority
that Desktop Search place reads. They do not infer boundaries from sampled
points or reconstruct polygons in the CLI. They are national reference data: no
project is selected, none is fenced, and no window is involved — since
2026-09-18 they call the gateway directly as the restored native user, so they
answer the same on a server, in CI and beside a desktop.

```bash
ds data admin-bounds list --country rwanda --level province --output json
ds data admin-bounds list --country rwanda --level village --parent-code 110101 --output json
ds data admin-bounds read --country rwanda --code 11010102 --output json
ds data admin-bounds read --country rwanda --code 11010102 --geometry-out ./gihanga.geojson --output json
```

`list` always returns one bounded hierarchy leg: provinces need no parent;
districts, sectors, cells and villages require the exact immediate parent code.
Rows are accepted only when every code has the requested level length and parent
prefix, every name is non-empty, and no code is duplicated. A leg with no units
answers `count: 0`, which is a fact about the hierarchy rather than a failure.
`read` returns code, name, level, geometry type, bounds, coordinate-position
count and SHA-256 for an exact Polygon or MultiPolygon. Other geometry types,
malformed coordinates, a response whose code or level does not match the
request, or a missing name are refused as unreadable authority data.

The coordinates are not printed: a province is half a megabyte of them. Pass
`--geometry-out <path.geojson>` to keep the exact bytes as a one-feature
`FeatureCollection` carrying `name`, `code` and `level` — the same three
properties the Desktop's own admin layer writes, so `ds map local register`
takes the file as it stands. An existing path is refused before the read, and
nothing is ever overwritten.

Rendering a boundary in a RUNNING desktop's map is not one of these commands.
That is presentation state — a local overlay and a camera move — and it belongs
to the `map.*` surface; Search place inside the application still does it
directly. `--to-map` was removed with the bridge on 2026-09-18.

The only declared country authority is `rwanda`. A service failure is a hard
refusal: never substitute a lattice, bounding rectangle or approximate polygon.
Malformed scope is `invalid_admin_scope`, raised here before anything is sent.
An authority that cannot answer is `auth_transient`; an answer outside the
exact hierarchy contract is `auth_response_unreadable`; the native identity
refusals (`headless_signed_out` and the rest) are the same ones every headless
command raises.

## `elevation attach`

Interpolates point sources through the native Desktop engine. CSV/TSV can name
their coordinate columns and CRS; geometry formats carry coordinates. A
`common-column` keeps each named surface all-or-nothing. Rwanda is the sole
provider; coverage holes stay explicit. The compatibility flag accepts only
`--fallback none`, which is also the default. Jobs above 4,000 parsed points require the
verified full local Rwanda DEM component; the Desktop component manager installs
or verifies it once before the operation retries. Smaller jobs may read exact
public COG ranges through the bounded Desktop cache.

Hypothetical requests are intentionally short and discoverable from the live
command descriptor:

```text
ds data elevation attach --source /data/poles.csv --out /data/poles-elevation.geojson --x-column longitude --y-column latitude --source-crs wgs84_lonlat
ds data elevation attach --source /data/alignment-points.tsv --out /data/alignment-elevation.geojson --common-column alignment --fallback none
```

## `elevation plan` and `elevation extract`

`plan` counts the points a boundary and sampling choice would generate without
reading the DEM or writing a file. It reports the 4,000-point browser/cloud
admission boundary and whether the Desktop needs the verified full Rwanda DEM
component. `extract` then generates that exact deterministic grid or
seeded-random cloud, samples the DEM locally, and writes a new GeoJSON plus a
CSV sibling. It never calls Cloud Run.

The area is either `--area <absolute GeoJSON|KML|KMZ|shapefile zip>` or
`--bbox west,south,east,north`. Grid extraction requires `--spacing-m`. Seeded-random
extraction requires `--seed` plus exactly one of `--count` or
`--density-per-km2`; no density or seed is guessed.

```text
ds data elevation plan --bbox "30.05,-1.95,30.06,-1.94" --mode grid --spacing-m 25
ds data elevation extract --area /data/sector.geojson --out /data/sector-elevation.geojson --mode seeded_random --seed 2026 --count 2000 --fallback none
```

Above 4,000 points, the Desktop component manager installs or verifies the
full local DEM and retries unchanged. A single extraction remains bounded; if
the requested cloud is beyond that bound, the command refuses with a split or
coarser-sampling remedy. Source-area attributes and provenance remain on every
generated point; any conflicting generated field is reported as a preserved
rename in the receipt.

## `terrain describe` and `terrain sample`

`data terrain sample` creates representative terrain samples from one explicit source: the governed Rwanda TIFF or a declared survey CSV surface. Discover `data terrain describe` first, then supply its strict native request file. Rust evaluates a two-dimensional surface at actual route XY, retains genuine terrain changes, removes points only within the requested discrete baseline error, and enforces adjustable density limits tightened by supplied engineering intervals or weight spans. Seeded longitudinal randomness changes sampling positions, never survey observations or terrain heights. Side profiles are off by default; explicit signed offsets select their widths and side observation randomness stays separate from nominal profile cuts. Output samples are derived/interpolated and retain settings, methods, input digests and source coverage; no model edit, source mixing, offset or datum adjustment. Raster resolution, sampling spacing, source uncertainty and simplification error are distinct. Missing surface readings remain gaps; unsupported authored breaklines are refused. Results cannot certify continuous unsampled terrain or missing engineering context.

## `inspect`

Reports what the source actually holds, so `convert` consumes a fact rather
than a guess. What comes back depends on the kind of source:

- **A table** — CSV, TSV, XLSX — reports `sheets`, each with its cleaned column
  names, row count, dropped count, and whichever columns look like coordinates.
  You still have to say which ones are, because "looks like" is not a decision
  the converter is entitled to make on its own.
- **A source carrying its own geometry** — GeoJSON, KML, KMZ, zipped
  Shapefile — reports `layers`, each with its feature count, geometry type and
  declared CRS. No coordinate columns are involved.

`carries_geometry` tells the two cases apart without you having to infer it
from which key is present.

## `convert`

Writes one GeoParquet file to the columnar format contract: WKB geometry,
CRS84 declared as explicit PROJJSON, SNAPPY, statistics on every column. The
full set of pinned decisions, and the reasoning for each, is in
`ds-network/docs/contracts/columnar-format-contract.md`.

Conversion is refused rather than guessed in two cases that matter:

- A table whose coordinate columns are not named. Run `inspect` and pass them,
  or pass `--attributes-only` if the table genuinely has no geometry.
- A source holding more than one layer, with no `--layer` given. The refusal
  names the layers rather than converting the first and silently dropping the
  rest.

Geometry is stored in CRS84 whatever the source used, so a NIX / Rwanda TM
shapefile is reprojected on the way in. Projected frames are applied at
analysis time; one frame at rest, many in use.

### The receipt

Every conversion returns a `source_digest` (sha256 of the source bytes) and a
`conversion_id` (that digest plus the canonical parameters). Re-converting an
unchanged source with unchanged options produces the same `conversion_id`, so
it is detectable and skippable, and a generated artifact can be matched back to
what produced it. `skipped_coordinate_rows` reports rows that had no usable
coordinate and therefore carry no geometry — reported, never silently dropped.

## `project-cache status` and `project-cache seed`

A project holds bounded, spatially indexed extracts of the datasets its
workflow declares, instead of downloading a whole national layer it will mostly
not use. Coverage is the design's own footprint, buffered and fused, so
neighbouring transformers share one acquisition.

`status` costs nothing and reaches no provider. `seed` is the one command here
that queries a geographic source, so it is confirmed.
For dataset discovery, `status --project <id> --summary --output json` returns
identities, source, residency, supported project seed and bounded read
commands, row caps, readiness, and coverage counts. The default response keeps
the detailed coverage geometry and acquisition history for diagnosis.

Both run headlessly, under the restored native user against the project
named by `--project` (the saved selection is never read), on this machine's
holdings — no paired Desktop. The holdings live under the shared
geographic data root (`rw.datasolutions.desktop.shared`, the same root the
desktop uses), so a desktop signed in as the same account on the same machine
reads the rooms a headless seed filled. The orchestration is
`ds-web/crates/ds-project-data` (shared with the desktop shell); every decision
is `ds-command-kernel`'s.

Two families of dataset, seeded differently:

* **National catalogue layers** (roads, rivers, wetlands, districts, … — what
  ds-brain publishes for the country from its own BigQuery/GCS): the published
  bundle is installed on the machine **once** (verified, expanded, indexed),
  and each project holds a cheap local subset of it. Nothing is downloaded on a
  read; a row without a published bundle is refused
  (`reference_bundle_unavailable`), never fetched from BigQuery.
* **Building footprints and elevation contours**: acquired **per project**
  through ds-brain's governed `query_print_context` door, into the project's
  rooms, decoded and bounded by the kernel.

`ds report project export --seed` runs the same acquisition for the printed
transformer only, so the first print request seeds; without `--seed` a print
never reaches a provider.

### What an omitted `--dataset` seeds

Every dataset this project's workflow **declares**, plus any it already holds.
The declaration is the printing context catalogue's own answer for the project —
decided once in `ds-command-kernel`'s `printing::context` for the Printing setup
page and this command alike, so the two can never disagree about what a project
needs. It is not everything published for the country: a catalogue layer the
default selection excludes, or one the catalogue reports unavailable, is not
declared.

**A computer that holds nothing yet seeds the declaration.** That is the point
of the rule: first use is when an operator needs this most, and refusing with
"this project holds no dataset yet" describes the exact state they were trying
to leave. Naming `--dataset` remains the explicit act of adding one dataset the
project does not declare; once added, it is refreshed by later unqualified runs
rather than silently dropped.

### `--refresh`: re-acquire what coverage already claims

Completed coverage cannot prove the rows under it are the provider's: a room
seeded before a provider or tiling fix can claim an area whose rows are wrong or
missing, and an ordinary seed then acquires nothing there. `--refresh` is the
one explicit door for that (`ds_project_data::refresh`): it re-acquires every
tile the plan needs, held or not, under the same `--yes` confirmation. Each tile
is committed alone and replaces that tile's rows; held rows are never removed
before their replacement lands, and a failed tile ends the refresh with its
cause on the dataset's row. It is never implicit — without the flag seed reads
only gaps, so the next ordinary seed is warm again. The receipt carries
`refresh: true`.

```bash
ds data project-cache seed --project gisagara --dataset google_open_buildings --refresh --yes --output json
```

### Partial is partial

Datasets are seeded one at a time and each keeps its own truth. One dataset's
failure records its own cause on its own row — a national subset whose bundle is
not installed on this computer, for instance — and never abandons the datasets
beside it. The run reports how many did not complete; `seeded` is never `ready`,
and a failed acquisition keeps every cell the project already paid for.

### Local, not shared

Every row says whether it is held on this computer (`local_holding`). A room is
one machine's holding: nothing here publishes anything, and two installations of
the same project legitimately hold different coverage. A dataset acquired on one
computer is acquired again on the next.

### `ready`, with its reason

`ready` is one decision, made once in the kernel: the index answers, nothing is
mid-flight, something completed, and completed covers requested. When it is
false the row names the first failing condition in `ready_reason` —
`index_absent`, `index_incomplete`, `acquisition_pending (n)`,
`nothing_completed`, `acquisition_failed: …`, `coverage_gap (n cells)` — and a
row without a room says why there is none: `not_seeded`, `bundle_unpublished`
(the catalogue lists it but publishes no bundle yet), or `cloud_resident`.
Stale coverage is reported beside the row and never gates `ready`.

### `--dataset` names, aliases and retired layers

`--dataset` takes an exact id, a catalogue layer name (`village_boundaries`),
or a retired alias (`rwanda_villages`, or its old id). An alias answers as its
authority row and the receipt says so in `answered_as`. A retired broad layer
with no single authority (`powerlines`, `elementary_school`, both retired on
2026-09-18) refuses `dataset_retired` naming the detailed alternatives
(`hv_line, mv_line, lv_line` / `primary_schools, secondary_schools`).

### Cloud-resident datasets: read bounded, seed per project

Every catalogue row carries `residency`: `bundle` (published once, held per
project) or `cloud` (BigQuery is the national holding: `rwanda_upi_parcels`,
`edcl_customers`, and any row whose source exceeds the catalogue's published
`residency_threshold_bytes`). A cloud row is never installed nationally —
`desktop data rwanda install --resource <cloud>` refuses `dataset_cloud_only`
— because moving a national table to the desktop defeats the point of holding
it in the cloud. It is read two ways:

* **Bounded, at the moment of need** — `upi lookup`, `customers query`,
  `parcels query` below answer one question inside one bound and keep nothing.
  Each names its project with `--project`; ds-brain authorizes the read
  against that project, never a saved selection.
* **Seeded for this project** — `seed --dataset edcl_customers` (or
  `rwanda_upi_parcels`) fills the project's coverage cells through the same
  bounded read, cell by cell, exactly as building footprints are seeded from
  their bundle: the rows then live in the project room, render offline, and
  are reused by every later question without another query. A cell too dense
  to hold whole under the 5,000-row cap refuses `acquisition_failed` naming
  the cell; narrow the design buffer. `status` reports cloud rows without a
  room as `cloud_resident`.

Contract: `ds-brain/docs/contracts/foundation-datasets.md`.

## `upi lookup`

One land parcel by its UPI, from the cloud-resident Rwanda parcels authority
(11.5 million polygons in BigQuery, never bundled). `--upi` takes the compact
form (`20506012183`) or the printed form (`2/05/06/01/2183`). The answer is a
receipt — dataset (id, layer, source table, `residency: cloud`, version), the
bound (the UPI), `rows_cap 1 / rows_returned / rows_total / truncated` — plus
the parcel's properties (province, district, sector, cell, village, parcel key,
area, dates) and the polygon's evidence (type, bounds, vertex count). The
polygon is not printed; `--geometry-out` keeps it as a one-feature GeoJSON
file that `ds map local register` takes as it stands.

Refusals: `upi_invalid` (not 8–20 digits), `upi_not_found` (the authority holds
no such parcel), `dataset_ambiguous` (two parcels carry the UPI — a data
defect, nothing is chosen for you). One lookup scans about 360 MB of the
clustered table (a fraction of a cent); governance and rate limiting are the
contract's open question, not built.

## `customers query`

The anonymized EDCL customer connections (one million points in BigQuery,
never bundled) inside exactly one bound:

* `--village <8-digit code>` / `--cell <6-digit code>` — customers **within the
  authority polygon** of `village_boundaries` / `cell_boundaries`, joined in
  the same query. Spatial on purpose: the customers table carries names, and
  names are not unique across Rwanda.
* `--bbox west,south,east,north` — a WGS84 rectangle of at most 25 km² (the
  kernel's envelope bound); larger is `bound_exceeded`.
* `--boundary <path.geojson>` — one WGS84 Polygon or MultiPolygon (bare, a
  Feature, or a one-feature FeatureCollection) whose envelope is at most
  25 km²: a corridor from `ds data vector buffer`, an admin unit from
  `ds data admin-bounds`, a drawn extent. Rows must intersect the polygon,
  not merely its envelope.
* `--transformer <name>` — the transformer's saved design extent, buffered by
  the project's design buffer through the same coverage plan a seed uses, sent
  as a rectangle; the receipt's `query.scope` names the transformer, buffer and
  rectangle.

`--limit` caps rows at up to 5,000 (the cap); `rows_total` is the count within
the bound and `truncated` says whether the cap cut it. The terminal prints
counts per cell/village and per customer segmentation; `--geometry-out` keeps
the points as a GeoJSON FeatureCollection. Customers carry segmentation, meter
type and category, payment method, connection year and their administrative
names — no identity.

## `parcels query`

The Rwanda UPI parcels (11.5 million polygons in BigQuery, never bundled)
that **intersect** exactly one bound — the same five bounds as `customers
query`, the same cap and receipt. The classic question, *which parcels does
this line's corridor cross*, is two commands:

```
ds data vector buffer --source ./one-mv-span.geojson --radius-m 6 --out ./corridor.geojson
ds data parcels query --project <id> --boundary ./corridor.geojson --geometry-out ./crossed.geojson
```

The source file must contain the actual line geometry. The buffer command
creates one polygon per source feature; `parcels query --boundary` takes one
polygon, so query bounded spans or sections separately and deduplicate UPI
across responses. The boundary envelope must remain within 25 km².

The terminal prints counts per sector/cell and the summed `source_area_m2`;
`--geometry-out` keeps the polygons (UPI, parcel key, administrative names,
area, dates) as a FeatureCollection that `ds map local register` takes as it
stands. Where a project will ask the question more than once, seed the
transformer's extents instead and read the room.

## `project-cache query`: export an already held layer

Use `status --project <id> --summary --output json` to find the exact layer,
its `seeded` state and completed coverage. After seeding, query one WGS84
Polygon or MultiPolygon that lies wholly inside that coverage:

```
ds data project-cache query --project <id> --dataset mv_line \
  --boundary ./corridor.geojson --geometry-out ./held-mv-page-1.geojson --output json
```

The local spatial index returns actual feature intersections. This works for
held village boundaries, LV/MV/HV lines and other project datasets; it does
not call BigQuery or fill a missing room. A missing room or uncovered boundary
refuses `project_dataset_not_held` instead of reporting an empty layer.

One response writes at most 5,000 features. Check `truncated`; when true, use
its `next.cursor` and `next.generation` together and write the next page to a
new path. A room changed between pages refuses `project_dataset_page_stale`,
so restart from the first page. `truncated: false` is the completeness proof
for that bound. The returned source version and generation identify the held
snapshot, and the output is a GeoJSON FeatureCollection ready for local-layer
registration.

## `spatial plan` and `spatial execute`: governed BigQuery geography

Use the same workflow for parcels, customers, village boundaries, power lines
and other project-visible BigQuery geography. `project-cache status --summary`
discovers exact catalogue layers and IDs. Each call names its authorized
`--project`; neither command reads a saved active project. The server chooses
the table and fields from its catalog, not from caller SQL.

`plan` accepts one WGS84 Polygon or MultiPolygon whose envelope is at most
25 km². It dry-runs the exact query and writes a plan containing the source
version, optional sector-boundary authority version, estimated bytes, billing
guard, geometry, result shape and plan hash. It does not bill a source scan.
`execute` rechecks those pins and executes that plan under its billing guard;
a changed source or query requires a fresh plan.

```
ds data spatial plan --project <id> --dataset rwanda_upi_parcels \
  --boundary ./corridor.geojson --result count --group-by sector \
  --out ./parcel-plan.json --output json
ds data spatial execute --project <id> --plan ./parcel-plan.json \
  --out ./parcel-sector-counts.json --output json
```

Parcel counts are distinct UPI, including parcels whose polygons cross the
corridor. Customer and other dataset counts are source rows. Sector grouping
uses exact intersections with the governed sector boundaries; a feature
touching two sectors contributes to both, while `overall_count` counts it
once. Count results are complete. For `--result features`, use `--limit`
(at most 5,000) and optional `--geometry-out`; execution reports the exact
`rows_total` and `truncated`. A truncated result cannot become a complete
GeoJSON local layer. Partition the geography and deduplicate by stable source
identity when the result exceeds the cap. A complete GeoJSON file can be
registered with `ds map local register`.
For analytics, convert that complete feature set with `ds data convert`
to GeoParquet; the shared columnar owner also provides bounded Arrow IPC/WKB
batches internally. The query receipt remains the source identity and cost
evidence regardless of the presentation format. The current IPC/WKB path is
not yet a GeoArrow extension array. A requested external delivery format needs
an explicit adapter and its own completeness check; changing the output format
does not require another BigQuery query.

## Native elevation comparison and adaptive terrain sampling

`data.elevation.compare` evaluates the published Rwanda TIFF and Terrarium at
identical points without modifying their inputs. It exposes separate nearest
and bilinear Terrarium readings and acquisition evidence. Comparisons are
diagnostic: they do not select a mixed source or apply a height correction.

`data.terrain.describe` returns the generated native request schema, adjustable
defaults, source choices and density/error semantics. `data.terrain.sample`
reads that strict request file and writes a fresh directory of native sampled
artifacts. Choose one explicit Rwanda or declared survey surface. Centerline
and optional signed side cuts are sampled at their actual XY; side profiles
default to off. Seeded positions, terrain-preserving reduction and supplied
engineering intervals control density. Random corridor and side observations
remain separate from the nominal profiles.

Generated samples retain derived provenance and never claim field measurement.
Missing terrain remains gaps. Discrete baseline error, raster resolution,
source uncertainty and engineering density are reported separately. Dry-run
validates local inputs and the query plan without provider acquisition, surface
queries or file creation. Sampling does not edit or publish a model. Read the
live command contracts for delivery arguments and refusals, and the native
[terrain contract](../../../ds-network/docs/contracts/adaptive-terrain-sampling.md)
for the mathematical boundary.

## What this is not

Not a caller-supplied SQL engine. Dataset, geometry and aggregation choices
remain typed and bounded; the source table, projection and billing guard are
server owned.

`convert` is not a DEM converter. A DEM is a surface, not a table — one value
per cell and no attributes — so it stays a Cloud-Optimized GeoTIFF read by byte
range or from the verified full Desktop component. `elevation attach` samples
that surface into a new point artifact; it does not rewrite the DEM.


### Shared vector request contract

`ds data vector describe --tool sample --output json` publishes the Rust-owned
input/output JSON Schemas, defaults, worked request examples, availability and
named refusals. Omit `--tool` to list all 94 catalogue descriptors plus the
collision report reader, including roadmap.
The UI reads the same descriptors through ds-network-wasm.
A full generated Buffer descriptor is in [vector-buffer.descriptor.json](vector-buffer.descriptor.json).

Every available command (`measure`, `buffer`, `sample`, `intersect`, `outliers`,
`random-points-area`, `collisions`) accepts `--request <json-text>` as an alternative to its
source and parameter flags. MCP's vector profile and data router project
these same command arguments; request is serialized JSON text there too.
A portable control request names an Arrow/GeoJSON/KML file in `source.file`,
with optional `source.layer_id` and `source.name` provenance. Intersect also needs
`against`. Native inline GeoJSON is an explicit import-file boundary; the
WASM runner receives binary IPC only. Download the web pane's content-named
Arrow inputs beside the copied request.
`parameters` follows the selected descriptor; `output` has `limit` (500 by
default, 1..20000), `projection` (`inline` or `complete`) and optional `name`.
The kernel applies omitted defaults. `--request` cannot mix with source,
parameter or limit flags (`vector_input_choice_invalid`). Delivery flags
`--out` and `--overwrite` are allowed. With explicit `--request`, exports retain
its Rust-normalized projection and point order. Flag-style `--out` asks for the
complete produced output, while input admission stays bounded.

`--dry-run` executes the same computation and returns preview counts, fields,
at most five sample features and warnings. It writes nothing, including when
`--out` is present. A result limit never claims to restore withheld source
features; read `more` and preview warnings.

Measure now also returns a derived layer with `length_m`, `area_m2` and
`vertices`, preserving source properties and ids; existing fields with these
names are replaced in the derived copy. The report and whole-document totals
remain available. Sample uses full precision `distance_m`, `source_id` and
zero-based `part_index`; its default `include_ends` is false. Buffer defaults to
25 metres and 8 arc segments. Intersect means line crossing points, not polygon
intersection. Random-area sampling defaults to 50/100 metre spacing, a
25 metre buffer for non-polygons and reproducible seed 0; spacing is per area.
Terrain enrichment remains the existing host service workflow. Outlier metrics
use the existing engine's metre projection for recognized geographic input
(and source units otherwise), with the 3.5 score threshold.

`vector_request_invalid` names unknown JSON fields, wrong types, enums, ranges,
missing sources, inverted spacing or all outlier detectors disabled. Follow
the published schema. `vector_tool_unknown` means no such descriptor exists;
list describe. `vector_tool_roadmap` means the proposed schema exists but the
runner is not shipped; choose status `available`. Existing document, eligibility,
distance, limit and engine refusals retain their codes; parameter remedies now point to the descriptor.

`ds data vector collisions` reads the existing reporter region layer as portable GeoJSON; it does not trigger detection. Its schema includes `loaded`, `computed` and nullable `last_read_computed`. Empty computed collections mean zero; a missing document and a held prior answer remain distinct. This is an additional report contract alongside the 94 geometry catalogue entries.

For random points, optional `sample_elevation: true` requires `elevations_m` in returned point order, with `null` for unavailable terrain. The web acquires terrain, resolves the input pane and then calls the same Rust runner. Copy that resolved JSON to reproduce the exact enriched layer natively. Missing or mismatched readings refuse as `vector_external_data_required`.

Use `ds mcp serve --exposure commands --profile vector` for descriptor discovery and all available vector commands. The existing datasets profile keeps its original four geometry primitives.

### Vector workflows

`ds data vector workflow describe --output json` returns a compact catalogue
of the seven available tools, their typed input/output ports, the workflow
schema, resource bounds, conditions and eight runnable examples. Roadmap tools
are excluded from this compact catalogue; full descriptors remain available
through `vector describe`. `--example 1` returns one complete JSON model.
The generated document schema is [vector-workflow.schema.json](vector-workflow.schema.json);
Rust validation adds graph, descriptor type and execution-bound checks.

A `ds.vector-workflow/v1` document has `name`, optional `description`, typed
`inputs`, `steps` and named `outputs`. Model inputs use `layer`, `number`,
`string`, `bool` or `enum`; they may have descriptions and defaults. Enum inputs
use `values`; number inputs may declare `unit` and `integer: true`. Layer inputs
may declare `geometry` kinds and field name/type pairs. `many: true` declares a
layer array. Supply input bindings with `--inputs <json-text>`.
For a local layer, bind `{"route":{"file":"lv-lines.arrow"}}` (GeoJSON and
KML files work too). File paths resolve relative to the workflow file. Raw
GeoJSON is an import at the file edge; `path` is not a layer-reference key.

Each step has a unique plain `id`, a descriptor `tool` id (command aliases also
resolve), and the same `request` object used by standalone tools. Anywhere in
that request, a single-member `{"$ref":"inputs.mv"}` object may replace a
literal. Step references use `steps.<id>.outputs.<port>`; ports are `layer`,
`report`, `preview`, `counts` and `fields_added`. Declared report/count fields
and numeric array indices can be referenced further, for example
`steps.metrics.outputs.report.totals.length_m`. Named model outputs are single
references; only exported outputs are written to disk.
To export appended geometry fields, reference the measure step's `outputs.layer`;
`outputs.report` exports the measurement report instead.

`ds data vector workflow validate --file model.json --output json` checks ids,
all references, graph cycles, types, geometry kinds, units, known parameter
constraints, conditions and iterator shapes. It returns topological order,
dependencies, typed step outputs and unbound required parameters. `run` requires
all inputs to be bound. Constraints on computed values are checked again when
the actual request resolves. Errors include `step_id`, JSON `path` and `remedy`.

Optional `when` accepts a boolean, a boolean reference, or an object with `op`.
`exists` and `non_empty` use `value`; `eq`, `ne`, `gt`, `gte`, `lt`, `lte` use
`left` and `right`; `all` and `any` use `conditions`; `not` uses `value`.
Conditions short-circuit. A false condition skips its step and yields null
layer/report ports; guard consumers of those ports with their own preconditions.

Optional `for_each` has `kind: features|groups|layers` and `over` (a reference or
literal). Feature iteration has `chunk_size` (1..64, default 1). Group iteration
requires `group_by`; layer iteration requires a layer array. Requests can refer
to `iterator.layer`, `iterator.index` and `iterator.key`. Layers are merged in
deterministic order with `workflow_iteration` and `workflow_key` fields;
iteration reports remain an array. Intermediate layers stay in bounded memory.

`ds data vector workflow run --file model.json --dry-run --output json` computes
each admitted intermediate so downstream counts stay accurate, returning a
preview per step and up to five features per public layer. It creates no files
or folders, even with `--out`. Actual execution uses the same native/WASM runner.
`--out <directory>` exports named layers as Arrow IPC, other outputs as JSON, and
`workflow-result.json` as a provenance receipt. Existing output files refuse;
there is no overwrite switch. Without `--out`, named layer references and bounded
previews are returned in the receipt; feature data is never returned inline.
On the first execution failure, CLI/MCP refuse with
`vector_workflow_step_failed`; `error.detail` keeps the completed outputs and
the underlying step error. With `--out`, completed layers survive as
`completed_<step>.arrow`, with a failure receipt.

Each output records the tool id/version, canonical SHA-256 request digest,
input/parameter digests and output digest. Fixed seeds and ordered execution
make repeated requests deterministic. The runner bounds documents and bindings
to 8 MiB each, retained typed data to 32 MiB, layers to 20,000 features, models to
64 steps and iterators to 128 chunks/groups/layers. Conservative output estimates
refuse excessive generators before kernel execution. This is a bounded inline
runner with lazy feature chunks; split larger data into multiple models. No intermediate files survive.

The eight embedded models are MV corridor buffer then measure; LV line points,
point buffers and measure; line crossing points then measure; seeded area points
then measure; outlier setbacks with a condition; points grouped by transformer;
chunked line sampling; and buffers over multiple layers. All run against embedded
fixtures in native and WASM tests. Building-within/count, nearest-building,
dissolve and split-line recipes still need their roadmap tools; discovery never
claims those algorithms are shipped.

Workflow refusals are `vector_workflow_invalid` (fix schema/bindings),
`vector_workflow_reference_invalid` (fix a named input/producer/port),
`vector_workflow_type_mismatch` (match geometry, scalar type or units),
`vector_workflow_cycle` (remove a back edge),
`vector_workflow_bound_exceeded` (increase spacing/chunk size or split the model),
`vector_tool_unknown` (choose a known id), `vector_tool_roadmap` (choose an
available tool), and `vector_workflow_step_failed` (inspect the underlying step
refusal and retained outputs). File reads can also return `source_unreadable`;
exports use the existing `output_refused` contract. Literal parameter admission
can retain `vector_request_invalid`, `vector_distance_out_of_range` and
`vector_limit_out_of_range` from the tool descriptor.

MCP exposes `data_vector_workflow_describe`, `data_vector_workflow_validate` and
`data_vector_workflow_run` in the bounded `vector` profile and through the data
chapter router. Their arguments and envelopes match the CLI. WASM exports
`vector_workflow_describe`, `vector_workflow_validate`,
`vector_workflow_validate_ipc` and `vector_workflow_run_ipc`; workflows have no human UI yet.

Feature data stays in typed Rust layers between steps and crosses WASM as
Arrow IPC. Tool execution returns receipts; `--out file.arrow` exports IPC and
`--out file.geojson` uses the explicit file writer. Only the web's MapLibre
renderer turns small results into GeoJSON. Rust selects vector-tile delivery
for large results; the web sends file bytes through the existing upload and tiling pipeline. `vector_data_bound_exceeded` names
an input or projected output beyond 32 MiB/20000 rows; split the layer or
increase spacing. Binary import may also refuse `vector_document_malformed`.

KML and KMZ conversion retains shared and inline styles, normal StyleMap entries,
archive-local style references, and embedded icons. The resulting GeoParquet
contains the layer style document under `ds:style_document`; feature properties
retain `kml_style_id`, decoded components and data-driven colours. Invalid styles
are reported in `kml_style_error` by placemark name while geometry is retained.
