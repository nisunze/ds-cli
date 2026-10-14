//! ds-geo-lite — the lightweight geometry a field device must carry.
//!
//! Contract: `docs/contracts/ds-portable-geometry.md`; ruling:
//! `docs/contracts/ds-kernel-network-boundary.md`.
//!
//! Everything here answers **yes** to the ruling's litmus test — "would we
//! expect this capability to work on an Android or iOS field device without
//! shipping the heavy network engine?" Great-circle distance, GeoJSON extent,
//! exact area predicates, and revision comparison are all things a surveyor
//! standing in a field, offline, needs.
//!
//! Types, primitives and vector operations are maintained here.
//! Network's `ds-geo-ops` delegates to this crate. There is one implementation,
//! and neither this crate nor the command kernel depends on Network.
//!
//! Admission, spatial predicates, region algebra, measurement and extents are
//! maintained here. Identity/comparison remain byte-pinned snapshots
//! from `vendor/ds-geo-lite/`, fenced by `tests/geo_lite_drift.rs` and
//! `scripts/sync-geo-lite.sh`.

#[cfg(feature = "overlay")]
pub mod admission;
#[cfg(feature = "revision")]
#[path = "../../../vendor/ds-geo-lite/geo/comparison.rs"]
pub mod comparison;
#[cfg(feature = "geojson")]
pub mod geojson;
#[cfg(feature = "revision")]
#[path = "../../../vendor/ds-geo-lite/geo/identity.rs"]
pub mod identity;
#[cfg(feature = "overlay")]
pub mod overlay;
#[cfg(feature = "overlay")]
pub mod spatial_predicate;
#[cfg(feature = "geojson")]
pub mod viewport;

#[cfg(feature = "tin")]
pub mod exact_delaunay;
#[cfg(feature = "line_corridor")]
pub mod line_corridor;
pub mod operations;
pub mod primitives;
#[cfg(feature = "projection")]
pub mod projection;
#[cfg(feature = "tin")]
pub mod projected_tin;
pub mod types;
#[cfg(feature = "vector_types")]
pub mod vector_types;
#[cfg(feature = "vector_types")]
pub mod window;
use types::Coord;
pub use types::EARTH_RADIUS_M;

/// Raw great-circle distance; callers have already admitted finite packets.
pub fn haversine_distance_m(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
    primitives::haversine::distance(Coord::new(a_lng, a_lat), Coord::new(b_lng, b_lat))
}

/// Existing module-path facade over the canonical primitive.
pub mod haversine {
    pub use crate::{EARTH_RADIUS_M, haversine_distance_m, haversine_m};
}

/// Decode an exact `[lng, lat, ...]` packet.
///
/// These APIs are exported directly to JavaScript as `Float64Array` entry
/// points, so JSON's non-finite-number protection does not apply. Reject a
/// partial pair or an IEEE non-finite value at this boundary rather than
/// letting it drift into geometry predicates and ordering code.
pub fn flat_to_coords(flat: &[f64]) -> Option<Vec<Coord>> {
    if !flat.len().is_multiple_of(2) || flat.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(
        flat.chunks_exact(2)
            .map(|c| Coord::new(c[0], c[1]))
            .collect(),
    )
}

/// Create evenly-spaced points along a LineString.
///
/// `coords_flat`:  [lng0, lat0, lng1, lat1, …]  (WGS-84 degrees)
/// `interval_m`:   spacing in meters (> 0)
/// `include_ends`: if true, always emit the first and last vertex
///
/// Returns a flat Float64Array:  [lng, lat, distance_m, lng, lat, distance_m, …]
/// where distance_m is the cumulative distance from the start of the line.
/// Returns empty for malformed coordinates or a non-finite/non-positive interval.
pub fn points_along_line(coords_flat: &[f64], interval_m: f64, include_ends: bool) -> Vec<f64> {
    let Some(coords) = flat_to_coords(coords_flat) else {
        return Vec::new();
    };
    let pts = operations::points_along::points_along(&coords, interval_m, include_ends);

    stationed_points_to_flat(pts)
}

/// Create evenly-spaced points along a LineString while preserving every
/// source vertex as an exact station.
///
/// Returns the same flat `[lng, lat, distance_m, ...]` packet as
/// [`points_along_line`]. Endpoints are source vertices and are always present.
pub fn points_along_line_with_vertices(coords_flat: &[f64], interval_m: f64) -> Vec<f64> {
    let Some(coords) = flat_to_coords(coords_flat) else {
        return Vec::new();
    };
    let pts = operations::points_along::points_along_with_vertices(&coords, interval_m);

    stationed_points_to_flat(pts)
}

fn stationed_points_to_flat(pts: Vec<types::StationedPoint>) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len() * 3);
    for p in pts {
        out.push(p.coord.lng);
        out.push(p.coord.lat);
        out.push(p.distance_m);
    }
    out
}

