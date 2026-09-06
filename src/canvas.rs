//! Closed headless camera intents. Spatial algorithms and numeric adapters
//! belong to ds-network; the command kernel validates and dispatches intent.
use ds_canvas::packed::camera_buffer;
#[cfg(test)]
use ds_canvas::packed::selection_buffer;
use ds_canvas::{Bounds, HostSurface, Point, Viewport};
use serde::Deserialize;
use serde_json::{json, Value};
pub const MAX_COMMAND_BYTES: usize = 1_048_576;
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum CameraIntent {
    Fit {},
    Pan {
        viewport: Viewport,
        dx: f64,
        dy: f64,
    },
    ZoomCentered {
        viewport: Viewport,
        factor: f64,
    },
    Project {
        bounds: Bounds,
        host: HostSurface,
        viewport: Viewport,
    },
    ZoomAt {
        bounds: Bounds,
        host: HostSurface,
        viewport: Viewport,
        anchor: Point,
        factor: f64,
    },
    CenterOn {
        bounds: Bounds,
        host: HostSurface,
        viewport: Viewport,
        point: Point,
    },
}
impl CameraIntent {
    pub fn evaluate(self) -> Result<Value, String> {
        fn base(b: Bounds, h: HostSurface, v: Viewport) -> Result<Vec<f64>, String> {
            HostSurface::new(h.width, h.height, h.device_pixel_ratio)?;
            Ok(vec![
                b.min_x, b.min_y, b.max_x, b.max_y, h.width, h.height, v.zoom, v.pan_x, v.pan_y,
            ])
        }
        let (operation, input) = match self {
            Self::Fit {} => (4, vec![]),
            Self::Pan { viewport, dx, dy } => {
                let mut b = vec![viewport.zoom, viewport.pan_x, viewport.pan_y];
                b.extend([dx, dy]);
                (1, b)
            }
            Self::ZoomCentered { viewport, factor } => {
                let mut b = vec![viewport.zoom, viewport.pan_x, viewport.pan_y];
                b.push(factor);
                (2, b)
            }
            Self::Project {
                bounds,
                host,
                viewport,
            } => (0, base(bounds, host, viewport)?),
            Self::ZoomAt {
                bounds,
                host,
                viewport,
                anchor,
                factor,
            } => {
                let mut b = base(bounds, host, viewport)?;
                b.extend([anchor.x, anchor.y, factor]);
                (3, b)
            }
            Self::CenterOn {
                bounds,
                host,
                viewport,
                point,
            } => {
                let mut b = base(bounds, host, viewport)?;
                b.extend([point.x, point.y]);
                (5, b)
            }
        };
        let out = camera_buffer(operation, &input)?;
        Ok(if operation == 0 {
            json!({"scale":out[0],"offset_x":out[1],"offset_y":out[2]})
        } else {
            json!({"zoom":out[0],"pan_x":out[1],"pan_y":out[2]})
        })
    }
}
pub fn evaluate(input: &[u8]) -> Result<Value, String> {
    if input.len() > MAX_COMMAND_BYTES {
        return Err("canvas command exceeds 1 MiB".into());
    }
    let request: CameraIntent = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    request.evaluate()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_and_pointer_buffer_have_same_camera() {
        let result=evaluate(br#"{"operation":"zoom_at","bounds":{"min_x":100,"min_y":20,"max_x":1100,"max_y":220},"host":{"width":1024,"height":424,"device_pixel_ratio":2},"viewport":{"zoom":1.4,"pan_x":-175,"pan_y":42},"anchor":{"x":730,"y":155},"factor":1.5}"#).unwrap();
        let raw = camera_buffer(
            3,
            &[
                100., 20., 1100., 220., 1024., 424., 1.4, -175., 42., 730., 155., 1.5,
            ],
        )
        .unwrap();
        assert_eq!(result, json!({"zoom":raw[0],"pan_x":raw[1],"pan_y":raw[2]}));
    }
    #[test]
    fn malformed_buffers_and_nonfinite_geometry_refuse() {
        assert!(camera_buffer(0, &[]).is_err());
        assert!(camera_buffer(99, &[]).is_err());
        assert!(selection_buffer(1, &[1.], &[0., 0.]).is_err());
        assert!(selection_buffer(1, &[f64::NAN, 0.], &[0., 0.]).is_err());
        assert!(selection_buffer(3, &vec![0.; 4096], &vec![0.; 4096]).is_err());
        assert!(evaluate(br#"{"operation":"fit","typo":true}"#).is_err());
    }
}
