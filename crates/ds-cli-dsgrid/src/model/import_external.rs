//! `ds dsgrid model import-external` — acquire an external `.dsgrid` file.
//!
//! Acquisition, and nothing else. The imported model does **not** take
//! Profile/editing occupancy, exactly as the application's own
//! `Import external model…` does not; the operator chooses it afterwards with
//! `set-active`. Naming it `import-external` rather than `import` is the
//! point: the reverted family's `import` meant "register this in my project",
//! which is a different act with a different authority and now has a different
//! name — `ds dsgrid publish-version`.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::model::{
    ABSOLUTE_PATH_REQUIRED, AMBIGUOUS, AUTH_CONTEXT_MISMATCH, DESCRIPTOR_ARG, LOCAL_TIMEOUT,
    MODEL_TOO_LARGE, NAME_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, UNREACHABLE, UNREADABLE,
    UNSUPPORTED, UNSUPPORTED_MODEL_SOURCE,
};

const PATH_ARG: Arg = Arg {
    name: "path",
    kind: ArgKind::Value,
    value: "<absolute-path.dsgrid>",
    required: true,
    default: None,
    choices: &[],
    summary: "The .dsgrid package to acquire, by absolute path on the application's machine.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.import-external",
    path: &["dsgrid", "model", "import-external"],
    contract: 1,
    summary: "Acquire an external .dsgrid file as a local model.",
    purpose: "\
Brings one `.dsgrid` package the operator already has into the paired \
application as a durable local model. This is source acquisition only: the \
imported model does not take Profile/editing occupancy, so choose it with \
`ds dsgrid model set-active` when you want to work in it. The path is read by \
the application — no model bytes cross this command in either direction — and \
a PLS-CADD workspace or `.bak` is refused by name, because converting one is \
`ds dsgrid-exchange`'s act, not this one's.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[PATH_ARG, NAME_ARG, DESCRIPTOR_ARG],
    output: "\
`status: imported`, the new opaque `model` id, its `name` and `revision`, the \
`imported_from` file name, `source_path`, `size_bytes`, and \
`became_active: false` — acquisition never activates.",
    examples: &[Example {
        command: "ds dsgrid model import-external --path /srv/models/kamonyi.dsgrid --output json",
        note: "Then `ds dsgrid model set-active --model <id>` to work in it.",
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
        ABSOLUTE_PATH_REQUIRED,
        UNSUPPORTED_MODEL_SOURCE,
        MODEL_TOO_LARGE,
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: crate::model::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = crate::model::external_dsgrid_path(inputs.require("path")?, "path")?;
    let mut arguments = Map::new();
    arguments.insert("path".into(), json!(path));
    if let Some(name) = inputs.value("name") {
        arguments.insert("name".into(), json!(name));
    }

    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    crate::model::invoke(
        &descriptor,
        &crate::model::MODEL_IMPORT,
        Value::Object(arguments),
        LOCAL_TIMEOUT,
    )
    .map_err(crate::model::classify)
}

pub fn render(data: &Value) -> String {
    format!(
        "imported {} · {}\n  from       {}\n  revision   {}\n  bytes      {}\n  active     {}\n",
        data["model"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or(""),
        data["imported_from"].as_str().unwrap_or("?"),
        data["revision"].as_str().unwrap_or("—"),
        data["size_bytes"].as_u64().unwrap_or(0),
        match data["active_model"].as_str() {
            Some(active) => format!("unchanged ({active})"),
            None => "unchanged (none)".to_string(),
        },
    )
}
