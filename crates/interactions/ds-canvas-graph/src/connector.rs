//! Free placed graph cards use the same anchors, route shapes and Curve as
//! authored-cell scenes. No placement changes are implied by routing a wire.
use crate::curve::{Curve, CurveSegment, Point};
use crate::error::{GeometryError, finite, finite_positive};
use crate::input::{EdgeShape, LayoutMetrics, Orientation, PortSide};
use crate::layout::{Anchor, orthogonal_points, shape_curve};
use crate::scene::{NodeRect, Rect};

/// Numeric host boundary: two rectangles, shape, stable lane, optional faces,
/// offsets, orientation and clearance. The result carries one Curve followed
/// by its native marker positions/tangents; no host chooses control points.
pub fn route_buffer(input: &[f64]) -> Result<Vec<f64>, GeometryError> {
    if input.len() != 16 && input.len() != 18 && input.len() != 21 {
        return Err(GeometryError::InvalidBuffer {
            detail:
                "connector input requires 16 values, optionally followed by boundary and padding"
                    .into(),
        });
    }
    for (index, value) in input.iter().enumerate() {
        finite(&format!("connector[{index}]"), *value)?;
    }
    let rect = |at: usize| -> Result<Rect, GeometryError> {
        let r = Rect {
            x: input[at],
            y: input[at + 1],
            width: finite_positive("width", input[at + 2])?,
            height: finite_positive("height", input[at + 3])?,
        };
        finite("right", r.max_x())?;
        finite("bottom", r.max_y())?;
        Ok(r)
    };
    let from = rect(0)?;
    let to = rect(4)?;
    let shape = match input[8] {
        0.0 => EdgeShape::Orthogonal,
        1.0 => EdgeShape::Straight,
        2.0 => EdgeShape::Quadratic,
        3.0 => EdgeShape::Cubic,
        _ => return Err(invalid("shape", input[8], "0..3")),
    };
    if input[9] < 0.0 || input[9] > 4096.0 || input[9].fract() != 0.0 {
        return Err(invalid("lane", input[9], "integer 0..4096"));
    }
    let orientation = match input[14] {
        0.0 => Orientation::Horizontal,
        1.0 => Orientation::Vertical,
        _ => return Err(invalid("orientation", input[14], "0 or 1")),
    };
    for value in &input[12..14] {
        if !(0.0..=1.0).contains(value) {
            return Err(invalid("portOffset", *value, "0..1"));
        }
    }
    if !(0.0..=1e6).contains(&input[15]) {
        return Err(invalid("clearance", input[15], "0..1000000"));
    }
    let a = from.center();
    let b = to.center();
    let stacked = match orientation {
        Orientation::Horizontal => (b.x - a.x).abs() < from.width.max(to.width) * 0.65,
        Orientation::Vertical => (b.y - a.y).abs() < from.height.max(to.height) * 0.65,
    };
    let forward = match orientation {
        Orientation::Horizontal => b.x >= a.x,
        Orientation::Vertical => b.y >= a.y,
    };
    let normal = match orientation {
        Orientation::Horizontal => PortSide::Right,
        Orientation::Vertical => PortSide::Bottom,
    };
    let side = if (input[9] as u32).is_multiple_of(2) {
        normal
    } else {
        normal.opposite()
    };
    let (default_from, default_to) = if stacked {
        (side, side)
    } else if forward {
        (normal, normal.opposite())
    } else {
        (normal.opposite(), normal)
    };
    let start_side = side_value(input[10])?.unwrap_or(default_from);
    let end_side = side_value(input[11])?.unwrap_or(default_to);
    let mut start = Anchor {
        point: from.point_on_side(start_side, input[12]),
        side: start_side,
    };
    let mut end = Anchor {
        point: to.point_on_side(end_side, input[13]),
        side: end_side,
    };
    if input.len() >= 18 {
        if !(0.0..=1e6).contains(&input[17]) {
            return Err(invalid("boundaryPadding", input[17], "0..1000000"));
        }
        match input[16] {
            0.0 => {}
            1.0 => {
                start = Anchor {
                    point: Point::new(from.max_x(), a.y),
                    side: PortSide::Right,
                };
                end = Anchor {
                    point: Point::new(to.max_x() - input[17], a.y),
                    side: PortSide::Left,
                };
                if (end.point.x - start.point.x).abs() > from.width {
                    return Ok(Vec::new());
                }
            }
            2.0 => {
                start = Anchor {
                    point: Point::new(from.x + input[17], b.y),
                    side: PortSide::Right,
                };
                end = Anchor {
                    point: Point::new(to.x, b.y),
                    side: PortSide::Left,
                };
                if (end.point.x - start.point.x).abs() > to.width {
                    return Ok(Vec::new());
                }
            }
            _ => {
                return Err(invalid(
                    "boundary",
                    input[16],
                    "0 none, 1 target or 2 source",
                ));
            }
        }
    }
    let metrics = LayoutMetrics {
        stub: input[15] + (input[9] / 2.0).floor() * 16.0,
        ..LayoutMetrics::default()
    };
    let node = |id: &str, rect| NodeRect {
        id: id.into(),
        column: 0,
        row: 0,
        rect,
        primary_label: Point::new(0.0, 0.0),
        secondary_label: Point::new(0.0, 0.0),
    };
    let from_node = node("source", from);
    let mut to_node = node("target", to);
    if !stacked {
        to_node.column = 1;
    }
    let points = orthogonal_points(orientation, &metrics, &from_node, &to_node, start, end);
    let mut curve = shape_curve(shape, start, end, &points, &metrics);
    if input.len() == 21 {
        finite_positive("projectionZoom", input[20])?;
        let project =
            |p: Point| Point::new(input[18] + p.x * input[20], input[19] + p.y * input[20]);
        curve.start = project(curve.start);
        curve.segments = curve
            .segments
            .iter()
            .map(|s| match s {
                CurveSegment::Line { to } => CurveSegment::Line { to: project(*to) },
                CurveSegment::Quadratic { control, to } => CurveSegment::Quadratic {
                    control: project(*control),
                    to: project(*to),
                },
                CurveSegment::Cubic {
                    control_a,
                    control_b,
                    to,
                } => CurveSegment::Cubic {
                    control_a: project(*control_a),
                    control_b: project(*control_b),
                    to: project(*to),
                },
            })
            .collect();
    }
    let mut result = curve_markers(&curve).to_vec();
    result.push(curve.segments.len() as f64);
    for segment in &curve.segments {
        let [a, b] = segment.control_slots();
        let end = segment.end();
        result.extend_from_slice(&[
            f64::from(segment.op_code()),
            a.x,
            a.y,
            b.x,
            b.y,
            end.x,
            end.y,
        ]);
    }
    if result.iter().any(|v| !v.is_finite()) {
        return Err(invalid("connector", 0.0, "finite output coordinates"));
    }
    Ok(result)
}

