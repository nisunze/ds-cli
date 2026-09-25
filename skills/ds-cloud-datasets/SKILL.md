---
name: ds-cloud-datasets
description: Answer bounded questions against the cloud-resident foundation datasets (UPI parcels, EDCL customers) through `ds` — which parcels a corridor crosses, which customers a boundary holds — and seed a project's own extents for reuse instead of downloading a national table.
metadata:
  ds-chapters: data
---

# Read the cloud datasets bounded; seed a project's need

The parcels and customers authorities are national tables held in BigQuery.
They are never installed on a desktop and never downloaded as a bundle:
moving them defeats the reason they live in the cloud. Every read is one
question inside one bound, capped at 5,000 rows, and the receipt says how
many rows the bound really holds.

Use the deployed CLI and read each command contract before invoking it:

```
ds capabilities data.parcels.query --output json
ds capabilities data.customers.query --output json
ds capabilities data.upi.lookup --output json
ds capabilities data.project-cache.status --output json
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
