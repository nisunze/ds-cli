//! Shared Cartesian predicates with explicit boundary and ray arithmetic modes.
use crate::cartesian::{Point, orientation};

#[derive(Clone, Copy, Debug)]
pub enum RingEdges {
    Previous,
    Next,
    Consecutive,
}
#[derive(Clone, Copy, Debug)]
pub enum RayArithmetic {
    ProductFirst,
    ParameterFirst,
    OffsetProduct,
}

/// Half-open even/odd containment. Boundary classification is a separate query.
/// Modes preserve source arithmetic and whether an open ring is closed implicitly.
pub fn ring_contains_by<T>(
    ring: &[T],
    point: Point,
    position: impl Fn(&T) -> Point,
    edges: RingEdges,
    arithmetic: RayArithmetic,
) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let count = match edges {
        RingEdges::Consecutive => ring.len() - 1,
        _ => ring.len(),
    };
    let mut inside = false;
    for i in 0..count {
        let j = match edges {
            RingEdges::Previous => {
                if i == 0 {
                    ring.len() - 1
                } else {
                    i - 1
                }
            }
            RingEdges::Next => (i + 1) % ring.len(),
            RingEdges::Consecutive => i + 1,
        };
        let a = position(&ring[i]);
        let b = position(&ring[j]);
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let x = match arithmetic {
                RayArithmetic::ProductFirst => {
                    (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
                }
                RayArithmetic::ParameterFirst => {
                    a[0] + ((point[1] - a[1]) / (b[1] - a[1])) * (b[0] - a[0])
                }
                RayArithmetic::OffsetProduct => {
                    a[0] + (point[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
                }
            };
            if point[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

#[derive(Clone, Copy, Debug)]
pub struct SegmentTolerance {
    /// Absolute determinant tolerance, in squared coordinate units.
    pub determinant: f64,
    /// Extra determinant tolerance = distance * max(segment length, minimum).
    pub distance: f64,
    pub minimum_length: f64,
    /// Coordinate bounding-box slack, in coordinate units.
    pub bounds: f64,
}

impl SegmentTolerance {
    pub const EXACT: Self = Self {
        determinant: 0.0,
        distance: 0.0,
        minimum_length: 0.0,
        bounds: 0.0,
    };
    pub const REGION: Self = Self {
        determinant: crate::tolerance::REGION_DETERMINANT,
        distance: 0.0,
        minimum_length: 0.0,
        bounds: crate::tolerance::REGION_BOUNDS,
    };
    pub const GRID_MODEL: Self = Self {
        determinant: 0.0,
        distance: 1e-10,
        minimum_length: 1.0,
        bounds: 1e-10,
    };
}

pub fn point_on_segment(point: Point, a: Point, b: Point, tolerance: SegmentTolerance) -> bool {
    let determinant = if tolerance.distance == 0.0 {
        tolerance.determinant
    } else {
        tolerance.determinant
            + tolerance.distance
                * (b[0] - a[0])
                    .hypot(b[1] - a[1])
                    .max(tolerance.minimum_length)
    };
    if orientation(a, b, point).abs() > determinant {
        return false;
    }
    point[0] >= a[0].min(b[0]) - tolerance.bounds
        && point[0] <= a[0].max(b[0]) + tolerance.bounds
        && point[1] >= a[1].min(b[1]) - tolerance.bounds
        && point[1] <= a[1].max(b[1]) + tolerance.bounds
}

pub fn orientation_sign(a: Point, b: Point, c: Point, epsilon: f64) -> i8 {
    let value = orientation(a, b, c);
    if value.abs() <= epsilon {
        0
    } else if value > 0.0 {
        1
    } else {
        -1
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Crossing {
    Quantized,
    RawSigns,
    RawProduct,
}

/// Boundary-inclusive intersection. Proper crossings use the separately named
/// Cartesian primitive; collinear contact belongs only to this operation.
pub fn segments_intersect(
    a: Point,
    b: Point,
    c: Point,
    d: Point,
    tolerance: SegmentTolerance,
    crossing: Crossing,
) -> bool {
    let o = [
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    ];
    let quantized = o.map(|v| {
        if v.abs() <= tolerance.determinant {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    });
    let contact = [(c, a, b), (d, a, b), (a, c, d), (b, c, d)]
        .into_iter()
        .enumerate()
        .any(|(i, (p, from, to))| {
            (!matches!(crossing, Crossing::Quantized) || quantized[i] == 0)
                && point_on_segment(p, from, to, tolerance)
        });
    contact
        || match crossing {
            Crossing::Quantized => {
                (quantized[0] > 0) != (quantized[1] > 0) && (quantized[2] > 0) != (quantized[3] > 0)
            }
            Crossing::RawSigns => (o[0] > 0.0) != (o[1] > 0.0) && (o[2] > 0.0) != (o[3] > 0.0),
            Crossing::RawProduct => o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0,
        }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointLocation {
    Outside,
    Boundary,
    Inside,
}

pub fn locate_in_ring_by<T>(
    ring: &[T],
    point: Point,
    position: impl Fn(&T) -> Point,
    tolerance: SegmentTolerance,
    edges: RingEdges,
    arithmetic: RayArithmetic,
) -> PointLocation {
    if ring.len() < 3 {
        return PointLocation::Outside;
    }
    let count = match edges {
        RingEdges::Consecutive => ring.len() - 1,
        _ => ring.len(),
    };
    for i in 0..count {
        let j = match edges {
            RingEdges::Previous => {
                if i == 0 {
                    ring.len() - 1
                } else {
                    i - 1
                }
            }
            RingEdges::Next => (i + 1) % ring.len(),
            RingEdges::Consecutive => i + 1,
        };
        if point_on_segment(point, position(&ring[i]), position(&ring[j]), tolerance) {
            return PointLocation::Boundary;
        }
    }
    if ring_contains_by(ring, point, position, edges, arithmetic) {
        PointLocation::Inside
    } else {
        PointLocation::Outside
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_open_and_boundary_classification_remain_distinct() {
        let ring = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let contains = |p| {
            ring_contains_by(
                &ring,
                p,
                |p| *p,
                RingEdges::Previous,
                RayArithmetic::ProductFirst,
            )
        };
        assert!(contains([0.0, 5.0]));
        assert!(!contains([10.0, 5.0]));
        for point in [[0.0, 5.0], [10.0, 5.0]] {
            assert_eq!(
                locate_in_ring_by(
                    &ring,
                    point,
                    |p| *p,
                    SegmentTolerance::EXACT,
                    RingEdges::Previous,
                    RayArithmetic::ProductFirst
                ),
                PointLocation::Boundary
            );
        }
    }
    #[test]
    fn contact_and_proper_crossing_are_distinct() {
        let (a, b, c, d) = ([0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [20.0, 0.0]);
        assert!(segments_intersect(
            a,
            b,
            c,
            d,
            SegmentTolerance::EXACT,
            Crossing::RawSigns
        ));
        assert!(!crate::cartesian::segments_cross_properly(a, b, c, d));
        assert!(!point_on_segment([2.0, 2.0], a, a, SegmentTolerance::EXACT));
    }
}
