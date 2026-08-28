//! `ds design attachment publish` — add one immutable revision.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, KIND_ARG, OBJECT_ARG, VERSION_ARG};

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
replacing it. Pass --version to bind the revision to one exact object version. \
Without --attachment this starts a new logical file; with it, the application \
reads that file's current version and adds a revision under it, so a concurrent \
publish is refused rather than overwritten.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        OBJECT_ARG,
        PATH_ARG,
        ATTACHMENT_ARG,
        VERSION_ARG,
        LABEL_ARG,
        PURPOSE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, the object, the `attachment` and `revision` ids, the revision `ordinal`, the file's `bytes`, and the attachment's committed `version`.",
    examples: &[Example {
        command: "ds design attachment publish --kind mv_model --object mv_line_a --path ./MV_LINE_A.bak --version rev_2 --yes",
        note: "A PLS-CADD backup is an ordinary opaque attachment; nothing parses it.",
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
        crate::INVALID_ANCHOR,
        crate::TOO_LARGE,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = crate::anchor(inputs)?;
    arguments.insert("path".into(), json!(inputs.require("path")?));
    for flag in ["attachment", "label", "purpose"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ATTACHMENT_PUBLISH,
        Value::Object(arguments),
        crate::PUBLISH_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
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
