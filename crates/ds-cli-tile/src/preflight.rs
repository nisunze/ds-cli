//! `ds tile preflight` — what a run would read, before it reads it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{LANE_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.preflight",
    path: &["tile", "preflight"],
    contract: 1,
    summary: "Inspect one output's sources: layers, rows, empties, blockers.",
    purpose: "\
Restores the native user, uses only its audience-fenced selected project, \
and asks ds-brain through the fixed tile preflight call to inspect every \
source layer, its rows and geometries, empty layers, invalid tables and any \
blocker — without starting anything. `ready` means a run would proceed; \
`empty` means it would retire the output; `blocked` names what must be fixed. \
No project, Desktop descriptor, URL, body or action override is accepted.",
    chapter: Chapter::VectorTiles,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TYPE_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, `type`, and `preflight` with \
status, layer/row/geometry counts, bounded layer details, empty layers, \
errors, warnings, projection state and message.",
    examples: &[Example {
        command: "ds tile preflight --type design --output json",
        note: "Read `.data.preflight.layers` to see which design layers carry rows.",
        runnable: false,
    }],
    refusals: crate::NATIVE_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let kind = crate::tile_type(inputs.require("type")?);
    let headless = ds_cli_auth::tile_preflight(inputs.require("lane")?, kind)?;
    let mut output = crate::preflight_project(&headless);
    output["type"] = Value::String(kind.token().to_owned());
    output["preflight"] = crate::preflight_json(headless.result());
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} tiles · {}\n",
        data["type"].as_str().unwrap_or("?"),
        crate::preflight_line(&data["preflight"])
    );
    for layer in data["preflight"]["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} {:>8} rows {:>8} geometries{}\n",
            crate::truncate(layer["table"].as_str().unwrap_or("?"), 28),
            layer["row_count"].as_u64().unwrap_or(0),
            layer["geometry_count"].as_u64().unwrap_or(0),
            if layer["empty"].as_bool() == Some(true) {
                "  (empty)"
            } else {
                ""
            },
        ));
    }
    out
}
