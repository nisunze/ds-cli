# Project dataset cache

A project holds **bounded extracts** of canonical geographic datasets instead of
a national download it will mostly not use. `project_dataset_cache` owns every
decision that shape implies: how far around a design counts as "the project",
how disconnected clusters fuse, what is held, what is missing, and what exactly
one acquisition asked for. Hosts own storage, transport and UI.

Three separations are load-bearing: room address versus acquisition identity,
requested versus completed coverage, and dataset independence. They are stated
in the module's own documentation and are not restated here.

## Project source assets and local computation (owner, 2026-10-12)

Footprints, contours and authorized custom spatial layers are reusable project
data. An installation acquires or computes a bounded extract locally, retains
its exact source-quality rows, and publishes a versioned project asset through
the existing authorized asset/publication boundary. Other authorized users
download the verified extract once and share the native project dataset room.
Printing, composition and analysis consume that room; they never independently
query an upstream provider or start a cloud compute fallback.

Arrow/GeoParquet supplies retained analytical rows and native spatial indexes;
GeoJSON is the mapping and interoperable export representation of those same
rows. Display vector tiles do not establish complete analytical source data.
An asset identifies its project, source and license, CRS, schema, stable feature
ids, source revision, completed coverage, content digest and extraction or
computation parameters. Contours additionally pin the admitted elevation
surface and interval/spacing choices. No substitute elevation source is chosen
silently when that surface has gaps.

Transformer metadata projects references to its footprint and contour rooms,
their native readiness/reason, completed coverage and exact source provenance.
It does not maintain a second independent "fetched" boolean or imply that
queued work is complete. The reference names the published asset revision and
local retained digest where available. Source choices distinguish Rwanda TIFF
from Terrarium and Microsoft from Google footprints, including versions and
parameters; an explicit source change creates a separate acquisition identity.
Reading this metadata never fetches or probes the linked dataset.

For an urban area with an admitted city-limit polygon, the acquisition planner
uses contiguous city coverage shared by its transformers instead of individual
transformer extracts. Preserve the polygon's authority/revision and present
the bounded coverage/cost plan before explicit acquisition; do not substitute
an invented city extent. Without an admitted city limit, retain the existing
bounded project/cluster coverage policy. Dataset geometry remains spatially
indexed so consumers can read a transformer or print extent from the shared
city extract without a provider query or full scan.

Deduplicate repeated source resources by logical provider/asset identity and
version, not by expiring download URLs. Feature merge keys include the source
identity and stable feature id; overlap at acquisition-cell boundaries must
not duplicate a footprint or contour. A source without stable ids requires a
declared deterministic geometry/property identity and measured collision
checks. Never merge distinct touching buildings, discard a different source's
row, or equate separate dataset versions merely because their URLs resemble
one another.

Expanded transformer or model bounds ask the shared owner for the difference
between requested and completed coverage. Acquire only that missing coverage;
merge it under stable source identities without duplicate boundary features or
overwriting dirty local rows. A changed source patches affected coverage and
explicit deletions under its revision fence. Requested, queued and partially
downloaded areas are never marked complete. Concurrent consumers coalesce the
same work. One failed dataset does not invalidate an independent held dataset.

CLI acquisition, refetch and publication are explicit operations with their
existing project permissions. An explicit refetch may deliberately replace
selected coverage. Cloud push retains an invalidation marker; no unused data
read or freshness check follows it. Offline consumers use held assets without
network probes; missing inputs answer an acquisition need. Shared central
storage is a distribution boundary, not permission for remote computation or
national-reference publication. Governed national extensions keep their
separate authority and immutable provenance review.

Offline map consumption may retain complete, immutable PMTiles packages under
a configurable byte budget, initially 1 GiB with a 5 GiB option. Show the exact
package sizes and coverage before download; admit only complete packages that
fit the chosen budget, verify their bytes, and reuse them without GCS range
requests. Do not silently lower zoom, omit features, or evict pinned inputs.
Explicit download/refresh controls and native retention policy own this choice.

Printing the highest admitted vector-tile zoom is an optional experiment, not
an analytical source or required print backend. Compare it against source-row
rendering using the same extent, physical scale, styles and furniture: feature
ids/counts, clipped and boundary geometry, labels, line weights, completeness,
latency, transferred bytes and actual request counts. A package's highest zoom
alone does not prove unsimplified or print-equivalent geometry. Retain the
source-row path unless this evidence establishes equivalence for the selected
output. This does not change the local-compute default or authorize deployment
of a cloud printing service.

