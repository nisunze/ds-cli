---
name: ds-cloud-datasets
description: Discover project-visible BigQuery geography, plan bounded costed spatial reads, execute pinned GeoJSON or sector counts, and use direct UPI, parcel and customer questions through `ds`.
metadata:
  ds-chapters: data
---

# Plan and read project-visible BigQuery geography

Parcels and customers are cloud-resident national tables. Other
project-visible BigQuery geography includes administrative boundaries and
LV/MV/HV lines; some of those layers also have local project holdings. Read
the catalog before choosing the cloud or held-layer route. Every cloud
request names its own authorized project; no server-wide active project is
used.

Use the deployed CLI and read each command contract before invoking it:

```
ds capabilities data.parcels.query --output json
ds capabilities data.customers.query --output json
ds capabilities data.upi.lookup --output json
ds capabilities data.project-cache.status --output json
ds capabilities data.project-cache.query --output json
ds capabilities data.spatial.plan --output json
ds capabilities data.spatial.execute --output json
```

## Choose the bound

Exactly one bound per call. Prefer the tightest one that states the question:

| Question | Bound |
|---|---|
| The parcels a line's corridor crosses | `--boundary <corridor.geojson>` from `ds data vector buffer` |
| The customers a transformer would serve | `--transformer <name>` (its buffered design extent) |
| Everything in one administrative unit | `--village <8-digit>` / `--cell <6-digit>` from `ds data admin-bounds list` |
| An area the user drew or holds | `--boundary <polygon.geojson>` or `--bbox w,s,e,n` |
| One parcel by its title number | `ds data upi lookup --project <id> --upi <UPI>` |

A boundary is one WGS84 Polygon or MultiPolygon (bare, a Feature, or a
one-feature FeatureCollection) whose envelope is at most 25 km². Larger
refuses `bound_exceeded`: split the corridor by alignment section or tighten
the buffer, never widen the cap.

## The corridor read

```
ds data vector buffer --source ./one-mv-span.geojson --radius-m 6 --out ./corridor.geojson --output json
ds data parcels query --project <id> --boundary ./corridor.geojson --geometry-out ./crossed.geojson --output json
```

The buffer source is a GeoJSON file containing the actual line geometry.
`data vector buffer` creates one polygon per source feature, while
`parcels query --boundary` accepts one polygon per call. Split a longer route
into bounded spans or sections; retain each bound and deduplicate UPI when
combining results. A projected continuation or drawing viewport is not a
line source.

Read `rows_total` and `truncated` before reporting a count. `truncated: true`
means the bound holds more than the cap answered; narrow the bound and read
again — never sum two truncated answers. `--geometry-out` keeps the exact
polygons; inspect `ds map local register` for how to prepare that file as a
local layer. Customers
inside the same corridor are the same call with `customers query`.

For a costed, complete aggregate or for another project-visible BigQuery
layer, plan the exact catalog dataset before execution:

```
ds data project-cache status --project <id> --summary --output json
ds data spatial plan --project <id> --dataset rwanda_upi_parcels \
  --boundary ./corridor.geojson --result count --group-by sector \
  --out ./parcel-plan.json --output json
ds data spatial execute --project <id> --plan ./parcel-plan.json \
  --out ./parcel-sector-counts.json --output json
```

Inspect estimated bytes and the billing guard in the plan. Execution checks
the source version and plan hash again. Parcel aggregates count distinct UPI;
other datasets count rows. A sector group uses exact geometry and may count a
feature in each sector it touches. `--result features` can write GeoJSON only
when `truncated` is false; split a dense boundary and deduplicate by stable
identity rather than treating a partial page as complete.
For analytics, convert a complete GeoJSON feature set through `ds data
convert` to GeoParquet. Use `ds data conversion-matrix` to inspect the
installed format contract before an external handoff; never rename a file
extension to imply a conversion that has not run.

## Seed instead of re-asking

When a project will ask the question more than once — every print, every
revision, an offline site visit — seed the dataset into the project like
building footprints:

```
ds data project-cache status --project <id> --summary --output json
ds data project-cache seed --project <id> --dataset edcl_customers --yes --output json
ds data project-cache seed --project <id> --dataset rwanda_upi_parcels --yes --output json
```

The seed plans the project's coverage cells from its design extents and
reads each cell through the same bounded query. A cell too dense for the cap
refuses `acquisition_failed` naming the cell; the remedy is a narrower design
buffer, not a partial room. Seeded rows render offline and are reused by every
later question; `status` reports the room and its coverage like any dataset.
A cloud row without a room reads `ready_reason: cloud_resident` — that is
where it lives, not a fault.

## Stop conditions

- `dataset_cloud_only` from `desktop data rwanda install`: expected; the
  dataset is read bounded or seeded per project, never installed nationally.
- `data_distribution_unavailable` / `auth_transient`: retry once later; the
  read is idempotent.
- `upi_not_found` / `dataset_ambiguous`: report the authority's answer as it
  stands; never guess a neighbouring parcel.

Governance and rate limiting of these reads are the contract's open question;
do not loop a bounded read to walk a whole district.

Stops at: the cloud authority — `ds` asks one bounded question or seeds one
project's extents; the national table, its governance and its rate limits stay
with the dataset owner, and a room refused as too dense goes back to the
operator for a narrower design buffer.
