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
    // Fit is relative to the whole scene. An absolute pixel-per-unit floor
    // crops long profiles and turns several zoom-out steps into no-ops.
    let scale = fit * clamp_zoom(viewport.zoom);
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

/// A host gesture in CSS pixels. It is transient camera input, never model data.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoomWindow {
    pub from: Point,
    pub to: Point,
}
impl ZoomWindow {
    pub fn validate(self) -> Result<(), String> {
        if !self.from.is_finite() || !self.to.is_finite() {
            return Err("canvas zoom window must be finite".into());
        }
        let width = (self.to.x - self.from.x).abs();
        let height = (self.to.y - self.from.y).abs();
        if !width.is_finite() || !height.is_finite() {
            return Err("canvas zoom window extent must be finite".into());
        }
        if width == 0. || height == 0. {
            return Err("canvas zoom window must have positive width and height".into());
        }
        Ok(())
    }
}

/// Fit the scene beneath a dragged window without changing its aspect ratio.
/// Both drag directions use the same center, existing zoom limits and padding.
pub fn zoom_window(
    bounds: &Bounds,
    host: &HostSurface,
    viewport: &Viewport,
    window: ZoomWindow,
) -> Result<Viewport, String> {
    window.validate()?;
    let current = projection(bounds, host, viewport);
    let width = (window.to.x - window.from.x).abs();
    let height = (window.to.y - window.from.y).abs();
    let center = current.unproject(Point::new(
        window.from.x / 2. + window.to.x / 2.,
        window.from.y / 2. + window.to.y / 2.,
    ));
    let factor = ((host.width - FIT_PADDING_PX).max(1.) / width)
        .min((host.height - FIT_PADDING_PX).max(1.) / height);
    let zoom = (clamp_zoom(viewport.zoom) * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let result = center_on(bounds, host, &Viewport { zoom, ..*viewport }, center);
    if ![result.zoom, result.pan_x, result.pan_y]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err("canvas zoom window projection must be finite".into());
    }
    Ok(result)
}

