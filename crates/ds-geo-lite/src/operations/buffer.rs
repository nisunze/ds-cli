//! Geodesic buffer — a Minkowski-sum-with-disc around point / line / polygon
//! geometry, returned as a single closed polygon ring (flat lng/lat).
//!
//! Segment capsules and vertex discs are constructed in spherical space with
//! `destination`, without a projection or an optional geometry dependency.
//! Their union is dissolved in output longitude/latitude coordinates: intersecting
//! edges are split and covered edges removed before tracing the exterior CCW.
//! As with the single-ring API for polygons, enclosed holes are not represented.

use crate::primitives::bearing::initial as bearing;
use crate::primitives::destination::destination;
use crate::primitives::haversine::distance as haversine;
use crate::types::Coord;

const MIN_QUAD_SEGS: u32 = 1;
const DEDUPE_EPS_M: f64 = 1e-6;

fn step_deg(quad_segs: u32) -> f64 {
    90.0 / (quad_segs.max(MIN_QUAD_SEGS) as f64)
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
        let needs_close = ring.last().is_none_or(|&last| last != first);
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

/// Corridor around an open polyline, with round caps and round joins.
/// Returns the counterclockwise exterior of the union of spherical capsules.
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
    exterior_union(
        pts.windows(2)
            .map(|p| capsule(p[0], p[1], r, quad_segs))
            .collect(),
        snap_deg(r),
    )
}

/// Outward buffer of a polygon exterior (either input orientation).
/// Holes are not modeled by this single-exterior-ring API.
pub fn buffer_polygon(ring_in: &[Coord], r: f64, quad_segs: u32) -> Vec<Coord> {
    if !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    let mut pts = dedupe(ring_in);
    if pts.len() > 1 && haversine(pts[0], *pts.last().unwrap()) <= DEDUPE_EPS_M {
        pts.pop();
    }
    if pts.len() < 3 {
        return buffer_line(&pts, r, quad_segs);
    }
    let mut pieces: Vec<_> = (0..pts.len())
        .map(|i| capsule(pts[i], pts[(i + 1) % pts.len()], r, quad_segs))
        .collect();
    pieces.push(pts);
    exterior_union(pieces, snap_deg(r))
}

/// Node coalescing tolerance in degrees for a buffer of radius `r` metres.
fn snap_deg(r: f64) -> f64 {
    (r / 111_320.0 * 1e-6).max(1e-12)
}

fn capsule(a: Coord, b: Coord, r: f64, quad_segs: u32) -> Vec<Coord> {
    let start = bearing(a, b);
    // Use the terminal bearing at b, rather than a's initial bearing, so the
    // normals remain perpendicular to the great-circle segment at both ends.
    let end = bearing(b, a) + 180.0;
    let step = step_deg(quad_segs);
    let mut ring = vec![
        destination(a, start + 90.0, r),
        destination(b, end + 90.0, r),
    ];
    ring.extend(arc(b, end + 90.0, -180.0, r, step));
    ring.push(destination(b, end - 90.0, r));
    ring.push(destination(a, start - 90.0, r));
    ring.extend(arc(a, start - 90.0, -180.0, r, step));
    ring
}

fn cross(a: Coord, b: Coord) -> f64 {
    a.lng * b.lat - a.lat * b.lng
}

fn sub(a: Coord, b: Coord) -> Coord {
    Coord::new(a.lng - b.lng, a.lat - b.lat)
}

fn area2(ring: &[Coord]) -> f64 {
    // Translate before summing, avoiding cancellation at nonzero longitude.
    let o = ring[0];
    (0..ring.len())
        .map(|i| cross(sub(ring[i], o), sub(ring[(i + 1) % ring.len()], o)))
        .sum()
}

/// Sweep along latitude when the points extend further north-south.
fn axis_is_lat(points: impl Iterator<Item = Coord>) -> bool {
    let b = bounds_of(points);
    b[3] - b[1] > b[2] - b[0]
}

/// `[min_lng, min_lat, max_lng, max_lat]` of a ring.
fn bounds(ring: &[Coord]) -> [f64; 4] {
    bounds_of(ring.iter().copied())
}

