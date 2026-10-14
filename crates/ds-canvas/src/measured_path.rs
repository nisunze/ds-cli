//! Indexed paths use the canonical toolkit; Canvas retains drag intent.
pub use ds_geo_lite::measured_path::MeasuredPath;

/// Relative, anchored drag along one constrained axis. Result: value and
/// clamp side (-1 minimum, 0 free, 1 maximum).
pub fn axis_drag(
    start: f64,
    anchor: f64,
    current: f64,
    units: f64,
    min: f64,
    max: f64,
) -> Result<[f64; 2], String> {
    if ![start, anchor, current, units, min, max]
        .iter()
        .all(|v| v.is_finite())
        || units <= 0.
        || min > max
    {
        return Err(
            "axis drag requires finite coordinates, positive scale and ordered bounds".into(),
        );
    }
    let raw = start + (current - anchor) / units;
    Ok(if raw < min {
        [min, -1.]
    } else if raw > max {
        [max, 1.]
    } else {
        [raw, 0.]
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gaps_reverse_segments_and_ties_are_stable() {
        let p = MeasuredPath::new(
            &[
                0., 10., 0., 100., 0., 0., 0., 1., 0., 0., 0., 10., 0., 100., 0., 20., 0., 200.,
                30., 0., 300.,
            ],
            &[0., 3., 30., 0., 7., 70., 1., 5., 50.],
        )
        .unwrap();
        assert_eq!(p.at(5., 2.), vec![0., 50., 2.]);
        assert!(p.at(15., 0.).is_empty());
        assert_eq!(p.snap(0., 5., 2.), vec![0., 30.]);
        assert_eq!(p.snap(1., 5., 2.), vec![2., 50.]);
        assert!(p.snap(0., 5., 1.).is_empty());
    }
    #[test]
    fn drag_is_anchored_and_clamped() {
        assert_eq!(axis_drag(100., 50., 60., 2., 0., 200.).unwrap(), [105., 0.]);
        assert_eq!(
            axis_drag(100., 50., 500., 2., 0., 200.).unwrap(),
            [200., 1.]
        );
        assert!(axis_drag(100., 50., 60., 0., 0., 200.).is_err());
    }
}
