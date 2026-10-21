use crate::primitives::intersection::{dist_sq, segment_segment};
use crate::types::Coord;

/// Extend a line to meet a target polyline (AutoCAD EXTEND).
///
/// Tries both endpoints. Extends whichever produces the shorter extension.
/// The endpoint moves along the line's existing direction until it hits the
/// target — or the target's extrapolation if the finite segments don't reach.
pub fn extend_to(line: &[Coord], target: &[Coord]) -> Option<Vec<Coord>> {
    if line.len() < 2 || target.len() < 2 {
        return None;
    }

    let n = line.len();

    // Try extending END: ray from line[n-2] → line[n-1] beyond line[n-1]
    let end_hit = ray_hit(line[n - 2], line[n - 1], target);

    // Try extending START: ray from line[1] → line[0] beyond line[0]
    let start_hit = ray_hit(line[1], line[0], target);

    match (start_hit, end_hit) {
        (Some((sp, sd)), Some((ep, ed))) => {
            if sd <= ed {
                Some(build_start_extend(line, sp))
            } else {
                Some(build_end_extend(line, ep))
            }
        }
        (Some((sp, _)), None) => Some(build_start_extend(line, sp)),
        (None, Some((ep, _))) => Some(build_end_extend(line, ep)),
        (None, None) => None,
    }
}

fn build_start_extend(line: &[Coord], hit: Coord) -> Vec<Coord> {
    let mut result = vec![hit];
    result.extend_from_slice(&line[1..]);
    result
}

fn build_end_extend(line: &[Coord], hit: Coord) -> Vec<Coord> {
    let mut result = line[..line.len() - 1].to_vec();
    result.push(hit);
    result
}

/// Cast a ray from `from` through `through` (and beyond) and find the closest
/// hit with any target segment's *infinite line*. This extrapolates the target
/// so non-touching lines still meet at their projected intersection.
///
/// Two passes:
///  1. Strict — hit must land on the finite target segment (u in [0,1]).
///  2. Relaxed — hit can land on the infinite line through any target segment.
///
/// Strict wins when available; relaxed is the extrapolation fallback.
fn ray_hit(from: Coord, through: Coord, target: &[Coord]) -> Option<(Coord, f64)> {
    let dir_lng = through.lng - from.lng;
    let dir_lat = through.lat - from.lat;

    // Far point along the ray (100° — covers any local geometry)
    let far = Coord::new(through.lng + dir_lng * 100.0, through.lat + dir_lat * 100.0);

    let mut strict: Option<(Coord, f64)> = None;
    let mut relaxed: Option<(Coord, f64)> = None;

    for seg in target.windows(2) {
        if let Some(ix) = segment_segment(through, far, seg[0], seg[1]) {
            // t >= 0  →  intersection is past `through` (in the extension direction)
            if ix.t < -1e-10 {
                continue;
            }

            let d = dist_sq(through, ix.point);

            // Strict: hit lands on the finite target segment
            if (-1e-10..=1.0 + 1e-10).contains(&ix.u)
                && strict.as_ref().is_none_or(|(_, best)| d < *best)
            {
                strict = Some((ix.point, d));
            }

            // Relaxed: hit lands on the infinite line through this segment
            // (extrapolated target)
            if relaxed.as_ref().is_none_or(|(_, best)| d < *best) {
                relaxed = Some((ix.point, d));
            }
        }
    }

    // Prefer strict (on-segment) hit, fall back to extrapolated
    strict.or(relaxed)
}