The historical provider paths described below explain existing decoding
formats; they do not authorize continued Cloud Run acquisition or Terrarium
fallback. Remaining implementation and installed acceptance live in
[OPEN §16](../../backlog/OPEN.md#16-map-data-datasets-and-screen-styling) and the
[printing fast path](../../backlog/STABILIZATION.md#immediate-printing-fast-path-owner-2026-10-12).

## 2026-09-11 — building decode refusals and the first-use seed rule

### Four named refusals, one per case

`decode_buildings` admits a complete source-quality acquisition or refuses it.
Until this date four different facts shared one sentence — *"building
acquisition contains an invalid or out-of-area footprint"* — so a room's
`last_error` could not distinguish a malformed provider row from a correct row
returned outside the area it was asked for. Each case now names itself and the
offending footprint (`#<index>`, plus the provider's own identifier when the row
carries one):

| Case | Refusal |
|---|---|
| Not a Feature, or properties are not an object | `building footprint <ref> has an invalid shape: …` |
| Geometry is absent or is not an area | `building footprint <ref> geometry is <type>, not Polygon/MultiPolygon` |
| Geometry carries no usable coordinate | `building footprint <ref> has no bounds: …` |
| Geometry cannot be parsed | `building footprint <ref> has unreadable geometry: <reason>` |
| Correct row, outside the requested area | `building footprint <ref> lies outside the requested area (rule: intersects; footprint bounds […]; area bounds […])` |

Every one is a refusal, exactly as before: no row is dropped, no partial page is
admitted, and no coverage is claimed. The refusal is persisted verbatim as the
room's `last_error`, so it is bounded (a provider identifier is quoted to 64
characters and stripped of control characters) and it names a row rather than
carrying one.

The out-of-area refusal carries the rule and both rectangles because that is the
whole evidence needed to separate the two remaining explanations without a
debugger: an area rule that is too strict, or a provider returning rows outside
the region it was given. The rule for `google_open_buildings` is `Intersects`
and it is an exact polygon-polygon test (`ds-geo-lite`
`spatial_predicate::selects_area`), not a bounding-box test — so a footprint
that straddles the edge of even a very thin area is ADMITTED. The named proof is
`tests/fixtures/buildings-decode/corpus.json`, run natively by
`tests/buildings_decode_corpus.rs` and through the vendored WASM by ds-web's
`src/tests/buildings-decode-parity.test.ts`.

### First use seeds what the project declares

`data.project-cache.seed` with no `--dataset` seeds every dataset the project's
workflow **declares**, including on a machine that holds nothing yet. The
declaration is the printing context catalogue's own answer for that project —
`printing::context::catalog`'s `default_on` rows plus the derived sources — so
"declared" is decided once, in the kernel, for the Printing setup page and the
command line alike. It is not "everything published for this country": an
excluded or unavailable catalogue layer is not declared.

Naming `--dataset` remains the explicit act of adding one dataset the project
has not declared. An active dataset the project already holds is always refreshed, so an
operator who deliberately seeded something outside the declaration keeps it.
Seeding is per dataset: one dataset's failure records its own `last_error` and
never abandons the rest, and `seeded` is never `ready`.

Retired broad layers (`powerlines`, `elementary_school`) may retain historical
rooms, but those rooms do not appear in the active overview or automatic seed
targets. Explicit requests by name or a retained room's digest refuse
`dataset_retired` with the detailed alternatives. Retirement does not remove
stored features or equate their coverage with a replacement.

Which datasets one run acts on is `seed_targets` (action `seed_targets`), not a
host's own arithmetic: it takes the declaration, the rooms that already exist
and the optional explicit `--dataset`, and answers with the ordered targets —
declared first, each group by label, so two machines seed in the same order — or
the typed refusal `project_dataset_scope_invalid` when the project declares
nothing and holds nothing. A held row that was never seeded is a dataset the
project *could* hold, not one it holds, and enters a run only by being declared.
Hosts supply the facts and map ids back to the providers that can hold them.

A refusal quotes provider strings — a footprint's identifier, a geometry's type
— bounded in characters and stripped of control characters, so a malformed
provider row can neither break the status line it is read in nor a host's
string handling.

## 2026-09-12 — contour decode

`decode_contours` admits the tiler's `/v1/contours/fetch` answer (reached
through ds-brain `query_print_context` with `context_kind: elevation_contours`)
into a room the same way `decode_buildings` admits a building page, so a
headless host and the desktop hold contours as one `elevation_contours`
dataset rather than the desktop's per-transformer IndexedDB retention.

