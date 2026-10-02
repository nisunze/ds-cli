//! Geodesic buffer — a Minkowski-sum-with-disc around point / line / polygon
//! geometry, returned as a single closed polygon ring (flat lng/lat).
//!
//! Construction is the classic "offset both sides, round caps + round joins"
//! sweep, done in spherical space with `destination` (no projection step, no
//! external geo crate — same model as the rest of geo-ops). Round joins are
//! emitted only on the convex side of each turn; the concave side is left to
//! self-overlap, which fills correctly under a non-zero winding rule. For the
//! simple polylines this domain buffers (LV/MV spans, service cables) the
//! ring is exact; for sharply self-overlapping inputs it is a valid filled
//! region rather than a dissolved boundary.

use crate::primitives::bearing::initial as bearing;
use crate::primitives::destination::destination;
use crate::primitives::haversine::distance as haversine;
use crate::types::Coord;

const MIN_QUAD_SEGS: u32 = 1;
const DEDUPE_EPS_M: f64 = 1e-6;

fn step_deg(quad_segs: u32) -> f64 {
    90.0 / (quad_segs.max(MIN_QUAD_SEGS) as f64)
}

/// Normalize a bearing delta to (-180, 180]. Negative = left turn
/// (counterclockwise), positive = right turn (clockwise).
fn norm_delta(mut d: f64) -> f64 {
    while d <= -180.0 {
        d += 360.0;
    }
    while d > 180.0 {
        d -= 360.0;
    }
    d
}

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

/// Arc points around `center`, starting at `from_deg` and sweeping by
/// `signed_sweep` degrees (negative = counterclockwise), at ~`step` deg
/// resolution. EXCLUDES both endpoints — the caller pushes the exact offset
/// points so adjacent edges stay watertight.
fn arc(center: Coord, from_deg: f64, signed_sweep: f64, r: f64, step: f64) -> Vec<Coord> {
    let steps = (signed_sweep.abs() / step).ceil().max(1.0) as usize;
    let mut out = Vec::with_capacity(steps.saturating_sub(1));
    for k in 1..steps {
        let b = from_deg + signed_sweep * (k as f64) / (steps as f64);
        out.push(destination(center, b, r));
    }
    out
}

fn close(mut ring: Vec<Coord>) -> Vec<Coord> {
    if let Some(&first) = ring.first() {
        let needs_close = ring
            .last()
            .is_none_or(|&last| haversine(last, first) > DEDUPE_EPS_M);
        if needs_close {
            ring.push(first);
        }
    }
    ring
}

/// Circle around a point — `quad_segs * 4` vertices, counterclockwise.
pub fn buffer_point(center: Coord, r: f64, quad_segs: u32) -> Vec<Coord> {
    if !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    let n = (quad_segs.max(MIN_QUAD_SEGS) * 4) as usize;
    let mut ring = Vec::with_capacity(n + 1);
    for i in 0..n {
        // Counterclockwise (GeoJSON exterior preference): bearing decreases.
        let b = -360.0 * (i as f64) / (n as f64);
        ring.push(destination(center, b, r));
    }
    close(ring)
}

