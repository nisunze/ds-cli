//! Random points in an area — a survey-mimicking scatter.
//!
//! Grid-accelerated dart-throwing rejection sampler: no two accepted points are
//! closer than `min_spacing_m` (a hard floor), and the target count is derived so
//! coverage density ≈ one point per `max_spacing_m`² cell (the density ceiling).
//! Random within those bounds, so the result reads like jittered survey control
//! points rather than a rigid grid.
//!
//! Pure geo-ops, no external crates:
//! - local equirectangular metres around the area's centroid latitude, so the
//!   spacing constraint is in true metres regardless of latitude;
//! - a sparse `HashMap` background grid keyed at `min_spacing/√2`, so neighbour
//!   rejection is O(1) and memory is O(accepted points), never O(bbox);
//! - even-odd point-in-polygon with holes honoured (outer inside, holes outside);
//! - a self-contained SplitMix64 PRNG (the crate has no `rand` dependency), so a
//!   given `seed` reproduces the exact point set.

use std::collections::HashMap;

use crate::types::Coord;

const M_PER_DEG_LAT: f64 = 111_320.0;
/// Cap dart-throwing at target × this, so sliver polygons can't spin forever.
const ATTEMPTS_PER_TARGET: usize = 30;
/// Absolute ceiling on points from one area, a backstop against a huge
/// area × dense spacing request.
const MAX_POINTS: usize = 100_000;

/// SplitMix64 — deterministic, dependency-free PRNG.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)` from the top 53 bits.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Sample the polygon `rings` (ring 0 = outer, the rest = holes). Returns the
/// accepted points, empty for degenerate input or zero coverage.
pub fn sample(
    rings: &[Vec<Coord>],
    min_spacing_m: f64,
    max_spacing_m: f64,
    seed: f64,
) -> Vec<Coord> {
    if rings.is_empty() || rings[0].len() < 4 {
        return Vec::new();
    }
    if !(min_spacing_m.is_finite() && min_spacing_m > 0.0) {
        return Vec::new();
    }
    if !(max_spacing_m.is_finite() && max_spacing_m >= min_spacing_m) {
        return Vec::new();
    }

    let outer = &rings[0];
    let mut min_lng = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lng = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for c in outer {
        if !c.lng.is_finite() || !c.lat.is_finite() {
            return Vec::new();
        }
        min_lng = min_lng.min(c.lng);
        max_lng = max_lng.max(c.lng);
        min_lat = min_lat.min(c.lat);
        max_lat = max_lat.max(c.lat);
    }
    // Degenerate extent. Every bound above is already finite — the ring has at
    // least four vertices and any non-finite one returns early — so a direct
    // `<=` is exact here and carries no NaN case for the negation to hide.
    if max_lng <= min_lng || max_lat <= min_lat {
        return Vec::new();
    }

    let center_lat = (min_lat + max_lat) * 0.5;
    let m_per_deg_lng = M_PER_DEG_LAT * center_lat.to_radians().cos().max(0.05);
    let to_x = |lng: f64| (lng - min_lng) * m_per_deg_lng;
    let to_y = |lat: f64| (lat - min_lat) * M_PER_DEG_LAT;

    let area_m2 = polygon_area_m2(rings, min_lng, min_lat, m_per_deg_lng);
    if area_m2 <= 0.0 {
        return Vec::new();
    }

    // Target ≈ one point per max-spacing² cell → the density ceiling.
    let target = ((area_m2 / (max_spacing_m * max_spacing_m)).ceil() as usize).clamp(1, MAX_POINTS);
    let max_attempts = target.saturating_mul(ATTEMPTS_PER_TARGET);

    let cell = min_spacing_m / std::f64::consts::SQRT_2;
    let min_sq = min_spacing_m * min_spacing_m;
    let mut grid: HashMap<(i64, i64), Vec<(f64, f64)>> = HashMap::new();

    let mut rng = SplitMix64::new(seed.to_bits());
    let mut out: Vec<Coord> = Vec::with_capacity(target);

    let mut attempts = 0;
    while attempts < max_attempts && out.len() < target {
        attempts += 1;
        let lng = min_lng + rng.next_f64() * (max_lng - min_lng);
        let lat = min_lat + rng.next_f64() * (max_lat - min_lat);
        if !point_in_area(lng, lat, rings) {
            continue;
        }
        let x = to_x(lng);
        let y = to_y(lat);
        let cx = (x / cell).floor() as i64;
        let cy = (y / cell).floor() as i64;

        let mut ok = true;
        'reject: for gy in (cy - 2)..=(cy + 2) {
            for gx in (cx - 2)..=(cx + 2) {
                if let Some(bucket) = grid.get(&(gx, gy)) {
                    for &(px, py) in bucket {
                        if (px - x) * (px - x) + (py - y) * (py - y) < min_sq {
                            ok = false;
                            break 'reject;
                        }
                    }
                }
            }
        }
        if !ok {
            continue;
        }

        grid.entry((cx, cy)).or_default().push((x, y));
        out.push(Coord::new(lng, lat));
    }

    out
}

