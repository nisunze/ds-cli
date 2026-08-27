//! `ds map design upload-to-local` — parse one archive layer into local state.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.upload-to-local",
    path: &["map", "design", "upload-to-local"],
    contract: 1,
    summary: "Parse one named archive layer into a local map layer.",
    purpose: "\
Names a desktop file and one exact layer inside it. The running application \
uses its native parser and bounded upload workspace, then adds that layer to \
the shared local-layer store. The CLI receives only a receipt and never \
transports feature rows. Nothing is staged into transformer design data.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        Arg::value("path", "<file>", "Desktop archive or design source path.").required(),
        Arg::value(
            "source-layer",
            "<name>",
            "Exact parsed layer name, e.g. lv_lines.",
        )
        .required(),
        Arg::value("name", "<text>", "Name of the new local layer.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "The new local layer id, source file and layer, feature count, and persisted=false.",
    examples: &[Example {
        command: "ds map design upload-to-local --path 'C:\\Designs\\agasharu.shp.zip' --source-layer lv_lines --name 'agasharu incoming lv lines' --output json",
        note: "Load only the incoming line layer for comparison.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the file or exact layer is unavailable, unsupported, mixed, or over the bound",
            remedy: "run `ds map design upload --path <file>` to inspect its exact parsed layer names",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = inputs.require("path")?;
    let source_layer = inputs.require("source-layer")?;
    let name = inputs.require("name")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_UPLOAD_TO_LOCAL,
        json!({ "path": path, "sourceLayer": source_layer, "name": name }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;
    Ok(json!({
        "project": result["project"],
        "path": result["path"],
        "file": result["fileName"],
        "source_layer": result["sourceLayer"],
        "layer": result["layerId"],
        "name": result["name"],
        "features": result["featureCount"],
        "persisted": result["persistedToProject"],
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "{} loaded as local layer\n  {} → {} · {}\n  layer  {}\n",
        data["name"].as_str().unwrap_or("archive layer"),
        data["file"].as_str().unwrap_or(""),
        data["source_layer"].as_str().unwrap_or(""),
        crate::plural(data["features"].as_u64().unwrap_or(0), "feature"),
        data["layer"].as_str().unwrap_or(""),
    )
}
