//! `ds map layer list` — canonical project-layer identities and ordering.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.list",
    path: &["map", "layer", "list"],
    contract: 1,
    summary: "List canonical project layers in render order.",
    purpose: "Reads the active project's assembled layer configuration through the signed-in desktop. Reports the canonical config id accepted by `map layer reorder`, separately from runtime MapLibre ids. The map itself need not be open. Use --refresh to rebuild metadata and styles at the API boundary.",
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
    output: "Project id, whether the map is open, canonical layer count, and bounded layers with config id, runtime ids, class, geometry, order, visibility, and style ref.",
    examples: &[
        Example {
            command: "ds map layer list --output json",
            note: "Use .data.layers[].id verbatim when planning order.",
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
    out
}