fn point_in_area(lng: f64, lat: f64, rings: &[Vec<Coord>]) -> bool {
    if !point_in_ring(lng, lat, &rings[0]) {
        return false;
    }
    for hole in &rings[1..] {
        if point_in_ring(lng, lat, hole) {
            return false; // inside a hole
        }
    }
    true
}

/// Even-odd ray cast against a ring of `[lng, lat]` positions.
fn point_in_ring(x: f64, y: f64, ring: &[Coord]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i].lng, ring[i].lat);
        let (xj, yj) = (ring[j].lng, ring[j].lat);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Planar shoelace area (m²): outer ring minus holes, in local metres.
fn polygon_area_m2(rings: &[Vec<Coord>], min_lng: f64, min_lat: f64, m_per_deg_lng: f64) -> f64 {
    let outer = shoelace_m2(&rings[0], min_lng, min_lat, m_per_deg_lng).abs();
    let mut holes = 0.0;
    for hole in &rings[1..] {
        holes += shoelace_m2(hole, min_lng, min_lat, m_per_deg_lng).abs();
    }
    (outer - holes).max(0.0)
}

fn shoelace_m2(ring: &[Coord], min_lng: f64, min_lat: f64, m_per_deg_lng: f64) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut j = n - 1;
    for i in 0..n {
        let xi = (ring[i].lng - min_lng) * m_per_deg_lng;
        let yi = (ring[i].lat - min_lat) * M_PER_DEG_LAT;
        let xj = (ring[j].lng - min_lng) * m_per_deg_lng;
        let yj = (ring[j].lat - min_lat) * M_PER_DEG_LAT;
        sum += xj * yi - xi * yj;
        j = i;
    }
    sum / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 0.01°×0.01° square near the equator (~1.1km per side).
    fn square() -> Vec<Vec<Coord>> {
        vec![vec![
            Coord::new(30.0, -2.0),
            Coord::new(30.01, -2.0),
            Coord::new(30.01, -1.99),
            Coord::new(30.0, -1.99),
            Coord::new(30.0, -2.0),
        ]]
    }

    #[test]
    fn all_points_inside_and_min_spacing_respected() {
        let rings = square();
        let pts = sample(&rings, 100.0, 200.0, 42.0);
        assert!(!pts.is_empty());
        for p in &pts {
            assert!(point_in_area(p.lng, p.lat, &rings));
        }
        // Pairwise min-distance holds (local-metre approximation).
        let m_per_deg_lng = M_PER_DEG_LAT * (-1.995_f64).to_radians().cos();
        for i in 0..pts.len() {
            for j in (i + 1)..pts.len() {
                let dx = (pts[i].lng - pts[j].lng) * m_per_deg_lng;
                let dy = (pts[i].lat - pts[j].lat) * M_PER_DEG_LAT;
                assert!((dx * dx + dy * dy).sqrt() >= 99.0);
            }
        }
    }

    #[test]
    fn deterministic_for_a_given_seed() {
        let rings = square();
        let a = sample(&rings, 80.0, 160.0, 7.0);
        let b = sample(&rings, 80.0, 160.0, 7.0);
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(b.iter()) {
            assert_eq!(p.lng, q.lng);
            assert_eq!(p.lat, q.lat);
        }
    }

    #[test]
    fn rejects_bad_spacing() {
        let rings = square();
        assert!(sample(&rings, 0.0, 100.0, 1.0).is_empty());
        assert!(sample(&rings, 100.0, 50.0, 1.0).is_empty());
        assert!(sample(&[], 100.0, 200.0, 1.0).is_empty());
    }

    #[test]
    fn holes_are_excluded() {
        // Outer square with a big central hole covering most of the area.
        let rings = vec![
            vec![
                Coord::new(30.0, -2.0),
                Coord::new(30.02, -2.0),
                Coord::new(30.02, -1.98),
                Coord::new(30.0, -1.98),
                Coord::new(30.0, -2.0),
            ],
            vec![
                Coord::new(30.004, -1.996),
                Coord::new(30.016, -1.996),
                Coord::new(30.016, -1.984),
                Coord::new(30.004, -1.984),
                Coord::new(30.004, -1.996),
            ],
        ];
        let pts = sample(&rings, 50.0, 100.0, 3.0);
        for p in &pts {
            assert!(point_in_area(p.lng, p.lat, &rings));
            // None should land inside the hole.
            assert!(!point_in_ring(p.lng, p.lat, &rings[1]));
        }
    }
}
