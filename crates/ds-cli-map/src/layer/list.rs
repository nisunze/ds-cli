//! `ds map layer list` — the canonical project layers as the drawer sees them:
//! families, roles, this machine's remembered visibility and zoom range.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

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
    let request = ds_layer_ops::ListRequest {
        refresh: inputs.switch("refresh"),
        limit: Some(crate::integer(inputs.require("limit")?, "limit", 1, 500)?),
        zoom: inputs
            .value("zoom")
            .map(|raw| crate::integer(raw, "zoom", 0, 24))
            .transpose()?,
    };
    let mut documents = ds_layer_ops::Native::new(inputs.require("lane")?);
    ds_layer_ops::list(
        &mut documents,
        &ds_layer_ops::Preferences::native()?,
        &request,
    )
}

pub fn render(data: &Value) -> String {
    ds_layer_ops::render_list(data)
}
