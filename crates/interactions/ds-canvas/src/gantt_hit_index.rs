//! The Gantt uniform-grid hit index, mechanically extracted from ds-web.
//! Coordinates are CSS pixels. Results name input ordinals only; model identity,
//! selection admission and authored edits stay with the UI's existing owners.
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}
#[derive(Clone, Debug)]
pub struct Region {
    pub id: String,
    pub target: String,
    pub bounds: Bounds,
    pub z: f64,
    pub tolerance: f64,
    pub polyline: Option<Vec<f64>>,
}
pub struct HitIndex {
    regions: Vec<Region>,
    cells: HashMap<u64, Vec<usize>>,
    expanded: HashMap<String, Bounds>,
    cell_size: f64,
}

// Rust's min/max ignore NaN; JavaScript's Math.min/Math.max propagate it.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
fn key(cx: f64, cy: f64) -> u64 {
    let value = (cx + 32_768.) * 65_536. + (cy + 32_768.);
    // Map<number> considers all NaNs equal and -0 equal to +0.
    if value.is_nan() {
        f64::NAN.to_bits()
    } else if value == 0. {
        0
    } else {
        value.to_bits()
    }
}
fn area(bounds: Bounds) -> f64 {
    js_max(0., bounds.right - bounds.left) * js_max(0., bounds.bottom - bounds.top)
}
fn compare(left: &Region, right: &Region) -> Ordering {
    for difference in [right.z - left.z, area(left.bounds) - area(right.bounds)] {
        if difference != 0. && !difference.is_nan() {
            return difference.partial_cmp(&0.).unwrap_or(Ordering::Equal);
        }
    }
    // JS string comparison is UTF-16 code-unit order, not UTF-8 order.
    left.id.encode_utf16().cmp(right.id.encode_utf16())
}
// Preserve the two-input Math.hypot arithmetic used by the existing browser
// implementation. A platform libm hypot differs by one ULP on ordinary points,
// which changes exact-tolerance hit verdicts. Scale before squaring as before.
pub(crate) fn js_hypot(x: f64, y: f64) -> f64 {
    let a = x.abs();
    let b = y.abs();
    if a == f64::INFINITY || b == f64::INFINITY {
        return f64::INFINITY;
    }
    let maximum = js_max(a, b);
    if maximum.is_nan() {
        return f64::NAN;
    }
    if maximum == 0. {
        return 0.;
    }
    let a = a / maximum;
    let b = b / maximum;
    (a * a + b * b).sqrt() * maximum
}
pub fn point_to_segment_distance(x: f64, y: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let length_sq = dx * dx + dy * dy;
    if length_sq == 0. {
        return js_hypot(x - ax, y - ay);
    }
    let t = js_min(1., js_max(0., ((x - ax) * dx + (y - ay) * dy) / length_sq));
    js_hypot(x - (ax + t * dx), y - (ay + t * dy))
}
pub fn point_to_polyline_distance(points: &[f64], x: f64, y: f64) -> f64 {
    if points.len() < 4 {
        return if points.len() >= 2 {
            js_hypot(x - points[0], y - points[1])
        } else {
            f64::INFINITY
        };
    }
    let mut best = f64::INFINITY;
    let mut index = 0;
    while index + 3 < points.len() {
        best = js_min(
            best,
            point_to_segment_distance(
                x,
                y,
                points[index],
                points[index + 1],
                points[index + 2],
                points[index + 3],
            ),
        );
        if best == 0. {
            break;
        }
        index += 2;
    }
    best
}
impl HitIndex {
    pub fn new(regions: Vec<Region>, cell_size: f64, edge_padding: f64) -> Self {
        let cell_size = js_max(4., cell_size);
        let edge_padding = js_max(0., edge_padding);
        let mut cells: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut expanded = HashMap::new();
        for (ordinal, region) in regions.iter().enumerate() {
            let mut bounds = region.bounds;
            if region.polyline.is_some() {
                let pad = if region.tolerance > 0. {
                    edge_padding
                } else {
                    0.
                };
                bounds = Bounds {
                    left: bounds.left - pad,
                    top: bounds.top - pad,
                    right: bounds.right + pad,
                    bottom: bounds.bottom + pad,
                };
            }
            expanded.insert(region.id.clone(), bounds);
            let mut cy = (bounds.top / cell_size).floor();
            let max_y = (bounds.bottom / cell_size).floor();
            while cy <= max_y {
                let mut cx = (bounds.left / cell_size).floor();
                let max_x = (bounds.right / cell_size).floor();
                while cx <= max_x {
                    cells.entry(key(cx, cy)).or_default().push(ordinal);
                    cx += 1.;
                }
                cy += 1.;
            }
        }
        Self {
            regions,
            cells,
            expanded,
            cell_size,
        }
    }
    pub fn size(&self) -> usize {
        self.regions.len()
    }
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
    fn sort(&self, found: &mut [usize]) {
        found.sort_by(|&left, &right| compare(&self.regions[left], &self.regions[right]));
    }
    pub fn pick_all(&self, x: f64, y: f64, targets: Option<&[String]>) -> Vec<usize> {
        let Some(bucket) = self.cells.get(&key(
            (x / self.cell_size).floor(),
            (y / self.cell_size).floor(),
        )) else {
            return vec![];
        };
        let mut found = Vec::new();
        for &ordinal in bucket {
            let region = &self.regions[ordinal];
            if targets.is_some_and(|targets| !targets.contains(&region.target)) {
                continue;
            }
            let bounds = self
                .expanded
                .get(&region.id)
                .copied()
                .unwrap_or(region.bounds);
            if x < bounds.left || x > bounds.right || y < bounds.top || y > bounds.bottom {
                continue;
            }
            if region
                .polyline
                .as_ref()
                .is_none_or(|points| point_to_polyline_distance(points, x, y) <= region.tolerance)
            {
                found.push(ordinal);
            }
        }
        self.sort(&mut found);
        found
    }
    pub fn query(&self, bounds: Bounds) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        let mut cy = (bounds.top / self.cell_size).floor();
        let max_y = (bounds.bottom / self.cell_size).floor();
        while cy <= max_y {
            let mut cx = (bounds.left / self.cell_size).floor();
            let max_x = (bounds.right / self.cell_size).floor();
            while cx <= max_x {
                for &ordinal in self.cells.get(&key(cx, cy)).into_iter().flatten() {
                    let region = &self.regions[ordinal];
                    if seen.contains(&region.id) {
                        continue;
                    }
                    let region_bounds = self
                        .expanded
                        .get(&region.id)
                        .copied()
                        .unwrap_or(region.bounds);
                    if region_bounds.right < bounds.left || region_bounds.left > bounds.right {
                        continue;
                    }
                    if region_bounds.bottom < bounds.top || region_bounds.top > bounds.bottom {
                        continue;
                    }
                    seen.insert(region.id.clone());
                    found.push(ordinal);
                }
                cx += 1.;
            }
            cy += 1.;
        }
        self.sort(&mut found);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn region(id: &str, bounds: [f64; 4], z: f64) -> Region {
        Region {
            id: id.into(),
            target: "task-bar".into(),
            bounds: Bounds {
                left: bounds[0],
                top: bounds[1],
                right: bounds[2],
                bottom: bounds[3],
            },
            z,
            tolerance: 0.,
            polyline: None,
        }
    }
    fn fixture() -> Vec<Region> {
        let mut edge = region("dep:d1", [44., -6., 56., 206.], 10.);
        edge.target = "dependency".into();
        edge.tolerance = 6.;
        edge.polyline = Some(vec![50., 0., 50., 200.]);
        let mut handle = region("task-handle-finish:a", [95., 0., 105., 20.], 40.);
        handle.target = "task-handle-finish".into();
        vec![region("bar:a", [0., 0., 100., 20.], 20.), handle, edge]
    }
    #[test]
    fn owning_pointer_order_filter_and_empty_controls() {
        let index = HitIndex::new(fixture(), 48., 0.);
        assert_eq!(index.pick_all(99., 10., None), [1, 0]);
        assert_eq!(index.pick_all(50., 10., None), [0, 2]);
        assert_eq!(index.pick_all(55., 150., None), [2]);
        assert!(index.pick_all(57., 150., None).is_empty());
        assert!(index.pick_all(1_000., 1_000., None).is_empty());
        assert_eq!(index.pick_all(99., 10., Some(&["task-bar".into()])), [0]);
        assert!(index.pick_all(50., 10., Some(&[])).is_empty());
        assert!(
            HitIndex::new(vec![], 48., 0.)
                .pick_all(0., 0., None)
                .is_empty()
        );
    }
    #[test]
    fn query_deduplicates_ids_in_resolution_order() {
        let index = HitIndex::new(fixture(), 48., 0.);
        assert_eq!(
            index.query(Bounds {
                left: -50.,
                top: -50.,
                right: 200.,
                bottom: 300.
            }),
            [1, 0, 2]
        );
        assert!(index.cell_count() > 1);
        assert_eq!(index.size(), 3);
    }
    #[test]
    fn bounds_padding_does_not_replace_exact_distance() {
        let index = HitIndex::new(vec![fixture().remove(2)], 48., 20.);
        assert_eq!(index.pick_all(56., 150., None), [0]);
        assert!(index.pick_all(60., 150., None).is_empty());
        assert_eq!(
            index.query(Bounds {
                left: 65.,
                top: 150.,
                right: 66.,
                bottom: 151.
            }),
            [0]
        );
        let mut diagonal = region("diagonal", [-6., -6., 106., 106.], 10.);
        diagonal.polyline = Some(vec![0., 0., 100., 0., 100., 100.]);
        diagonal.tolerance = 6.;
        let diagonal = HitIndex::new(vec![diagonal], 48., 0.);
        assert!(diagonal.pick_all(20., 60., None).is_empty());
        assert_eq!(diagonal.pick_all(20., 3., None), [0]);
    }
    #[test]
    fn negative_overscan_and_stable_utf16_ties() {
        let index = HitIndex::new(
            vec![region("negative", [-220., -80., -120., -60.], 20.)],
            48.,
            0.,
        );
        assert_eq!(index.pick_all(-150., -70., None), [0]);
        assert!(index.pick_all(-150., -100., None).is_empty());
        let index = HitIndex::new(
            vec![
                region("\u{e000}", [0., 0., 10., 10.], 20.),
                region("\u{10000}", [0., 0., 10., 10.], 20.),
            ],
            48.,
            0.,
        );
        assert_eq!(index.pick_all(5., 5., None), [1, 0]);
    }
    #[test]
    fn duplicate_id_uses_last_expanded_bounds_but_first_query_identity() {
        let index = HitIndex::new(
            vec![
                region("same", [0., 0., 10., 10.], 20.),
                region("same", [5., 0., 15., 10.], 20.),
            ],
            48.,
            0.,
        );
        assert!(index.pick_all(2., 5., None).is_empty());
        assert_eq!(index.pick_all(12., 5., None), [0, 1]);
        assert_eq!(
            index.query(Bounds {
                left: 12.,
                top: 0.,
                right: 13.,
                bottom: 10.
            }),
            [0]
        );
    }
    #[test]
    fn aliased_cell_keys_only_add_checked_candidates() {
        let index = HitIndex::new(
            vec![
                region("one", [0., 0., 1., 1.], 20.),
                region("two", [48., -3_145_728., 49., -3_145_727.], 20.),
            ],
            48.,
            0.,
        );
        assert_eq!(index.cell_count(), 1);
        assert_eq!(index.pick_all(0., 0., None), [0]);
        assert_eq!(index.pick_all(48., -3_145_728., None), [1]);
    }
    #[test]
    fn nonfinite_numeric_behavior_stays_javascript_compatible() {
        let index = HitIndex::new(fixture(), f64::NAN, 0.);
        assert_eq!(index.cell_count(), 0);
        let index = HitIndex::new(fixture(), f64::INFINITY, 0.);
        assert_eq!(index.pick_all(50., 10., None), [0, 2]);
        assert!(index.pick_all(f64::NAN, 10., None).is_empty());
        let index = HitIndex::new(fixture(), 48., f64::NAN);
        assert_eq!(index.pick_all(50., 10., None), [0]);
        let mut a = region("b", [0., 0., 10., 10.], f64::NAN);
        a.polyline = None;
        let index = HitIndex::new(vec![a, region("a", [0., 0., 10., 10.], 0.)], 48., 0.);
        assert_eq!(index.pick_all(5., 5., None), [1, 0]);
    }
    #[test]
    fn captured_browser_rounding_preserves_exact_tolerance_verdict() {
        let x = -257.34363589435816;
        let y = 320.5309163313359;
        let points = vec![
            -40.42561026290059,
            -202.8548817615956,
            214.00375291705132,
            -167.10777231492102,
        ];
        let expected = 566.5563727372806_f64;
        assert_eq!(
            point_to_polyline_distance(&points, x, y).to_bits(),
            expected.to_bits()
        );
        let mut edge = region("rounding-edge", [-1_000., -1_000., 1_000., 1_000.], 10.);
        edge.polyline = Some(points);
        edge.tolerance = expected;
        assert_eq!(
            HitIndex::new(vec![edge.clone()], 48., 0.).pick_all(x, y, None),
            [0]
        );
        edge.tolerance = expected.next_down();
        assert!(
            HitIndex::new(vec![edge], 48., 0.)
                .pick_all(x, y, None)
                .is_empty()
        );
    }
    #[test]
    fn segment_and_polyline_owning_metric_cases() {
        assert_eq!(point_to_segment_distance(5., 3., 0., 0., 10., 0.), 3.);
        assert_eq!(point_to_segment_distance(-4., 0., 0., 0., 10., 0.), 4.);
        assert_eq!(
            point_to_polyline_distance(&[0., 0., 10., 0., 10., 20.], 12., 19.),
            2.
        );
        assert_eq!(point_to_polyline_distance(&[0., 0.], 3., 4.), 5.);
        assert_eq!(point_to_polyline_distance(&[], 0., 0.), f64::INFINITY);
        assert!(point_to_polyline_distance(&[0., 0., f64::NAN, 1.], 1., 1.).is_nan());
    }
}
