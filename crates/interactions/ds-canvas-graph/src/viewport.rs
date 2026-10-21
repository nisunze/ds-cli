//! World-to-window projection.
//!
//! The host owns the window: canvas size, device-pixel ratio, and the pointer
//! and wheel events that express intent. It does not own the transform. Screen
//! pixels, window size and zoom are a projection of the scene; they are never
//! canonical graph state, so nothing here can write back into a node position.
//!
//! The projection is the same fit-then-pan camera the reviewed Canvas2D
//! surfaces already use, restated as the authority rather than as one of two
//! implementations. A renderer applies it once:
//!
//! ```text
//! ctx.setTransform(scale, 0, 0, scale, offsetX, offsetY)
//! ```

use serde::{Deserialize, Serialize};

use crate::curve::Point;
use crate::error::{GeometryError, finite, finite_positive};
use crate::scene::Bounds;

pub const MIN_ZOOM: f64 = 0.2;
/// Effectively unbounded for engineering use while keeping float precision:
/// past ~1e10 the anchor math in `offset + world * scale` visibly degrades.
pub const MAX_ZOOM: f64 = 1.0e6;
pub const FIT_PADDING_PX: f64 = 24.0;
pub const MIN_SCALE: f64 = 0.01;

/// The host's window facts, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSurface {
    pub width: f64,
    pub height: f64,
    pub device_pixel_ratio: f64,
}

impl Default for HostSurface {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            device_pixel_ratio: 1.0,
        }
    }
}

impl HostSurface {
    pub fn new(width: f64, height: f64, device_pixel_ratio: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            width: finite_positive("host.width", width)?,
            height: finite_positive("host.height", height)?,
            device_pixel_ratio: finite_positive("host.devicePixelRatio", device_pixel_ratio)?,
        })
    }
}

/// User camera intent. `zoom` is relative to the fitted scale, so a scene that
/// changes size does not silently change what zoom 1 means.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::fitted()
    }
}

impl Viewport {
    /// The camera that shows the whole scene.
    pub const fn fitted() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }

    pub fn new(zoom: f64, pan_x: f64, pan_y: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            zoom: clamp_zoom(finite("viewport.zoom", zoom)?),
            pan_x: finite("viewport.panX", pan_x)?,
            pan_y: finite("viewport.panY", pan_y)?,
        })
    }
}

/// Zoom outside the supported band is clamped, not refused: it is a camera
/// preference, and every value still describes a drawable view. A *non-finite*
/// zoom is refused by [`Viewport::new`] before it reaches here.
pub fn clamp_zoom(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// The resolved world-to-window transform.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub device_pixel_ratio: f64,
}

impl Projection {
    pub fn project(&self, world: Point) -> Point {
        Point::new(
            self.offset_x + world.x * self.scale,
            self.offset_y + world.y * self.scale,
        )
    }

    pub fn unproject(&self, screen: Point) -> Point {
        Point::new(
            (screen.x - self.offset_x) / self.scale,
            (screen.y - self.offset_y) / self.scale,
        )
    }
}

/// Resolve the transform for one scene, window and camera.
pub fn projection(bounds: &Bounds, host: &HostSurface, viewport: &Viewport) -> Projection {
    let scene_width = bounds.width().max(1.0);
    let scene_height = bounds.height().max(1.0);
    let available_width = (host.width - FIT_PADDING_PX).max(1.0);
    let available_height = (host.height - FIT_PADDING_PX).max(1.0);
    let fit = (available_width / scene_width).min(available_height / scene_height);
    let scale = (fit * clamp_zoom(viewport.zoom)).max(MIN_SCALE);
    Projection {
        scale,
        offset_x: (host.width - scene_width * scale) / 2.0 - bounds.min_x * scale + viewport.pan_x,
        offset_y: (host.height - scene_height * scale) / 2.0 - bounds.min_y * scale
            + viewport.pan_y,
        device_pixel_ratio: host.device_pixel_ratio,
    }
}

/// Translate the camera by a screen-pixel delta.
pub fn pan(viewport: &Viewport, dx: f64, dy: f64) -> Result<Viewport, GeometryError> {
    Ok(Viewport {
        zoom: viewport.zoom,
        pan_x: viewport.pan_x + finite("pan.dx", dx)?,
        pan_y: viewport.pan_y + finite("pan.dy", dy)?,
    })
}

/// Zoom about the window centre, keeping the current centre stable.
pub fn zoom_centered(viewport: &Viewport, factor: f64) -> Result<Viewport, GeometryError> {
    let factor = finite("zoom.factor", factor)?;
    let zoom = clamp_zoom(viewport.zoom * factor);
    let ratio = zoom / clamp_zoom(viewport.zoom);
    Ok(Viewport {
        zoom,
        pan_x: viewport.pan_x * ratio,
        pan_y: viewport.pan_y * ratio,
    })
}

/// Zoom about a screen anchor, keeping the world point under it still. This is
/// the wheel gesture; getting it wrong is the classic "the diagram ran away
/// from the cursor" bug, so it is computed once here.
pub fn zoom_at(
    bounds: &Bounds,
    host: &HostSurface,
    viewport: &Viewport,
    anchor_x: f64,
    anchor_y: f64,
    factor: f64,
) -> Result<Viewport, GeometryError> {
    let anchor = Point::new(
        finite("zoomAt.anchorX", anchor_x)?,
        finite("zoomAt.anchorY", anchor_y)?,
    );
    let factor = finite("zoomAt.factor", factor)?;
    let current = projection(bounds, host, viewport);
    let zoom = clamp_zoom(viewport.zoom * factor);
    if zoom == viewport.zoom {
        return Ok(*viewport);
    }
    let world = current.unproject(anchor);
    let fitted = projection(
        bounds,
        host,
        &Viewport {
            zoom,
            pan_x: 0.0,
            pan_y: 0.0,
        },
    );
    Ok(Viewport {
        zoom,
        pan_x: anchor.x - fitted.offset_x - world.x * fitted.scale,
        pan_y: anchor.y - fitted.offset_y - world.y * fitted.scale,
    })
}

/// Put one world point in the middle of the window at the current zoom.
pub fn center_on(
    bounds: &Bounds,
    host: &HostSurface,
    viewport: &Viewport,
    world_x: f64,
    world_y: f64,
) -> Result<Viewport, GeometryError> {
    let world = Point::new(
        finite("centerOn.worldX", world_x)?,
        finite("centerOn.worldY", world_y)?,
    );
    let fitted = projection(
        bounds,
        host,
        &Viewport {
            zoom: viewport.zoom,
            pan_x: 0.0,
            pan_y: 0.0,
        },
    );
    Ok(Viewport {
        zoom: clamp_zoom(viewport.zoom),
        pan_x: host.width / 2.0 - fitted.offset_x - world.x * fitted.scale,
        pan_y: host.height / 2.0 - fitted.offset_y - world.y * fitted.scale,
    })
}
