use crate::primitives::intersection::polyline_polyline;
use crate::types::Coord;

/// Find all intersection points between two polylines.
pub fn find_all(a: &[Coord], b: &[Coord]) -> Vec<Coord> {
    polyline_polyline(a, b)
        .into_iter()
        .map(|h| h.point)
        .collect()
}
