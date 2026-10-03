//! One typed gateway to the polygon engine, independent of coordinate selection.
use crate::region::RegionPredicate;
pub use geo::algorithm::unary_union;
use geo::{Area, BoundingRect, Intersects, Line, Point, Relate, Validation};
pub use geo::{Coord, LineString, Polygon, Rect};

pub enum PolygonError {
    DistinctVertices,
    Empty,
    ExtentExceeded,
    Topology(String),
    ZeroArea,
}

pub struct PlanarPolygon {
    polygon: Polygon<f64>,
    pub bounds: Rect<f64>,
}
impl PlanarPolygon {
    /// Geometry-only validation. Cardinality, coordinate and identity limits are
    /// caller admission; the explicit extent limit uses the caller's units.
    pub fn new(mut vertices: Vec<[f64; 2]>, extent_limit: f64) -> Result<Self, PolygonError> {
        if vertices.first() == vertices.last() {
            vertices.pop();
        }
        if vertices.len() < 3
            || (0..vertices.len()).any(|i| vertices[i] == vertices[(i + 1) % vertices.len()])
        {
            return Err(PolygonError::DistinctVertices);
        }
        let polygon = Polygon::new(
            vertices
                .into_iter()
                .map(|[x, y]| (x, y))
                .collect::<Vec<_>>()
                .into(),
            vec![],
        );
        let bounds = polygon.bounding_rect().ok_or(PolygonError::Empty)?;
        if bounds.width() > extent_limit || bounds.height() > extent_limit {
            return Err(PolygonError::ExtentExceeded);
        }
        polygon
            .check_validation()
            .map_err(|e| PolygonError::Topology(e.to_string()))?;
        if polygon.unsigned_area() <= 0.0 {
            return Err(PolygonError::ZeroArea);
        }
        Ok(Self { polygon, bounds })
    }
    pub fn selects_segment(&self, a: [f64; 2], b: [f64; 2], mode: RegionPredicate) -> bool {
        if a == b {
            return self.polygon.intersects(&Point::new(a[0], a[1]));
        }
        let line = Line::new((a[0], a[1]), (b[0], b[1]));
        match mode {
            RegionPredicate::Intersects => self.polygon.intersects(&line),
            RegionPredicate::Within => self.polygon.relate(&line).is_covers(),
        }
    }
}
pub fn ring_anchor_cmp(left: &LineString<f64>, right: &LineString<f64>) -> std::cmp::Ordering {
    let anchor = |ring: &LineString<f64>| {
        ring.0
            .iter()
            .min_by(|left, right| {
                left.x
                    .total_cmp(&right.x)
                    .then_with(|| left.y.total_cmp(&right.y))
            })
            .map(|coord| (coord.x, coord.y))
    };
    match (anchor(left), anchor(right)) {
        (Some(left), Some(right)) => left
            .0
            .total_cmp(&right.0)
            .then_with(|| left.1.total_cmp(&right.1)),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub fn canonical_ring_xy(ring: &LineString<f64>) -> Vec<[f64; 2]> {
    let mut coordinates = ring.0.clone();
    if coordinates.len() > 1 && coordinates.first() == coordinates.last() {
        coordinates.pop();
    }
    if coordinates.is_empty() {
        return Vec::new();
    }
    let start = coordinates
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            left.x
                .total_cmp(&right.x)
                .then_with(|| left.y.total_cmp(&right.y))
        })
        .map(|(index, _)| index)
        .unwrap_or(0);
    coordinates.rotate_left(start);
    coordinates.push(coordinates[0]);
    coordinates
        .into_iter()
        .map(|coord| [coord.x, coord.y])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_covers_checks_the_full_segment_and_boundary_overlap() {
        let polygon = PlanarPolygon::new(
            vec![
                [0., 0.],
                [4., 0.],
                [4., 4.],
                [3., 4.],
                [3., 1.],
                [1., 1.],
                [1., 4.],
                [0., 4.],
            ],
            10.,
        )
        .ok()
        .unwrap();
        assert!(polygon.selects_segment([0., 0.], [4., 0.], RegionPredicate::Within));
        assert!(polygon.selects_segment([0.5, 3.], [3.5, 3.], RegionPredicate::Intersects));
        assert!(!polygon.selects_segment([0.5, 3.], [3.5, 3.], RegionPredicate::Within));
        assert!(polygon.selects_segment([0., 0.], [0., 0.], RegionPredicate::Within));
    }
}