fn invalid(field: &str, value: f64, expected: &str) -> GeometryError {
    GeometryError::InvalidMetric {
        field: field.into(),
        value: value.to_string(),
        expected: expected.into(),
    }
}
fn side_value(value: f64) -> Result<Option<PortSide>, GeometryError> {
    Ok(match value {
        -1.0 => None,
        0.0 => Some(PortSide::Left),
        1.0 => Some(PortSide::Right),
        2.0 => Some(PortSide::Top),
        3.0 => Some(PortSide::Bottom),
        _ => return Err(invalid("side", value, "-1 or 0..3")),
    })
}

/// Marker-only transport for curves already authored by a retained scene.
pub fn markers_buffer(input: &[f64]) -> Result<Vec<f64>, GeometryError> {
    let output = curve_markers(&decode_curve(input)?);
    if output.iter().any(|value| !value.is_finite()) {
        return Err(invalid("curve", 0.0, "finite marker coordinates"));
    }
    Ok(output.to_vec())
}

/// The painted segment list is also the hit geometry, for every route shape.
pub fn distance_buffer(input: &[f64], x: f64, y: f64) -> Result<f64, GeometryError> {
    finite("pointerX", x)?;
    finite("pointerY", y)?;
    let curve = decode_curve(input)?;
    Ok(crate::curve::polyline_distance(
        &curve.flatten(32),
        Point::new(x, y),
    ))
}

