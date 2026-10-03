//! Averaged-normal, flat-ended metric corridors. This preserves the existing
//! LV/MV construction; it is distinct from miter-scaled, round-cap corridors.

use crate::cartesian::{Point, distance};

#[derive(Clone, Copy, Debug)]
pub struct AveragedNormalOptions {
    /// Length in the caller's coordinate units below which a leg uses +X.
    pub minimum_leg_length: f64,
    /// Dimensionless norm below which a join uses the following direction.
    pub minimum_join_norm: f64,
}

impl AveragedNormalOptions {
    pub const LV: Self = Self {
        minimum_leg_length: 1e-9,
        minimum_join_norm: 1e-9,
    };
    pub const MV: Self = Self {
        minimum_leg_length: 0.0,
        minimum_join_norm: 0.0,
    };
}

fn normalize(vector: Point, minimum: f64) -> Option<Point> {
    let length = distance(vector, [0.0, 0.0]);
    if length <= minimum {
        None
    } else {
        Some([vector[0] / length, vector[1] / length])
    }
}

/// Offset left then right in reverse source order and close at the first point.
/// The caller retains CRS selection, admission and any subsequent ring dedupe.
pub fn averaged_normal(
    points: &[Point],
    half_width: f64,
    options: AveragedNormalOptions,
) -> Option<Vec<Point>> {
    if points.len() < 2 || half_width <= 0.0 {
        return None;
    }
    let unit = |a: Point, b: Point| {
        normalize([b[0] - a[0], b[1] - a[1]], options.minimum_leg_length).unwrap_or([1.0, 0.0])
    };
    let mut left = Vec::with_capacity(points.len());
    let mut right = Vec::with_capacity(points.len());
    for index in 0..points.len() {
        let direction = if index == 0 {
            unit(points[0], points[1])
        } else if index == points.len() - 1 {
            unit(points[index - 1], points[index])
        } else {
            let previous = unit(points[index - 1], points[index]);
            let next = unit(points[index], points[index + 1]);
            normalize(
                [previous[0] + next[0], previous[1] + next[1]],
                options.minimum_join_norm,
            )
            .unwrap_or(next)
        };
        let normal = [-direction[1], direction[0]];
        left.push([
            points[index][0] + normal[0] * half_width,
            points[index][1] + normal[1] * half_width,
        ]);
        right.push([
            points[index][0] - normal[0] * half_width,
            points[index][1] - normal[1] * half_width,
        ]);
    }
    right.reverse();
    left.extend(right);
    left.push(left[0]);
    Some(left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_order_flat_ends_and_reversal_fallback_are_preserved() {
        assert_eq!(
            averaged_normal(&[[0., 0.], [10., 0.]], 1., AveragedNormalOptions::LV).unwrap(),
            [[0., 1.], [10., 1.], [10., -1.], [0., -1.], [0., 1.]]
        );
        let ring = averaged_normal(
            &[[0., 0.], [1., 0.], [0., 0.]],
            1.,
            AveragedNormalOptions::LV,
        )
        .unwrap();
        assert_eq!(ring[1], [1., -1.]);
        assert_eq!(ring.len(), 7);
        assert_eq!(ring.first(), ring.last());
    }

    #[test]
    fn short_legs_keep_the_distinct_lv_and_mv_cutoffs() {
        let points = [[0., 0.], [0., 1e-10]];
        let lv = averaged_normal(&points, 1., AveragedNormalOptions::LV).unwrap();
        let mv = averaged_normal(&points, 1., AveragedNormalOptions::MV).unwrap();
        assert_eq!(lv[0], [0., 1.]);
        assert_eq!(mv[0], [-1., 0.]);
        assert!(averaged_normal(&points, 0., AveragedNormalOptions::LV).is_none());
        assert!(averaged_normal(&points[..1], 1., AveragedNormalOptions::LV).is_none());
    }
}
