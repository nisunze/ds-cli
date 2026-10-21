//! The free surface of a sheet as rectangles a schedule can be placed in.
//!
//! What is left of the printable area after the fixed furniture is an
//! irregular region. Two views of it drive placement: the **maximal free
//! rectangles** (every largest axis-aligned empty rectangle, for a person or
//! an agent choosing where to put a panel), and the **skyline** from a corner
//! — a series of stepping rectangles, one per adjacent panel column, each as
//! tall as the free space in its column touching the anchored edge. A
//! workbook schedule is split across those steps: each panel takes as many
//! rows as its step can hold, so the schedule fills the irregular surface
//! instead of assuming panels of equal height. The engine offers these
//! splits to its packing search beside the balanced ones; nothing here moves
//! a frame by itself.
use serde::{Deserialize, Serialize};

pub type Rect = [f64; 4];

/// The sheet corner a stack of panels grows from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub const CORNERS: [Corner; 4] = [
    Corner::BottomRight,
    Corner::BottomLeft,
    Corner::TopRight,
    Corner::TopLeft,
];

const EPS: f64 = 1e-6;
/// The most free rectangles reported; the largest come first.
pub const MAX_FREE_RECTANGLES: usize = 32;

fn overlaps(a: Rect, b: Rect, gap: f64) -> bool {
    a[0] < b[0] + b[2] + gap - EPS
        && a[0] + a[2] + gap > b[0] + EPS
        && a[1] < b[1] + b[3] + gap - EPS
        && a[1] + a[3] + gap > b[1] + EPS
}

fn valid(area: Rect, obstacles: &[Rect], gap: f64) -> Result<(), String> {
    if area.iter().any(|v| !v.is_finite()) || area[2] <= 0. || area[3] <= 0. {
        return Err("the placement area must be a finite rectangle with size".into());
    }
    if obstacles.len() > 256
        || obstacles
            .iter()
            .any(|r| r.iter().any(|v| !v.is_finite()) || r[2] < 0. || r[3] < 0.)
    {
        return Err("placement obstacles are at most 256 finite rectangles".into());
    }
    if !gap.is_finite() || !(0. ..=50.).contains(&gap) {
        return Err("the placement gap is 0 through 50 mm".into());
    }
    Ok(())
}

