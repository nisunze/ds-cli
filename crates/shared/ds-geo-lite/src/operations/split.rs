use crate::primitives::interpolation::lerp;
use crate::primitives::intersection::{polyline_polyline, project_onto_polyline};
use crate::types::Coord;

/// Split a line at the point nearest to `at`.
///
/// Returns two coordinate sequences (before split, after split),
/// or None if the line can't be split there.
pub fn split_at_point(line: &[Coord], at: Coord) -> Option<(Vec<Coord>, Vec<Coord>)> {
    if line.len() < 2 {
        return None;
    }

    let param = project_onto_polyline(at, line);
    split_at_param(line, param)
}

/// Split a line at every intersection with a cutting geometry.
///
/// Returns N+1 segments (N = number of intersections), sorted along the line.
/// Returns None if no intersections.
pub fn split_with_line(line: &[Coord], cutter: &[Coord]) -> Option<Vec<Vec<Coord>>> {
    if line.len() < 2 || cutter.len() < 2 {
        return None;
    }

    let hits = polyline_polyline(line, cutter);
    if hits.is_empty() {
        return None;
    }

    // Collect unique split parameters (deduplicate near-coincident hits)
    let mut params: Vec<f64> = Vec::with_capacity(hits.len());
    for h in &hits {
        if params
            .last()
            .is_none_or(|&prev| (h.param - prev).abs() > 1e-10)
        {
            params.push(h.param);
        }
    }

    let mut segments: Vec<Vec<Coord>> = Vec::with_capacity(params.len() + 1);
    let mut prev_param = 0.0_f64;

    for &p in &params {
        let seg_idx = p.floor() as usize;
        let t = p - seg_idx as f64;
        let seg_idx = seg_idx.min(line.len() - 2);
        let split_pt = lerp(line[seg_idx], line[seg_idx + 1], t);

        // Build segment from the previous split point up to split_pt
        let from_seg = prev_param.floor() as usize;
        let from_t = prev_param - from_seg as f64;

        let mut piece: Vec<Coord> = Vec::new();

        if prev_param == 0.0 {
            // First piece starts at line[0]
            piece.extend_from_slice(&line[..=seg_idx]);
        } else {
            // Intermediate piece starts at previous split point
            let from_seg_c = from_seg.min(line.len() - 2);
            let prev_pt = lerp(line[from_seg_c], line[from_seg_c + 1], from_t);
            piece.push(prev_pt);
            let start_vtx = if from_t > 1.0 - 1e-10 {
                from_seg_c + 2
            } else {
                from_seg_c + 1
            };
            if start_vtx <= seg_idx && start_vtx < line.len() {
                piece.extend_from_slice(&line[start_vtx..=seg_idx]);
            }
        }

        if t > 1e-10 {
            piece.push(split_pt);
        }

        if piece.len() >= 2 {
            segments.push(piece);
        }

        prev_param = p;
    }

    // Final piece: from last split to end of line
    let last_seg = prev_param.floor() as usize;
    let last_t = prev_param - last_seg as f64;
    let last_seg = last_seg.min(line.len() - 2);
    let last_pt = lerp(line[last_seg], line[last_seg + 1], last_t);

    let mut tail: Vec<Coord> = vec![last_pt];
    let start_vtx = if last_t < 1.0 - 1e-10 {
        last_seg + 1
    } else {
        last_seg + 2
    };
    if start_vtx < line.len() {
        tail.extend_from_slice(&line[start_vtx..]);
    }
    if tail.len() >= 2 {
        segments.push(tail);
    }

    if segments.len() >= 2 {
        Some(segments)
    } else {
        None
    }
}

/// Internal: split at a specific parameter along the polyline.
fn split_at_param(line: &[Coord], param: f64) -> Option<(Vec<Coord>, Vec<Coord>)> {
    let seg = param.floor() as usize;
    let t = param - seg as f64;
    let seg = seg.min(line.len() - 2);

    let split_pt = lerp(line[seg], line[seg + 1], t);

    let mut first: Vec<Coord> = line[..=seg].to_vec();
    if t > 1e-10 {
        first.push(split_pt);
    }

    let mut second: Vec<Coord> = vec![split_pt];
    if t < 1.0 - 1e-10 {
        second.extend_from_slice(&line[seg + 1..]);
    } else {
        let next = seg + 2;
        if next < line.len() {
            second.extend_from_slice(&line[next..]);
        }
    }

    if first.len() >= 2 && second.len() >= 2 {
        Some((first, second))
    } else {
        None
    }
}
