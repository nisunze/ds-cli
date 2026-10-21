use crate::primitives::intersection::{polyline_polyline, project_onto_polyline};
use crate::types::Coord;

/// Trim a line at its intersection with a cutting edge (AutoCAD TRIM).
///
/// `keep_point` is where the user right-clicked on the source line — the
/// portion containing this point is kept, the other side is trimmed away.
pub fn trim_at(line: &[Coord], cutter: &[Coord], keep_point: Coord) -> Option<Vec<Coord>> {
    if line.len() < 2 || cutter.len() < 2 {
        return None;
    }

    let hits = polyline_polyline(line, cutter);
    if hits.is_empty() {
        return None;
    }

    let keep_param = project_onto_polyline(keep_point, line);

    // Find the nearest intersection to the keep point (by parameter along line)
    let nearest = hits.iter().min_by(|a, b| {
        (a.param - keep_param)
            .abs()
            .total_cmp(&(b.param - keep_param).abs())
    })?;

    let split_seg = nearest.seg;
    let split_t = nearest.t;
    let split_pt = nearest.point;

    if keep_param <= nearest.param {
        // Keep START → split_point
        let mut result: Vec<Coord> = line[..=split_seg].to_vec();
        if split_t > 1e-10 {
            result.push(split_pt);
        }
        if result.len() >= 2 {
            Some(result)
        } else {
            None
        }
    } else {
        // Keep split_point → END
        let mut result = vec![split_pt];
        if split_t < 1.0 - 1e-10 {
            result.extend_from_slice(&line[split_seg + 1..]);
        } else {
            // Split at very end of segment — start from next vertex
            let next = split_seg + 1;
            if next < line.len() {
                result.extend_from_slice(&line[next..]);
            }
        }
        if result.len() >= 2 {
            Some(result)
        } else {
            None
        }
    }
}
