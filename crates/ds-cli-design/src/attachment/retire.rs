//! `ds design attachment retire` — soft-delete a file or one of its revisions.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::attachment::download::{ATTACHMENT_ARG, REVISION_ARG};

const RESTORE_ARG: Arg = Arg {
    name: "restore",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Bring the retired file or revision back.",
};

pub static COMMAND: Command = Command {
    id: "design.attachment.retire",
    path: &["design", "attachment", "retire"],
    contract: 1,
    summary: "Retire an attachment or one of its revisions, reversibly.",
    purpose: "\
Soft-deletes the whole logical file, or with --revision just one revision. \
Nothing is erased: the record and its bytes stay, --restore brings it back, and \
a retired revision remains downloadable by its exact id. When the retired \
revision was the current latest, the pointer falls back to the newest remaining \
ready revision — derived, never invented, and cleared honestly when nothing \
ready is left. The application reads the file's current version and retires \
under it, so a concurrent publish is refused rather than overwritten.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[ATTACHMENT_ARG, REVISION_ARG, RESTORE_ARG, DESCRIPTOR_ARG],
    output: "The project, the `attachment`, the `revision` if one was named, the file's `state`, the resulting `latest` pointer, and the committed `version`.",
    examples: &[Example {
        command: "ds design attachment retire --attachment att-site-a-bak --revision rev-2 --yes",
        note: "Omit --revision to retire the whole file.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("attachment".into(), json!(inputs.require("attachment")?));
    if let Some(revision) = inputs.value("revision") {
        arguments.insert("revision".into(), json!(revision));
    }
    if inputs.switch("restore") {
        arguments.insert("restore".into(), json!(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ATTACHMENT_RETIRE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "{}{} is now {} · latest {} · v{}\n",
        data["attachment"].as_str().unwrap_or("?"),
        data["revision"]
            .as_str()
            .map(|revision| format!(" {revision}"))
            .unwrap_or_default(),
        data["state"].as_str().unwrap_or("?"),
        data["latest"].as_str().unwrap_or("none"),
        data["version"].as_u64().unwrap_or(0),
    )
}
