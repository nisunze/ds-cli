//! Screen-space Profile label placement. Text metrics and locale collation are
//! host facts; label policy and authored model semantics are not owned here.
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Orientation {
    Auto,
    Right,
    Left,
    Above,
    Below,
    Vertical,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Right,
    Center,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Baseline {
    Bottom,
    Top,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub align: Align,
    pub baseline: Baseline,
    pub rotation_rad: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct OrderRequest {
    pub anchor: Point,
    pub priority: f64,
    pub collation_rank: usize,
    pub enabled: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct MeasuredLabel {
    pub input_index: usize,
    pub anchor: Point,
    pub orientation: Orientation,
    pub required: bool,
    pub text_width: f64,
    pub line_count: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub width: f64,
    pub height: f64,
    pub line_height: f64,
    pub font_height: f64,
    pub padding: f64,
    pub margin: f64,
    pub cell_size: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedLabel {
    pub input_index: usize,
    pub placement: Placement,
    pub bounds: Bounds,
}

fn numeric_order(left: f64, right: f64) -> Ordering {
    // JS sort treats NaN comparator results as zero; its || chain then tries
    // the next tie-breaker. Preserve that behavior for host numeric facts.
    let difference = left - right;
    if difference.is_nan() || difference == 0.0 {
        Ordering::Equal
    } else if difference < 0.0 {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}
pub fn label_order(requests: &[OrderRequest], width: f64, height: f64) -> Vec<usize> {
    let mut ordered: Vec<_> = requests
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled)
        .collect();
    ordered.sort_by(|(li, l), (ri, r)| {
        numeric_order(r.priority, l.priority)
            .then_with(|| numeric_order(l.anchor.x, r.anchor.x))
            .then_with(|| numeric_order(l.anchor.y, r.anchor.y))
            .then_with(|| l.collation_rank.cmp(&r.collation_rank))
            .then_with(|| li.cmp(ri))
    });
    ordered
        .into_iter()
        .filter(|(_, r)| {
            !(r.anchor.x < 0.0 || r.anchor.x > width || r.anchor.y < 0.0 || r.anchor.y > height)
        })
        .map(|(index, _)| index)
        .collect()
}
pub fn placement(anchor: Point, orientation: Orientation) -> Placement {
    let (x, y, align, baseline, rotation_rad) = match orientation {
        Orientation::Left => (
            anchor.x - 6.0,
            anchor.y - 3.0,
            Align::Right,
            Baseline::Bottom,
            0.0,
        ),
        Orientation::Above => (
            anchor.x,
            anchor.y - 8.0,
            Align::Center,
            Baseline::Bottom,
            0.0,
        ),
        Orientation::Below => (anchor.x, anchor.y + 8.0, Align::Center, Baseline::Top, 0.0),
        Orientation::Vertical => (
            anchor.x + 6.0,
            anchor.y - 5.0,
            Align::Left,
            Baseline::Bottom,
            -std::f64::consts::PI / 2.0,
        ),
        Orientation::Auto | Orientation::Right => (
            anchor.x + 6.0,
            anchor.y - 3.0,
            Align::Left,
            Baseline::Bottom,
            0.0,
        ),
    };
    Placement {
        x,
        y,
        align,
        baseline,
        rotation_rad,
    }
}
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
fn local_bounds(p: Placement, width: f64, lines: usize, o: Options) -> Bounds {
    let left = match p.align {
        Align::Right => -width,
        Align::Center => -width / 2.0,
        Align::Left => 0.0,
    };
    let right = match p.align {
        Align::Right => 0.0,
        Align::Center => width / 2.0,
        Align::Left => width,
    };
    let top = if p.baseline == Baseline::Top {
        0.0
    } else {
        -o.font_height
    };
    Bounds {
        left,
        top,
        right,
        bottom: top + o.font_height + js_max(0.0, lines as f64 - 1.0) * o.line_height,
    }
}
fn screen_bounds(p: Placement, b: Bounds, padding: f64) -> Bounds {
    // These are exactly Math.cos/Math.sin of the two supported rotations.
    // Pin the vertical constant instead of changing it through platform libm.
    let (cosine, sine) = if p.rotation_rad == 0.0 {
        (1.0, 0.0)
    } else {
        (6.123233995736766e-17, -1.0)
    };
    let mut xs = Vec::with_capacity(4);
    let mut ys = Vec::with_capacity(4);
    for (x, y) in [
        (b.left, b.top),
        (b.right, b.top),
        (b.right, b.bottom),
        (b.left, b.bottom),
    ] {
        xs.push(p.x + x * cosine - y * sine);
        ys.push(p.y + x * sine + y * cosine);
    }
    Bounds {
        left: xs.iter().copied().fold(f64::INFINITY, js_min) - padding,
        top: ys.iter().copied().fold(f64::INFINITY, js_min) - padding,
        right: xs.into_iter().fold(f64::NEG_INFINITY, js_max) + padding,
        bottom: ys.into_iter().fold(f64::NEG_INFINITY, js_max) + padding,
    }
}
fn nudge(mut p: Placement, b: Bounds, o: Options) -> Option<(Placement, Bounds)> {
    if b.right - b.left > o.width - o.margin * 2.0 || b.bottom - b.top > o.height - o.margin * 2.0 {
        return None;
    }
    let dx = if b.left < o.margin {
        o.margin - b.left
    } else if b.right > o.width - o.margin {
        o.width - o.margin - b.right
    } else {
        0.0
    };
    let dy = if b.top < o.margin {
        o.margin - b.top
    } else if b.bottom > o.height - o.margin {
        o.height - o.margin - b.bottom
    } else {
        0.0
    };
    p.x += dx;
    p.y += dy;
    Some((
        p,
        Bounds {
            left: b.left + dx,
            top: b.top + dy,
            right: b.right + dx,
            bottom: b.bottom + dy,
        },
    ))
}
fn intersects(l: Bounds, r: Bounds) -> bool {
    l.left < r.right && l.right > r.left && l.top < r.bottom && l.bottom > r.top
}
fn overlap(l: Bounds, r: Bounds) -> f64 {
    let width = js_min(l.right, r.right) - js_max(l.left, r.left);
    let height = js_min(l.bottom, r.bottom) - js_max(l.top, r.top);
    if width > 0.0 && height > 0.0 {
        width * height
    } else {
        0.0
    }
}
struct CollisionGrid {
    cell_size: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    bounds: Vec<Bounds>,
}
impl CollisionGrid {
    fn keys(&self, b: Bounds) -> Vec<(i64, i64)> {
        let extents = [b.left, b.right, b.top, b.bottom].map(|v| (v / self.cell_size).floor());
        // NaN bounds visited zero cells in the original JS for-loops. Infinity
        // cannot represent a finite screen cell and is never indexed.
        if !extents.iter().all(|v| v.is_finite()) {
            return Vec::new();
        }
        let [left, right, top, bottom] = extents.map(|v| v as i64);
        let mut keys = Vec::new();
        for x in left..=right {
            for y in top..=bottom {
                keys.push((x, y));
            }
        }
        keys
    }
    fn nearby(&self, b: Bounds) -> Vec<Bounds> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for key in self.keys(b) {
            for &index in self.cells.get(&key).into_iter().flatten() {
                if seen.insert(index) {
                    found.push(self.bounds[index]);
                }
            }
        }
        found
    }
    fn insert(&mut self, b: Bounds) {
        let index = self.bounds.len();
        for key in self.keys(b) {
            self.cells.entry(key).or_default().push(index);
        }
        self.bounds.push(b);
    }
}
pub fn layout_labels(requests: &[MeasuredLabel], options: Options) -> Vec<PlacedLabel> {
    let mut grid = CollisionGrid {
        cell_size: options.cell_size,
        cells: HashMap::new(),
        bounds: Vec::new(),
    };
    let mut placed = Vec::new();
    for request in requests {
        if request.line_count == 0 {
            continue;
        }
        let orientations = if request.orientation == Orientation::Auto {
            let horizontal = if request.anchor.x > options.width / 2.0 {
                [Orientation::Left, Orientation::Right]
            } else {
                [Orientation::Right, Orientation::Left]
            };
            let vertical = if request.anchor.y < options.height * 0.3 {
                [Orientation::Below, Orientation::Above]
            } else {
                [Orientation::Above, Orientation::Below]
            };
            vec![
                horizontal[0],
                horizontal[1],
                vertical[0],
                vertical[1],
                Orientation::Vertical,
            ]
        } else {
            vec![request.orientation]
        };
        let candidates: Vec<_> = orientations
            .into_iter()
            .filter_map(|orientation| {
                let p = placement(request.anchor, orientation);
                nudge(
                    p,
                    screen_bounds(
                        p,
                        local_bounds(p, request.text_width, request.line_count, options),
                        options.padding,
                    ),
                    options,
                )
            })
            .collect();
        let mut candidate = candidates
            .iter()
            .find(|(_, b)| !grid.nearby(*b).into_iter().any(|item| intersects(*b, item)))
            .copied();
        if candidate.is_none() && request.required {
            let mut scored: Vec<_> = candidates
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let score = grid
                        .nearby(item.1)
                        .into_iter()
                        .fold(0.0, |total, b| total + overlap(item.1, b));
                    (index, score, *item)
                })
                .collect();
            scored.sort_by(|l, r| numeric_order(l.1, r.1).then_with(|| l.0.cmp(&r.0)));
            candidate = scored.first().map(|(_, _, item)| *item);
        }
        if let Some((placement, bounds)) = candidate {
            grid.insert(bounds);
            placed.push(PlacedLabel {
                input_index: request.input_index,
                placement,
                bounds,
            });
        }
    }
    placed
}
#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> Options {
        Options {
            width: 300.0,
            height: 180.0,
            line_height: 13.0,
            font_height: 11.0,
            padding: 3.0,
            margin: 4.0,
            cell_size: 64.0,
        }
    }
    fn label(index: usize, x: f64, y: f64, orientation: Orientation) -> MeasuredLabel {
        MeasuredLabel {
            input_index: index,
            anchor: Point { x, y },
            orientation,
            required: false,
            text_width: 30.0,
            line_count: 1,
        }
    }
    #[test]
    fn explicit_orientation_placements_preserved() {
        let a = Point { x: 100.0, y: 80.0 };
        assert_eq!(placement(a, Orientation::Right).x, 106.0);
        assert_eq!(placement(a, Orientation::Left).align, Align::Right);
        assert_eq!(placement(a, Orientation::Above).y, 72.0);
        assert_eq!(placement(a, Orientation::Below).baseline, Baseline::Top);
        assert_eq!(
            placement(a, Orientation::Vertical).rotation_rad,
            -std::f64::consts::PI / 2.0
        );
    }
    #[test]
    fn priority_anchor_collation_and_stable_input_order_are_preserved() {
        let requests = [
            OrderRequest {
                anchor: Point { x: 100.0, y: 80.0 },
                priority: 0.0,
                collation_rank: 0,
                enabled: true,
            },
            OrderRequest {
                anchor: Point { x: 100.0, y: 80.0 },
                priority: 100.0,
                collation_rank: 1,
                enabled: true,
            },
            OrderRequest {
                anchor: Point { x: 100.0, y: 80.0 },
                priority: 0.0,
                collation_rank: 0,
                enabled: true,
            },
            OrderRequest {
                anchor: Point { x: 100.0, y: 80.0 },
                priority: 0.0,
                collation_rank: 2,
                enabled: true,
            },
        ];
        assert_eq!(label_order(&requests, 300.0, 180.0), [1, 0, 2, 3]);
        let mut selected = label(1, 100.0, 80.0, Orientation::Right);
        selected.required = true;
        let placed = layout_labels(
            &[selected, label(0, 100.0, 80.0, Orientation::Right)],
            options(),
        );
        assert_eq!(
            placed.iter().map(|l| l.input_index).collect::<Vec<_>>(),
            [1]
        );
    }
    #[test]
    fn auto_tries_other_side_before_culling() {
        let labels = layout_labels(
            &[
                label(0, 100.0, 80.0, Orientation::Auto),
                label(1, 100.0, 80.0, Orientation::Auto),
            ],
            options(),
        );
        assert_eq!(labels.len(), 2);
        assert!(labels[0].placement.x > 100.0);
        assert!(labels[1].placement.x < 100.0);
        assert!(!intersects(labels[0].bounds, labels[1].bounds));
    }
    #[test]
    fn dense_vertical_run_is_nonoverlapping() {
        let requests: Vec<_> = (0..80)
            .map(|i| {
                let mut r = label(i, 20.0 + i as f64 * 3.0, 100.0, Orientation::Vertical);
                r.text_width = 48.0;
                r.line_count = 2;
                r
            })
            .collect();
        let labels = layout_labels(&requests, options());
        assert!(labels.len() > 1 && labels.len() < 20);
        for (i, left) in labels.iter().enumerate() {
            for right in &labels[i + 1..] {
                assert!(!intersects(left.bounds, right.bounds));
            }
        }
    }
    #[test]
    fn edge_labels_nudge_inside_canvas_and_required_overlap_keeps_first_tie() {
        let edge = layout_labels(&[label(0, 299.0, 2.0, Orientation::Auto)], options())[0];
        assert!(
            edge.bounds.left >= 4.0
                && edge.bounds.top >= 4.0
                && edge.bounds.right <= 296.0
                && edge.bounds.bottom <= 176.0
        );
        let first = label(0, 100.0, 80.0, Orientation::Right);
        let mut second = first;
        second.input_index = 1;
        second.required = true;
        assert_eq!(layout_labels(&[first, second], options()).len(), 2);
    }
    #[test]
    fn nonfinite_host_metrics_preserve_nan_and_infinite_outcomes() {
        let mut r = label(0, 100.0, 80.0, Orientation::Right);
        r.text_width = f64::NAN;
        let labels = layout_labels(&[r], options());
        assert_eq!(labels.len(), 1);
        assert!(labels[0].bounds.left.is_nan());
        r.text_width = f64::INFINITY;
        assert!(layout_labels(&[r], options()).is_empty());
        r.text_width = 30.0;
        r.anchor.x = f64::NAN;
        assert!(layout_labels(&[r], options())[0].bounds.right.is_nan());
    }
}
