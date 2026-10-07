# Project dataset cache

A project holds **bounded extracts** of canonical geographic datasets instead of
a national download it will mostly not use. `project_dataset_cache` owns every
decision that shape implies: how far around a design counts as "the project",
how disconnected clusters fuse, what is held, what is missing, and what exactly
one acquisition asked for. Hosts own storage, transport and UI.

Three separations are load-bearing: room address versus acquisition identity,
requested versus completed coverage, and dataset independence. They are stated
in the module's own documentation and are not restated here.

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