The tiler sends two `FeatureCollection`s, `contours_intermediate` and
`contours_index`, with `intermediate_features` / `index_features` counts and no
`source_receipt`. The decoder requires `status: success`, both layers present
as collections, each count equal to its collection, every line a `LineString`
or `MultiLineString` lying in the requested area under the dataset's
`Intersects` rule, and every line's `contour_kind` equal to the kind of the
layer it arrived in. The admitted page carries both kinds in one `features`
array — the print-context assembler splits them back on `contour_kind` into
`CONTOURS_INDEX_LAYER` / `CONTOURS_INTERMEDIATE_LAYER` — and a synthesised
receipt `{source, source_version, minor_interval_m, index_interval_m,
effective_spacing_m, grid, sampled_points, unresolved_points}` that
`validate_source_receipt` accepts. `sourceVersion` is the tiler's
`source_version` (the DEM base URL, or the Terrarium zoom when the DEM did not
cover the area), so `freshness` re-acquires when the surface changes. A
complete empty page is admitted.

Refusals follow the building decoder's discipline — one sentence per
distinguishable fault, naming the layer and the row (`contour line #<index>
[`<id>`] in `<layer>` …`), the out-of-area case carrying the rule and both
rectangles — and are exact, because they are persisted as the room's
`last_error`. The named proof is `tests/fixtures/contours-decode/corpus.json`,
run by `tests/contours_decode_corpus.rs`; the request door is
`{action: "decode_contours", schema, response, area}`.

## 2026-09-20 — `ready` names its reason; residency

`readiness(index_state, pending, completed, requested, last_error)` is the one
decision behind a room's `ready`; the store calls it and every projection
echoes `ready_reason` (`index_absent`, `index_incomplete`,
`acquisition_pending (n)`, `nothing_completed`, `acquisition_failed: …`,
`coverage_gap (n cells)`, or `null` when ready). Rows without a room say why
there is none: `not_seeded`, `bundle_unpublished`, or `cloud_resident` for a
dataset that is never held on any machine. `CatalogEntry` carries `residency`
and `unpublished` for that. Contract: [foundation-datasets.md](foundation-datasets.md)
R5–R6.

## 2026-10-20 — the building decoder leaves

Brain's `query_print_context` answers 501, so no page or host acquires
building footprints through it any more, and `decode_buildings` with its
per-footprint admission leaves. Footprints arrive through the publisher path
`ds data project-cache seed` admits; the four named refusals above describe
that retired decoder.

## 2026-10-22 — seed once per project: shared revisions

Rooms stay one principal's local holding; `project_dataset_revisions` decides
the project's shared answer for footprints and contours. One compute artifact
head per (project, dataset, parameters): engine `project_dataset`, operation =
dataset id, variant = `p` + 40 hex of the shared identity (id, quality,
parameters, CRS — never the room provider or principal). The head holds a
`manifest` (`ds.project-dataset-revisions/v1`: ordered revisions, each with its
completed coverage, source version, bounded receipt, feature count and the
SHA-256/size of its `rev-NNNNNN` gzip GeoJSON-lines delta) and every delta.
Decisions, all pure: `decode` admits a chain (additive fields accepted;
order, identity, digests and bounds refused whole); `delta` = wanted ∩ held −
published (a refresh republishes what it completed); `pending` names the
revisions a room has not applied; `base_fingerprint` is the SHA-256 of the
manifest a publication was planned on (`sha256("")` for the first) — the fence
Brain checks before a revision lands; `chain_freshness` reports coverage
produced from a version other than the one acquisition reads now, never acting
on it; `validate_open` admits exactly a manifest plus one delta on the native
publication door, outside the report outbox's engine registry;
`google_open_buildings::tiles` names the publisher's own S2 level-6 tiles a
set of cells reaches (with the centroid margin); `tasks` is the typed
request/response contract with the Reporter engine (footprint pages, contour
lattice, contour pieces on fixed 1 km national-grid tiles). Orchestration:
`ds-project-data/docs/contract.md` § Seed once per project.
