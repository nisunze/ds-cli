//! Native form of Profile's established fit/pan/anchor-zoom primitives.
use crate::{Bounds, Point};
use serde::{Deserialize, Serialize};
pub const MIN_ZOOM: f64 = 0.2;
pub const MAX_ZOOM: f64 = 1_000_000.;
pub const FIT_PADDING_PX: f64 = 24.;
fn finite_pan(value: f64) -> f64 {
    if value.is_finite() { value } else { 0. }
}
pub fn clamp_zoom(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HostSurface {
    pub width: f64,
    pub height: f64,
    pub device_pixel_ratio: f64,
}
impl HostSurface {
    pub fn new(width: f64, height: f64, dpr: f64) -> Result<Self, String> {
        if ![width, height, dpr]
            .iter()
            .all(|v| v.is_finite() && *v > 0.)
        {
            return Err("canvas dimensions must be finite and positive".into());
        }
        Ok(Self {
            width,
            height,
            device_pixel_ratio: dpr,
        })
    }
}
impl Default for HostSurface {
    fn default() -> Self {
        Self {
            width: 1.,
            height: 1.,
            device_pixel_ratio: 1.,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
}
impl Viewport {
    pub const fn fitted() -> Self {
        Self {
            zoom: 1.,
            pan_x: 0.,
            pan_y: 0.,
        }
    }
}
impl Default for Viewport {
    fn default() -> Self {
        Self::fitted()
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Projection {
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub device_pixel_ratio: f64,
}
impl Projection {
    pub fn project(self, p: Point) -> Point {
        Point::new(
            self.offset_x + p.x * self.scale,
            self.offset_y + p.y * self.scale,
        )
    }
    pub fn unproject(self, p: Point) -> Point {
        Point::new(
            (p.x - self.offset_x) / self.scale,
            (p.y - self.offset_y) / self.scale,
        )
    }
}
pub fn projection(bounds: &Bounds, host: &HostSurface, viewport: &Viewport) -> Projection {
    let w = bounds.width().max(1.);
    let h = bounds.height().max(1.);
    let fit =
        ((host.width - FIT_PADDING_PX).max(1.) / w).min((host.height - FIT_PADDING_PX).max(1.) / h);
    let scale = (fit * clamp_zoom(viewport.zoom)).max(0.01);
    Projection {
        scale,
        offset_x: (host.width - w * scale) / 2. - bounds.min_x * scale + finite_pan(viewport.pan_x),
        offset_y: (host.height - h * scale) / 2. - bounds.min_y * scale
            + finite_pan(viewport.pan_y),
        device_pixel_ratio: host.device_pixel_ratio,
    }
}
pub fn pan(viewport: &Viewport, dx: f64, dy: f64) -> Viewport {
    Viewport {
        zoom: viewport.zoom,
        pan_x: finite_pan(viewport.pan_x) + finite_pan(dx),
        pan_y: finite_pan(viewport.pan_y) + finite_pan(dy),
    }
}
pub fn zoom_centered(viewport: &Viewport, factor: f64) -> Viewport {
    let zoom = clamp_zoom(viewport.zoom * if factor.is_finite() { factor } else { 1. });
    let ratio = zoom / clamp_zoom(viewport.zoom);
    Viewport {
        zoom,
        pan_x: finite_pan(viewport.pan_x) * ratio,
        pan_y: finite_pan(viewport.pan_y) * ratio,
    }
}
pub fn zoom_at(
    bounds: &Bounds,
    host: &HostSurface,
    viewport: &Viewport,
    anchor: Point,
    factor: f64,
) -> Viewport {
    let current = projection(bounds, host, viewport);
    let zoom = clamp_zoom(viewport.zoom * if factor.is_finite() { factor } else { 1. });
    if zoom == viewport.zoom {
        return Viewport { zoom, ..*viewport };
    }
    let world = current.unproject(anchor);
    let fitted = projection(
        bounds,
        host,
        &Viewport {
            zoom,
            pan_x: 0.,
            pan_y: 0.,
        },
    );
    Viewport {
        zoom,
        pan_x: anchor.x - fitted.offset_x - world.x * fitted.scale,
        pan_y: anchor.y - fitted.offset_y - world.y * fitted.scale,
    }
}
/// Center a scene coordinate without changing the current zoom.
pub fn center_on(
    bounds: &Bounds,
    host: &HostSurface,
    viewport: &Viewport,
    point: Point,
) -> Viewport {
    let fitted = projection(
        bounds,
        host,
        &Viewport {
            pan_x: 0.,
            pan_y: 0.,
            ..*viewport
        },
    );
    Viewport {
        zoom: clamp_zoom(viewport.zoom),
        pan_x: host.width / 2. - fitted.offset_x - point.x * fitted.scale,
        pan_y: host.height / 2. - fitted.offset_y - point.y * fitted.scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds() -> Bounds {
        Bounds {
            min_x: 100.,
            min_y: 20.,
            max_x: 1100.,
            max_y: 220.,
        }
    }
    fn host() -> HostSurface {
        HostSurface::new(1024., 424., 2.).unwrap()
    }
    #[test]
    fn profile_fit_fixture_is_unchanged() {
        let p = projection(&bounds(), &host(), &Viewport::fitted());
        assert_eq!(p.scale, 1.);
        assert_eq!(p.project(Point::new(100., 20.)), Point::new(12., 112.));
    }
    #[test]
    fn profile_cursor_anchor_is_unchanged() {
        let v = Viewport {
            zoom: 1.4,
            pan_x: -175.,
            pan_y: 42.,
        };
        let a = Point::new(730., 155.);
        let world = projection(&bounds(), &host(), &v).unproject(a);
        let next = zoom_at(&bounds(), &host(), &v, a, 1.5);
        assert!(
            projection(&bounds(), &host(), &next)
                .project(world)
                .distance_to(a)
                < 1e-9
        );
    }
    #[test]
    fn profile_invalid_pan_and_deep_zoom_contract() {
        assert_eq!(
            pan(
                &Viewport {
                    zoom: 1.,
                    pan_x: f64::NAN,
                    pan_y: 4.
                },
                f64::NAN,
                6.
            ),
            Viewport {
                zoom: 1.,
                pan_x: 0.,
                pan_y: 10.
            }
        );
        assert_eq!(clamp_zoom(f64::INFINITY), 1.);
        assert_eq!(
            zoom_centered(
                &Viewport {
                    zoom: MAX_ZOOM / 2.,
                    pan_x: 0.,
                    pan_y: 0.
                },
                10.
            )
            .zoom,
            MAX_ZOOM
        );
    }
}
