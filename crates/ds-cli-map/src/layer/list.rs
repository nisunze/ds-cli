//! `ds map layer list` — canonical project layers plus loaded runtime roots.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::native::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.list",
    path: &["map", "layer", "list"],
    contract: 3,
    summary: "List canonical project layers through the native user client.",
    purpose: "Reads the selected native project's assembled layer configuration without a desktop. Only layers[].id is accepted by map layer reorder. Map visibility and loaded Notes/PM roots belong to their runtime host and are not inferred by this headless read. Use --refresh to rebuild canonical metadata and styles at the API boundary.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::switch(
            "refresh",
            "Force the governed layer configuration to be rebuilt.",
        ),
        Arg::value(
            "limit",
            "<n>",
            "Report at most this many canonical layers; 1..500.",
        )
        .default("100"),
        LANE_ARG,
    ],
    output: "Lane, selected project id, canonical layer_count and bounded layers with id, label, class, geometry, order, runtimeIds and styleRef; more reports truncation. No desktop runtime state is read.",
    examples: &[
        Example {
            command: "ds map layer list --output json",
            note: "Use .data.layers[].id verbatim when planning order; runtime_layers are never reorder ids.",
            runnable: false,
        },
        Example {
            command: "ds map layer list --refresh --output json",
            note: "Re-resolve metadata without requiring the map page to be open.",
            runnable: false,
        },
    ],
    refusals: super::native::NATIVE_LIST_REFUSALS,
    reference: Some("docs/reference/map.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 500)?;
    let headless = ds_cli_auth::layer_config(inputs.require("lane")?, inputs.switch("refresh"))?;
    let bytes = serde_json::to_vec(headless.result().document()).expect("validated layer document");
    let projection =
        ds_command_kernel::project_layer_catalog(&bytes, limit as usize).map_err(|_| {
            Failure::invalid(
                "auth_response_unreadable",
                "layer catalogue violates its bounded projection",
            )
            .remedy("update ds and report the layer contract failure")
        })?;
    let mut result: Value = serde_json::from_str(&projection).expect("kernel projection is JSON");
    result["project"] = json!(headless.project_id());
    result["lane"] = json!(headless.lane());
    result["refreshed"] = json!(inputs.switch("refresh"));
    Ok(result)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} · {} canonical layers\n",
        data["project"].as_str().unwrap_or("?"),
        data["layer_count"]
    );
    for layer in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<38} {:>6}  {:<12} {}\n",
            layer["id"].as_str().unwrap_or("?"),
            layer["order"],
            layer["geometry"].as_str().unwrap_or("?"),
            layer["label"].as_str().unwrap_or("?")
        ));
    }
    let runtime = data["runtime_layers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !runtime.is_empty() {
        out.push_str(&format!(
            "{} loaded runtime roots (read-only)\n",
            runtime.len()
        ));
    }
    for root in runtime {
        out.push_str(&format!(
            "  {:<20} {:<15} {:>6} mapped · {:>6} table-only · {}\n",
            root["logicalRoot"].as_str().unwrap_or("?"),
            root["authority"].as_str().unwrap_or("?"),
            root["featureCount"].as_u64().unwrap_or(0),
            root["nonGeometricCount"].as_u64().unwrap_or(0),
            root["freshness"].as_str().unwrap_or("?")
        ));
        for child in root["children"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "    {:<12} {:>6}  {:<8} {}\n",
                child["geometry"].as_str().unwrap_or("?"),
                child["featureCount"].as_u64().unwrap_or(0),
                child["styleState"].as_str().unwrap_or("?"),
                child["styleRef"].as_str().unwrap_or("—")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render;
    use serde_json::json;

    #[test]
    fn renders_runtime_roots_separately_from_reorderable_layers() {
        let text = render(&json!({
            "project": "p1",
            "layer_count": 1,
            "layers": [{ "id": "canonical", "order": 10, "geometry": "Point", "label": "Customers" }],
            "runtime_layers": [{
                "logicalRoot": "personal_notes",
                "authority": "account_private",
                "featureCount": 2,
                "nonGeometricCount": 3,
                "freshness": "fresh",
                "children": [{ "geometry": "Point", "featureCount": 2, "styleState": "ready", "styleRef": "ud/personal_notes_point" }]
            }]
        }));
        assert!(text.contains("1 canonical layers"));
        assert!(text.contains("1 loaded runtime roots (read-only)"));
        assert!(text.contains("personal_notes"));
        assert!(text.contains("ud/personal_notes_point"));
        assert!(text.contains("ready"));
    }
}