/// Corridor around an open polyline — offset both sides by `r`, round caps at
/// the two ends, round joins on convex corners.
pub fn buffer_line(pts_in: &[Coord], r: f64, quad_segs: u32) -> Vec<Coord> {
    if !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    let pts = dedupe(pts_in);
    if pts.len() < 2 {
        return pts
            .first()
            .map_or_else(Vec::new, |&p| buffer_point(p, r, quad_segs));
    }
    let st = step_deg(quad_segs);
    let n = pts.len();
    let b: Vec<f64> = (0..n - 1).map(|i| bearing(pts[i], pts[i + 1])).collect();
    let mut ring: Vec<Coord> = Vec::new();

    // Right side, forward (offset normal = bearing + 90°).
    for i in 0..n - 1 {
        ring.push(destination(pts[i], b[i] + 90.0, r));
        ring.push(destination(pts[i + 1], b[i] + 90.0, r));
        if i + 1 < n - 1 {
            let delta = norm_delta(b[i + 1] - b[i]);
            if delta < 0.0 {
                // Left turn → right side is the convex (outer) side → round it.
                ring.extend(arc(pts[i + 1], b[i] + 90.0, delta, r, st));
            }
        }
    }
    // End cap (bulge forward): (b_last + 90) sweep −180 → (b_last − 90).
    ring.extend(arc(pts[n - 1], b[n - 2] + 90.0, -180.0, r, st));

    // Left side, backward (offset normal = bearing − 90°).
    for i in (0..n - 1).rev() {
        ring.push(destination(pts[i + 1], b[i] - 90.0, r));
        ring.push(destination(pts[i], b[i] - 90.0, r));
        if i >= 1 {
            let delta = norm_delta(b[i] - b[i - 1]);
            if delta > 0.0 {
                // Right turn → left side is convex → round it (sweeping back).
                ring.extend(arc(pts[i], b[i] - 90.0, -delta, r, st));
            }
        }
    }
    // Start cap (bulge backward): (b0 − 90) sweep −180 → (b0 + 90).
    ring.extend(arc(pts[0], b[0] - 90.0, -180.0, r, st));

    close(ring)
}

