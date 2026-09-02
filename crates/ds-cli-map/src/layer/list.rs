//! `ds map layer list` — canonical project layers plus loaded runtime roots.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.list",
    path: &["map", "layer", "list"],
    contract: 2,
    summary: "List canonical project layers and loaded Notes/PM map roots.",
    purpose: "Reads the active project's assembled layer configuration through the signed-in desktop and separately reports any loaded account-private Notes and project-owned PM runtime roots. Only `layers[].id` is accepted by `map layer reorder`; runtime roots and their geometry children are read-only discovery. The map itself need not be open. Use --refresh to rebuild only canonical metadata and styles at the API boundary.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
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
        DESCRIPTOR_ARG,
    ],
    output: "Project id, whether the map is open, canonical layer count and bounded `layers`; plus `runtime_layer_count` and loaded `runtime_layers`, each with logical root, authority, source id, row/mapped/table-only counts, freshness, and only-present Point/LineString/Polygon children.",
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
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 500)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::LAYERS_LIST,
        json!({
            "scope": "project",
            "refresh": inputs.switch("refresh"),
            "limit": limit,
        }),
        crate::API_TIMEOUT,
    )
    .map_err(super::classify)?;
    Ok(json!({
        "project": result["project"],
        "map_open": result["mapOpen"],
        "layer_count": result["configuredTotal"],
        "layers": result["configured"],
        "runtime_layer_count": result["runtimeTotal"],
        "runtime_layers": result["runtime"],
        "runtime_layers_truncated": result["runtimeTruncated"],
        "refreshed": result["refreshed"],
    }))
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
                "    {:<12} {:>6}  {}\n",
                child["geometry"].as_str().unwrap_or("?"),
                child["featureCount"].as_u64().unwrap_or(0),
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
                "children": [{ "geometry": "Point", "featureCount": 2, "styleRef": "ud/personal_notes_point" }]
            }]
        }));
        assert!(text.contains("1 canonical layers"));
        assert!(text.contains("1 loaded runtime roots (read-only)"));
        assert!(text.contains("personal_notes"));
        assert!(text.contains("ud/personal_notes_point"));
    }
}
