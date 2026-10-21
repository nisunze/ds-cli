//! Neutral Cartesian point and rectangular-window admission values.
//! No viewport transform, pointer gesture, model or host behavior lives here.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn distance_to(self, other: Self) -> f64 {
        crate::cartesian::distance_with(
            [self.x, self.y],
            [other.x, other.y],
            crate::cartesian::DistanceArithmetic::Hypot,
        )
    }
}
/// Two Cartesian corners. Admission is independent of the coordinate space.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoomWindow {
    pub from: Point,
    pub to: Point,
}
impl ZoomWindow {
    pub fn validate(self) -> Result<(), String> {
        if !self.from.is_finite() || !self.to.is_finite() {
            return Err("canvas zoom window must be finite".into());
        }
        let width = (self.to.x - self.from.x).abs();
        let height = (self.to.y - self.from.y).abs();
        if !width.is_finite() || !height.is_finite() {
            return Err("canvas zoom window extent must be finite".into());
        }
        if width == 0. || height == 0. {
            return Err("canvas zoom window must have positive width and height".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, ZoomWindow};

    #[test]
    fn rectangle_admission_preserves_reversal_and_refusal_messages() {
        let from = Point::new(12., 30.);
        let to = Point::new(32., 60.);
        for window in [ZoomWindow { from, to }, ZoomWindow { from: to, to: from }] {
            assert_eq!(window.validate(), Ok(()));
        }
        for to in [Point::new(12., 60.), Point::new(32., 30.)] {
            assert_eq!(
                ZoomWindow { from, to }.validate().unwrap_err(),
                "canvas zoom window must have positive width and height"
            );
        }
        assert_eq!(
            ZoomWindow {
                from,
                to: Point::new(f64::NAN, 60.)
            }
            .validate()
            .unwrap_err(),
            "canvas zoom window must be finite"
        );
        assert_eq!(
            ZoomWindow {
                from: Point::new(-f64::MAX, 0.),
                to: Point::new(f64::MAX, 1.),
            }
            .validate()
            .unwrap_err(),
            "canvas zoom window extent must be finite"
        );
    }

    #[test]
    fn point_distance_keeps_hypot_arithmetic() {
        let from = Point::new(1e200, 0.);
        let to = Point::new(0., 1e200);
        assert_eq!(from.distance_to(to), 1e200_f64.hypot(1e200));
        assert!(from.is_finite());
        assert!(!Point::new(f64::INFINITY, 0.).is_finite());
    }
}
