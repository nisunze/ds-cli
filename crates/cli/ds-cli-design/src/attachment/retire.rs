//! `ds design attachment retire` — soft-delete a file or one of its revisions.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
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

const REASON_ARG: Arg = Arg::value(
    "reason",
    "<text>",
    "Why, for the audit record (up to 2000 characters).",
);

pub static COMMAND: Command = Command {
    id: "design.attachment.retire",
    path: &["design", "attachment", "retire"],
    contract: 1,
    summary: "Retire an attachment or one of its revisions, reversibly.",
    purpose: "Retire or restore a file or one exact revision for the explicit project. Records and bytes remain; latest points to the newest ready revision or clears. The captured pointer fence refuses concurrent changes. --reason goes to the audit record.",
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
        REASON_ARG,
    ],
    output: "The project, the `attachment`, the `revision` if one was named, the file's `state`, the resulting `latest` pointer, and the committed `version`.",
    examples: &[Example {
        command: "ds design attachment retire --project <id> --attachment att-site-a-bak --revision rev-2 --reason \"superseded by the v2 submission\" --yes",
        note: "Omit --revision to retire the whole file.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED, crate::CONFIRMATION_REQUIRED],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::Retire {
            attachment: inputs.require("attachment")?.into(),
            revision: inputs.value("revision").map(str::to_owned),
            restore: inputs.switch("restore"),
            reason: inputs.value("reason").map(str::to_owned),
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
