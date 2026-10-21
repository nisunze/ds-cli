//! Adapt the permanent Canvas primitives without introducing another camera.
use crate::model::{Bounds, Point, Screen, Viewport};
use ds_canvas::{viewport as core, Point as CanvasPoint};
pub struct Camera {
    pub bounds: Bounds,
    pub width: f64,
    pub height: f64,
    pub viewport: Viewport,
}
impl Camera {
    fn host(&self) -> core::HostSurface {
        core::HostSurface {
            width: self.width,
            height: self.height,
            device_pixel_ratio: 1.,
        }
    }
    fn view(&self) -> core::Viewport {
        core::Viewport {
            zoom: self.viewport.zoom,
            pan_x: self.viewport.pan_x,
            pan_y: self.viewport.pan_y,
        }
    }
    pub fn projection(&self) -> core::Projection {
        core::projection(&self.bounds.into(), &self.host(), &self.view())
    }
    pub fn project(&self, p: Point) -> Screen {
        let p = self
            .projection()
            .project(CanvasPoint::new(p.scene_x, p.scene_y));
        Screen { x: p.x, y: p.y }
    }
    pub fn unproject(&self, p: Screen) -> Point {
        let p = self.projection().unproject(CanvasPoint::new(p.x, p.y));
        Point::xy(p.x, p.y)
    }
    pub fn zoom(&self, p: Screen, factor: f64) -> Viewport {
        from(core::zoom_at(
            &self.bounds.into(),
            &self.host(),
            &self.view(),
            CanvasPoint::new(p.x, p.y),
            factor,
        ))
    }
    pub fn pan(&self, dx: f64, dy: f64) -> Viewport {
        from(core::pan(&self.view(), dx, dy))
    }
    pub fn center(&self, p: Point) -> Viewport {
        from(core::center_on(
            &self.bounds.into(),
            &self.host(),
            &self.view(),
            CanvasPoint::new(p.scene_x, p.scene_y),
        ))
    }
    pub fn rebase(&self, p: Option<Point>) -> Viewport {
        from(core::rebase_if_offscreen(
            &self.bounds.into(),
            &self.host(),
            &self.view(),
            p.map(|p| CanvasPoint::new(p.scene_x, p.scene_y)),
        ))
    }
    pub fn window(&self, a: Screen, b: Screen) -> Result<Viewport, String> {
        core::zoom_window(
            &self.bounds.into(),
            &self.host(),
            &self.view(),
            core::ZoomWindow {
                from: CanvasPoint::new(a.x, a.y),
                to: CanvasPoint::new(b.x, b.y),
            },
        )
        .map(from)
    }
}
fn from(v: core::Viewport) -> Viewport {
    Viewport {
        zoom: v.zoom,
        pan_x: v.pan_x,
        pan_y: v.pan_y,
    }
}