/// Every maximal empty rectangle of `area` around the obstacles (each grown
/// by `gap`), largest first, at most [`MAX_FREE_RECTANGLES`].
pub fn free_rectangles(area: Rect, obstacles: &[Rect], gap: f64) -> Result<Vec<Rect>, String> {
    valid(area, obstacles, gap)?;
    let [ax, ay, aw, ah] = area;
    let blocked: Vec<Rect> = obstacles
        .iter()
        .map(|r| [r[0] - gap, r[1] - gap, r[2] + 2. * gap, r[3] + 2. * gap])
        .filter(|r| overlaps(*r, area, 0.))
        .collect();
    let mut xs = vec![ax, ax + aw];
    let mut ys = vec![ay, ay + ah];
    for r in &blocked {
        xs.extend([r[0], r[0] + r[2]]);
        ys.extend([r[1], r[1] + r[3]]);
    }
    let clamp = |v: &mut Vec<f64>, lo: f64, hi: f64| {
        v.retain(|c| *c >= lo - EPS && *c <= hi + EPS);
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| (*a - *b).abs() < EPS);
    };
    clamp(&mut xs, ax, ax + aw);
    clamp(&mut ys, ay, ay + ah);
    let empty = |r: Rect| !blocked.iter().any(|b| overlaps(r, *b, 0.));
    let mut found: Vec<Rect> = Vec::new();
    for (i, &x0) in xs.iter().enumerate() {
        for &x1 in &xs[i + 1..] {
            for (j, &y0) in ys.iter().enumerate() {
                for &y1 in &ys[j + 1..] {
                    let r = [x0, y0, x1 - x0, y1 - y0];
                    if r[2] < EPS || r[3] < EPS || !empty(r) {
                        continue;
                    }
                    // Maximal: it cannot grow to a neighbouring grid line on any side.
                    let grow = |candidate: Rect| {
                        candidate[2] > EPS && candidate[3] > EPS && empty(candidate)
                    };
                    let left = xs[..i]
                        .last()
                        .is_some_and(|&px| grow([px, y0, x1 - px, y1 - y0]));
                    let right = xs
                        .iter()
                        .find(|&&nx| nx > x1 + EPS)
                        .is_some_and(|&nx| grow([x0, y0, nx - x0, y1 - y0]));
                    let up = ys[..j]
                        .last()
                        .is_some_and(|&py| grow([x0, py, x1 - x0, y1 - py]));
                    let down = ys
                        .iter()
                        .find(|&&ny| ny > y1 + EPS)
                        .is_some_and(|&ny| grow([x0, y0, x1 - x0, ny - y0]));
                    if left || right || up || down {
                        continue;
                    }
                    if !found
                        .iter()
                        .any(|f| f.iter().zip(r.iter()).all(|(a, b)| (a - b).abs() < EPS))
                    {
                        found.push(r);
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| {
        (b[2] * b[3])
            .total_cmp(&(a[2] * a[3]))
            .then(b[2].total_cmp(&a[2]))
            .then(a[0].total_cmp(&b[0]))
            .then(a[1].total_cmp(&b[1]))
    });
    found.truncate(MAX_FREE_RECTANGLES);
    Ok(found)
}

/// The stepping rectangles from `corner`: for panel columns of `panel_w`
/// stepping inward along the anchored horizontal edge, the height of the
/// free space in each column touching that edge (obstacles grown by `gap`).
/// Columns the corner furniture blocks entirely are skipped until the first
/// usable one — a stack sits beside a title block, not on it — and the
/// stack ends at the next blocked column, because panels are contiguous.
pub fn skyline(
    area: Rect,
    obstacles: &[Rect],
    gap: f64,
    corner: Corner,
    panel_w: f64,
    max_panels: usize,
) -> Result<Vec<Rect>, String> {
    valid(area, obstacles, gap)?;
    if !panel_w.is_finite() || panel_w <= 0. || max_panels == 0 || max_panels > 64 {
        return Err("a skyline needs a positive panel width and 1 through 64 panels".into());
    }
    let [ax, ay, aw, ah] = area;
    let blocked: Vec<Rect> = obstacles
        .iter()
        .map(|r| [r[0] - gap, r[1] - gap, r[2] + 2. * gap, r[3] + 2. * gap])
        .collect();
    let mut steps = Vec::new();
    let columns = (aw / panel_w).floor() as usize;
    for k in 0..columns {
        if steps.len() == max_panels {
            break;
        }
        let x = match corner {
            Corner::TopLeft | Corner::BottomLeft => ax + k as f64 * panel_w,
            Corner::TopRight | Corner::BottomRight => ax + aw - (k + 1) as f64 * panel_w,
        };
        if x < ax - EPS || x + panel_w > ax + aw + EPS {
            break;
        }
        let column = [x, ay, panel_w, ah];
        // The free extent touching the anchored edge: up to the nearest
        // obstacle in this column on the far side.
        let height = match corner {
            Corner::TopLeft | Corner::TopRight => {
                let limit = blocked
                    .iter()
                    .filter(|b| overlaps(**b, column, 0.))
                    .map(|b| b[1])
                    .fold(ay + ah, f64::min);
                limit - ay
            }
            Corner::BottomLeft | Corner::BottomRight => {
                let limit = blocked
                    .iter()
                    .filter(|b| overlaps(**b, column, 0.))
                    .map(|b| b[1] + b[3])
                    .fold(ay, f64::max);
                ay + ah - limit
            }
        };
        if height <= EPS {
            if steps.is_empty() {
                continue;
            }
            break;
        }
        let y = match corner {
            Corner::TopLeft | Corner::TopRight => ay,
            _ => ay + ah - height,
        };
        steps.push([x, y, panel_w, height]);
    }
    Ok(steps)
}

/// Rows per adjacent panel that fill the steps in order: each takes what its
/// height holds (`key_mm` above the rows, `row_mm` per row, two rows of
/// heading and slack) until every row is placed. `None` when the steps
/// cannot hold them all.
pub fn panel_rows(
    steps: &[Rect],
    key_mm: f64,
    row_mm: f64,
    total_rows: usize,
) -> Option<Vec<usize>> {
    if row_mm.is_nan() || row_mm <= 0. || !key_mm.is_finite() || total_rows == 0 {
        return None;
    }
    let mut counts = Vec::new();
    let mut left = total_rows;
    for step in steps {
        if left == 0 {
            break;
        }
        let capacity = (((step[3] - key_mm) / row_mm).floor() as isize - 2).max(0) as usize;
        if capacity == 0 {
            continue;
        }
        let take = capacity.min(left);
        counts.push(take);
        left -= take;
    }
    (left == 0).then_some(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_rectangles_are_maximal_and_largest_first() {
        // A 100 × 100 sheet with a 40 × 40 block in the lower right corner.
        let free = free_rectangles([0., 0., 100., 100.], &[[60., 60., 40., 40.]], 0.).unwrap();
        assert_eq!(free.len(), 2);
        // Equal areas: the wider one first.
        assert_eq!(free[0], [0., 0., 100., 60.]);
        assert_eq!(free[1], [0., 0., 60., 100.]);
        // The gap grows the obstacle.
        let free = free_rectangles([0., 0., 100., 100.], &[[60., 60., 40., 40.]], 5.).unwrap();
        assert_eq!(free[0], [0., 0., 100., 55.]);
        assert_eq!(free[1], [0., 0., 55., 100.]);
        // Nothing blocked: the area itself.
        assert_eq!(
            free_rectangles([1., 1., 10., 10.], &[], 0.).unwrap(),
            vec![[1., 1., 10., 10.]]
        );
        assert!(free_rectangles([0., 0., 0., 10.], &[], 0.).is_err());
    }

    #[test]
    fn the_skyline_steps_inward_from_the_corner_and_stops_at_a_wall() {
        // A small title block in the lower-right corner and a wider legend above it.
        let fixed = [[80., 85., 20., 15.], [70., 50., 30., 25.]];
        let steps = skyline(
            [0., 0., 100., 100.],
            &fixed,
            0.,
            Corner::BottomRight,
            20.,
            4,
        )
        .unwrap();
        // The corner column is the title block's: skipped. The next column
        // is free below the legend (25 mm), then full-height columns.
        assert_eq!(
            steps,
            [
                [60., 75., 20., 25.],
                [40., 0., 20., 100.],
                [20., 0., 20., 100.],
                [0., 0., 20., 100.]
            ]
        );
        let steps = skyline([0., 0., 100., 100.], &fixed, 0., Corner::BottomLeft, 30., 8).unwrap();
        // Two full columns; the third (x 60..90) meets the title block at the
        // bottom edge and ends the contiguous stack.
        assert_eq!(steps, [[0., 0., 30., 100.], [30., 0., 30., 100.]]);
        let steps = skyline([0., 0., 100., 100.], &fixed, 0., Corner::TopRight, 20., 8).unwrap();
        assert_eq!(steps[0], [80., 0., 20., 50.]);
        assert_eq!(steps[1], [60., 0., 20., 50.]);
        assert_eq!(steps[2], [40., 0., 20., 100.]);
        let wall = [[0., 0., 100., 100.]];
        assert!(
            skyline([0., 0., 100., 100.], &wall, 0., Corner::TopLeft, 10., 4)
                .unwrap()
                .is_empty()
        );
        assert!(skyline([0., 0., 100., 100.], &[], 0., Corner::TopLeft, 0., 4).is_err());
        // The gap grows the furniture: the column beside the title block is
        // now within 5 mm of it and the stack starts one column further in.
        let steps = skyline(
            [0., 0., 100., 100.],
            &fixed,
            5.,
            Corner::BottomRight,
            20.,
            2,
        )
        .unwrap();
        assert_eq!(steps, [[40., 0., 20., 100.], [20., 0., 20., 100.]]);
    }

    #[test]
    fn panels_fill_the_steps_in_order() {
        let steps = [
            [80., 0., 20., 50.],
            [60., 0., 20., 50.],
            [40., 0., 20., 100.],
        ];
        // 4 mm rows, 6 mm key: a 50 mm step holds floor(44/4)-2 = 9 rows, a 100 mm one 21.
        assert_eq!(panel_rows(&steps, 6., 4., 20), Some(vec![9, 9, 2]));
        assert_eq!(panel_rows(&steps, 6., 4., 9), Some(vec![9]));
        assert_eq!(panel_rows(&steps, 6., 4., 40), None);
        assert_eq!(panel_rows(&[], 6., 4., 1), None);
        assert_eq!(panel_rows(&steps, 6., 0., 1), None);
    }
}
