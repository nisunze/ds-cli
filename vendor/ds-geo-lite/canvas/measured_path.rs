//! Indexed measured polylines. Profile stations are the first consumer;
//! the primitive knows only group ordinals and caller-provided scalar values.
//! No interpolation bridges a gap between supplied segments.
use rstar::{AABB, RTree, RTreeObject};
#[derive(Clone)]
struct Segment {
    row: usize,
    group: f64,
    a: [f64; 3],
    b: [f64; 3],
}
impl RTreeObject for Segment {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [self.a[0].min(self.b[0]), 0.],
            [self.a[0].max(self.b[0]), 0.],
        )
    }
}
#[derive(Clone)]
struct Ordinate {
    row: usize,
    group: f64,
    x: f64,
    value: f64,
}
impl RTreeObject for Ordinate {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.x, self.group])
    }
}
pub struct MeasuredPath {
    segments: RTree<Segment>,
    ordinates: RTree<Ordinate>,
}
impl MeasuredPath {
    /// Segment rows: group, x0, y0, value0, x1, y1, value1.
    /// Ordinate rows: group, x, value. Results preserve original row order ties.
    pub fn new(segments: &[f64], ordinates: &[f64]) -> Result<Self, String> {
        if !segments.len().is_multiple_of(7)
            || !ordinates.len().is_multiple_of(3)
            || segments.len() > 700_000
            || ordinates.len() > 300_000
            || !segments.iter().chain(ordinates).all(|v| v.is_finite())
        {
            return Err(
                "measured paths require finite packed rows, at most 100000 segments and ordinates"
                    .into(),
            );
        }
        let valid_group = |v: f64| v >= 0. && v.fract() == 0. && v <= 1_000_000.;
        if !segments
            .chunks_exact(7)
            .chain(ordinates.chunks_exact(3))
            .all(|v| valid_group(v[0]))
        {
            return Err("measured path groups must be integer ordinals".into());
        }
        Ok(Self {
            segments: RTree::bulk_load(
                segments
                    .chunks_exact(7)
                    .enumerate()
                    .map(|(row, v)| Segment {
                        row,
                        group: v[0],
                        a: [v[1], v[2], v[3]],
                        b: [v[4], v[5], v[6]],
                    })
                    .collect(),
            ),
            ordinates: RTree::bulk_load(
                ordinates
                    .chunks_exact(3)
                    .enumerate()
                    .map(|(row, v)| Ordinate {
                        row,
                        group: v[0],
                        x: v[1],
                        value: v[2],
                    })
                    .collect(),
            ),
        })
    }
    /// group, interpolated value, vertical distance; empty if outside coverage.
    pub fn at(&self, x: f64, y: f64) -> Vec<f64> {
        if !x.is_finite() || !y.is_finite() {
            return vec![];
        }
        let mut best: Option<(f64, usize, f64, f64)> = None;
        for s in self
            .segments
            .locate_in_envelope_intersecting(&AABB::from_point([x, 0.]))
        {
            let span = s.b[0] - s.a[0];
            let t = if span == 0. { 0. } else { (x - s.a[0]) / span };
            let distance = (y - (s.a[1] + t * (s.b[1] - s.a[1]))).abs();
            if best.is_none_or(|(d, row, _, _)| distance < d || (distance == d && s.row < row)) {
                best = Some((distance, s.row, s.group, s.a[2] + t * (s.b[2] - s.a[2])));
            }
        }
        best.map_or_else(Vec::new, |(d, _, g, v)| vec![g, v, d])
    }
    /// original ordinate row, value; empty means preserve the caller's hit.
    pub fn snap(&self, group: f64, x: f64, tolerance: f64) -> Vec<f64> {
        if ![group, x, tolerance].iter().all(|v| v.is_finite()) || tolerance <= 0. {
            return vec![];
        }
        let mut best: Option<(f64, usize, f64)> = None;
        for o in self
            .ordinates
            .locate_in_envelope_intersecting(&AABB::from_corners(
                [x - tolerance, group],
                [x + tolerance, group],
            ))
        {
            let distance = (o.x - x).abs();
            if best.is_none_or(|(d, row, _)| distance < d || (distance == d && o.row < row)) {
                best = Some((distance, o.row, o.value));
            }
        }
        best.map_or_else(Vec::new, |(_, row, v)| vec![row as f64, v])
    }
}
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
