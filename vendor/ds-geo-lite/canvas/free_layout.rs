//! Free-placement rectangles for page, paper and other Canvas2D editors.
//! Reuses the Profile camera and rectangle primitives without importing graph
//! cells, ports or domain state. Hot interaction results are numeric buffers.
use crate::{Bounds, Point, Projection, Rect};

pub fn handles(rect: Rect) -> [Point; 8] {
    let cx = rect.center().x;
    let cy = rect.center().y;
    [
        Point::new(rect.x, rect.y),
        Point::new(cx, rect.y),
        Point::new(rect.max_x(), rect.y),
        Point::new(rect.max_x(), cy),
        Point::new(rect.max_x(), rect.max_y()),
        Point::new(cx, rect.max_y()),
        Point::new(rect.x, rect.max_y()),
        Point::new(rect.x, cy),
    ]
}
/// -1 means no hit; otherwise index * 16 + handle, where 0 is body and 1..8
/// follows clockwise handles from the upper-left. Selected handles win.
pub fn hit(
    rects: &[Rect],
    selected: Option<usize>,
    screen: Point,
    projection: &Projection,
    radius_px: f64,
) -> i32 {
    if !screen.is_finite() || !radius_px.is_finite() || radius_px < 0. {
        return -1;
    }
    let world = projection.unproject(screen);
    if let Some(index) = selected.filter(|i| *i < rects.len()) {
        for (handle, point) in handles(rects[index]).iter().enumerate() {
            if projection.project(*point).distance_to(screen) <= radius_px {
                return (index * 16 + handle + 1) as i32;
            }
        }
    }
    rects
        .iter()
        .rposition(|r| r.contains(world))
        .map_or(-1, |i| (i * 16) as i32)
}
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub rect: Rect,
    pub guide_x: Option<f64>,
    pub guide_y: Option<f64>,
    pub clamped: bool,
}
impl Placement {
    pub fn buffer(&self) -> Vec<f64> {
        vec![
            self.rect.x,
            self.rect.y,
            self.rect.width,
            self.rect.height,
            self.guide_x.unwrap_or(f64::NAN),
            self.guide_y.unwrap_or(f64::NAN),
            if self.clamped { 1. } else { 0. },
        ]
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub index: usize,
    pub handle: u8,
    pub delta: Point,
    pub grid: f64,
    pub tolerance: f64,
    pub minimum: f64,
}
fn snap(value: f64, targets: &[f64], grid: f64, tolerance: f64) -> (f64, Option<f64>) {
    let mut best = value;
    let mut gap = tolerance;
    let mut guide = None;
    for target in targets {
        let d = (*target - value).abs();
        if d < gap {
            best = *target;
            gap = d;
            guide = Some(*target)
        }
    }
    if guide.is_none() && grid > 0. {
        let target = (value / grid).round() * grid;
        if (target - value).abs() <= tolerance {
            best = target;
        }
    }
    (best, guide)
}
/// Delta is always measured from the original authored rectangle. Repeated
/// pointer previews never accumulate rounding or mutate the durable model.
pub fn place(rects: &[Rect], bounds: Bounds, intent: Drag) -> Result<Placement, String> {
    let original = *rects.get(intent.index).ok_or("unknown canvas rectangle")?;
    if intent.handle > 8
        || !intent.delta.is_finite()
        || ![
            intent.grid,
            intent.tolerance,
            intent.minimum,
            bounds.min_x,
            bounds.min_y,
            bounds.max_x,
            bounds.max_y,
        ]
        .iter()
        .all(|v| v.is_finite())
        || intent.grid < 0.
        || intent.tolerance < 0.
        || intent.minimum <= 0.
        || bounds.width() < intent.minimum
        || bounds.height() < intent.minimum
    {
        return Err("invalid canvas placement intent".into());
    }
    if rects.iter().any(|r| {
        ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
            || r.width < intent.minimum
            || r.height < intent.minimum
            || r.x < bounds.min_x
            || r.y < bounds.min_y
            || r.max_x() > bounds.max_x
            || r.max_y() > bounds.max_y
    }) {
        return Err("canvas rectangles must fit the finite scene bounds".into());
    }
    let mut xs = vec![
        bounds.min_x,
        (bounds.min_x + bounds.max_x) / 2.,
        bounds.max_x,
    ];
    let mut ys = vec![
        bounds.min_y,
        (bounds.min_y + bounds.max_y) / 2.,
        bounds.max_y,
    ];
    for (index, rect) in rects.iter().enumerate() {
        if index != intent.index {
            xs.extend([rect.x, rect.center().x, rect.max_x()]);
            ys.extend([rect.y, rect.center().y, rect.max_y()]);
        }
    }
    let mut next = original;
    let mut resized_clamped = false;
    let mut gx = None;
    let mut gy = None;
    if intent.handle == 0 {
        next.x += intent.delta.x;
        next.y += intent.delta.y;
        let mut dx = None::<f64>;
        let mut dy = None::<f64>;
        for offset in [0., next.width / 2., next.width] {
            let (value, guide) = snap(next.x + offset, &xs, 0., intent.tolerance);
            let correction = value - next.x - offset;
            if guide.is_some() && dx.is_none_or(|held| correction.abs() < held.abs()) {
                dx = Some(correction);
                gx = guide;
            }
        }
        for offset in [0., next.height / 2., next.height] {
            let (value, guide) = snap(next.y + offset, &ys, 0., intent.tolerance);
            let correction = value - next.y - offset;
            if guide.is_some() && dy.is_none_or(|held| correction.abs() < held.abs()) {
                dy = Some(correction);
                gy = guide;
            }
        }
        next.x += dx.unwrap_or_else(|| snap(next.x, &[], intent.grid, intent.tolerance).0 - next.x);
        next.y += dy.unwrap_or_else(|| snap(next.y, &[], intent.grid, intent.tolerance).0 - next.y);
    } else {
        let left = [1, 7, 8].contains(&intent.handle);
        let right = [3, 4, 5].contains(&intent.handle);
        let top = [1, 2, 3].contains(&intent.handle);
        let bottom = [5, 6, 7].contains(&intent.handle);
        if left {
            let (x, g) = snap(
                original.x + intent.delta.x,
                &xs,
                intent.grid,
                intent.tolerance,
            );
            gx = g;
            next.x = x.clamp(bounds.min_x, original.max_x() - intent.minimum);
            resized_clamped |= next.x != x;
            next.width = original.max_x() - next.x;
        }
        if right {
            let (x, g) = snap(
                original.max_x() + intent.delta.x,
                &xs,
                intent.grid,
                intent.tolerance,
            );
            gx = g;
            next.width = x.clamp(original.x + intent.minimum, bounds.max_x) - original.x;
            resized_clamped |= next.max_x() != x;
        }
        if top {
            let (y, g) = snap(
                original.y + intent.delta.y,
                &ys,
                intent.grid,
                intent.tolerance,
            );
            gy = g;
            next.y = y.clamp(bounds.min_y, original.max_y() - intent.minimum);
            resized_clamped |= next.y != y;
            next.height = original.max_y() - next.y;
        }
        if bottom {
            let (y, g) = snap(
                original.max_y() + intent.delta.y,
                &ys,
                intent.grid,
                intent.tolerance,
            );
            gy = g;
            next.height = y.clamp(original.y + intent.minimum, bounds.max_y) - original.y;
            resized_clamped |= next.max_y() != y;
        }
    }
    let wanted = next;
    next.x = next.x.clamp(bounds.min_x, bounds.max_x - next.width);
    next.y = next.y.clamp(bounds.min_y, bounds.max_y - next.height);
    let clamped = resized_clamped || next != wanted;
    if clamped {
        gx = None;
        gy = None;
    }
    Ok(Placement {
        rect: next,
        guide_x: gx,
        guide_y: gy,
        clamped,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            width: 20.,
            height: 10.,
        }
    }
    fn bounds() -> Bounds {
        Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 100.,
            max_y: 100.,
        }
    }
    #[test]
    fn resize_reports_constraint_and_removes_unapplied_guides() {
        let original = Rect {
            x: 10.,
            y: 10.,
            width: 20.,
            height: 20.,
        };
        let result = place(
            &[original],
            Bounds {
                min_x: 0.,
                min_y: 0.,
                max_x: 100.,
                max_y: 100.,
            },
            Drag {
                index: 0,
                handle: 1,
                delta: Point::new(100., 100.),
                grid: 1.,
                tolerance: 6.,
                minimum: 5.,
            },
        )
        .unwrap();
        assert_eq!(
            result.rect,
            Rect {
                x: 25.,
                y: 25.,
                width: 5.,
                height: 5.
            }
        );
        assert!(result.clamped);
        assert_eq!((result.guide_x, result.guide_y), (None, None));
    }
    #[test]
    fn selected_handles_win_over_overlapping_bodies() {
        let projection = Projection {
            scale: 3.,
            offset_x: 12.,
            offset_y: 4.,
            device_pixel_ratio: 2.,
        };
        let rects = [rect(10., 10.), rect(10., 10.)];
        assert_eq!(
            hit(
                &rects,
                None,
                projection.project(Point::new(20., 15.)),
                &projection,
                6.
            ),
            16
        );
        assert_eq!(
            hit(
                &rects,
                Some(0),
                projection.project(Point::new(30., 20.)),
                &projection,
                6.
            ),
            5
        );
    }
    #[test]
    fn snapping_is_from_original_and_reports_alignment() {
        let rects = [rect(10., 10.), rect(50., 40.)];
        let intent = Drag {
            index: 0,
            handle: 0,
            delta: Point::new(19.7, 29.8),
            grid: 1.,
            tolerance: 0.6,
            minimum: 1.,
        };
        let result = place(&rects, bounds(), intent).unwrap();
        assert_eq!(result.rect, rect(30., 40.));
        assert_eq!(result.guide_x, Some(50.));
        assert_eq!(result.guide_y, Some(40.));
        assert_eq!(place(&rects, bounds(), intent).unwrap().rect, result.rect);
    }
    #[test]
    fn resizing_keeps_opposite_corner_and_minimum_size() {
        let rects = [rect(10., 10.)];
        let result = place(
            &rects,
            bounds(),
            Drag {
                index: 0,
                handle: 1,
                delta: Point::new(200., 200.),
                grid: 0.,
                tolerance: 0.,
                minimum: 2.,
            },
        )
        .unwrap();
        assert_eq!(
            result.rect,
            Rect {
                x: 28.,
                y: 18.,
                width: 2.,
                height: 2.
            }
        );
    }
    #[test]
    fn moving_cannot_escape_page() {
        let rects = [rect(10., 10.)];
        let result = place(
            &rects,
            bounds(),
            Drag {
                index: 0,
                handle: 0,
                delta: Point::new(-100., 200.),
                grid: 0.,
                tolerance: 0.,
                minimum: 1.,
            },
        )
        .unwrap();
        assert_eq!(result.rect, rect(0., 90.));
        assert!(result.clamped);
    }
}
