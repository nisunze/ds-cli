//! Great-circle distance on a spherical Earth.
//!
//! PARTIAL snapshot — see `../MANIFEST.json`. `ds-geo-ops` spreads this one
//! function over three files (`primitives/haversine.rs::distance`,
//! `lib.rs::haversine_m`, `types.rs::EARTH_RADIUS_M`) whose siblings pull the
//! whole primitives/operations tree, so the cut is at item level rather than
//! file level: the bodies below are transcribed, not re-derived. `ds-geo-ops`
//! keeps its own copy — this is a snapshot, and the drift test is what keeps
//! the two honest.

/// Earth mean radius (meters) — WGS-84 volumetric.
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Haversine great-circle distance between two WGS-84 points (meters),
/// **without** the non-finite guard.
///
/// This is the raw primitive. `ds-geo-ops` composes it inside operations that
/// have already validated their coordinate packets, so it pays the finiteness
/// check once at the packet boundary rather than per segment. New callers
/// almost always want [`haversine_m`] instead.
pub fn haversine_distance_m(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
    let dlat = (b_lat - a_lat).to_radians();
    let dlng = (b_lng - a_lng).to_radians();
    let h = (dlat / 2.0).sin().powi(2)
        + a_lat.to_radians().cos() * b_lat.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    EARTH_RADIUS_M * 2.0 * h.sqrt().atan2((1.0 - h).sqrt())
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
    haversine_distance_m(a_lng, a_lat, b_lng, b_lat)
}
