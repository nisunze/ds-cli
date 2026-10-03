//! Oriented envelopes in the caller's explicit Cartesian coordinate space.
use geo::{LineString, MinimumRotatedRect};
/// Enclose finite points and expand by a nonnegative buffer in the same units.
/// CRS selection, admission and output rounding belong to the consumer.
pub fn oriented_buffered(metric: &[[f64; 2]], buffer_m: f64) -> Vec<[f64; 2]> {
    if metric.is_empty() {
        return Vec::new();
    }
    let cloud: LineString<f64> = metric.iter().map(|p| (p[0], p[1])).collect();
    let rectangle = cloud.minimum_rotated_rect();
    let axis = rectangle
        .as_ref()
        .and_then(|r| {
            r.exterior().lines().find_map(|line| {
                let dx = line.end.x - line.start.x;
                let dy = line.end.y - line.start.y;
                let length = dx.hypot(dy);
                (length > 1e-9).then_some([dx / length, dy / length])
            })
        })
        .unwrap_or([1.0, 0.0]); // The zero-area single-point rectangle has no direction.
    let origin = metric[0];
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for point in metric {
        let dx = point[0] - origin[0];
        let dy = point[1] - origin[1];
        let x = dx * axis[0] + dy * axis[1];
        let y = -dx * axis[1] + dy * axis[0];
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    min_x -= buffer_m;
    max_x += buffer_m;
    min_y -= buffer_m;
    max_y += buffer_m;
    let mut result = Vec::with_capacity(5);
    for [x, y] in [
        [min_x, min_y],
        [max_x, min_y],
        [max_x, max_y],
        [min_x, max_y],
        [min_x, min_y],
    ] {
        result.push([
            origin[0] + x * axis[0] - y * axis[1],
            origin[1] + x * axis[1] + y * axis[0],
        ]);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn point_line_and_empty_extents_keep_their_exact_boundaries() {
        assert!(oriented_buffered(&[], 1.).is_empty());
        let ring = oriented_buffered(&[[2., 3.]], 1.);
        assert_eq!(ring, vec![[1., 2.], [3., 2.], [3., 4.], [1., 4.], [1., 2.]]);
        let line = oriented_buffered(&[[2., 3.], [8., 3.]], 1.);
        assert_eq!(line.len(), 5);
        assert_eq!(line.first(), line.last());
        assert_eq!(line.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min), 1.);
        assert_eq!(
            line.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
            9.
        );
    }
}