/// Keep a saved camera while the scene still intersects the host. A camera is
/// stored in CSS pixels, so another window size or a later scene can otherwise
/// leave every engineering mark outside the canvas. When that happens, prefer
/// the selected scene point; without one, return the whole-scene fit.
pub fn rebase_if_offscreen(
    bounds: &Bounds,
    host: &HostSurface,
    viewport: &Viewport,
    selected: Option<Point>,
) -> Viewport {
    let current = Viewport {
        zoom: clamp_zoom(viewport.zoom),
        pan_x: finite_pan(viewport.pan_x),
        pan_y: finite_pan(viewport.pan_y),
    };
    let projected = projection(bounds, host, &current);
    let left = projected.offset_x + bounds.min_x * projected.scale;
    let right = projected.offset_x + bounds.max_x * projected.scale;
    let top = projected.offset_y + bounds.min_y * projected.scale;
    let bottom = projected.offset_y + bounds.max_y * projected.scale;
    let margin = 8_f64.min(host.width / 4.).min(host.height / 4.);
    if right >= margin
        && left <= host.width - margin
        && bottom >= margin
        && top <= host.height - margin
    {
        return current;
    }
    match selected.filter(|point| {
        point.is_finite()
            && point.x >= bounds.min_x
            && point.x <= bounds.max_x
            && point.y >= bounds.min_y
            && point.y <= bounds.max_y
    }) {
        Some(point) => center_on(bounds, host, &current, point),
        None => Viewport::fitted(),
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
    fn window_fit_centers_the_original_scene_point_and_preserves_aspect() {
        let viewport = Viewport {
            zoom: 2.,
            pan_x: 170.,
            pan_y: -30.,
        };
        let window = ZoomWindow {
            from: Point::new(212., 112.),
            to: Point::new(812., 312.),
        };
        let before = projection(&bounds(), &host(), &viewport);
        let center = before.unproject(Point::new(512., 212.));
        let fitted = zoom_window(&bounds(), &host(), &viewport, window).unwrap();
        let after = projection(&bounds(), &host(), &fitted);
        assert!((after.project(center).x - 512.).abs() < 1e-9);
        assert!((after.project(center).y - 212.).abs() < 1e-9);
        assert!((after.scale / before.scale - 1000. / 600.).abs() < 1e-9);
        assert_eq!(
            zoom_window(
                &bounds(),
                &host(),
                &viewport,
                ZoomWindow {
                    from: window.to,
                    to: window.from
                }
            )
            .unwrap(),
            fitted
        );
    }
    #[test]
    fn window_fit_obeys_zoom_limits_and_refuses_degenerate_input() {
        for (width, expected) in [(1e-8, MAX_ZOOM), (1e-320, MAX_ZOOM), (1e12, MIN_ZOOM)] {
            let fitted = zoom_window(
                &bounds(),
                &host(),
                &Viewport::fitted(),
                ZoomWindow {
                    from: Point::new(0., 0.),
                    to: Point::new(width, width),
                },
            )
            .unwrap();
            assert_eq!(fitted.zoom, expected);
        }
        for to in [
            Point::new(0., 10.),
            Point::new(10., 0.),
            Point::new(f64::NAN, 10.),
        ] {
            assert!(
                zoom_window(
                    &bounds(),
                    &host(),
                    &Viewport::fitted(),
                    ZoomWindow {
                        from: Point::new(0., 0.),
                        to
                    }
                )
                .is_err()
            );
        }
        assert!(
            zoom_window(
                &bounds(),
                &host(),
                &Viewport::fitted(),
                ZoomWindow {
                    from: Point::new(-f64::MAX, 0.),
                    to: Point::new(f64::MAX, 10.),
                }
            )
            .is_err()
        );
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

    #[test]
    fn stale_camera_rebases_to_selected_support_without_changing_zoom() {
        let bounds = Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 100.,
            max_y: 100.,
        };
        let host = HostSurface::new(300., 300., 1.).unwrap();
        let stale = Viewport {
            zoom: 2.,
            pan_x: 1_000.,
            pan_y: 0.,
        };
        let selected = Point::new(80., 50.);
        let before = projection(&bounds, &host, &stale);
        assert!(before.project(Point::new(0., 0.)).x > host.width);
        let restored = rebase_if_offscreen(&bounds, &host, &stale, Some(selected));
        assert_eq!(restored.zoom, 2.);
        let screen = projection(&bounds, &host, &restored).project(selected);
        assert!((screen.x - host.width / 2.).abs() < 1e-9);
        assert!((screen.y - host.height / 2.).abs() < 1e-9);
        assert_eq!(
            rebase_if_offscreen(&bounds, &host, &restored, Some(selected)),
            restored
        );
        // A later pointer pan can leave the same scene again; the next native
        // camera check returns to the selected support without a scene build.
        let dragged_away = pan(&restored, -2_000., 0.);
        assert_eq!(
            rebase_if_offscreen(&bounds, &host, &dragged_away, Some(selected)),
            restored
        );
    }

    #[test]
    fn visible_camera_survives_and_resize_rebases_without_selection() {
        let bounds = Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 100.,
            max_y: 100.,
        };
        let large = HostSurface::new(300., 300., 1.).unwrap();
        let small = HostSurface::new(50., 50., 1.).unwrap();
        let camera = Viewport {
            zoom: 1.,
            pan_x: 120.,
            pan_y: 0.,
        };
        assert_eq!(rebase_if_offscreen(&bounds, &large, &camera, None), camera);
        assert_eq!(
            rebase_if_offscreen(&bounds, &small, &camera, None),
            Viewport::fitted()
        );
        let offscreen = Viewport {
            zoom: 1.,
            pan_x: 1_000.,
            pan_y: 0.,
        };
        assert_eq!(
            rebase_if_offscreen(&bounds, &large, &offscreen, Some(Point::new(999., 0.))),
            Viewport::fitted()
        );
    }

    #[test]
    fn eighteen_band_profile_fits_and_zoom_out_changes_its_scale() {
        // Eighteen anonymous 10 km alignments. A long Profile has a fit scale
        // below 0.01 CSS px/m; an absolute floor clips both ends and makes
        // zoom-out appear capped in the popped-out window.
        let bounds = Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 180_000.,
            max_y: 100.,
        };
        let host = HostSurface::new(1_440., 820., 1.).unwrap();
        let fitted = projection(&bounds, &host, &Viewport::fitted());
        let left = fitted.project(Point::new(bounds.min_x, 0.)).x;
        let right = fitted.project(Point::new(bounds.max_x, 0.)).x;
        assert!(
            left >= 0. && right <= host.width,
            "wide Profile must fully fit: {left}..{right}"
        );
        let zoomed_out = projection(
            &bounds,
            &host,
            &Viewport {
                zoom: 0.5,
                ..Viewport::fitted()
            },
        );
        assert!(
            zoomed_out.scale < fitted.scale,
            "zoom-out must change painted scale"
        );
    }
}
