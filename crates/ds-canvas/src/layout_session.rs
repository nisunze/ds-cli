//! Retained free-layout interaction over shared Profile-derived primitives.
use crate::{Bounds, HostSurface, Point, Projection, Rect, Viewport, free_layout, viewport};

pub struct LayoutSession {
    grid: f64,
    minimum: f64,
    rects: Vec<Rect>,
    bounds: Bounds,
    host: HostSurface,
    viewport: Viewport,
    projection: Projection,
}
impl LayoutSession {
    pub fn new(bounds: &[f64], rects: &[f64], grid: f64, minimum: f64) -> Result<Self, String> {
        if bounds.len() != 4
            || !rects.len().is_multiple_of(4)
            || rects.len() > 4096
            || !bounds.iter().all(|v| v.is_finite())
            || !grid.is_finite()
            || grid < 0.
            || !minimum.is_finite()
            || minimum <= 0.
        {
            return Err("layout requires finite bounds, at most 1024 rectangles, nonnegative grid and positive minimum".into());
        }
        let bounds = Bounds {
            min_x: bounds[0],
            min_y: bounds[1],
            max_x: bounds[2],
            max_y: bounds[3],
        };
        if bounds.width() < minimum || bounds.height() < minimum {
            return Err("layout bounds are too small or unordered".into());
        }
        let rects: Vec<Rect> = rects
            .chunks_exact(4)
            .map(|r| Rect {
                x: r[0],
                y: r[1],
                width: r[2],
                height: r[3],
            })
            .collect();
        if !rects.is_empty() {
            free_layout::place(
                &rects,
                bounds,
                free_layout::Drag {
                    index: 0,
                    handle: 0,
                    delta: Point::new(0., 0.),
                    grid: 0.,
                    tolerance: 0.,
                    minimum,
                },
            )?;
        }
        let host = HostSurface::default();
        let viewport = Viewport::default();
        let projection = viewport::projection(&bounds, &host, &viewport);
        Ok(Self {
            grid,
            minimum,
            rects,
            bounds,
            host,
            viewport,
            projection,
        })
    }
    pub fn project(&mut self, width: f64, height: f64, dpr: f64) -> Result<Vec<f64>, String> {
        self.host = HostSurface::new(width, height, dpr).map_err(|e| e.to_string())?;
        self.refresh();
        Ok(self.camera())
    }
    pub fn camera(&self) -> Vec<f64> {
        vec![
            self.projection.scale,
            self.projection.offset_x,
            self.projection.offset_y,
            self.projection.device_pixel_ratio,
            self.viewport.zoom,
            self.viewport.pan_x,
            self.viewport.pan_y,
        ]
    }
    pub fn fit(&mut self) -> Vec<f64> {
        self.viewport = Viewport::fitted();
        self.refresh();
        self.camera()
    }
    pub fn pan(&mut self, dx: f64, dy: f64) -> Result<Vec<f64>, String> {
        self.viewport = viewport::pan(&self.viewport, dx, dy);
        self.refresh();
        Ok(self.camera())
    }
    pub fn zoom(&mut self, x: f64, y: f64, factor: f64) -> Result<Vec<f64>, String> {
        self.viewport = viewport::zoom_at(
            &self.bounds,
            &self.host,
            &self.viewport,
            Point::new(x, y),
            factor,
        );
        self.refresh();
        Ok(self.camera())
    }
    pub fn rectangles(&self) -> Vec<f64> {
        self.rects
            .iter()
            .flat_map(|r| [r.x, r.y, r.width, r.height])
            .collect()
    }
    pub fn handles(&self, index: usize) -> Result<Vec<f64>, String> {
        let rect = *self.rects.get(index).ok_or("unknown element")?;
        Ok(free_layout::handles(rect)
            .iter()
            .flat_map(|p| [p.x, p.y])
            .collect())
    }
    pub fn hit(&self, x: f64, y: f64, selected: i32) -> i32 {
        free_layout::hit(
            &self.rects,
            usize::try_from(selected).ok(),
            Point::new(x, y),
            &self.projection,
            6.,
        )
    }
    pub fn drag(
        &self,
        index: usize,
        handle: u8,
        pointer: &[f64],
        snap: bool,
    ) -> Result<Vec<f64>, String> {
        if pointer.len() != 4 {
            return Err("pointer buffer requires start/current XY pairs".into());
        }
        let start = self
            .projection
            .unproject(Point::new(pointer[0], pointer[1]));
        let point = self
            .projection
            .unproject(Point::new(pointer[2], pointer[3]));
        free_layout::place(
            &self.rects,
            self.bounds,
            free_layout::Drag {
                index,
                handle,
                delta: Point::new(point.x - start.x, point.y - start.y),
                grid: if snap { self.grid } else { 0. },
                tolerance: if snap { 6. / self.projection.scale } else { 0. },
                minimum: self.minimum,
            },
        )
        .map(|p| p.buffer())
    }
    /// Only pointer-up commits canonical geometry; host never calculates a rect.
    pub fn apply_rect(&mut self, index: usize, rect: &[f64]) -> Result<Vec<f64>, String> {
        if rect.len() != 4 || index >= self.rects.len() {
            return Err("invalid rectangle patch".into());
        }
        let mut next = self.rects.clone();
        next[index] = Rect {
            x: rect[0],
            y: rect[1],
            width: rect[2],
            height: rect[3],
        };
        free_layout::place(
            &next,
            self.bounds,
            free_layout::Drag {
                index,
                handle: 0,
                delta: Point::new(0., 0.),
                grid: 0.,
                tolerance: 0.,
                minimum: self.minimum,
            },
        )?;
        self.rects = next;
        Ok(rect.to_vec())
    }
}
impl LayoutSession {
    fn refresh(&mut self) {
        self.projection = viewport::projection(&self.bounds, &self.host, &self.viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_layout_interaction_uses_css_pixels_and_commits_only_valid_rects() {
        let mut s =
            LayoutSession::new(&[0., 0., 100., 100.], &[20., 10., 30., 20.], 1., 2.).unwrap();
        let camera = s.project(124., 124., 2.).unwrap();
        assert_eq!(&camera[..3], &[1., 12., 12.]);
        assert_eq!(&s.project(124., 124., 3.).unwrap()[..3], &camera[..3]);
        assert_eq!(s.hit(32., 22., -1), 0);
        assert_eq!(s.hit(32., 22., 0), 1);
        let preview = s.drag(0, 0, &[40., 30., 52., 36.], false).unwrap();
        assert_eq!(&preview[..4], &[32., 16., 30., 20.]);
        assert_eq!(s.rectangles(), vec![20., 10., 30., 20.]);
        s.apply_rect(0, &preview[..4]).unwrap();
        let held = s.rectangles();
        assert!(s.apply_rect(0, &[200., 0., 10., 10.]).is_err());
        assert_eq!(s.rectangles(), held);
    }
}
