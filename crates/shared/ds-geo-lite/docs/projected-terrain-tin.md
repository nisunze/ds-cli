# Projected terrain TIN contract

`ds_geo_lite::projected_tin` (feature `tin`) owns bounded interpolation of
terrain XYZ observations in an explicitly projected, Cartesian metre frame.
It is independent of provider, source priority, vertical datum, route geometry,
stationing, profile display widths and sampling policy. A consumer admits those
engineering facts before constructing this surface. It must not combine
incompatible sources or label DEM observations as surveyed.

## Input and ownership

`ProjectedTerrainTin::build(points, options, breaklines)` accepts typed
`ProjectedTerrainPoint { x_m, y_m, z_m }` rows. It preserves every supplied row
and its order in `points()`. Neither XYZ nor source observations are adjusted
or smoothed. Derived triangles index that original input order.

The caller must explicitly provide `ProjectedTerrainTinOptions {
max_triangle_edge_m }`. It must be finite and greater than one micrometre.
Each triangle with any plan edge longer than the limit is omitted, leaving
uncovered terrain rather than connecting across an unsupported survey gap.
An edge limit controls plan distance between observations; it is not a route
station-gap setting or a claim of survey accuracy.

XYZ must be finite. Points within one micrometre in XY are duplicates for
triangulation. Identical Z may share one derived vertex while the original
observations remain retained. Any different Z at colocated XY refuses the
build. At least three unique, noncollinear points are required. A valid surface
may have no admissible triangles after edge filtering; every query then
returns no coverage. Nonempty authored breaklines produce
`UnsupportedBreaklines`; this unconstrained implementation never silently
ignores a supplied breakline.

## Surface and queries

The surface uses the shared Spade-backed Delaunay adapter and Cartesian
barycentric interpolation. Numerical translation into a local origin improves
projected-coordinate predicates without geographic conversion or rescaling.
Spade's public underflow mitigation rounds derived coordinates below its
minimum supported magnitude (about 1.8e-43 m) to zero; source XYZ stays intact.
This is the DS mathematical choice; it does not assert PLS-CADD's unpublished
numerical implementation.

`sample(x_m, y_m)` and `sample_many(&[[x_m, y_m], ...])` evaluate at the actual
plan coordinate. `Ok(None)` means outside supported triangles, including
outside the convex hull and inside long-edge gaps. Negative interpolation
weights are rejected, so even small extrapolation beyond coverage is refused.
No nearest observation, route projection or synthetic height substitutes for
missing terrain. Nonfinite query coordinates refuse rather than becoming
no-data. Affine terrain planes are reproduced; sampled ridges and valleys
are retained rather than flattened by drawing-only smoothing.

## Resource admission

One local build admits at most 6,000 observations and at most 1,000,000 m of
extent per axis. These admission limits remain in place with the upstream
triangulator; consumers divide larger surveys into supported local surfaces
rather than building a whole project point cloud. Individual query batches
admit at most 20,000 queries and at most
10,000,000 candidate triangle checks. A spatial cell index limits queries to
triangles whose bounds can cover that cell. A dense batch exceeding the work
budget refuses before yielding any partial result; callers may split it into
smaller batches. Source and query count limits precede their allocations.

Legacy `terrain_delaunay` and `tin_sample_many` continue to accept their
documented WGS84 packets. Projected metres must never be supplied to them.

The public integration test `projected_tin_contract` proves affine planes
under alternating transverse observations, centreline and +/-6 m queries,
sampled ridges/valleys, exact input preservation, hull and gap behavior,
duplicate and nonfinite refusals, reproducibility and resource admission.
