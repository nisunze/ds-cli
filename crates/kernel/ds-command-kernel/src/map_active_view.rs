//! Deterministic validation of a camera command for the 2D map. The kernel
//! owns the accepted camera vocabulary; hosts only apply the resulting plan.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const REQUEST_SCHEMA: &str = "ds.map.active-view.request/v1";
pub const PLAN_SCHEMA: &str = "ds.map.active-view.plan/v1";
pub const MAX_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub center: [f64; 2],
    pub zoom: f64,
    pub bearing: f64,
    pub pitch: f64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Request {
    #[serde(rename = "camera.set")]
    CameraSet {
        schema: String,
        center: [f64; 2],
        zoom: f64,
        bearing: f64,
        pitch: f64,
    },
}

pub(crate) fn validate_camera(camera: &Camera) -> Result<(), String> {
    if ![
        camera.center[0],
        camera.center[1],
        camera.zoom,
        camera.bearing,
        camera.pitch,
    ]
    .iter()
    .all(|value| value.is_finite())
        || !(-180.0..=180.0).contains(&camera.center[0])
        || !(-85.0..=85.0).contains(&camera.center[1])
        || !(0.0..=24.0).contains(&camera.zoom)
        || !(0.0..=85.0).contains(&camera.pitch)
        || !(-360.0..=360.0).contains(&camera.bearing)
    {
        return Err("camera must contain finite WGS84 center, zoom 0..24, pitch 0..85 and bearing -360..360".into());
    }
    Ok(())
}

pub fn evaluate(input: &[u8]) -> Result<Value, String> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err("active map request exceeds its 4 KiB bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|_| "active map request does not match its closed contract".to_string())?;
    match request {
        Request::CameraSet {
            schema,
            center,
            zoom,
            bearing,
            pitch,
        } => {
            if schema != REQUEST_SCHEMA {
                return Err("active map request schema is unsupported".into());
            }
            let camera = Camera {
                center,
                zoom,
                bearing,
                pitch,
            };
            validate_camera(&camera)?;
            Ok(json!({
                "schema": PLAN_SCHEMA,
                "operation": "camera.set",
                "camera": camera,
            }))
        }
    }
}

#[cfg(test)]
#[path = "map_active_view_tests.rs"]
mod tests;
