//! Projected-metre vertex deduplication shared by the design session and the
//! Network engine, so the edit surface and the network builders agree on
//! what "same vertex" means. Neutral geometry: no session state, no domain.

use crate::projection::{ProjectionMetadata, projected_distance};

/// Foundation vertex-dedupe tolerance, in projected metres. Any two
/// vertices closer than this on the projected plane are treated as the
/// same node by session sanitization, geometry editing, and downstream
/// domain processing (network builders re-export this under their own
/// names — see `ds_network::network::NODE_DEDUPE_TOLERANCE_M`).
pub const VERTEX_DEDUPE_TOLERANCE_M: f64 = 0.5;

/// Collapse consecutive vertices within `tolerance_m` (projected metric).
/// Preserves first + last vertex of an open line so the line's connection
/// points survive a snap-onto-neighbor mutation.
pub fn dedupe_line_coords(
    coords: &mut Vec<[f64; 2]>,
    projection: ProjectionMetadata,
    tolerance_m: f64,
) {
    if !tolerance_m.is_finite() || tolerance_m <= 0.0 || coords.len() < 3 {
        // < 3: open line needs both endpoints; nothing in the middle to drop.
        return;
    }
    // Interior pass — never visits the last index, so the endpoint survives.
    let mut i = 1;
    while i + 1 < coords.len() {
        let prev = projection.project_coord(coords[i - 1]);
        let curr = projection.project_coord(coords[i]);
        if projected_distance(prev, curr) <= tolerance_m {
            coords.remove(i);
        } else {
            i += 1;
        }
    }
    // Trailing-edge pass — drop the penultimate, not the endpoint.
    while coords.len() > 2 {
        let n = coords.len();
        let pen = projection.project_coord(coords[n - 2]);
        let last = projection.project_coord(coords[n - 1]);
        if projected_distance(pen, last) > tolerance_m {
            break;
        }
        coords.remove(n - 2);
    }
}

/// Collapse consecutive vertices within `tolerance_m` on a closed ring.
/// Ring is `[v0, v1, …, vN, v0]` (first == last). Preserves closure and at
/// least 3 unique vertices + the closure copy (4 total).
pub fn dedupe_ring_coords(
    coords: &mut Vec<[f64; 2]>,
    projection: ProjectionMetadata,
    tolerance_m: f64,
) {
    if !tolerance_m.is_finite() || tolerance_m <= 0.0 || coords.len() < 5 {
        return;
    }
    let closed = coords.first() == coords.last();
    if closed {
        coords.pop();
    }
    let min_open = if closed { 3 } else { 2 };
    let mut i = 1;
    while i < coords.len() && coords.len() > min_open {
        let prev = projection.project_coord(coords[i - 1]);
        let curr = projection.project_coord(coords[i]);
        if projected_distance(prev, curr) <= tolerance_m {
            coords.remove(i);
        } else {
            i += 1;
        }
    }
    if closed {
        // Wrap-around: if new-last is within tol of new-first, drop new-last.
        while coords.len() > min_open {
            let first = projection.project_coord(coords[0]);
            let last = projection.project_coord(*coords.last().unwrap());
            if projected_distance(first, last) > tolerance_m {
                break;
            }
            coords.pop();
        }
        let close = coords[0];
        coords.push(close);
    }
}

#[cfg(test)]
#[path = "vertex_dedupe_tests.rs"]
mod tests;
