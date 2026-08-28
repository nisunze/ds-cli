//! Project-layer ordering and desktop-local remote tile references.

pub mod add;
pub mod list;
pub mod remote_list;
pub mod remove;
pub mod reorder;
pub mod visibility;

use ds_cli_contract::outcome::Failure;
use serde_json::{Value, json};

pub fn classify(failure: Failure) -> Failure {
    crate::classify_design_failure(failure)
}

pub fn render_remote(layer: &Value) -> String {
    format!(
        "{:<28} {:<8} {:<7} {}\n",
        layer["id"].as_str().unwrap_or("?"),
        layer["kind"].as_str().unwrap_or("?"),
        if layer["visible"].as_bool().unwrap_or(false) {
            "visible"
        } else {
            "hidden"
        },
        layer["name"].as_str().unwrap_or("?"),
    )
}

pub fn remote_result(result: Value) -> Value {
    json!({
        "layer": result["id"].clone(),
        "name": result["name"].clone(),
        "kind": result["kind"].clone(),
        "url": result["url"].clone(),
        "tile_size": result["tileSize"].clone(),
        "visible": result["visible"].clone(),
        "persisted": result["persisted"].clone(),
        "map_updated": result["mapUpdated"].clone(),
    })
}
