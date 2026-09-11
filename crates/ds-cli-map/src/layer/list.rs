//! `ds map layer list` — the canonical project layers as the drawer sees them:
//! families, roles, this machine's remembered visibility and zoom range.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::native::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.list",
    path: &["map", "layer", "list"],
    contract: 4,
    summary: "List canonical project layers through the native user client.",
    purpose: "Reads the selected native project's assembled layer document without a desktop and projects it through the shared layer kernel: one row per canonical layer with its runtime family and roles (primary, label, boundary, d3), this machine's remembered visibility folded over the family (the same answer the drawer shows), whether its source is present, and — with --zoom — whether it renders at that zoom. Only layers[].id is accepted by map layer reorder/show/hide. Loaded Notes/PM roots belong to their runtime host and are not inferred here. Use --refresh to rebuild canonical metadata and styles at the API boundary.",
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
        Arg::value(
            "zoom",
            "<level>",
            "Also report whether each layer renders at this zoom; 0..24.",
        ),
        LANE_ARG,
    ],
    output: "Lane, selected project id, canonical layer_count and bounded layers with id, label, class, geometry, order, runtime_ids, style_ref, roles, visibility (count, any_visible, all_visible, next), source_state and in_zoom_range; more reports truncation; visibility_source names the native store. No desktop runtime state is read.",
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
    let zoom = inputs
        .value("zoom")
        .map(|raw| crate::integer(raw, "zoom", 0, 24))
        .transpose()?;
    let lane = inputs.require("lane")?;
    let headless = ds_cli_auth::layer_config(lane, inputs.switch("refresh"))?;
    let preferences = super::read_preferences(lane, headless.project_id())?;
    let mut request = json!({
        "schema": ds_command_kernel::layer_state::SCHEMA,
        "project": headless.project_id(),
        "document": headless.result().document(),
        "preferences": preferences,
        "op": {"kind": "catalog", "limit": limit},
    });
    if let Some(zoom) = zoom {
        request["zoom"] = json!(zoom);
    }
    let mut result = super::ask_layer_state(&request)?;
    result["project"] = json!(headless.project_id());
    result["lane"] = json!(headless.lane());
    result["refreshed"] = json!(inputs.switch("refresh"));
    result["visibility_source"] = json!("native_local");
    Ok(result)
}

/// One word for a folded visibility: `visible`, `partial` or `hidden`.
pub fn visibility_word(visibility: &Value) -> &'static str {
    match (
        visibility["all_visible"].as_bool().unwrap_or(false),
        visibility["any_visible"].as_bool().unwrap_or(false),
    ) {
        (true, _) => "visible",
        (false, true) => "partial",
        (false, false) => "hidden",
    }
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} · {} canonical layers\n",
        data["project"].as_str().unwrap_or("?"),
        data["layer_count"]
    );
    for layer in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<38} {:>6}  {:<12} {:<8} {}\n",
            layer["id"].as_str().unwrap_or("?"),
            layer["order"],
            layer["geometry"].as_str().unwrap_or("?"),
            visibility_word(&layer["visibility"]),
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
            "layers": [{ "id": "canonical", "order": 10, "geometry": "Point", "label": "Customers", "visibility": { "count": 2, "any_visible": true, "all_visible": false, "next": false } }],
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
        assert!(text.contains("partial"));
        assert!(text.contains("1 loaded runtime roots (read-only)"));
        assert!(text.contains("personal_notes"));
        assert!(text.contains("ud/personal_notes_point"));
        assert!(text.contains("ready"));
    }
}
