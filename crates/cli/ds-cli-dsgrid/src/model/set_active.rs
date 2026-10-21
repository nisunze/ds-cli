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

const REFUSALS: &[ds_cli_contract::spec::Refusal; workspace::REFUSALS.len() + 2] = &refusals();
const fn refusals() -> [ds_cli_contract::spec::Refusal; workspace::REFUSALS.len() + 2] {
    let mut all = [workspace::DRAFT_PENDING; workspace::REFUSALS.len() + 2];
    let mut index = 0;
    while index < workspace::REFUSALS.len() {
        all[index] = workspace::REFUSALS[index];
        index += 1;
    }
    all[index] = workspace::DRAFT_PENDING;
    all[index + 1] = workspace::REVISION_CONFLICT;
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.model.set-active",
    path: &["dsgrid", "model", "set-active"],
    contract: 3,
    summary: "Open one of this machine's working copies as the active one.",
    purpose: "\
Selects the working copy an editing session starts from on this machine. \
Idempotent: the active copy reports `changed: false` and stays unchanged. \
Local state; no project authority. Older packages use the one external migration before activation. It preserves original bytes and receipt locally, saves strict-current bytes, and digest-fences the local catalogue. No project publication or authored revision bump.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[MODEL_ARG, workspace::LANE_ARG, workspace::ACCOUNT_ARG],
    output: "\
`status` (`active` or `unchanged`), `active_model`, `changed`, the model's \
`name` and `revision`, and `previous_active_model` when it moved. `format_migration` names original/current SHA, saved original/current packages and preservation receipt; null when already current.",
    examples: &[Example {
        command: "ds dsgrid model set-active --model gm-local-7 --output json",
        note: "Read .data.changed; false means it was already the active model.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    let located = workspace::locate(inputs, &id)?;
    let migration = workspace::prepare_open(&located)?;
    let outcome = workspace::execute_in(&located.scope, Op::SetActive { id: id.clone() }, None)?;
    let opened = outcome
        .model
        .as_ref()
        .ok_or_else(|| Failure::internal("local_model_store_unavailable", "nothing was opened"))?;
    Ok(json!({
        "status": if outcome.active_changed { "active" } else { "unchanged" },
        "active_model": outcome.catalogue.active,
        "changed": outcome.active_changed,
        "format_migration": migration,
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
