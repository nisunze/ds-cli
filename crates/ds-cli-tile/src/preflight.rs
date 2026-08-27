//! `ds tile preflight` — what a run would read, before it reads it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.preflight",
    path: &["tile", "preflight"],
    contract: 1,
    summary: "Inspect one output's sources: layers, rows, empties, blockers.",
    purpose: "\
Asks ds-brain to look at the sources a run of this type would tile — every \
layer with its row and geometry counts, the empty ones, invalid tables, and \
any blocker — without starting anything. `ready` means a run would proceed; \
`empty` means the sources hold nothing and a run would retire the output; \
`blocked` names what must be fixed first.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TYPE_ARG, DESCRIPTOR_ARG],
    output: "\
`project`, `type` and `preflight` with `status`, `expected_layer_count`, \
`will_export_layers`, `total_rows`, `layers` (table, row_count, \
geometry_count, empty), `empty_layers`, `errors`, `warnings`, `message`.",
    examples: &[Example {
        command: "ds tile preflight --type design --output json",
        note: "Read `.data.preflight.layers` to see which design layers carry rows.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::TILE_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/tile.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TILE_PREFLIGHT,
        json!({ "type": inputs.require("type")? }),
        crate::RUN_TIMEOUT,
    )
    .map_err(crate::classify_tile_failure)
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
