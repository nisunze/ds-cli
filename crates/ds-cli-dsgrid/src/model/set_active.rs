//! `ds dsgrid model set-active` — make one local model the active one.
//!
//! "Active" here is exactly one thing: the single open session occupying
//! Profile and editing in the paired application. That fact is persisted by
//! the session itself; this command asks the application to make the
//! transition and never writes the record, so there is no second notion of
//! "current" anywhere. It is browser-local and says nothing about any project.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::model::workspace;
use ds_command_kernel::local_models::Op;

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The working copy to open, by the id `ds dsgrid model list` reports.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.set-active",
    path: &["dsgrid", "model", "set-active"],
    contract: 1,
    summary: "Open one of this machine's working copies as the active one.",
    purpose: "\
Makes one working copy the one an editing session starts from on this \
machine. Idempotent: naming the copy that is already open reports \
`changed: false` and touches nothing, so a retry after a lost answer is safe. \
This is local state and reaches no project; it is not a claim about any \
project catalogue revision.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[MODEL_ARG, workspace::LANE_ARG, workspace::ACCOUNT_ARG],
    output: "\
`status` (`active` or `unchanged`), `active_model`, `changed`, the model's \
`name` and `revision`, and `previous_active_model` when it moved.",
    examples: &[Example {
        command: "ds dsgrid model set-active --model gm-local-7 --output json",
        note: "Read .data.changed; false means it was already the active model.",
        runnable: false,
    }],
    refusals: workspace::REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    let outcome = workspace::execute(inputs, Op::SetActive { id: id.clone() }, None)?;
    let opened = outcome
        .model
        .as_ref()
        .ok_or_else(|| Failure::internal("local_model_store_unavailable", "nothing was opened"))?;
    Ok(json!({
        "status": if outcome.active_changed { "active" } else { "unchanged" },
        "active_model": outcome.catalogue.active,
        "changed": outcome.active_changed,
        "model": workspace::row(opened, outcome.catalogue.active.as_deref()),
    }))
}

pub fn render(data: &Value) -> String {
    let model = data["active_model"].as_str().unwrap_or("?");
    if !data["changed"].as_bool().unwrap_or(false) {
        return format!(
            "unchanged · {model} was already active · {}\n",
            data["name"].as_str().unwrap_or(""),
        );
    }
    format!(
        "active {model} · {}\n  revision   {}\n  previous   {}\n",
        data["name"].as_str().unwrap_or(""),
        data["revision"].as_str().unwrap_or("—"),
        data["previous_active_model"].as_str().unwrap_or("none"),
    )
}