/// Outward buffer of a polygon's exterior ring — offset every edge outward by
/// `r` with round joins on convex corners. Orientation is detected so the
/// offset always grows the polygon (works for CW or CCW input). Holes are not
/// modeled here; the caller buffers the outer ring.
pub fn buffer_polygon(ring_in: &[Coord], r: f64, quad_segs: u32) -> Vec<Coord> {
    if !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    let mut pts = dedupe(ring_in);
    // Drop the explicit closing vertex if present; we treat the ring cyclically.
    if pts.len() >= 2 {
        let first = pts[0];
        if haversine(*pts.last().unwrap(), first) <= DEDUPE_EPS_M {
            pts.pop();
        }
    }
    let n = pts.len();
    if n < 3 {
        return buffer_line(&pts, r, quad_segs);
    }
    let st = step_deg(quad_segs);

    // Shoelace sign (planar lng/lat is fine for orientation only).
    let area2: f64 = (0..n)
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            a.lng * b.lat - b.lng * a.lat
        })
        .sum();
    let ccw = area2 > 0.0;
    // Outward normal: CCW ring → interior on the left → outward is +90 (right);
    // CW ring → outward is −90 (left).
    let out_sign = if ccw { 90.0 } else { -90.0 };

    let eb: Vec<f64> = (0..n).map(|i| bearing(pts[i], pts[(i + 1) % n])).collect();
    let mut ring: Vec<Coord> = Vec::new();
    for i in 0..n {
        ring.push(destination(pts[i], eb[i] + out_sign, r));
        ring.push(destination(pts[(i + 1) % n], eb[i] + out_sign, r));
        let delta = norm_delta(eb[(i + 1) % n] - eb[i]);
        // Convex (outer) corner: CCW turns left (delta<0), CW turns right (delta>0).
        let convex = if ccw { delta < 0.0 } else { delta > 0.0 };
        if convex {
            ring.extend(arc(pts[(i + 1) % n], eb[i] + out_sign, delta, r, st));
        }
    }
    close(ring)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(ring: &[Coord]) -> (f64, f64, f64, f64) {
        let mut min_lng = f64::INFINITY;
        let mut max_lng = f64::NEG_INFINITY;
        let mut min_lat = f64::INFINITY;
        let mut max_lat = f64::NEG_INFINITY;
        for c in ring {
            assert!(c.lng.is_finite() && c.lat.is_finite(), "non-finite vertex");
            min_lng = min_lng.min(c.lng);
            max_lng = max_lng.max(c.lng);
            min_lat = min_lat.min(c.lat);
            max_lat = max_lat.max(c.lat);
        }
        (min_lng, min_lat, max_lng, max_lat)
    }

    fn is_closed(ring: &[Coord]) -> bool {
        ring.len() >= 4 && haversine(ring[0], *ring.last().unwrap()) <= DEDUPE_EPS_M
    }

    #[test]
    fn point_buffer_is_a_circle_of_radius_r() {
        let center = Coord::new(30.0, -2.0);
        let ring = buffer_point(center, 100.0, 8);
        assert!(is_closed(&ring));
        assert_eq!(ring.len(), 8 * 4 + 1);
        for c in &ring {
            assert!(
                (haversine(center, *c) - 100.0).abs() < 0.2,
                "vertex off-radius"
            );
        }
    }

    #[test]
    fn line_buffer_extends_r_perpendicular_and_caps_the_ends() {
        // ~horizontal segment near the equator.
        let pts = [Coord::new(30.0, 0.0), Coord::new(30.01, 0.0)];
        let ring = buffer_line(&pts, 50.0, 8);
        assert!(is_closed(&ring));
        let (min_lng, min_lat, max_lng, max_lat) = bbox(&ring);
        // Half-height ≈ r on each side.
        let half_h_m = haversine(Coord::new(30.0, 0.0), Coord::new(30.0, max_lat));
        assert!(
            (half_h_m - 50.0).abs() < 1.0,
            "perpendicular extent {half_h_m}"
        );
        assert!((haversine(Coord::new(30.0, 0.0), Coord::new(30.0, min_lat)) - 50.0).abs() < 1.0);
        // End caps push ≈ r beyond each endpoint.
        let beyond_start = haversine(Coord::new(30.0, 0.0), Coord::new(min_lng, 0.0));
        let beyond_end = haversine(Coord::new(30.01, 0.0), Coord::new(max_lng, 0.0));
        assert!(
            (beyond_start - 50.0).abs() < 1.0,
            "start cap {beyond_start}"
        );
        assert!((beyond_end - 50.0).abs() < 1.0, "end cap {beyond_end}");
    }

    #[test]
    fn line_buffer_handles_a_bend_without_nans() {
        let pts = [
            Coord::new(30.0, 0.0),
            Coord::new(30.01, 0.0),
            Coord::new(30.01, 0.01),
        ];
        let ring = buffer_line(&pts, 30.0, 6);
        assert!(is_closed(&ring));
        assert!(ring.len() > 8);
    }

    #[test]
    fn polygon_buffer_grows_the_box_outward() {
        // ~square ring (CCW), ~1km sides near equator.
        let ring_in = [
            Coord::new(30.0, 0.0),
            Coord::new(30.009, 0.0),
            Coord::new(30.009, 0.009),
            Coord::new(30.0, 0.009),
            Coord::new(30.0, 0.0),
        ];
        let (i_min_lng, i_min_lat, i_max_lng, i_max_lat) = bbox(&ring_in);
        let out = buffer_polygon(&ring_in, 40.0, 6);
        assert!(is_closed(&out));
        let (o_min_lng, o_min_lat, o_max_lng, o_max_lat) = bbox(&out);
        // Each side pushed outward by ≈ r.
        assert!(o_min_lng < i_min_lng && o_min_lat < i_min_lat);
        assert!(o_max_lng > i_max_lng && o_max_lat > i_max_lat);
        let grew = haversine(Coord::new(i_max_lng, 0.0), Coord::new(o_max_lng, 0.0));
        assert!((grew - 40.0).abs() < 1.5, "outward growth {grew}");
    }

    #[test]
    fn non_positive_radius_yields_nothing() {
        assert!(buffer_point(Coord::new(0.0, 0.0), 0.0, 8).is_empty());
        assert!(buffer_line(&[Coord::new(0.0, 0.0), Coord::new(1.0, 0.0)], -5.0, 8).is_empty());
    }
}
