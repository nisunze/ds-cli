//! Cartesian round-capped corridor buffering. Callers admit finite XYZ points.
//! Units are explicit; Z is retained as a payload, never used in the offset.
const CORRIDOR_CAP_SEGMENTS: usize = 16;
pub fn rounded_corridor(route: &[[f64; 3]], width: f64) -> Option<Vec<[f64; 3]>> {
    if route.len() < 2 || !width.is_finite() || width <= 0.0 {
        return None;
    }
    let mut normals = Vec::with_capacity(route.len() - 1);
    for pair in route.windows(2) {
        let dx = pair[1][0] - pair[0][0];
        let dy = pair[1][1] - pair[0][1];
        let length = dx.hypot(dy);
        if length <= f64::EPSILON {
            return None;
        }
        normals.push((-dy / length, dx / length));
    }
    let offset_at = |index: usize, side: f64| -> (f64, f64) {
        let normal = if index == 0 {
            normals[0]
        } else if index + 1 == route.len() {
            normals[normals.len() - 1]
        } else {
            let previous = normals[index - 1];
            let next = normals[index];
            let sum = (previous.0 + next.0, previous.1 + next.1);
            let magnitude = sum.0.hypot(sum.1);
            if magnitude <= 1.0e-12 {
                next
            } else {
                let miter = (sum.0 / magnitude, sum.1 / magnitude);
                let projection = miter.0 * next.0 + miter.1 * next.1;
                let scale = if projection.abs() <= 1.0e-12 {
                    width
                } else {
                    (width / projection).clamp(-10.0 * width, 10.0 * width)
                };
                return (
                    route[index][0] + side * miter.0 * scale,
                    route[index][1] + side * miter.1 * scale,
                );
            }
        };
        (
            route[index][0] + side * normal.0 * width,
            route[index][1] + side * normal.1 * width,
        )
    };

    let mut vertices = Vec::with_capacity(route.len() * 2 + CORRIDOR_CAP_SEGMENTS * 2 + 1);
    for (index, node) in route.iter().enumerate() {
        let (x_m, y_m) = offset_at(index, 1.0);
        vertices.push([x_m, y_m, node[2]]);
    }
    // Round the far end: sweep from the offset side just walked, around through
    // the forward tangent, onto the opposite side the reverse walk starts on.
    let far = route[route.len() - 1];
    vertices.extend(cap_arc(
        (far[0], far[1]),
        normals[normals.len() - 1],
        width,
        far[2],
    ));
    for (index, node) in route.iter().enumerate().rev() {
        let (x_m, y_m) = offset_at(index, -1.0);
        vertices.push([x_m, y_m, node[2]]);
    }
    // Round the near end, closing back onto the first vertex.
    let near = route[0];
    vertices.extend(cap_arc(
        (near[0], near[1]),
        (-normals[0].0, -normals[0].1),
        width,
        near[2],
    ));
    vertices.push(vertices[0]);
    Some(vertices)
}

/// Interior points of a round corridor end cap: a clockwise half turn of radius
/// `width` about `center`, starting from the offset direction `from` and landing
/// on `-from`, which carries it through the route tangent.
///
/// Both endpoints are excluded — the two offset walks already push the exact
/// terminal points, so the ring stays watertight and no interior vertex moves.
fn cap_arc(center: (f64, f64), from: (f64, f64), width: f64, z_m: f64) -> Vec<[f64; 3]> {
    let start = from.1.atan2(from.0);
    (1..CORRIDOR_CAP_SEGMENTS)
        .map(|step| {
            let angle =
                start - std::f64::consts::PI * (step as f64) / (CORRIDOR_CAP_SEGMENTS as f64);
            [
                center.0 + width * angle.cos(),
                center.1 + width * angle.sin(),
                z_m,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_caps_keep_authored_offsets_and_endpoint_heights() {
        let ring = rounded_corridor(&[[0., 0., 3.], [10., 0., 5.]], 2.).unwrap();
        assert_eq!(ring.len(), 35);
        assert_eq!(ring[0], [0., 2., 3.]);
        assert_eq!(ring[1], [10., 2., 5.]);
        assert_eq!(ring.first(), ring.last());
        assert_eq!(ring.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min), -2.);
        assert_eq!(
            ring.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
            12.
        );
        assert!(ring.iter().all(|p| p[2] == 3. || p[2] == 5.));
        assert!(rounded_corridor(&[[0., 0., 3.], [0., 0., 5.]], 2.).is_none());
    }
}
