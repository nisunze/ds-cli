//! `ds map local remove` — remove one prepared local layer and its own payload.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_layer_store::prepared::{self, Op};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.local.remove",
    path: &["map", "local", "remove"],
    contract: 1,
    summary: "Remove one prepared local layer and its copied payload.",
    purpose: "Removes one row from this lane and account's catalogue and deletes exactly the files the kernel's receipt releases — the layer's own copied payload, and nothing else. The receipt can never name the file the layer was prepared from, a project asset or another layer's payload, so this cannot reach an original. Needs --yes. Machine-local: it removes no project layer and no governed tile.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        super::LAYER_ARG.required(),
        super::LANE_ARG,
        super::ACCOUNT_ARG,
    ],
    output: "The layer id, removed true, what the receipt released, the files actually deleted, and the catalogue revision after the write.",
    examples: &[Example {
        command: "ds map local remove --layer sketch-1758000000000-0 --yes --output json",
        note: "Deletes the copied payload only; the file it was prepared from stays.",
        runnable: false,
    }],
    refusals: &[
        super::STORE_REFUSED,
        super::CONFIRMATION_REQUIRED,
        super::UNKNOWN_LAYER,
        super::MALFORMED_DESCRIPTOR,
        super::DUPLICATE_LAYER,
        super::SCOPE_MISMATCH,
        super::UNSUPPORTED_SOURCE_KIND,
    ],
    reference: Some("docs/reference/map.md"),
    availability: super::availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    // A local file write is not a gated effect class, so the gate is declared
    // here: deleting a prepared layer's payload is not undoable from the CLI.
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "removing a prepared local layer deletes its copied payload",
        )
        .remedy(super::CONFIRMATION_REQUIRED.remedy));
    }
    let scope = super::scope(inputs)?;
    let answer = prepared::execute(
        &scope,
        Op::Remove {
            id: inputs.require("layer")?.trim().to_owned(),
        },
    )
    .map_err(super::refuse)?;
    let released = answer["released"].as_array().cloned().unwrap_or_default();
    Ok(super::stamped(
        &scope,
        &answer,
        json!({
            "layer": answer["receipt"]["id"].clone(),
            "removed": answer["receipt"]["removed"].clone(),
            "release": answer["receipt"]["release"].clone(),
            "deleted": released.len(),
            "deleted_files": released,
        }),
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "removed {} · {} payload file(s) deleted\n",
        data["layer"].as_str().unwrap_or("?"),
        data["deleted"],
    )
}
