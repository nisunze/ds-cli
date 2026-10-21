//! The one curve representation every route uses.
//!
//! A route is a start point plus an ordered list of segments, each of which is
//! a line, a quadratic, or a cubic with its control points stated explicitly.
//! An orthogonal elbow is not a different kind of thing from a bezier — it is a
//! run of `Line` segments — so a renderer walks exactly one structure and a
//! future ellipse, parabola or spline extends this enum instead of adding a
//! second geometry path beside it.
//!
//! Control points are geometry, not style. They are computed here so no host
//! ever picks a bezier handle: a handle chosen in TypeScript is a position, and
//! positions are not TypeScript's to choose.

use ds_geo_lite::cartesian::{self, DistanceArithmetic};
use serde::{Deserialize, Serialize};

/// A finite world-space point. World coordinates are canonical; screen pixels
/// are a projection of these and never the other way round.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub fn distance_to(self, other: Self) -> f64 {
        cartesian::distance_with(
            [self.x, self.y],
            [other.x, other.y],
            DistanceArithmetic::Hypot,
        )
    }

    pub(crate) fn lerp(self, other: Self, t: f64) -> Self {
        let [x, y] = cartesian::interpolate([self.x, self.y], [other.x, other.y], t);
        Self::new(x, y)
    }
}

/// Wire op codes for [`CurveSegment`]. They are also the packed-buffer opcodes,
/// so the numeric contract is stated once.
pub const CURVE_OP_LINE: u8 = 1;
pub const CURVE_OP_QUADRATIC: u8 = 2;
pub const CURVE_OP_CUBIC: u8 = 3;

/// One drawing step. The variants map 1:1 onto `lineTo`, `quadraticCurveTo`
/// and `bezierCurveTo`, which is why a renderer needs no geometry knowledge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op")]
pub enum CurveSegment {
    #[serde(rename = "line")]
    Line { to: Point },
    #[serde(rename = "quadratic")]
    Quadratic { control: Point, to: Point },
    #[serde(rename = "cubic")]
    Cubic {
        control_a: Point,
        control_b: Point,
        to: Point,
    },
}

impl CurveSegment {
    pub fn op_code(&self) -> u8 {
        match self {
            Self::Line { .. } => CURVE_OP_LINE,
            Self::Quadratic { .. } => CURVE_OP_QUADRATIC,
            Self::Cubic { .. } => CURVE_OP_CUBIC,
        }
    }

    /// Terminal point of this segment — the start of the next one.
    pub fn end(&self) -> Point {
        match self {
            Self::Line { to } | Self::Quadratic { to, .. } | Self::Cubic { to, .. } => *to,
        }
    }

    /// Control points in packed-buffer slot order. Unused slots read as the
    /// segment's own endpoint so a consumer that ignores `op` still sees
    /// coordinates inside the curve rather than a spurious origin.
    pub fn control_slots(&self) -> [Point; 2] {
        match self {
            Self::Line { to } => [*to, *to],
            Self::Quadratic { control, to } => [*control, *to],
            Self::Cubic {
                control_a,
                control_b,
                ..
            } => [*control_a, *control_b],
        }
    }

    fn sample(&self, from: Point, t: f64) -> Point {
        let xy = |p: Point| [p.x, p.y];
        let [x, y] = match self {
            Self::Line { to } => cartesian::interpolate(xy(from), xy(*to), t),
            Self::Quadratic { control, to } => {
                cartesian::quadratic(xy(from), xy(*control), xy(*to), t)
            }
            Self::Cubic {
                control_a,
                control_b,
                to,
            } => cartesian::cubic(xy(from), xy(*control_a), xy(*control_b), xy(*to), t),
        };
        Point::new(x, y)
    }
}

/// A complete route: one start point plus its segments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Curve {
    pub start: Point,
    pub segments: Vec<CurveSegment>,
}

impl Curve {
    pub fn new(start: Point, segments: Vec<CurveSegment>) -> Self {
        Self { start, segments }
    }

