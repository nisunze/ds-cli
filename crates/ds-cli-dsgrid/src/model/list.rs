//! `ds dsgrid model list` — every local model, and which one is active.
//!
//! The entry point for the whole family: `set-active` and `publish-version`
//! both need an opaque local model id, and until now only the application's
//! own Grid Models panel could supply one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::model::{
    AMBIGUOUS, AUTH_CONTEXT_MISMATCH, DEFAULT_LIST_LIMIT, DESCRIPTOR_ARG, INVALID_NUMBER,
    LOCAL_TIMEOUT, MAX_LIST_LIMIT, NOT_PAIRED, PAIRING_REJECTED, REFUSED, UNREACHABLE, UNREADABLE,
    UNSUPPORTED,
};

const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some(DEFAULT_LIST_LIMIT),
    choices: &[],
    summary: "Rows in one page (1-500). The total is always reported.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.list",
    path: &["dsgrid", "model", "list"],
    contract: 1,
    summary: "List DS GridDesign's local models and which one is active.",
    purpose: "\
Names every DS Grid model held locally by the paired application, whether or \
not it has a live worker, with the opaque id every other command in this \
family needs. A model that is open but has not reached its first checkpoint is \
included, so the list never denies something the operator is looking at. Reads \
no project: a session with none selected is an ordinary session here.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[LIMIT_ARG, DESCRIPTOR_ARG],
    output: "\
`active_model`, the matched `total`, `more` when the page was cut, and rows of \
`model`, `name`, `active`, `open`, `revision` and — where the model has one — \
`content_digest`, `size_bytes` and the `project_binding` it was published \
from. Never model content.",
    examples: &[Example {
        command: "ds dsgrid model list --output json",
        note: "Read .data.models[].model to feed set-active or publish-version.",
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
        INVALID_NUMBER,
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: crate::model::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The declared default is sent rather than left implicit: the application
    // has a default of its own, and a help screen that states one number while
    // a second one applies is the kind of drift this repository refuses.
    let limit = crate::model::integer(
        inputs.value("limit").unwrap_or(DEFAULT_LIST_LIMIT),
        "limit",
        1,
        MAX_LIST_LIMIT,
    )?;
    let mut arguments = Map::new();
    arguments.insert("limit".into(), json!(limit));

    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    crate::model::invoke(
        &descriptor,
        &crate::model::MODEL_LIST,
        Value::Object(arguments),
        LOCAL_TIMEOUT,
    )
    .map_err(crate::model::classify)
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let active = data["active_model"].as_str().unwrap_or("");
    let mut out = format!(
        "{} local · active {}\n",
        crate::model::plural(total, "DS Grid model"),
        if active.is_empty() { "none" } else { active },
    );
    let rows = data["models"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for row in rows {
        out.push_str(&crate::model::model_line(row, active));
    }
    if data["more"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  … {} more; raise --limit\n",
            total.saturating_sub(rows.len() as u64)
        ));
    }
    out
}
