//! `ds pm record attach` — put one file on one record, in one command.
//!
//! The record is read first, so a file is never reserved on an id that is not
//! a record the caller may read. The bytes then go through the same Kernel
//! upload flow as a task attachment: a retry of the same file completes the
//! caller's interrupted row on this record instead of adding another, and only
//! a row ds-brain verified `ready` on this record counts. This surface reads
//! the file and shapes the receipt. It decides nothing.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use ds_client_core::project_management::{AttachmentFile, Command as PMCommand};
use serde_json::{Value, json};

use super::RECORD_ARG;
use crate::task::attach::{FILE_INVALID, read_attachment};

const FILE: Arg = Arg::value(
    "file",
    "<path>",
    "The local file to attach, from 1 byte up to the 5 TiB storage maximum; it streams from disk and its file name is kept.",
)
.required();
const CONTENT_TYPE: Arg = Arg::value(
    "content-type",
    "<type/subtype>",
    "Media type to record; named from the file extension when omitted.",
);

pub const UNVERIFIED: Refusal = Refusal {
    code: "attachment_unverified",
    when: "the server did not verify a ready row of this file on this record",
    remedy: "read the record; repeat only if its pmAttachments list no ready row of this file",
};

pub static COMMAND: Command = Command {
    id: "pm.record.attach",
    path: &["pm", "record", "attach"],
    contract: 1,
    summary: "Attach one local file to a record or project note (needs --yes).",
    purpose: "\
Puts a file on an existing correspondence record or project note through the \
same resumable upload a task attachment uses. The record is read first; the \
server reserves a row, the bytes go straight to storage, and the server \
checks the stored object's size and type before the row becomes ready; only \
a ready row on the named record counts. Attaching the same file again after \
an interruption completes your unfinished row instead of adding one. \
`ds pm record read` lists the record's uploads, ready or not, under \
pmAttachments. Headless on the named project.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        RECORD_ARG,
        FILE,
        CONTENT_TYPE,
        crate::LANE_ARG,
    ],
    output: "\
project, record, the server-verified ready `attachment` row (id, version, \
original_name, content_type, size_bytes, state), the local `bytes` and \
`sha256`, and `restarted` when an interrupted upload was completed. The \
upload session never appears.",
    examples: &[Example {
        command: "ds pm record attach --project <exact-id> --record <record-id> --file ./EPCGIS_1.RAR --yes --output json",
        note: "`attachment.state` is `ready` only after the server verified the stored bytes; an interrupted run is resumed by the same command.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<24>(&[
        FILE_INVALID,
        UNVERIFIED,
        crate::RECORD_NOT_FOUND,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["upload", "document", "archive", "correspondence"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// Attach one already-read file to one record and accept only a verified
/// ready row on it: the receipt `pm record create --attach` also returns.
pub(crate) fn upload(
    lane: &str,
    project: &str,
    record: &str,
    file: &AttachmentFile,
) -> Result<Value, Failure> {
    let result = ds_cli_auth::project_management_for_project(
        lane,
        project,
        &PMCommand::RecordAttach {
            record_id: record.to_owned(),
            file: file.clone(),
        },
    )?
    .into_result();
    let item = &result["item"];
    if item["state"] != "ready"
        || item["linked_entity_type"] != "record"
        || item["linked_entity_id"].as_str() != Some(record)
        || item["size_bytes"].as_u64() != Some(file.len())
        || result["sha256"].as_str() != Some(file.sha256().as_str())
    {
        return Err(Failure::invalid(
            UNVERIFIED.code,
            "the attachment answer was not a ready row on this record",
        )
        .remedy(UNVERIFIED.remedy));
    }
    Ok(json!({
        "attachment": item,
        "bytes": file.len(),
        "sha256": file.sha256(),
        "restarted": result["restarted"] == true,
    }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let record = inputs.require("record")?;
    let file = read_attachment(inputs.require("file")?, inputs.value("content-type"))?;
    crate::correspondence(
        lane,
        project,
        &Action::RecordRead {
            record_id: record.to_owned(),
        },
    )?;
    let mut receipt = upload(lane, project, record, &file)?;
    receipt["project"] = json!(project);
    receipt["record"] = json!(record);
    Ok(receipt)
}

pub fn render(data: &Value) -> String {
    let item = &data["attachment"];
    format!(
        "attached {} to record {} · {} · {} bytes · {}{}\n  sha256 {}\n",
        item["original_name"].as_str().unwrap_or("?"),
        data["record"].as_str().unwrap_or("?"),
        item["id"].as_str().unwrap_or("?"),
        data["bytes"].as_u64().unwrap_or(0),
        item["state"].as_str().unwrap_or("?"),
        if data["restarted"] == true {
            " · resumed"
        } else {
            ""
        },
        data["sha256"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;
