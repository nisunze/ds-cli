//! Project-layer ordering and desktop-local remote tile references.

pub mod add;
pub mod list;
pub mod native;
pub mod project_visibility;
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

pub const LOCAL_STORE_REFUSAL: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "local_layer_refused",
    when: "the overlay is invalid, missing, or its local store cannot be persisted",
    remedy: "check the overlay and local data directory; DS_LAYER_HOME may name an absolute shared directory",
};
pub fn local_edit(edit: ds_layer_store::OverlayEdit) -> Result<Value, Failure> {
    let result = ds_layer_store::execute(edit).map_err(|message| {
        Failure::invalid("local_layer_refused", message).remedy(LOCAL_STORE_REFUSAL.remedy)
    })?;
    let mut receipt = result["receipt"].clone();
    receipt["persisted"] = result["persisted"].clone();
    receipt["revision"] = result["revision"].clone();
    receipt["mapUpdated"] = Value::Null;
    Ok(receipt)
}

pub fn local_availability() -> ds_cli_contract::spec::Availability {
    ds_cli_contract::spec::Availability::Available
}

/// One shared layer-state question. The kernel's own request errors are ours
/// (a malformed question or a stale project), never the backend's.
pub fn ask_layer_state(request: &Value) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(request).expect("layer state request encodes");
    let answer = ds_command_kernel::layer_state::evaluate(&bytes).map_err(|message| {
        if message.starts_with("project_context_changed") {
            Failure::conflict("project_context_changed", message)
                .remedy("run the command again against the current selected project")
        } else {
            Failure::internal("layer_state_refused", message)
                .remedy("update ds and report the layer-state contract failure")
        }
    })?;
    Ok(serde_json::from_str(&answer).expect("kernel answers JSON"))
}

/// This machine's remembered toggles for one lane and project.
pub fn read_preferences(
    lane: &str,
    project: &str,
) -> Result<std::collections::BTreeMap<String, bool>, Failure> {
    ds_layer_store::visibility::read(lane, project).map_err(|message| {
        Failure::invalid("local_layer_refused", message).remedy(LOCAL_STORE_REFUSAL.remedy)
    })
}
