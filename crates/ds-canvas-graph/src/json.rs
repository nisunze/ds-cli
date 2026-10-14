//! Durable and debug interchange.
//!
//! JSON exists here for three jobs: accepting an authored graph once, dumping
//! a scene a human or a fixture can read, and publishing the packed-buffer
//! descriptor. It is deliberately absent from the hot path — a pointer move
//! that serializes a tree is a pointer move that drops frames.

use serde::Serialize;

use crate::error::GeometryError;
use crate::input::GraphInput;
use crate::scene::Scene;

/// Parse an authored graph. Unknown fields are refused: a typo in an input key
/// would otherwise become a silently missing node or edge.
pub fn input_from_json(text: &str) -> Result<GraphInput, GeometryError> {
    serde_json::from_str(text).map_err(|error| GeometryError::InvalidJson {
        detail: error.to_string(),
    })
}

pub fn to_json<T: Serialize>(value: &T) -> Result<String, GeometryError> {
    serde_json::to_string(value).map_err(|error| GeometryError::InvalidJson {
        detail: error.to_string(),
    })
}

#[derive(Serialize)]
struct SceneEnvelope<'a> {
    revision: u32,
    scene: &'a Scene,
}

/// The whole scene as JSON, stamped with the revision it describes.
pub fn scene_to_json(scene: &Scene, revision: u32) -> Result<String, GeometryError> {
    to_json(&SceneEnvelope { revision, scene })
}

/// A typed refusal as JSON, for a host that logs or forwards it. `code` is the
/// stable machine key; `message` is for a human reading a console.
pub fn error_to_json(error: &GeometryError) -> String {
    let payload = serde_json::json!({
        "code": error.code(),
        "message": error.to_string(),
        "error": error,
    });
    payload.to_string()
}
