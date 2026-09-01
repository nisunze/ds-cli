//! `ds dsgrid model create-local` — one empty local model.
//!
//! Named `create-local` rather than `create` because the reverted family used
//! the bare verb for something else entirely: registering a model in a
//! project. This creates nothing outside the paired application.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::model::{
    AMBIGUOUS, AUTH_CONTEXT_MISMATCH, DESCRIPTOR_ARG, LOCAL_TIMEOUT, NOT_PAIRED, PAIRING_REJECTED,
    REFUSED, UNREACHABLE, UNREADABLE, UNSUPPORTED, UNSUPPORTED_GRID_CRS,
};

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Display name for the new model. The application names it if omitted.",
};

const CRS_ARG: Arg = Arg {
    name: "crs",
    kind: ArgKind::Value,
    value: "<crs>",
    required: false,
    default: None,
    choices: &[],
    summary: "Projected metric coordinate system, e.g. EPSG:32735. The app's default if omitted.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.create-local",
    path: &["dsgrid", "model", "create-local"],
    contract: 1,
    summary: "Create one empty local DS Grid model and open it.",
    purpose: "\
Creates one empty model in the paired application's own local store, through \
the same call its Grid Models panel makes for `New local model…`. The new \
model opens as the active one — this is the single local command that changes \
which model occupies Profile and editing — so the receipt says so rather than \
leaving it to be discovered. It reaches no project and registers nothing in a \
catalogue; publishing a revision is `ds dsgrid publish-version`.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[NAME_ARG, CRS_ARG, DESCRIPTOR_ARG],
    output: "\
`status: created`, the new opaque `model` id, its `name`, `crs` and first \
`revision`, plus `active_model` and `became_active`.",
    examples: &[Example {
        command: "ds dsgrid model create-local --name \"Kamonyi MV\" --crs EPSG:32735 --output json",
        note: "Read .data.model; the new model is already the active one.",
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
        UNSUPPORTED_GRID_CRS,
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: crate::model::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The set of authorable coordinate systems belongs to the application, so
    // it is not copied here: an unsupported one comes back as its own named
    // refusal rather than as a guess made locally against a stale list.
    let mut arguments = Map::new();
    for (flag, key) in [("name", "name"), ("crs", "crs")] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(key.into(), json!(value));
        }
    }

    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    crate::model::invoke(
        &descriptor,
        &crate::model::MODEL_CREATE,
        Value::Object(arguments),
        LOCAL_TIMEOUT,
    )
    .map_err(crate::model::classify)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "created {} · {}\n",
        data["model"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or(""),
    );
    if let Some(crs) = data["crs"].as_str() {
        out.push_str(&format!("  crs        {crs}\n"));
    }
    out.push_str(&format!(
        "  revision   {}\n  active     {}\n",
        data["revision"].as_str().unwrap_or("—"),
        if data["became_active"].as_bool().unwrap_or(false) {
            "yes, this model now occupies Profile"
        } else {
            "unchanged"
        },
    ));
    out
}
