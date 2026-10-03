# Geometry tolerance policy

Owner: `ds-geo-lite`. Applies to portable vector primitives and their consumers.

Coordinate space, operation semantics and named tolerance parameters determine
an operation's result. A migration preserves its consumer's existing parameters;
a change requires an explicit rule and a differential regression test. There is
no universal epsilon and no implicit CRS selection.

## Units and roles

- Geodesic distance/area use the existing 6,371,000 m sphere. Projected engineering
  distances retain their explicitly chosen metric CRS; they are not haversine.
- Cartesian distance preserves squared-sum arithmetic by default. Hypot distance
  and fused segment denominators are explicit compatibility options. A caller
  with a stored length may supply its denominator; squared-length degeneracy
  thresholds remain explicit. None is a silent numerical replacement.
- Computational degeneracy, coordinate bounds, determinant/area tolerances,
  dimensionless segment parameters, metric reach and screen pixels are separate.
- A user-configured physical tolerance is not permission to spend that tolerance
  on engineering limits or move retained infrastructure.
- Output rounding belongs to the codec/report boundary, never to primitive math.

## Line operations

- Cartesian polyline projection skips zero-length segments and retains the first
  equally-near segment. Empty/singleton/wholly degenerate lines have no foot.
- Station slicing takes caller-supplied nondecreasing stations and sorted cuts.
  A cut within the supplied vertex tolerance uses the exact authored vertex;
  endpoints do not manufacture pieces. All payloads interpolate at the same t.
- LV station slicing keeps its 0.001 m vertex tolerance. Its 0.05 m cut merging,
  0.5 m split reach and 0.1 m default welding remain domain decisions; zero
  welding still disables the cadence, and retained endpoints remain immovable.
- Editing parameter slicing retains its 1e-10 parameter tolerance during the
  mechanical move. Its terminal-parameter behavior is a separately characterized
  migration, not an incidental consequence of changing owners.
- Degree-space crossing retains its existing determinant tolerance and does not
  return collinear overlaps. Proper interior crossing, any contact and region
  boundary crossing are distinct queries.

## Predicates, samples and admission

Boundary-inclusive, interior-only and half-open ownership are distinct modes.
Hole interior and hole boundary are distinct. Elevation's half-open lattice
ownership must continue to allocate a shared border to exactly one area.

GeoJSON measurement rejects malformed positions rather than bridging them.
Viewport extent skips invalid WGS84 positions; strict selection admission rejects
invalid geometry. Shared walkers must expose these modes instead of guessing.

Sampling seeds, source order, nearest ties, buffer cap/join/miter options and
native byte/decimal fidelity are observable contracts. Preserve them through
relocation. The polygon Boolean backend remains the existing geo/i_overlay
engine. Native TIN admission/edge limits remain in the native consumer.

Remaining compatibility thresholds and numerical differences are inventoried in
`ds-network/docs/design/geometry-toolkit-consolidation.md`. New consumers choose
one documented mode; they do not add private epsilons or geometric repair.

## Boolean operations and coordinate traversal

`overlay::polygons` is the shared polygon engine boundary for intersection,
union, difference and symmetric difference. It uses the existing `geo` engine,
its even-odd fill rule and planar WGS84 longitude/latitude coordinates. Each
input must be a valid, finite, closed Polygon/MultiPolygon with at most 50,000
coordinates. Invalid topology is refused; no private epsilon or healing pass
is applied. Output is always a 2D MultiPolygon, including empty results. Feature
attributes and CRS selection belong to the consumer.

`geojson::map_coordinates` owns the former binding's recursive XY traversal.
The consumer supplies the coordinate transform. Additional ordinates, properties,
authored bbox fields, null geometries and malformed leaves keep their previous
behavior. The traversal does not choose a projection or invent a bbox update.

## Cartesian envelopes, corridors and planar selection

- Rotated envelopes retain the `geo` minimum-rectangle engine, the first edge
  longer than 1e-9 caller units as the axis, and the single-point axis fallback.
  Projection selection and inversion stay with the caller.
- Round corridors retain 16 segments per half-turn, endpoint Z payloads,
  rejection of legs of length <= f64::EPSILON, the 1e-12 normal/projection
  degeneracy thresholds and the ten-width miter clamp. These are the existing
  Grid geometry rules, now owned here; no engineering rule is inferred.
- Planar lasso selection uses the same polygon engine's full-segment covers
  relation, including boundary overlap and concave excursions. Cardinality,
  coordinate bounds, extent limits, IDs and held-scene pins remain explicit
  consumer admission. Stable ring anchors preserve orientation and source tie
  order while closing the emitted ring.
- Cartesian path measurement streams its coordinate mapper once per source
  vertex, retaining the original forward summation and arithmetic mode.

`line_corridor::averaged_normal` retains the LV/MV flat-end construction and
forward ring order. LV uses a 1e-9 metre leg cutoff and 1e-9 dimensionless join
norm cutoff; MV uses exact zero for both. Degenerate legs use +X and cancelling
join directions use the next leg. These named compatibility options preserve
existing coordinates; changing their thresholds requires a rule and regression.
The independent `line_corridor` feature adds no dependencies and does not enable
Grid's rounded-corridor implementation.

Borrowed-pole segment projection keeps squared-sum arithmetic and the named
`POLE_SOURCE_SEGMENT_MIN_SQUARED_LENGTH_M2` threshold (`f64::EPSILON` square
metres). Such short legs project to their first endpoint. The source's local
frame, retained-pole reach and eligibility decisions remain consumer rules.
