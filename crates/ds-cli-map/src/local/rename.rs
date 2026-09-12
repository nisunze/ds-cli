//! `ds map local rename` — rename one prepared local layer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_layer_store::prepared::{self, Op};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.local.rename",
    path: &["map", "local", "rename"],
    contract: 1,
    summary: "Rename one prepared local layer.",
    purpose: "Asks the shared kernel to rename one row of this lane and account's catalogue. A rename is not a re-provenance: a layer you drew keeps its name inside its own source, while an imported layer keeps the name it had inside the file it came from, and neither the payload nor the file it was prepared from is touched. Machine-local: no project layer is renamed by this.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        super::LAYER_ARG.required(),
        Arg::value("name", "<text>", "New layer name; 1 to 200 characters.").required(),
        super::LANE_ARG,
        super::ACCOUNT_ARG,
    ],
    output: "The layer id, its new name, what the kernel applied, and the catalogue revision after the write.",
    examples: &[Example {
        command: "ds map local rename --layer sketch-1758000000000-0 --name 'Access roads' --output json",
        note: "Take the id from ds map local list.",
        runnable: false,
    }],
    refusals: &[
        super::STORE_REFUSED,
        super::UNKNOWN_LAYER,
        super::MALFORMED_DESCRIPTOR,
        super::DUPLICATE_LAYER,
        super::SCOPE_MISMATCH,
        super::UNSUPPORTED_SOURCE_KIND,
    ],
    reference: Some("docs/reference/map.md"),
    availability: super::availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let name = inputs.require("name")?.trim().to_owned();
    let scope = super::scope(inputs)?;
    let answer = prepared::execute(
        &scope,
        Op::Rename {
            id: inputs.require("layer")?.trim().to_owned(),
            name: name.clone(),
        },
    )
    .map_err(super::refuse)?;
    Ok(super::stamped(
        &scope,
        &answer,
        json!({
            "layer": answer["receipt"]["id"].clone(),
            "name": name,
            "applied": answer["receipt"]["applied"].clone(),
        }),
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "renamed {} · {}\n",
        data["layer"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
    )
}
