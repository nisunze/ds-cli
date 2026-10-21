//! Line offset (AutoCAD OFFSET) — a parallel copy of a polyline at a fixed
//! perpendicular distance on one side. Unlike `buffer` (a closed region with
//! round caps), offset returns a single open polyline with **miter** corners:
//! adjacent offset segments are extended to their intersection, with a bevel
//! fallback past the miter limit so sharp turns don't spike to infinity.
//!
//! Geodesic perpendicular (via `destination`), planar segment intersection
//! (via `segment_segment`) — the same hybrid the rest of geo-ops uses.

use crate::primitives::bearing::initial as bearing;
use crate::primitives::destination::destination;
use crate::primitives::haversine::distance as haversine;
use crate::primitives::intersection::segment_segment;
use crate::types::Coord;

const DEDUPE_EPS_M: f64 = 1e-6;
/// Beyond `MITER_LIMIT * distance` from the original vertex, the miter spike
/// is replaced by a bevel (the two offset endpoints connected directly).
const MITER_LIMIT: f64 = 8.0;

fn dedupe(pts: &[Coord]) -> Vec<Coord> {
    let mut out: Vec<Coord> = Vec::with_capacity(pts.len());
    for &p in pts {
        match out.last() {
            Some(&last) if haversine(last, p) <= DEDUPE_EPS_M => {}
            _ => out.push(p),
        }
    }
    out
}

/// Offset `pts` by `distance_m` meters. `right_side` = right of travel
/// (bearing + 90°); otherwise left (bearing − 90°). Returns the offset
/// polyline, or empty on failure / `distance_m <= 0`.
pub fn offset_line(pts_in: &[Coord], distance_m: f64, right_side: bool) -> Vec<Coord> {
    if !distance_m.is_finite() || distance_m <= 0.0 {
        return Vec::new();
    }
    let pts = dedupe(pts_in);
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let sign = if right_side { 90.0 } else { -90.0 };

    // Per-segment offset endpoints.
    let mut off_a: Vec<Coord> = Vec::with_capacity(n - 1);
    let mut off_b: Vec<Coord> = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let b = bearing(pts[i], pts[i + 1]);
        off_a.push(destination(pts[i], b + sign, distance_m));
        off_b.push(destination(pts[i + 1], b + sign, distance_m));
    }

    let mut out: Vec<Coord> = Vec::with_capacity(n);
    out.push(off_a[0]);
    for i in 0..n - 2 {
        // Corner at original vertex pts[i+1], between offset segment i and i+1.
        match segment_segment(off_a[i], off_b[i], off_a[i + 1], off_b[i + 1]) {
            Some(ix) if haversine(pts[i + 1], ix.point) <= MITER_LIMIT * distance_m => {
                out.push(ix.point); // miter corner
            }
            _ => {
                // Collinear (no intersection) or miter too long → bevel.
                out.push(off_b[i]);
                out.push(off_a[i + 1]);
            }
        }
    }
    out.push(off_b[n - 2]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_line_offsets_parallel_at_distance() {
        // East-bound segment near the equator.
        let pts = [Coord::new(30.0, 0.0), Coord::new(30.01, 0.0)];
        let right = offset_line(&pts, 50.0, true);
        assert_eq!(right.len(), 2);
        // Right of east-travel = south → latitude decreases.
        assert!(right[0].lat < 0.0 && right[1].lat < 0.0);
        assert!((haversine(pts[0], right[0]) - 50.0).abs() < 0.5);
        assert!((haversine(pts[1], right[1]) - 50.0).abs() < 0.5);

        // Left side mirrors to the north.
        let left = offset_line(&pts, 50.0, false);
        assert!(left[0].lat > 0.0 && left[1].lat > 0.0);
    }

    #[test]
    fn corner_produces_a_miter_vertex() {
        // East then north (an L).
        let pts = [
            Coord::new(30.0, 0.0),
            Coord::new(30.01, 0.0),
            Coord::new(30.01, 0.01),
        ];
        let out = offset_line(&pts, 20.0, true);
        // One miter vertex → same count as input (3).
        assert_eq!(out.len(), 3);
        for c in &out {
            assert!(c.lng.is_finite() && c.lat.is_finite());
        }
    }

    #[test]
    fn non_positive_distance_is_empty() {
        let pts = [Coord::new(0.0, 0.0), Coord::new(1.0, 0.0)];
        assert!(offset_line(&pts, 0.0, true).is_empty());
        assert!(offset_line(&pts, -3.0, true).is_empty());
    }
}