fn bounds_of(points: impl Iterator<Item = Coord>) -> [f64; 4] {
    points.fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[x0, y0, x1, y1], p| [x0.min(p.lng), y0.min(p.lat), x1.max(p.lng), y1.max(p.lat)],
    )
}

#[derive(Clone)]
struct Edge {
    a: Coord,
    b: Coord,
    piece: usize,
    cuts: Vec<(f64, Coord)>,
}

fn parameter(p: Coord, a: Coord, b: Coord) -> f64 {
    let d = sub(b, a);
    if d.lng.abs() >= d.lat.abs() {
        (p.lng - a.lng) / d.lng
    } else {
        (p.lat - a.lat) / d.lat
    }
}

fn on_segment(p: Coord, a: Coord, b: Coord) -> bool {
    let d = sub(b, a);
    let v = sub(p, a);
    // Subtracting longitude near 30 degrees carries absolute coordinate
    // roundoff, even when the edge is only micrometres long.
    let coord_error = 8.0
        * f64::EPSILON
        * p.lng
            .abs()
            .max(a.lng.abs())
            .max(b.lng.abs())
            .max(p.lat.abs())
            .max(a.lat.abs())
            .max(b.lat.abs())
            .max(1.0);
    let tolerance = coord_error * (d.lng.abs() + d.lat.abs());
    let t = parameter(p, a, b);
    let slack = coord_error / d.lng.abs().max(d.lat.abs());
    cross(d, v).abs() <= tolerance && t >= -slack && t <= 1.0 + slack
}

// -1 outside, 0 on boundary, 1 inside. No geometric epsilon that could erase
// narrow corridors: use the same straight-edge predicates as the output ring.
fn location(p: Coord, ring: &[Coord]) -> i8 {
    let mut inside = false;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        if on_segment(p, a, b) {
            return 0;
        }
        if (a.lat > p.lat) != (b.lat > p.lat)
            && p.lng < a.lng + (p.lat - a.lat) * (b.lng - a.lng) / (b.lat - a.lat)
        {
            inside = !inside;
        }
    }
    if inside { 1 } else { -1 }
}

