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

```text
inspect → (choose the sheet, layer, or coordinate columns) → convert
```

## `admin-bounds attach`

Writes a new CSV, TSV, or GeoJSON elevation-point file carrying `province`,
`district`, `sector`, `cell`, `village`, and `code_village`. Geometry,
elevation values, and non-empty operator-supplied admin values are preserved.
CSV/TSV callers name longitude and latitude columns explicitly; GeoJSON uses
feature geometry. The source is never overwritten.

## `elevation attach`

Interpolates point sources through the native Desktop engine. CSV/TSV can name
their coordinate columns and CRS; geometry formats carry coordinates. A
`common-column` keeps each named surface all-or-nothing. AWS Terrarium fallback
is explicit and can be disabled. Jobs above 4,000 parsed points require the
verified full local Rwanda DEM component; the Desktop component manager installs
or verifies it once before the operation retries. Smaller jobs may read exact
public COG ranges through the bounded Desktop cache.

Hypothetical requests are intentionally short and discoverable from the live
command descriptor:

```text
ds data elevation attach --source /data/poles.csv --out /data/poles-elevation.geojson --x-column longitude --y-column latitude --source-crs wgs84_lonlat
ds data elevation attach --source /data/alignment-points.tsv --out /data/alignment-elevation.geojson --common-column alignment --fallback none
```

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

## What this is not

Not a query engine. `ds data` writes formats; reading and reducing them is a
separate decision, and deliberately not made yet.

`convert` is not a DEM converter. A DEM is a surface, not a table — one value
per cell and no attributes — so it stays a Cloud-Optimized GeoTIFF read by byte
range or from the verified full Desktop component. `elevation attach` samples
that surface into a new point artifact; it does not rewrite the DEM.
