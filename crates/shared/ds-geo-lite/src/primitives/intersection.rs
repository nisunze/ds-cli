use crate::types::Coord;

/// Result of a segment-segment intersection test.
#[derive(Clone, Debug)]
pub struct Intersection {
    pub point: Coord,
    /// Parameter on first segment (0 = start, 1 = end). Values outside [0,1]
    /// mean the intersection lies on the infinite line, past the segment endpoints.
    pub t: f64,
    /// Parameter on second segment.
    pub u: f64,
}

/// Test intersection of segments (a1→a2) and (b1→b2).
/// Always returns a result if the lines aren't parallel, even if the intersection
/// is outside the segments. Callers decide which range of t/u is valid.
pub fn segment_segment(a1: Coord, a2: Coord, b1: Coord, b2: Coord) -> Option<Intersection> {
    let dx_a = a2.lng - a1.lng;
    let dy_a = a2.lat - a1.lat;
    let dx_b = b2.lng - b1.lng;
    let dy_b = b2.lat - b1.lat;

    let denom = dx_a * dy_b - dy_a * dx_b;
    if denom.abs() < 1e-15 {
        return None; // parallel or coincident
    }

    let dx_ab = b1.lng - a1.lng;
    let dy_ab = b1.lat - a1.lat;

    let t = (dx_ab * dy_b - dy_ab * dx_b) / denom;
    let u = (dx_ab * dy_a - dy_ab * dx_a) / denom;

    let point = Coord::new(a1.lng + t * dx_a, a1.lat + t * dy_a);
    Some(Intersection { point, t, u })
}

/// Intersection with global position along polyline A.
#[derive(Clone, Debug)]
pub struct PolylineHit {
    pub point: Coord,
    /// Segment index on A.
    pub seg: usize,
    /// Parameter within the segment (0..1).
    pub t: f64,
    /// Global parameter: seg + t.
    pub param: f64,
}

/// Find all strict intersections (both t,u in [0,1]) between two polylines.
/// Results sorted by parameter along A.
pub fn polyline_polyline(a: &[Coord], b: &[Coord]) -> Vec<PolylineHit> {
    let mut hits = Vec::new();
    let eps = 1e-10;

    for i in 0..a.len().saturating_sub(1) {
        for j in 0..b.len().saturating_sub(1) {
            if let Some(ix) = segment_segment(a[i], a[i + 1], b[j], b[j + 1])
                && ix.t >= -eps
                && ix.t <= 1.0 + eps
                && ix.u >= -eps
                && ix.u <= 1.0 + eps
            {
                let t_c = ix.t.clamp(0.0, 1.0);
                hits.push(PolylineHit {
                    point: ix.point,
                    seg: i,
                    t: t_c,
                    param: i as f64 + t_c,
                });
            }
        }
    }

    hits.sort_by(|a, b| a.param.total_cmp(&b.param));
    hits
}

/// Squared distance between two coords (degree-space — for comparison only).
pub fn dist_sq(a: Coord, b: Coord) -> f64 {
    let dx = a.lng - b.lng;
    let dy = a.lat - b.lat;
    dx * dx + dy * dy
}

/// Project a point onto a polyline. Returns the global parameter (seg_index + t).
pub fn project_onto_polyline(point: Coord, poly: &[Coord]) -> f64 {
    let mut best_param = 0.0;
    let mut best_dist = f64::MAX;

    for i in 0..poly.len().saturating_sub(1) {
        let (a, b) = (poly[i], poly[i + 1]);
        let dx = b.lng - a.lng;
        let dy = b.lat - a.lat;
        let len_sq = dx * dx + dy * dy;

        let t = if len_sq < 1e-30 {
            0.0
        } else {
            ((point.lng - a.lng) * dx + (point.lat - a.lat) * dy) / len_sq
        };
        let t_c = t.clamp(0.0, 1.0);
        let proj = Coord::new(a.lng + t_c * dx, a.lat + t_c * dy);
        let d = dist_sq(point, proj);

        if d < best_dist {
            best_dist = d;
            best_param = i as f64 + t_c;
        }
    }

    best_param
}
