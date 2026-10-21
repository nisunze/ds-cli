//! Profile lasso semantics, shared by native and WASM hosts.
use crate::Point;
pub fn rectangle(a: Point, b: Point) -> [Point; 4] {
    [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]
}
pub fn point_in_polygon(polygon: &[Point], point: Point) -> bool {
    ds_geo_lite::predicates::ring_contains_by(
        polygon,
        [point.x, point.y],
        |p| [p.x, p.y],
        ds_geo_lite::predicates::RingEdges::Previous,
        ds_geo_lite::predicates::RayArithmetic::ProductFirst,
    )
}
pub fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    ds_geo_lite::predicates::segments_intersect(
        [a.x, a.y],
        [b.x, b.y],
        [c.x, c.y],
        [d.x, d.y],
        ds_geo_lite::predicates::SegmentTolerance::EXACT,
        ds_geo_lite::predicates::Crossing::RawSigns,
    )
}
pub fn polyline_intersects_polygon(polygon: &[Point], points: &[Point]) -> bool {
    if points.iter().any(|&p| point_in_polygon(polygon, p)) {
        return true;
    }
    points.windows(2).any(|s| {
        polygon
            .iter()
            .enumerate()
            .any(|(i, &p)| segments_intersect(s[0], s[1], p, polygon[(i + 1) % polygon.len()]))
    })
}
pub fn distance_to_segment(point: Point, from: Point, to: Point) -> f64 {
    ds_geo_lite::cartesian::point_segment_distance_with(
        [point.x, point.y],
        [from.x, from.y],
        [to.x, to.y],
        ds_geo_lite::cartesian::DistanceArithmetic::Hypot,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crossing_and_collinear_touch_preserve_profile_lasso_semantics() {
        let box_ = rectangle(Point::new(0., 0.), Point::new(10., 10.));
        assert!(polyline_intersects_polygon(
            &box_,
            &[Point::new(-5., 5.), Point::new(15., 5.)]
        ));
        assert!(polyline_intersects_polygon(
            &box_,
            &[Point::new(-5., 0.), Point::new(0., 0.)]
        ));
        assert!(!polyline_intersects_polygon(
            &box_,
            &[Point::new(-5., -1.), Point::new(15., -1.)]
        ));
        assert!(!point_in_polygon(&[], Point::new(0., 0.)));
    }
    #[test]
    fn concave_polygon_does_not_select_its_bounding_box() {
        let p = [
            Point::new(0., 0.),
            Point::new(10., 0.),
            Point::new(2., 2.),
            Point::new(0., 10.),
        ];
        assert!(!point_in_polygon(&p, Point::new(6., 6.)));
        assert!(point_in_polygon(&p, Point::new(1., 1.)));
    }
    #[test]
    fn hit_distance_clamps_to_endpoints_and_handles_zero_length() {
        let a = Point::new(0., 0.);
        let b = Point::new(10., 0.);
        assert_eq!(distance_to_segment(Point::new(5., 3.), a, b), 3.);
        assert_eq!(distance_to_segment(Point::new(14., 3.), a, b), 5.);
        assert_eq!(distance_to_segment(Point::new(4., 3.), a, a), 5.);
    }
}
