//! Profile lasso semantics, shared by native and WASM hosts.
use crate::Point;
pub fn rectangle(a: Point, b: Point) -> [Point; 4] {
    [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]
}
pub fn point_in_polygon(polygon: &[Point], point: Point) -> bool {
    if polygon.is_empty() {
        return false;
    }
    let mut inside = false;
    let mut b = polygon[polygon.len() - 1];
    for &a in polygon {
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
        b = a;
    }
    inside
}
pub fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let cross =
        |p: Point, q: Point, r: Point| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let on = |p: Point, q: Point, r: Point| {
        p.x.min(r.x) <= q.x && q.x <= p.x.max(r.x) && p.y.min(r.y) <= q.y && q.y <= p.y.max(r.y)
    };
    let (abc, abd, cda, cdb) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    (abc == 0. && on(a, c, b))
        || (abd == 0. && on(a, d, b))
        || (cda == 0. && on(c, a, d))
        || (cdb == 0. && on(c, b, d))
        || ((abc > 0.) != (abd > 0.) && (cda > 0.) != (cdb > 0.))
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
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length2 = dx * dx + dy * dy;
    if length2 == 0. {
        return point.distance_to(from);
    }
    let t = (((point.x - from.x) * dx + (point.y - from.y) * dy) / length2).clamp(0., 1.);
    point.distance_to(Point::new(from.x + t * dx, from.y + t * dy))
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