fn decode_curve(input: &[f64]) -> Result<Curve, GeometryError> {
    if input.len() < 3
        || input[2] < 0.0
        || input[2] > 4096.0
        || input[2].fract() != 0.0
        || input.len() != 3 + input[2] as usize * 7
    {
        return Err(GeometryError::InvalidBuffer {
            detail: "curve marker buffer must contain a start, count and complete segments".into(),
        });
    }
    for (index, value) in input.iter().enumerate() {
        finite(&format!("curve[{index}]"), *value)?;
    }
    let mut segments = Vec::with_capacity(input[2] as usize);
    for values in input[3..].chunks_exact(7) {
        let to = Point::new(values[5], values[6]);
        segments.push(match values[0] {
            1.0 => CurveSegment::Line { to },
            2.0 => CurveSegment::Quadratic {
                control: Point::new(values[1], values[2]),
                to,
            },
            3.0 => CurveSegment::Cubic {
                control_a: Point::new(values[1], values[2]),
                control_b: Point::new(values[3], values[4]),
                to,
            },
            _ => return Err(invalid("curveOp", values[0], "1..3")),
        });
    }
    Ok(Curve::new(Point::new(input[0], input[1]), segments))
}

/// Start/end use the exact Bézier derivative (with degenerate controls skipped).
/// The midpoint uses the same flattened arc length used by native hit/bounds.
pub fn curve_markers(curve: &Curve) -> [f64; 9] {
    let points = curve.flatten(32);
    let angle = |a: Point, b: Point| (b.y - a.y).atan2(b.x - a.x);
    let first = curve
        .segments
        .first()
        .map(|s| match s {
            CurveSegment::Line { to } => vec![*to],
            CurveSegment::Quadratic { control, to } => vec![*control, *to],
            CurveSegment::Cubic {
                control_a,
                control_b,
                to,
            } => vec![*control_a, *control_b, *to],
        })
        .unwrap_or_default();
    let first = first
        .into_iter()
        .find(|p| *p != curve.start)
        .unwrap_or(curve.start);
    let end = curve.end();
    let previous = if curve.segments.len() > 1 {
        curve.segments[curve.segments.len() - 2].end()
    } else {
        curve.start
    };
    let last = curve
        .segments
        .last()
        .map(|s| match s {
            CurveSegment::Line { .. } => vec![previous],
            CurveSegment::Quadratic { control, .. } => vec![*control, previous],
            CurveSegment::Cubic {
                control_a,
                control_b,
                ..
            } => vec![*control_b, *control_a, previous],
        })
        .unwrap_or_default();
    let last = last.into_iter().find(|p| *p != end).unwrap_or(end);
    let half = crate::curve::polyline_length(&points) / 2.0;
    let mut travelled = 0.0;
    let mut middle = curve.start;
    let mut mid_angle = 0.0;
    for pair in points.windows(2) {
        let leg = pair[0].distance_to(pair[1]);
        if leg > 0.0 && travelled + leg >= half {
            middle = pair[0].lerp(pair[1], (half - travelled) / leg);
            mid_angle = angle(pair[0], pair[1]);
            break;
        }
        travelled += leg;
    }
    [
        curve.start.x,
        curve.start.y,
        end.x,
        end.y,
        angle(curve.start, first),
        angle(last, end),
        middle.x,
        middle.y,
        mid_angle,
    ]
}
