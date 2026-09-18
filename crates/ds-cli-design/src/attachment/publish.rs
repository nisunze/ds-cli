//! `ds design attachment publish` — add one immutable revision.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{KIND_ARG, OBJECT_ARG, VERSION_ARG};

const PATH_ARG: Arg = Arg {
    name: "path",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "The file to publish. Any type; nothing here parses it.",
};

const ATTACHMENT_ARG: Arg = Arg {
    name: "attachment",
    kind: ArgKind::Value,
    value: "<attachment-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Add a revision to this existing file instead of starting a new one.",
};

const LABEL_ARG: Arg = Arg {
    name: "label",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "What the file is. Defaults to its name.",
};

const PURPOSE_ARG: Arg = Arg {
    name: "purpose",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Why it is attached, e.g. native_workspace or client_deliverable.",
};

pub static COMMAND: Command = Command {
    id: "design.attachment.publish",
    path: &["design", "attachment", "publish"],
    contract: 1,
    summary: "Publish one immutable file revision onto a design object.",
    purpose: "\
Uploads the file to a server-minted session and registers one immutable \
revision. Earlier bytes are never touched: each revision owns its own storage \
object, its own server-verified SHA-256 and its own generation, so a new \
`.bak` for a later version sits alongside the earlier one rather than \
replacing it. Use an explicit --project without a desktop. LV --version is an assigned vN; MV --version is its content revision id, never governance vN. Pass --version to bind the revision to one exact object version. \
Without --attachment this starts a new logical file; with it, the native owner \
reads that file's current version and adds a revision under it, so a concurrent \
publish is refused rather than overwritten.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        OBJECT_ARG,
        PATH_ARG,
        ATTACHMENT_ARG,
        VERSION_ARG,
        LABEL_ARG,
        PURPOSE_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "The project, the object, the `attachment` and `revision` ids, the revision `ordinal`, the file's `bytes`, and the attachment's committed `version`.",
    examples: &[Example {
        command: "ds design attachment publish --project <id> --kind mv_model --object mv_line_a --path ./MV_LINE_A.bak --version rev_2 --yes",
        note: "A PLS-CADD backup is an ordinary opaque attachment; nothing parses it.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED, crate::CONFIRMATION_REQUIRED],
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    use std::io::Read;
    let path = std::path::Path::new(inputs.require("path")?);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(ds_client_core::design_attachments::MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| {
            Failure::invalid("design_attachment_refused", error.to_string())
                .remedy("Choose a readable opaque file up to512MiB")
        })?;
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Failure::invalid(
                "design_attachment_refused",
                "File name is not readable UTF-8",
            )
            .remedy("Use a portable file name")
        })?;
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::Publish {
            object: super::object(inputs)?,
            attachment: inputs.value("attachment").map(str::to_owned),
            file: file.into(),
            label: inputs.value("label").map(str::to_owned),
            purpose: inputs.value("purpose").map(str::to_owned),
            bytes,
        },
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "published r{} of {} on {} · {} bytes · v{}\n",
        data["ordinal"].as_u64().unwrap_or(0),
        data["attachment"].as_str().unwrap_or("?"),
        data["object"].as_str().unwrap_or("?"),
        data["bytes"].as_u64().unwrap_or(0),
        data["version"].as_u64().unwrap_or(0),
    )
}