    /// A curve of `Line` segments through every supplied point. This is how an
    /// orthogonal route becomes a curve without a second representation.
    pub fn polyline_curve(points: &[Point]) -> Self {
        match points.split_first() {
            None => Self::new(Point::new(0.0, 0.0), Vec::new()),
            Some((start, rest)) => Self::new(
                *start,
                rest.iter()
                    .map(|to| CurveSegment::Line { to: *to })
                    .collect(),
            ),
        }
    }

    pub fn end(&self) -> Point {
        self.segments.last().map_or(self.start, CurveSegment::end)
    }

    /// Flatten to a polyline. `steps` subdivisions per curved segment; a line
    /// contributes only its endpoint, so an orthogonal route flattens back to
    /// exactly the points it was built from — hit testing and bounds see the
    /// authored corners, not a resampled approximation of them.
    pub fn flatten(&self, steps: u32) -> Vec<Point> {
        let steps = steps.max(1);
        let mut out = Vec::with_capacity(self.segments.len() * steps as usize + 1);
        out.push(self.start);
        let mut cursor = self.start;
        for segment in &self.segments {
            match segment {
                CurveSegment::Line { to } => out.push(*to),
                _ => {
                    for step in 1..=steps {
                        let t = f64::from(step) / f64::from(steps);
                        out.push(segment.sample(cursor, t));
                    }
                }
            }
            cursor = segment.end();
        }
        out
    }
}

/// Total length of a polyline.
pub(crate) fn polyline_length(points: &[Point]) -> f64 {
    cartesian::polyline_length_by(points, |p| [p.x, p.y], DistanceArithmetic::Hypot)
}

/// The point half of the way along a polyline by arc length. Used for label
/// anchors: it is orientation-neutral and stable under any route shape, and on
/// a symmetric elbow it lands exactly on the middle of the connecting leg.
pub(crate) fn polyline_midpoint(points: &[Point]) -> Point {
    match points {
        [] => Point::new(0.0, 0.0),
        [single] => *single,
        _ => {
            let half = polyline_length(points) / 2.0;
            if half <= 0.0 {
                return points[0];
            }
            let mut travelled = 0.0;
            for pair in points.windows(2) {
                let leg = pair[0].distance_to(pair[1]);
                if travelled + leg >= half {
                    let t = if leg > 0.0 {
                        (half - travelled) / leg
                    } else {
                        0.0
                    };
                    return pair[0].lerp(pair[1], t);
                }
                travelled += leg;
            }
            points[points.len() - 1]
        }
    }
}

/// Shortest distance from `point` to a polyline.
pub(crate) fn polyline_distance(points: &[Point], point: Point) -> f64 {
    match points {
        [] => f64::INFINITY,
        [single] => single.distance_to(point),
        _ => points
            .windows(2)
            .map(|pair| segment_distance(pair[0], pair[1], point))
            .fold(f64::INFINITY, f64::min),
    }
}

fn segment_distance(a: Point, b: Point, point: Point) -> f64 {
    cartesian::point_segment_distance_with(
        [point.x, point.y],
        [a.x, a.y],
        [b.x, b.y],
        DistanceArithmetic::Hypot,
    )
}

/// Drop repeated points and collapse collinear runs so an orthogonal route has
/// exactly the corners it needs. Two readings of one graph must produce the
/// same vertex list, not the same picture drawn with different vertex counts.
pub(crate) fn simplify_polyline(points: &[Point]) -> Vec<Point> {
    cartesian::simplify_forward_collinear_by(points, |p| [p.x, p.y])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplification_keeps_an_authored_vertex_at_a_rounded_turn() {
        let points = [
            Point::new(7191584856.923134, -1474724867.5598488),
            Point::new(1957430568.2409458, -106873244.3426876),
            Point::new(-16920353649.379017, 4826494470.982347),
        ];
        assert_eq!(simplify_polyline(&points), points);
    }
}
