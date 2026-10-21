//! `ds dsgrid model unlink` — forget which PLS-CADD workspace a working copy
//! was pinned to.
//!
//! The inverse of `ds dsgrid model link`. The working copy, its package and
//! the workspace folder are untouched; only this machine's catalogue row
//! stops naming the folder, so `ds dsgrid-exchange sync` no longer writes into
//! it. A copy that carries no link is refused by name rather than reported as
//! unlinked.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::Op;
use serde_json::{Value, json};

use crate::model::{pls_source, workspace};

const MODEL_ARG: Arg = Arg::value(
    "model",
    "<model-id>",
    "The working copy whose PLS-CADD workspace link to remove.",
)
.required();

pub static COMMAND: Command = Command {
    id: "dsgrid.model.unlink",
    path: &["dsgrid", "model", "unlink"],
    contract: 1,
    summary: "Remove a working copy's link to its PLS-CADD workspace.",
    purpose: "Stops `ds dsgrid-exchange sync` writing into the folder `ds dsgrid model link` recorded. The working copy, its package and the workspace folder are unchanged; relink with `ds dsgrid model link`. A copy with no link is refused as local_model_request_invalid.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[MODEL_ARG, workspace::LANE_ARG, workspace::ACCOUNT_ARG],
    output: "status unlinked, the model row (pls_source now null) and the removed link {path, digest, pls_version, member_versions, member_count, linked_at}.",
    examples: &[Example {
        command: "ds dsgrid model unlink --model local-5ff16cd0a3d6416b --account <uid> --output json",
        note: "Stop syncing this copy into its PLS-CADD folder.",
        runnable: false,
    }],
    refusals: workspace::REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["pls-cadd", "disconnect", "unpin"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    // The link being removed, read from the catalogue before the transition.
    // The kernel refuses the transition itself if the copy is absent or has
    // no link, so this read decides nothing.
    let removed = workspace::read(inputs)?
        .models
        .iter()
        .find(|model| model.id == id)
        .and_then(|model| model.pls_source.clone());
    let outcome = workspace::execute(inputs, Op::Unlink { id }, None)?;
    let row = outcome.model.as_ref().ok_or_else(|| {
        Failure::internal("local_model_store_unavailable", "nothing was unlinked")
    })?;
    Ok(json!({
        "status": "unlinked",
        "model": workspace::row(row, outcome.catalogue.active.as_deref()),
        "removed": pls_source::link_json(removed.as_ref()),
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "unlinked {} · {}\n  removed    {}\n",
        data["model"]["model"].as_str().unwrap_or("?"),
        data["model"]["name"].as_str().unwrap_or(""),
        data["removed"]["path"]
            .as_str()
            .unwrap_or("(no link recorded)"),
    )
}
