//! `ds design attachment retire` — soft-delete a file or one of its revisions.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

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
ready is left. The native client captures the file's current version and retires \
under it, so a concurrent publish is refused rather than overwritten.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
        ATTACHMENT_ARG,
        REVISION_ARG,
        RESTORE_ARG,
    ],
    output: "The project, the `attachment`, the `revision` if one was named, the file's `state`, the resulting `latest` pointer, and the committed `version`.",
    examples: &[Example {
        command: "ds design attachment retire --project <id> --attachment att-site-a-bak --revision rev-2 --yes",
        note: "Omit --revision to retire the whole file.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED, crate::CONFIRMATION_REQUIRED],
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::Retire {
            attachment: inputs.require("attachment")?.into(),
            revision: inputs.value("revision").map(str::to_owned),
            restore: inputs.switch("restore"),
        },
    )
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
