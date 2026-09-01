//! `ds dsgrid model set-active` — make one local model the active one.
//!
//! "Active" here is exactly one thing: the single open session occupying
//! Profile and editing in the paired application. That fact is persisted by
//! the session itself; this command asks the application to make the
//! transition and never writes the record, so there is no second notion of
//! "current" anywhere. It is browser-local and says nothing about any project.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::model::{
    AMBIGUOUS, AUTH_CONTEXT_MISMATCH, DESCRIPTOR_ARG, LOCAL_MODEL_NOT_FOUND, LOCAL_TIMEOUT,
    NOT_PAIRED, PAIRING_REJECTED, REFUSED, UNREACHABLE, UNREADABLE, UNSUPPORTED,
};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The local model to open, by the id `ds dsgrid model list` reports.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.set-active",
    path: &["dsgrid", "model", "set-active"],
    contract: 1,
    summary: "Open one local model as the active model in Profile.",
    purpose: "\
Makes one local model the single open session occupying Profile and editing, \
reusing the application's own occupancy path — the same transition its \
`Open in Profile…` row action performs. Idempotent: naming the model that is \
already active reports `changed: false` and touches nothing, so a retry after \
a lost answer is safe. This is local state and reaches no project; it is not \
a claim about any project catalogue revision.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[MODEL_ARG, DESCRIPTOR_ARG],
    output: "\
`status` (`active` or `unchanged`), `active_model`, `changed`, the model's \
`name` and `revision`, and `previous_active_model` when it moved.",
    examples: &[Example {
        command: "ds dsgrid model set-active --model gm-local-7 --output json",
        note: "Read .data.changed; false means it was already the active model.",
        runnable: false,
    }],
    refusals: &[
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        UNSUPPORTED,
        UNREADABLE,
        AUTH_CONTEXT_MISMATCH,
        LOCAL_MODEL_NOT_FOUND,
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: crate::model::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = json!({ "model": inputs.require("model")? });

    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    crate::model::invoke(
        &descriptor,
        &crate::model::MODEL_SET_ACTIVE,
        arguments,
        LOCAL_TIMEOUT,
    )
    .map_err(crate::model::classify)
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
