//! `ds map design layer-to-local` — copy project design data to a local layer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.layer-to-local",
    path: &["map", "design", "layer-to-local"],
    contract: 1,
    summary: "Copy one design layer into a reviewable local map layer.",
    purpose: "\
Asks the running application to copy one current transformer design layer into \
its shared local-layer store. The application reads IndexedDB/cloud-backed \
design state and writes the local layer; the CLI receives only a bounded \
receipt, never raw design features. The project design is not changed.",
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        Arg::value("layer", "<name>", "Exact design layer name, e.g. lv_lines.").required(),
        Arg::value("name", "<text>", "Name of the new local layer.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "The local layer id, geometry-independent feature count, source transformer and layer, and persisted=false.",
    examples: &[Example {
        command: "ds map design layer-to-local --transformer agasharu --layer lv_lines --name 'agasharu approved base' --output json",
        note: "Create the authoritative comparison base without exporting it through the CLI.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the project, transformer, or exact layer is unavailable or unsupported",
            remedy: "check the active project and run `ds map design read --transformer <name>`",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let layer = inputs.require("layer")?;
    let name = inputs.require("name")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_LAYER_TO_LOCAL,
        json!({ "transformer": transformer, "layer": layer, "name": name }),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;
    Ok(json!({
        "project": result["project"],
        "transformer": result["transformer"],
        "source_layer": result["sourceLayer"],
        "source": result["source"],
        "layer": result["layerId"],
        "name": result["name"],
        "features": result["featureCount"],
        "persisted": result["persistedToProject"],
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "{} copied to local layer\n  {}.{} · {}\n  layer  {}\n",
        data["name"].as_str().unwrap_or("design layer"),
        data["transformer"].as_str().unwrap_or(""),
        data["source_layer"].as_str().unwrap_or(""),
        crate::plural(data["features"].as_u64().unwrap_or(0), "feature"),
        data["layer"].as_str().unwrap_or(""),
    )
}