pub fn coords_to_flat(coords: &[Coord]) -> Vec<f64> {
    coords.iter().flat_map(|c| [c.lng, c.lat]).collect()
}

/// Find all intersection points between two polylines.
///
/// Returns flat [lng, lat, lng, lat, …].
pub fn find_intersections(a_flat: &[f64], b_flat: &[f64]) -> Vec<f64> {
    let (Some(a), Some(b)) = (flat_to_coords(a_flat), flat_to_coords(b_flat)) else {
        return Vec::new();
    };
    coords_to_flat(&operations::intersections::find_all(&a, &b))
}

/// Haversine great-circle distance between two WGS-84 points (meters).
/// One source of truth — the prior ds-web TS sites
/// (`measure.ts:haversineM`, `entry-codec.ts:haversineM`,
/// `WorkingAreaDialog:haversineKm`, `FormFillPanel:R=6371000`,
/// `PropertiesPanel`'s dynamic import) all routed through five copies
/// with two different Earth radii; this primitive replaces all of them.
pub fn haversine_m(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
    if ![a_lng, a_lat, b_lng, b_lat]
        .iter()
        .all(|value| value.is_finite())
    {
        return f64::NAN;
    }
    primitives::haversine::distance(Coord::new(a_lng, a_lat), Coord::new(b_lng, b_lat))
}

/// Spherical polygon area for a single closed ring (m²). `coords_flat`
/// is `[lng0, lat0, lng1, lat1, …]`. Inner rings (holes) must be
/// summed separately and subtracted by the caller; this primitive
/// answers "what's the area enclosed by THIS ring?" Returns `0.0` for a
/// malformed coordinate packet.
pub fn spherical_polygon_area_m2(coords_flat: &[f64]) -> f64 {
    let Some(coords) = flat_to_coords(coords_flat) else {
        return 0.0;
    };
    primitives::spherical_area::ring_area(&coords)
}

/// Buffer a geometry by `radius_m` meters → a single closed polygon ring.
///
/// `kind`: 0 = Point (`coords_flat` = `[lng, lat]`), 1 = LineString,
/// 2 = Polygon (outer ring). `quad_segs` = arc segments per 90°, so a circle
/// has `quad_segs * 4` vertices. Returns a flat closed ring
/// `[lng, lat, …, lng0, lat0]`, or empty on failure / invalid radius.
pub fn buffer_geometry(kind: u32, coords_flat: &[f64], radius_m: f64, quad_segs: u32) -> Vec<f64> {
    let Some(coords) = flat_to_coords(coords_flat) else {
        return Vec::new();
    };
    let ring = match kind {
        0 if coords.len() == 1 => operations::buffer::buffer_point(coords[0], radius_m, quad_segs),
        0 => Vec::new(),
        1 => operations::buffer::buffer_line(&coords, radius_m, quad_segs),
        2 => operations::buffer::buffer_polygon(&coords, radius_m, quad_segs),
        _ => Vec::new(),
    };
    coords_to_flat(&ring)
}

/// Initial geodesic bearing from a→b in degrees (0 = north, clockwise).
pub fn bearing_deg(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
    if ![a_lng, a_lat, b_lng, b_lat]
        .iter()
        .all(|value| value.is_finite())
    {
        return f64::NAN;
    }
    primitives::bearing::initial(Coord::new(a_lng, a_lat), Coord::new(b_lng, b_lat))
}

/// Geodesic destination — travel `distance_m` from `(lng, lat)` along
/// `bearing_deg`. Returns `[lng, lat]`. Inverse of `bearing_deg` + `haversine_m`.
pub fn destination_point(lng: f64, lat: f64, bearing_deg: f64, distance_m: f64) -> Vec<f64> {
    if ![lng, lat, bearing_deg, distance_m]
        .iter()
        .all(|value| value.is_finite())
    {
        return Vec::new();
    }
    let p = primitives::destination::destination(Coord::new(lng, lat), bearing_deg, distance_m);
    vec![p.lng, p.lat]
}

#[cfg(any(
    feature = "editing",
    feature = "profile",
    feature = "tin",
    feature = "sampling"
))]
mod flat_ops;
#[cfg(any(
    feature = "editing",
    feature = "profile",
    feature = "tin",
    feature = "sampling"
))]
pub use flat_ops::*;

pub mod cartesian;
pub mod predicates;
pub mod tolerance;

#[cfg(feature = "geojson")]
pub mod measurement;
#[cfg(feature = "geojson")]
pub mod region;

#[cfg(feature = "codec")]
pub mod wkb;

#[cfg(feature = "index")]
pub mod measured_path;

#[cfg(feature = "corridor")]
pub mod corridor;
#[cfg(feature = "overlay")]
pub mod extent;
#[cfg(feature = "planar")]
pub mod planar;
/// Index types are supplied through the geometry owner; consumers retain rows.
#[cfg(feature = "index")]
pub mod index {
    pub use rstar::*;
}
