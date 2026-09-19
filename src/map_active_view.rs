//! Deterministic validation for commands which change the active map renderer.
//! The kernel owns the accepted camera/provider vocabulary; hosts only apply
//! the resulting plan to the renderer they already own.

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
    #[serde(rename = "renderer.configure")]
    RendererConfigure {
        schema: String,
        mode: String,
        #[serde(default)]
        provider: Option<String>,
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
        Request::RendererConfigure {
            schema,
            mode,
            provider,
        } => {
            if schema != REQUEST_SCHEMA {
                return Err("active map request schema is unsupported".into());
            }
            let renderer = match (mode.as_str(), provider.as_deref()) {
                ("2d", None) => json!({ "mode": "2d" }),
                ("3d", Some("aws-terrain")) => {
                    json!({ "mode": "3d", "provider": "aws-terrain" })
                }
                ("3d", Some("google")) => json!({ "mode": "3d", "provider": "google" }),
                ("2d", Some(_)) => {
                    return Err("2d renderer configuration does not accept a provider".into());
                }
                ("3d", None) => {
                    return Err("3d renderer configuration requires aws-terrain or google".into());
                }
                _ => {
                    return Err(
                        "renderer must be 2d, or 3d with provider aws-terrain or google".into(),
                    );
                }
            };
            Ok(json!({
                "schema": PLAN_SCHEMA,
                "operation": "renderer.configure",
                "renderer": renderer,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(value: Value) -> Result<Value, String> {
        evaluate(&serde_json::to_vec(&value).unwrap())
    }

    #[test]
    fn accepts_camera_and_both_explicit_3d_providers() {
        let camera = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "camera.set",
            "center": [30.0619, -1.9441], "zoom": 18.5, "pitch": 68.0, "bearing": -32.0
        }))
        .unwrap();
        assert_eq!(camera["camera"]["center"], json!([30.0619, -1.9441]));
        for provider in ["aws-terrain", "google"] {
            let plan = command(json!({
                "schema": REQUEST_SCHEMA, "operation": "renderer.configure",
                "mode": "3d", "provider": provider
            }))
            .unwrap();
            assert_eq!(plan["renderer"]["provider"], provider);
        }
    }

    #[test]
    fn refuses_ambiguous_renderer_combinations_and_invalid_camera() {
        for request in [
            json!({"schema": REQUEST_SCHEMA, "operation": "renderer.configure", "mode": "3d"}),
            json!({"schema": REQUEST_SCHEMA, "operation": "renderer.configure", "mode": "2d", "provider": "google"}),
            json!({"schema": REQUEST_SCHEMA, "operation": "renderer.configure", "mode": "3d", "provider": "auto"}),
            json!({"schema": REQUEST_SCHEMA, "operation": "camera.set", "center": [30.0, -91.0], "zoom": 10.0, "pitch": 40.0, "bearing": 0.0}),
        ] {
            assert!(command(request).is_err());
        }
    }

    #[test]
    fn rejects_unknown_fields_and_non_finite_values() {
        assert!(command(json!({
            "schema": REQUEST_SCHEMA, "operation": "camera.set",
            "center": [30.0, -1.0], "zoom": 10.0, "pitch": 40.0, "bearing": 0.0,
            "duration": 500
        }))
        .is_err());
        let raw = br#"{"schema":"ds.map.active-view.request/v1","operation":"camera.set","center":[30,-1],"zoom":1e999,"pitch":40,"bearing":0}"#;
        assert!(evaluate(raw).is_err());
    }
}
