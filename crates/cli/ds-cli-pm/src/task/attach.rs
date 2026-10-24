//! `ds pm task attach` — put one file on one task, in one command.
//!
//! The task attachment routes already exist behind `POST /api/v1/pm`:
//! `start_attachment_upload` reserves the row and mints one resumable session,
//! the bytes go straight to storage, and `finalize_attachment_upload` makes
//! ds-brain verify the stored object before the row turns `ready`. The core's
//! `TaskAttach` door runs those three steps; this surface reads the file and
//! shapes the receipt. It decides nothing.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_management::{AttachmentFile, Command as PMCommand};
use serde_json::{Value, json};

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

pub const FILE_INVALID: Refusal = Refusal {
    code: "attachment_file_invalid",
    when: "--file cannot be read, is empty or above 5 TiB, changed while read, has no plain file name, or --content-type is not one type/subtype",
    remedy: "pass a readable file not being written, and omit --content-type or give one like application/pdf",
};
pub const UNVERIFIED: Refusal = Refusal {
    code: "attachment_unverified",
    when: "the server's attachment answer is incomplete, names another task, or was not verified ready",
    remedy: "read the task again with `ds pm task read`; repeat the attach only if no ready attachment is listed",
};

pub static COMMAND: Command = Command {
    id: "pm.task.attach",
    path: &["pm", "task", "attach"],
    contract: 1,
    summary: "Attach one local file to a task (needs --yes).",
    purpose: "\
Puts a document, photo or drawing on an existing Project Work task in one \
command. The server reserves a row, the bytes go straight to storage, and the \
server checks the stored object's size and type before the row becomes \
ready; only a ready row on the named task counts. Headless on the named \
project. Attaching the same file again after an interruption completes your \
unfinished row instead of adding one.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::TASK_ARG,
        FILE,
        CONTENT_TYPE,
        crate::LANE_ARG,
    ],
    output: "\
project, task, the server-verified ready `attachment` row (id, version, \
original_name, content_type, size_bytes, state), the local `bytes` and \
`sha256`, `restarted`, and the duplicate rows `reclaimed` or `unreclaimed`. \
The upload session never appears.",
    examples: &[Example {
        command: "ds pm task attach --project <exact-id> --task <task-id> --file ./crossing-survey.pdf --yes --output json",
        note: "`attachment.state` is `ready` only after the server verified the stored bytes.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<23>(&[FILE_INVALID, UNVERIFIED]),
    reference: Some("docs/reference/pm.md"),
    search: &["upload", "document", "photo", "drawing"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn file_invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(FILE_INVALID.code, message).remedy(FILE_INVALID.remedy)
}

/// The file to attach, measured and hashed in bounded reads (never held in
/// memory), then checked by the core's own rules.
fn attachment(inputs: &Inputs) -> Result<AttachmentFile, Failure> {
    read_attachment(inputs.require("file")?, inputs.value("content-type"))
}

pub(crate) fn read_attachment(
    path: &str,
    content_type: Option<&str>,
) -> Result<AttachmentFile, Failure> {
    let path = std::path::Path::new(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| file_invalid("--file has no plain UTF-8 file name"))?;
    AttachmentFile::from_path(name, content_type, path)
        .map_err(|error| file_invalid(format!("--file: {error}")))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let task = inputs.require("task")?;
    let file = attachment(inputs)?;
    let result = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &PMCommand::TaskAttach {
            task_id: task.to_owned(),
            file: file.clone(),
        },
    )?
    .into_result();
    let item = &result["item"];
    if item["state"] != "ready"
        || item["linked_entity_id"].as_str() != Some(task)
        || item["size_bytes"].as_u64() != Some(file.len())
        || result["sha256"].as_str() != Some(file.sha256().as_str())
    {
        return Err(Failure::invalid(
            UNVERIFIED.code,
            "the attachment answer was not a ready row on this task",
        )
        .remedy(UNVERIFIED.remedy));
    }
    Ok(json!({
        "project": project,
        "task": task,
        "attachment": item,
        "bytes": file.len(),
        "sha256": file.sha256(),
        "restarted": result["restarted"] == true,
        "reclaimed": result["reclaimed"],
        "unreclaimed": result["unreclaimed"],
    }))
}

pub fn render(data: &Value) -> String {
    let item = &data["attachment"];
    format!(
        "attached {} to task {} · {} · {} bytes · {}\n  sha256 {}\n",
        item["original_name"].as_str().unwrap_or("?"),
        data["task"].as_str().unwrap_or("?"),
        item["id"].as_str().unwrap_or("?"),
        data["bytes"].as_u64().unwrap_or(0),
        item["state"].as_str().unwrap_or("?"),
        data["sha256"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;