/// Dissolve a connected set of rings. All decisions are on the constructed
/// coordinates; this is topology cleanup, not a projection of the input.
fn exterior_union(mut pieces: Vec<Vec<Coord>>, snap: f64) -> Vec<Coord> {
    // Put all pieces in the same longitude branch (including dateline routes).
    let origin = pieces[0][0].lng;
    for ring in &mut pieces {
        for p in ring.iter_mut() {
            p.lng = origin + (p.lng - origin + 180.0).rem_euclid(360.0) - 180.0;
        }
        if area2(ring) < 0.0 {
            ring.reverse();
        }
    }
    let mut edges = Vec::new();
    for (piece, ring) in pieces.iter().enumerate() {
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if a != b {
                edges.push(Edge {
                    a,
                    b,
                    piece,
                    cuts: vec![(0.0, a), (1.0, b)],
                });
            }
        }
    }
    // Only edges whose extents overlap along the longer axis can meet. A sweep
    // over that axis finds them in O(n log n + k) instead of testing all pairs
    // (a 3,000-vertex route took over a minute); the candidates are then
    // visited in the all-pairs (i, j) order, so every cut is pushed exactly as
    // before and the output is unchanged.
    let along_lat = axis_is_lat(edges.iter().flat_map(|e| [e.a, e.b]));
    let span = |e: &Edge| {
        let (p, q) = if along_lat {
            (e.a.lat, e.b.lat)
        } else {
            (e.a.lng, e.b.lng)
        };
        (p.min(q), p.max(q))
    };
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&x, &y| {
        span(&edges[x])
            .0
            .total_cmp(&span(&edges[y]).0)
            .then(x.cmp(&y))
    });
    let mut pairs = Vec::new();
    for (k, &x) in order.iter().enumerate() {
        let end = span(&edges[x]).1;
        for &y in &order[k + 1..] {
            if span(&edges[y]).0 > end {
                break;
            }
            if edges[x].piece != edges[y].piece {
                pairs.push((x.min(y), x.max(y)));
            }
        }
    }
    pairs.sort_unstable();
    // Split both edges at the *same* coordinate, including collinear overlap
    // endpoints. Bounding-box pruning keeps distant segments out of this work.
    for (i, j) in pairs {
        {
            let (a, b, c, d) = (edges[i].a, edges[i].b, edges[j].a, edges[j].b);
            if a.lng.min(b.lng) > c.lng.max(d.lng)
                || c.lng.min(d.lng) > a.lng.max(b.lng)
                || a.lat.min(b.lat) > c.lat.max(d.lat)
                || c.lat.min(d.lat) > a.lat.max(b.lat)
            {
                continue;
            }
            let u = sub(b, a);
            let v = sub(d, c);
            let w = sub(c, a);
            let den = cross(u, v);
            if den != 0.0 {
                let t = cross(w, v) / den;
                let q = cross(w, u) / den;
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&q) {
                    let p = if t == 0.0 {
                        a
                    } else if t == 1.0 {
                        b
                    } else if q == 0.0 {
                        c
                    } else if q == 1.0 {
                        d
                    } else {
                        Coord::new(a.lng + t * u.lng, a.lat + t * u.lat)
                    };
                    edges[i].cuts.push((t, p));
                    edges[j].cuts.push((q, p));
                }
            } else {
                for p in [a, b, c, d] {
                    if on_segment(p, a, b) && on_segment(p, c, d) {
                        edges[i].cuts.push((parameter(p, a, b), p));
                        edges[j].cuts.push((parameter(p, c, d), p));
                    }
                }
            }
        }
    }
    let mut nodes = Vec::<Coord>::new();
    let mut node_bins = std::collections::BTreeMap::<(i64, i64), Vec<usize>>::new();
    let mut boundary = std::collections::BTreeSet::new();
    // Coalesce one point computed twice: an intersection along two edges, or a
    // joint offset built by both neighbouring capsules. On a straight multi-span
    // section the two capsules reach the shared offset through bearings that
    // differ only by roundoff, about 2e-13 of the radius; left apart, the
    // boundary breaks and the ring comes back empty. `snap` is far above that
    // noise and far below the arc vertex spacing (about a fifth of the radius).
    let mut intern = |p: Coord| {
        let eps = snap;
        let key = ((p.lng / eps).floor() as i64, (p.lat / eps).floor() as i64);
        for x in key.0 - 1..=key.0 + 1 {
            for y in key.1 - 1..=key.1 + 1 {
                if let Some(ids) = node_bins.get(&(x, y)) {
                    for &id in ids {
                        if (nodes[id].lng - p.lng).abs() <= eps
                            && (nodes[id].lat - p.lat).abs() <= eps
                        {
                            return id;
                        }
                    }
                }
            }
        }
        let id = nodes.len();
        nodes.push(p);
        node_bins.entry(key).or_default().push(id);
        id
    };
    // A midpoint can be inside, or on the boundary of, a piece only within the
    // piece's box; the margin covers the boundary predicate's roundoff. Pieces
    // are windowed along the sweep axis, and the few unusually wide ones (the
    // input polygon in `buffer_polygon`) are always checked.
    const MARGIN: f64 = 1e-9;
    let boxes: Vec<[f64; 4]> = pieces.iter().map(|ring| bounds(ring)).collect();
    let low = |b: &[f64; 4]| if along_lat { b[1] } else { b[0] };
    let extent = |b: &[f64; 4]| if along_lat { b[3] - b[1] } else { b[2] - b[0] };
    let mut extents: Vec<f64> = boxes.iter().map(extent).collect();
    extents.sort_by(f64::total_cmp);
    let reach = extents[extents.len() * 99 / 100];
    let (wide, mut windowed): (Vec<usize>, Vec<usize>) =
        (0..pieces.len()).partition(|&id| extent(&boxes[id]) > reach);
    windowed.sort_by(|&x, &y| low(&boxes[x]).total_cmp(&low(&boxes[y])).then(x.cmp(&y)));
    for edge in &mut edges {
        edge.cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        edge.cuts.dedup_by(|a, b| a.0 == b.0);
        for cut in edge.cuts.windows(2) {
            let a = cut[0].1;
            let b = cut[1].1;
            let mid = Coord::new(a.lng + (b.lng - a.lng) * 0.5, a.lat + (b.lat - a.lat) * 0.5);
            let s = if along_lat { mid.lat } else { mid.lng };
            let first = windowed.partition_point(|&id| low(&boxes[id]) < s - reach - MARGIN);
            let last = windowed.partition_point(|&id| low(&boxes[id]) <= s + MARGIN);
            let candidates = windowed[first..last]
                .iter()
                .chain(&wide)
                .copied()
                .filter(|&id| {
                    let [x0, y0, x1, y1] = boxes[id];
                    mid.lng >= x0 - MARGIN
                        && mid.lng <= x1 + MARGIN
                        && mid.lat >= y0 - MARGIN
                        && mid.lat <= y1 + MARGIN
                });
            let covered = candidates.map(|id| (id, &pieces[id])).any(|(id, ring)| {
                if id == edge.piece {
                    return false;
                }
                match location(mid, ring) {
                    1 => true,
                    0 => {
                        // Coincident boundaries: opposite direction is internal;
                        // same direction is retained once, by source order.
                        (0..ring.len()).any(|i| {
                            let c = ring[i];
                            let d = ring[(i + 1) % ring.len()];
                            on_segment(mid, c, d)
                                && (id < edge.piece || {
                                    let u = sub(b, a);
                                    let v = sub(d, c);
                                    u.lng * v.lng + u.lat * v.lat < 0.0
                                })
                        })
                    }
                    _ => false,
                }
            });
            if !covered {
                let u = intern(a);
                let v = intern(b);
                if u != v {
                    boundary.insert((u, v));
                }
            }
        }
    }
    let mut outgoing = vec![Vec::new(); nodes.len()];
    for &(a, b) in &boundary {
        outgoing[a].push(b);
    }
    let mut result = Vec::new();
    let mut largest = 0.0;
    while let Some(&(start, next)) = boundary.first() {
        let mut ids = vec![start];
        let (mut prev, mut at) = (start, next);
        while boundary.remove(&(prev, at)) {
            if at == start {
                break;
            }
            ids.push(at);
            let back = sub(nodes[prev], nodes[at]);
            let back_angle = back.lat.atan2(back.lng);
            let next = outgoing[at]
                .iter()
                .copied()
                .filter(|&b| boundary.contains(&(at, b)))
                .min_by(|&a, &b| {
                    let angle = |id| {
                        let v = sub(nodes[id], nodes[at]);
                        (back_angle - v.lat.atan2(v.lng)).rem_euclid(std::f64::consts::TAU)
                    };
                    angle(a).total_cmp(&angle(b)).then(a.cmp(&b))
                });
            let Some(next) = next else {
                break;
            };
            prev = at;
            at = next;
        }
        if at == start && ids.len() >= 3 {
            let ring: Vec<_> = ids.into_iter().map(|i| nodes[i]).collect();
            let area = area2(&ring);
            if area > largest {
                largest = area;
                result = ring;
            }
        }
    }
    close(result)
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

    // Exact inclusive segment predicate: no epsilon can hide a proper crossing,
    // collinear overlap, or a nonadjacent vertex touching an edge.
    fn intersects(a: Coord, b: Coord, c: Coord, d: Coord) -> bool {
        let orient = |a, b, c| cross(sub(b, a), sub(c, a));
        let between = |p: Coord, a: Coord, b: Coord| {
            p.lng >= a.lng.min(b.lng)
                && p.lng <= a.lng.max(b.lng)
                && p.lat >= a.lat.min(b.lat)
                && p.lat <= a.lat.max(b.lat)
        };
        let (u, v, w, z) = (
            orient(a, b, c),
            orient(a, b, d),
            orient(c, d, a),
            orient(c, d, b),
        );
        ((u > 0.0 && v < 0.0 || u < 0.0 && v > 0.0) && (w > 0.0 && z < 0.0 || w < 0.0 && z > 0.0))
            || u == 0.0 && between(c, a, b)
            || v == 0.0 && between(d, a, b)
            || w == 0.0 && between(a, c, d)
            || z == 0.0 && between(b, c, d)
    }

    fn assert_simple(ring: &[Coord]) {
        assert!(is_closed(ring), "open or empty ring: {ring:?}");
        assert_eq!(ring[0], *ring.last().unwrap(), "closure must be exact");
        let n = ring.len() - 1;
        for i in 0..n {
            assert_ne!(ring[i], ring[i + 1], "zero length edge");
            for j in i + 2..n {
                if i == 0 && j == n - 1 {
                    continue;
                }
                assert!(
                    !intersects(ring[i], ring[i + 1], ring[j], ring[j + 1]),
                    "crossing {i}/{j}"
                );
            }
        }
        assert!(area2(ring) > 0.0, "exterior must be CCW");
    }

    // Independent analytic Minkowski oracle: at each x, union the vertical
    // intervals from exact endpoint discs and segment rectangles. Midpoint
    // integration (12000 slices) measures area without using the buffer's joins
    // or its dissolution algorithm, including nonlocal overlaps at large radii.
    fn analytic_area(pts: &[Coord], r: f64, polygon: bool) -> f64 {
        let (xmin, _, xmax, _) = bbox(pts);
        let dx = (xmax - xmin + 2.0 * r) / 12000.0;
        let mut area = 0.0;
        for k in 0..12000 {
            let x = xmin - r + (k as f64 + 0.5) * dx;
            let mut intervals = Vec::<(f64, f64)>::new();
            for p in pts {
                if (x - p.lng).abs() < r {
                    let h = (r * r - (x - p.lng).powi(2)).sqrt();
                    intervals.push((p.lat - h, p.lat + h));
                }
            }
            let edges = if polygon { pts.len() } else { pts.len() - 1 };
            for i in 0..edges {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                let d = sub(b, a);
                let len = d.lng.hypot(d.lat);
                let offset = Coord::new(-r * d.lat / len, r * d.lng / len);
                let rect = [
                    Coord::new(a.lng + offset.lng, a.lat + offset.lat),
                    Coord::new(b.lng + offset.lng, b.lat + offset.lat),
                    Coord::new(b.lng - offset.lng, b.lat - offset.lat),
                    Coord::new(a.lng - offset.lng, a.lat - offset.lat),
                ];
                let ys = vertical_slice(&rect, x);
                if ys.len() >= 2 {
                    intervals.push((ys[0], *ys.last().unwrap()));
                }
            }
            if polygon {
                let ys = vertical_slice(pts, x);
                for pair in ys.chunks_exact(2) {
                    intervals.push((pair[0], pair[1]));
                }
            }
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut end = f64::NEG_INFINITY;
            for (lo, hi) in intervals {
                if hi > end {
                    area += (hi - lo.max(end)) * dx;
                    end = hi;
                }
            }
        }
        area
    }

    fn vertical_slice(ring: &[Coord], x: f64) -> Vec<f64> {
        let mut ys = Vec::new();
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if (a.lng > x) != (b.lng > x) {
                ys.push(a.lat + (x - a.lng) * (b.lat - a.lat) / (b.lng - a.lng));
            }
        }
        ys.sort_by(f64::total_cmp);
        ys
    }

    fn prove_buffer(xy: &[(f64, f64)], polygon: bool) {
        let scale = crate::types::EARTH_RADIUS_M * std::f64::consts::PI / 180.0;
        let planar: Vec<_> = xy.iter().map(|&(x, y)| Coord::new(x, y)).collect();
        let pts: Vec<_> = xy
            .iter()
            .map(|&(x, y)| Coord::new(30.0 + x / scale, y / scale))
            .collect();
        for r in [0.5, 2.0, 20.0, 100.0] {
            let ring = if polygon {
                buffer_polygon(&pts, r, 32)
            } else {
                buffer_line(&pts, r, 32)
            };
            assert_simple(&ring);
            assert_eq!(
                ring,
                if polygon {
                    buffer_polygon(&pts, r, 32)
                } else {
                    buffer_line(&pts, r, 32)
                },
                "deterministic output"
            );
            let meters: Vec<_> = ring
                .iter()
                .map(|p| Coord::new((p.lng - 30.0) * scale, p.lat * scale))
                .collect();
            let actual = area2(&meters) * 0.5;
            let expected = analytic_area(&planar, r, polygon);
            // 0.15% covers 32-quarter-segment chord error (<0.05%), numerical
            // integration and spherical/planar differences at these <250m scales.
            assert!(
                (actual - expected).abs() / expected < 0.0015,
                "r={r}: area {actual}, analytic {expected}"
            );
            let (a, b, c, d) = bbox(&meters);
            let (e, f, g, h) = bbox(&planar);
            // Arc tessellation misses extrema by at most r*(1-cos(pi/128)).
            let tol = 0.001 * r + 0.0001;
            for (got, want) in [(a, e - r), (b, f - r), (c, g + r), (d, h + r)] {
                assert!(
                    (got - want).abs() < tol,
                    "r={r}: extent {got}, analytic {want}"
                );
            }
            if polygon {
                let mut reversed = pts.clone();
                reversed.reverse();
                let other = buffer_polygon(&reversed, r, 32);
                assert_simple(&other);
                assert!((area2(&other) / area2(&ring) - 1.0).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn obtuse_bend_has_simple_minkowski_boundary() {
        prove_buffer(&[(0., 0.), (50., 0.), (93.301270189, 25.)], false);
    }
    #[test]
    fn right_bend_has_simple_minkowski_boundary() {
        prove_buffer(&[(0., 0.), (50., 0.), (50., 50.)], false);
    }
    #[test]
    fn acute_bend_has_simple_minkowski_boundary() {
        prove_buffer(&[(0., 0.), (50., 0.), (25., 43.301270189)], false);
    }
    #[test]
    fn hairpin_has_simple_minkowski_boundary() {
        prove_buffer(&[(0., 0.), (50., 0.), (0.759612349, 8.682408883)], false);
        // Complete reversal exercises coincident capsule edges as well.
        prove_buffer(&[(0., 0.), (50., 0.), (0., 0.)], false);
    }
    #[test]
    fn short_legs_have_simple_minkowski_boundary() {
        prove_buffer(&[(0., 0.), (50., 0.), (50., 1.), (100., 1.)], false);
    }
    #[test]
    fn s_bend_has_simple_minkowski_boundary() {
        prove_buffer(&[(0., 0.), (20., 0.), (20., 20.), (40., 20.)], false);
    }
    #[test]
    fn concave_polygon_has_simple_minkowski_boundary() {
        prove_buffer(
            &[
                (0., 0.),
                (50., 0.),
                (50., 20.),
                (20., 20.),
                (20., 50.),
                (0., 50.),
            ],
            true,
        );
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

    // A whole route, not one bend: 200 alternating 25 m legs at a 2 m radius.
    // The sweep visits only nearby capsules; every bend must still dissolve.
    #[test]
    fn a_long_zigzag_route_has_a_simple_boundary() {
        let mut route = vec![Coord::new(29.8469, -2.6256)];
        for leg in 0..200 {
            let bearing = if leg % 2 == 0 { 60.0 } else { 120.0 };
            route.push(destination(*route.last().unwrap(), bearing, 25.0));
        }
        let ring = buffer_line(&route, 2.0, 8);
        assert_simple(&ring);
        let (_, _, max_lng, _) = bbox(&ring);
        let (_, _, route_max_lng, _) = bbox(&route);
        assert!(
            max_lng > route_max_lng,
            "the end cap extends past the route"
        );
    }

    // Gisagara M3 tension section (three spans on one straight line, as exported
    // by `ds dsgrid-exchange convert`): its 2 m buffer came back empty because the
    // joint offsets built by the two neighbouring capsules differed by roundoff.
    #[test]
    fn a_straight_multi_span_section_keeps_its_buffer() {
        let section = [
            Coord::new(29.814310244341684, -2.609957170764343),
            Coord::new(29.814550356843622, -2.6100127785853275),
            Coord::new(29.81487938110083, -2.610088977438698),
            Coord::new(29.815184275010516, -2.6101595878296404),
        ];
        assert_simple(&buffer_line(&section, 2.0, 8));
    }

    // Collinear spans of uneven length carry roundoff micro-turns at every joint.
    // Every radius the vector command admits must still trace a closed ring.
    #[test]
    fn straight_spans_keep_their_buffer_at_every_radius() {
        let mut line = vec![Coord::new(29.8256716, -2.6722028)];
        for length in [60.63, 64.95, 59.95, 54.96, 54.96, 64.95, 59.95, 64.79] {
            line.push(destination(*line.last().unwrap(), 62.281, length));
        }
        for r in [0.01, 0.5, 2.0, 25.0, 1_000.0] {
            let ring = buffer_line(&line, r, 8);
            assert!(!ring.is_empty(), "radius {r} m produced no ring");
            assert_simple(&ring);
        }
    }
}
