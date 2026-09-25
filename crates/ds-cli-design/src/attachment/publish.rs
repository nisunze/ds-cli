//! `ds design attachment publish` — add one immutable revision.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
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

const VERSION_DIGEST_ARG: Arg = Arg::value(
    "version-digest",
    "<sha256>",
    "MV only: the pinned revision's model digest. Omitted with an MV --version, it is read from the catalogue.",
);
const MEDIA_TYPE_ARG: Arg = Arg::value(
    "media-type",
    "<type/subtype>",
    "Declared media type; application/octet-stream when omitted.",
);
const NO_LATEST_ARG: Arg = Arg::switch(
    "no-latest",
    "Record the revision without moving latest, e.g. to back-fill a historical file.",
);
const SOURCE_KIND_ARG: Arg = Arg::value(
    "source-kind",
    "<token>",
    "Where the bytes came from, e.g. pls_cadd or client; recorded, never interpreted.",
);
const SOURCE_REF_ARG: Arg = Arg::value(
    "source-ref",
    "<text>",
    "A provenance reference, e.g. the submission or transmittal it shipped with.",
);

pub static COMMAND: Command = Command {
    id: "design.attachment.publish",
    path: &["design", "attachment", "publish"],
    contract: 1,
    summary: "Publish one immutable file revision onto a design object.",
    purpose: "Upload exact opaque bytes for the explicit project through a server-granted Storage session, then finalize an immutable digest/generation. LV version pins use vN; MV pins use the exact content revision a submission was published as, and carry its model digest (read from the catalogue unless --version-digest names it), so a delivered .bak binds to the package it describes. Existing files capture a pointer fence, refusing concurrent changes. --no-latest back-fills without moving latest.",
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
        VERSION_DIGEST_ARG,
        LABEL_ARG,
        PURPOSE_ARG,
        MEDIA_TYPE_ARG,
        SOURCE_KIND_ARG,
        SOURCE_REF_ARG,
        NO_LATEST_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "The project, the object, its `object_version` and `object_version_digest`, the `attachment` and `revision` ids, the revision `ordinal`, the file's `bytes`, `digest` and `media_type`, whether it is now `latest`, and the attachment's committed `version`.",
    examples: &[Example {
        command: "ds design attachment publish --project <id> --kind mv_model --object mv_line_a --path ./MV_LINE_A.bak --version rev_2 --purpose native_workspace --source-kind pls_cadd --yes",
        note: "A PLS-CADD backup is an ordinary opaque attachment; nothing parses it. The model digest of rev_2 is pinned beside it.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED, crate::CONFIRMATION_REQUIRED],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
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
            media_type: inputs.value("media-type").map(str::to_owned),
            source_kind: inputs.value("source-kind").map(str::to_owned),
            source_reference: inputs.value("source-ref").map(str::to_owned),
            make_latest: !inputs.switch("no-latest"),
            bytes,
        },
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "published r{} of {} on {}{} · {} bytes · v{}{}\n",
        data["ordinal"].as_u64().unwrap_or(0),
        data["attachment"].as_str().unwrap_or("?"),
        data["object"].as_str().unwrap_or("?"),
        data["object_version"]
            .as_str()
            .map(|version| format!(" @ {version}"))
            .unwrap_or_default(),
        data["bytes"].as_u64().unwrap_or(0),
        data["version"].as_u64().unwrap_or(0),
        if data["latest"] == false {
            " · latest unchanged"
        } else {
            ""
        },
    )
}
