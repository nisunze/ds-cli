//! Cartesian primitives. Coordinates, distances and stations use the caller's
//! explicit space; these functions never select a CRS or round authored values.
//! Tolerances are documented in `docs/contracts/geometry-tolerance-policy.md`.

pub type Point = [f64; 2];

pub fn squared_distance(a: Point, b: Point) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

/// Euclidean distance, preserving the existing LV squared-sum arithmetic.
pub fn distance(a: Point, b: Point) -> f64 {
    distance_with(a, b, DistanceArithmetic::SquaredSum)
}

/// Arithmetic is explicit because report/station contracts can pin float bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceArithmetic {
    SquaredSum,
    Hypot,
}

pub fn distance_with(a: Point, b: Point, arithmetic: DistanceArithmetic) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    match arithmetic {
        DistanceArithmetic::SquaredSum => squared_distance(a, b).sqrt(),
        DistanceArithmetic::Hypot => dx.hypot(dy),
    }
}

pub fn interpolate(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Denominator evaluation is explicit because fused arithmetic can move a foot.
#[derive(Clone, Copy, Debug)]
pub enum SegmentDenominator {
    SquaredSum,
    FusedSum,
    Provided(f64),
}

/// Unclamped segment parameter. A zero-length segment returns zero.
pub fn segment_parameter(point: Point, a: Point, b: Point) -> f64 {
    segment_parameter_with(point, a, b, SegmentDenominator::SquaredSum, 0.0)
}

/// Preserve a caller's denominator and squared-length degeneracy threshold.
pub fn segment_parameter_with(
    point: Point,
    a: Point,
    b: Point,
    arithmetic: SegmentDenominator,
    minimum_squared_length: f64,
) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let denom = match arithmetic {
        SegmentDenominator::SquaredSum => dx * dx + dy * dy,
        SegmentDenominator::FusedSum => dx.mul_add(dx, dy * dy),
        SegmentDenominator::Provided(value) => value,
    };
    if denom <= minimum_squared_length {
        return 0.0;
    }
    ((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / denom
}

pub fn point_segment_distance(point: Point, a: Point, b: Point) -> f64 {
    point_segment_distance_with(point, a, b, DistanceArithmetic::SquaredSum)
}

pub fn point_segment_distance_with(
    point: Point,
    a: Point,
    b: Point,
    arithmetic: DistanceArithmetic,
) -> f64 {
    point_segment_distance_with_options(
        point,
        a,
        b,
        arithmetic,
        SegmentDenominator::SquaredSum,
        0.0,
    )
}

pub fn point_segment_distance_with_options(
    point: Point,
    a: Point,
    b: Point,
    distance_arithmetic: DistanceArithmetic,
    denominator: SegmentDenominator,
    minimum_squared_length: f64,
) -> f64 {
    let t =
        segment_parameter_with(point, a, b, denominator, minimum_squared_length).clamp(0.0, 1.0);
    distance_with(point, interpolate(a, b, t), distance_arithmetic)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolylineProjection {
    pub point: Point,
    pub segment_index: usize,
    pub parameter: f64,
    pub offset: f64,
    pub station: f64,
}

/// Project onto admitted Cartesian coordinates, skipping zero-length segments.
/// Ties retain the first source segment, matching LV connection/welding order.
/// Empty, singleton and wholly degenerate lines return `None`.
pub fn project_onto_polyline_by<T>(
    point: Point,
    line: &[T],
    position: impl Fn(&T) -> Point,
) -> Option<PolylineProjection> {
    let mut best: Option<PolylineProjection> = None;
    let mut station = 0.0;
    for (segment_index, pair) in line.windows(2).enumerate() {
        let a = position(&pair[0]);
        let b = position(&pair[1]);
        let length = distance(a, b);
        if length <= 0.0 {
            continue;
        }
        let parameter = segment_parameter(point, a, b).clamp(0.0, 1.0);
        let foot = interpolate(a, b, parameter);
        let offset = distance(point, foot);
        if best.as_ref().is_none_or(|current| offset < current.offset) {
            best = Some(PolylineProjection {
                point: foot,
                segment_index,
                parameter,
                offset,
                station: station + parameter * length,
            });
        }
        station += length;
    }
    best
}

pub fn cumulative_stations_by<T>(line: &[T], position: impl Fn(&T) -> Point) -> Vec<f64> {
    let mut stations = vec![0.0; line.len()];
    for index in 1..line.len() {
        stations[index] =
            stations[index - 1] + distance(position(&line[index - 1]), position(&line[index]));
    }
    stations
}

/// Split at sorted station cuts, preserving exact source vertices.
/// `stations` must contain one nondecreasing station per source vertex. A cut
/// within `vertex_tolerance` of a vertex uses that vertex, never a reprojected
/// approximation. Endpoint cuts do not manufacture a piece. Payloads (including
/// geographic coordinates or Z) are interpolated by the caller at the same t.
/// This is the LV station-splitting contract, independent of its feature types.
pub fn split_at_stations_with<T: Clone>(
    stations: &[f64],
    cuts: &[f64],
    vertex_tolerance: f64,
    vertex: impl Fn(usize) -> T,
    interpolate: impl Fn(&T, &T, f64) -> T,
) -> Vec<Vec<T>> {
    if stations.is_empty() {
        return Vec::new();
    }
    let mut pieces = Vec::new();
    let mut current = vec![vertex(0)];
    let mut cuts = cuts.iter().copied().peekable();
    for index in 0..stations.len() - 1 {
        while let Some(&cut) = cuts.peek() {
            if cut < stations[index] - vertex_tolerance {
                cuts.next();
            } else if cut <= stations[index] + vertex_tolerance {
                cuts.next();
                if index > 0 && current.len() > 1 {
                    pieces.push(std::mem::take(&mut current));
                    current = vec![vertex(index)];
                }
            } else {
                break;
            }
        }
        let length = stations[index + 1] - stations[index];
        while let Some(&cut) = cuts.peek() {
            if cut < stations[index + 1] - vertex_tolerance {
                cuts.next();
                let t = if length > 0.0 {
                    (cut - stations[index]) / length
                } else {
                    0.0
                };
                let point = interpolate(&vertex(index), &vertex(index + 1), t);
                current.push(point.clone());
                pieces.push(std::mem::take(&mut current));
                current = vec![point];
            } else {
                break;
            }
        }
        current.push(vertex(index + 1));
    }
    pieces.push(current);
    pieces
}

/// Proper interior crossing; endpoint contact and collinear overlap are false.
pub fn segments_cross_properly(a: Point, b: Point, c: Point, d: Point) -> bool {
    let d1 = orientation(c, d, a);
    let d2 = orientation(c, d, b);
    let d3 = orientation(a, b, c);
    let d4 = orientation(a, b, d);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

pub fn orientation(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Signed area with the original cyclic shoelace evaluation order.
pub fn signed_ring_area_by<T>(ring: &[T], position: impl Fn(&T) -> Point) -> f64 {
    let mut twice = 0.0;
    for index in 0..ring.len() {
        let a = position(&ring[index]);
        let b = position(&ring[(index + 1) % ring.len()]);
        twice += a[0] * b[1] - b[0] * a[1];
    }
    twice / 2.0
}

pub fn polyline_length_by<T>(
    line: &[T],
    position: impl Fn(&T) -> Point,
    arithmetic: DistanceArithmetic,
) -> f64 {
    length_of_points(line.iter().map(position), arithmetic)
}

/// Streaming accumulation evaluates each coordinate/projection exactly once.
pub fn length_of_points(
    points: impl IntoIterator<Item = Point>,
    arithmetic: DistanceArithmetic,
) -> f64 {
    let mut previous = None;
    let mut total = 0.0;
    for point in points {
        if let Some(before) = previous {
            total += distance_with(before, point, arithmetic);
        }
        previous = Some(point);
    }
    total
}

/// De Casteljau sampling, retaining the Canvas curve evaluation order.
pub fn quadratic(from: Point, control: Point, to: Point, t: f64) -> Point {
    interpolate(
        interpolate(from, control, t),
        interpolate(control, to, t),
        t,
    )
}

pub fn cubic(from: Point, control_a: Point, control_b: Point, to: Point, t: f64) -> Point {
    let a = interpolate(from, control_a, t);
    let b = interpolate(control_a, control_b, t);
    let c = interpolate(control_b, to, t);
    interpolate(interpolate(a, b, t), interpolate(b, c, t), t)
}

/// Remove exact repeated payloads and forward collinear runs. Reversals remain.
pub fn simplify_forward_collinear_by<T: Clone + PartialEq>(
    points: &[T],
    position: impl Fn(&T) -> Point,
) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(points.len());
    for point in points {
        if out.last().is_some_and(|last| last == point) {
            continue;
        }
        out.push(point.clone());
    }
    if out.len() < 3 {
        return out;
    }
    let mut simplified = vec![out[0].clone()];
    for window in out.windows(3) {
        let (a, b, c) = (
            position(&window[0]),
            position(&window[1]),
            position(&window[2]),
        );
        // Preserve the original Canvas origin-vector determinant. An
        // algebraically equivalent adjacent-vector form can round differently.
        let cross = orientation(a, b, c);
        let forward = (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]);
        if cross != 0.0 || forward < 0.0 {
            simplified.push(window[1].clone());
        }
    }
    simplified.push(out[out.len() - 1].clone());
    simplified
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn length_projection_is_streamed_once_per_source_vertex() {
        let calls = std::cell::Cell::new(0);
        let points = [[0., 0.], [3., 4.], [6., 8.]];
        assert_eq!(
            polyline_length_by(
                &points,
                |point| {
                    calls.set(calls.get() + 1);
                    *point
                },
                DistanceArithmetic::SquaredSum
            ),
            10.
        );
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn collinear_reduction_preserves_a_rounded_origin_vector_turn() {
        let points = [
            [7191584856.923134, -1474724867.5598488],
            [1957430568.2409458, -106873244.3426876],
            [-16920353649.379017, 4826494470.982347],
        ];
        // The adjacent-vector determinant rounds to zero, while the original
        // calculation retains this authored middle point.
        assert_eq!(simplify_forward_collinear_by(&points, |p| *p), points);
    }

    #[test]
    fn collinear_reduction_retains_original_rounded_zero_behavior() {
        let points = [
            [542504588.846221, 969537270.7349102],
            [-627777760.5482227, -938698922.4377396],
            [-1798060109.942666, -2846935115.6103897],
        ];
        // The original determinant rounds to zero here. Changing its arithmetic
        // would retain a vertex that Canvas previously removed.
        assert_eq!(
            simplify_forward_collinear_by(&points, |p| *p),
            [points[0], points[2]]
        );
    }

    #[test]
    fn segment_modes_keep_threshold_and_fused_hit_arithmetic_explicit() {
        let (point, a, b) = ([5., 3.], [0., 0.], [10., 0.]);
        assert_eq!(
            point_segment_distance_with_options(
                point,
                a,
                b,
                DistanceArithmetic::Hypot,
                SegmentDenominator::FusedSum,
                0.0
            ),
            3.0
        );
        assert_eq!(
            segment_parameter_with(point, a, b, SegmentDenominator::Provided(100.0), 100.0),
            0.0
        );
        assert_eq!(
            segment_parameter_with(point, a, a, SegmentDenominator::FusedSum, 0.0),
            0.0
        );
    }

    #[test]
    fn nearest_ties_keep_the_first_segment_and_zero_segments_are_skipped() {
        let line = [[0.0, 0.0], [0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let hit = project_onto_polyline_by([9.0, 1.0], &line, |p| *p).unwrap();
        assert_eq!(hit.segment_index, 1);
        assert_eq!(hit.point, [9.0, 0.0]);
        assert_eq!(hit.station, 9.0);
        assert_eq!(hit.offset, 1.0);
        assert!(project_onto_polyline_by([9.0, 1.0], &[[0.0, 0.0]; 2], |p| *p).is_none());
    }

    #[test]
    fn station_cuts_preserve_payload_vertices_and_total_length() {
        let line = [[0.0, 0.0], [30.0, 0.0], [30.0, 40.0]];
        let stations = cumulative_stations_by(&line, |p| *p);
        let pieces = split_at_stations_with(
            &stations,
            &[30.0, 50.0],
            0.001,
            |i| line[i],
            |a, b, t| interpolate(*a, *b, t),
        );
        assert_eq!(
            pieces,
            vec![
                vec![line[0], line[1]],
                vec![line[1], [30.0, 20.0]],
                vec![[30.0, 20.0], line[2]]
            ]
        );
        let total: f64 = pieces
            .iter()
            .flat_map(|part| part.windows(2))
            .map(|pair| distance(pair[0], pair[1]))
            .sum();
        assert_eq!(total, 70.0);
    }

    #[test]
    fn station_cuts_at_endpoints_are_noops_and_near_vertices_use_authored_values() {
        let line = [[0.0, 0.0], [30.0, 0.0], [30.0, 40.0]];
        let stations = [0.0, 30.0, 70.0];
        let split = |cuts: &[f64]| {
            split_at_stations_with(
                &stations,
                cuts,
                0.001,
                |i| line[i],
                |a, b, t| interpolate(*a, *b, t),
            )
        };
        assert_eq!(split(&[0.0, 70.0]), vec![line.to_vec()]);
        assert_eq!(
            split(&[30.0001]),
            vec![vec![line[0], line[1]], vec![line[1], line[2]]]
        );
    }
}
