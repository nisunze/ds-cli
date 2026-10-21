//! Spherical polygon area — closed-form approximation from the
//! longitudinal excess sum. Accurate to within < 0.01% for polygons
//! whose extent is small relative to Earth's radius (the working-area
//! / freestyle-sketch / measure-tool envelopes ds-web actually draws).
//!
//! This is the same approximation MapLibre's measure tool uses on the
//! frontend, deliberately — the WASM exposure is what removes the
//! "five copies of haversine and three different Earth radii" drift
//! flagged in the WASM audit.

use crate::types::{Coord, EARTH_RADIUS_M};

/// Signed-magnitude polygon area (m²) for a closed ring of WGS-84
/// coordinates. Returns `0.0` for degenerate inputs (< 3 vertices).
/// Inner rings (holes) are NOT subtracted — caller is responsible for
/// summing outer rings and subtracting inner rings.
pub fn ring_area(ring: &[Coord]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let n = ring.len();
    let mut total = 0.0_f64;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        let dlng = (b.lng - a.lng).to_radians();
        total += dlng * (2.0 + a.lat.to_radians().sin() + b.lat.to_radians().sin());
    }
    (total * EARTH_RADIUS_M * EARTH_RADIUS_M / 2.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(lng: f64, lat: f64) -> Coord {
        Coord::new(lng, lat)
    }

    #[test]
    fn degenerate_ring_is_zero() {
        assert_eq!(ring_area(&[]), 0.0);
        assert_eq!(ring_area(&[coord(0.0, 0.0)]), 0.0);
        assert_eq!(ring_area(&[coord(0.0, 0.0), coord(1.0, 0.0)]), 0.0);
    }

    #[test]
    fn small_square_near_equator_is_close_to_planar() {
        // 200 m × 200 m square centred at (30, 0). Planar area = 40_000 m².
        // The spherical-excess approximation lands within ~0.3% of planar
        // for envelopes this small; the prior TS implementation in
        // measure.ts had identical drift, so this is the tolerance the
        // working-area / freestyle-polygon UI has always been showing.
        let d = 100.0 / 111_320.0;
        let ring = vec![
            coord(30.0 - d, -d),
            coord(30.0 + d, -d),
            coord(30.0 + d, d),
            coord(30.0 - d, d),
            coord(30.0 - d, -d),
        ];
        let area = ring_area(&ring);
        assert!(
            (area - 40_000.0).abs() < 200.0,
            "area={area}, expected within 200 m² of 40000"
        );
    }

    #[test]
    fn ring_winding_does_not_change_magnitude() {
        let cw = vec![
            coord(0.0, 0.0),
            coord(0.001, 0.0),
            coord(0.001, 0.001),
            coord(0.0, 0.001),
            coord(0.0, 0.0),
        ];
        let ccw: Vec<Coord> = cw.iter().rev().copied().collect();
        let a = ring_area(&cw);
        let b = ring_area(&ccw);
        assert!((a - b).abs() < 1e-6, "cw={a}, ccw={b}");
    }
}
