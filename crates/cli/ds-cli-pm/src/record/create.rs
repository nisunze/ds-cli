//! `ds pm record create` — file one exchange with an outside party.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use ds_client_core::project_management::AttachmentFile;
use serde_json::{Value, json};

use super::{
    AFFECTS_ARG, BODY_ARG, BODY_FILE_ARG, CATEGORY_ARG, CHANNEL_ARG, DIRECTION_ARG, DOCUMENT_ARG,
    EXTERNAL_NAME_ARG, HAPPENED_AT_ARG, ID_ARG, MEETING_NOTE_ARG, PARTY_ARG, REFERENCE_ARG,
    RESPONSE_DUE_ARG, RESPONSE_OWED_BY_ARG, SOURCE_ASSET_ARG, SOURCE_MESSAGE_ARG, SUBJECT_ARG,
};
use crate::LANE_ARG;

const ATTACH_ARG: Arg = Arg {
    name: "attach",
    kind: ArgKind::Repeated,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Attach files after filing; up to 10, any size, all checked first.",
};
const ATTACH_PARTIAL: Refusal = Refusal {
    code: "record_attachments_incomplete",
    when: "the record was filed but an upload was not verified ready",
    remedy: "keep the record id; attach the missing files with `ds pm record attach`",
};

pub static COMMAND: Command = Command {
    id: "pm.record.create",
    path: &["pm", "record", "create"],
    contract: 1,
    summary: "File a letter, email, call, meeting or transmittal as a record.",
    purpose: "\
Files one exchange with an outside party so it can be threaded, owed and \
blocked on, authored against the ingested .eml or screenshot (--source-asset), \
the in-app message (--source-message) or the meeting note (--meeting-note); an \
answer in a thread is `ds pm record reply`. Name the parties by id and say who owes the next answer with \
--response-owed-by: a party, or a member by email. A submission or \
transmittal carries at least one registered --document. Sensitivity is the \
strictest of the assets named. The same --id twice is refused, not \
duplicated. --attach files the record, then uploads each file as \
`ds pm record attach` does.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: crate::authored_args![
        CATEGORY_ARG.required(),
        CHANNEL_ARG.required(),
        DIRECTION_ARG.required(),
        SUBJECT_ARG.required(),
        BODY_ARG,
        BODY_FILE_ARG,
        HAPPENED_AT_ARG,
        REFERENCE_ARG,
        PARTY_ARG,
        EXTERNAL_NAME_ARG,
        SOURCE_ASSET_ARG,
        SOURCE_MESSAGE_ARG,
        MEETING_NOTE_ARG,
        RESPONSE_OWED_BY_ARG,
        RESPONSE_DUE_ARG,
        AFFECTS_ARG,
        DOCUMENT_ARG,
        ID_ARG,
        ATTACH_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "\
The project and `record` — id, thread, category, channel, direction, subject, \
`responseStatus`, who owes the answer and by when, sensitivity, what it was \
authored against — plus `attachments`, `documents`, `blockedTasks`, \
`resultingTasks` and `threadTotal`, exactly what `ds pm record read` answers. \
--attach adds the verified `uploads`.",
    examples: &[Example {
        command: "ds pm record create --category review --channel email --direction inbound --subject \"MV plan and profile — not approved\" --happened-at 2026-09-14 --party p_acme --source-asset a_1x2y3z4a5b6c --affects scope,schedule --yes --project <exact-id>",
        note: "The .eml was ingested first with `ds assets ingest`; read .data.record.id to reply or to make a task --from-record.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<36>(&[
        crate::RECORD_SOURCE_REQUIRED,
        crate::INVALID_RESPONSE_OWNER,
        crate::PARTY_NOT_FOUND,
        crate::ASSET_NOT_FOUND,
        crate::DOCUMENT_REQUIRED,
        crate::DOCUMENT_NOT_REGISTERED,
        crate::RECORD_EXISTS,
        crate::BOUND_EXCEEDED,
        crate::INVALID_DATE,
        crate::INVALID_STAMP,
        crate::INVALID_EMAIL,
        crate::INVALID_VALUE,
        crate::task::attach::FILE_INVALID,
        crate::record::attach::UNVERIFIED,
        ATTACH_PARTIAL,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "mail",
        "file letter",
        "call log",
        "minutes",
        "rfi",
        "instruction",
        "submission",
        "review",
        "who owes",
        "ball court",
        "response due",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// Every --attach file, read and bounded before anything is filed.
pub(crate) fn attachments(inputs: &Inputs) -> Result<Vec<AttachmentFile>, Failure> {
    let mut files = Vec::new();
    for path in inputs.repeated("attach") {
        files.push(crate::task::attach::read_attachment(path, None)?);
        ds_client_core::project_management::create_attached::validate_files(&files).map_err(
            |_| {
                Failure::invalid(
                    crate::task::attach::FILE_INVALID.code,
                    "a record or note takes 1 to 10 distinct attachments",
                )
                .remedy("pass up to 10 distinct readable files")
            },
        )?;
    }
    Ok(files)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let data = super::fields(inputs, true)?;
    let files = attachments(inputs)?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let report = crate::authoring::correspondence(
        inputs,
        lane,
        project,
        Action::RecordCreate {
            id: inputs.value("id").map(str::to_owned),
            data,
        },
    )?;
    let mut view = super::view(report)?;
    if files.is_empty() {
        return Ok(view);
    }
    let record = view["record"]["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            Failure::internal(
                crate::PLAN_UNREADABLE.code,
                "the filed record answered no id to attach to",
            )
            .remedy(crate::PLAN_UNREADABLE.remedy)
        })?
        .to_owned();
    let mut uploads = Vec::new();
    for (index, file) in files.iter().enumerate() {
        match super::attach::upload(lane, project, &record, file) {
            Ok(receipt) => uploads.push(receipt),
            Err(failure) => {
                return Err(Failure::failed(
                    ATTACH_PARTIAL.code,
                    "Record filed; not all attachments were verified ready",
                )
                .remedy(ATTACH_PARTIAL.remedy)
                .detail(
                    json!({"project": project, "recordId": record, "uploads": uploads,
                    "failed_attachment_index": index, "requested_attachment_count": files.len(),
                    "failure_code": failure.code()}),
                ));
            }
        }
    }
    view["uploads"] = Value::Array(uploads);
    Ok(view)
}

pub fn render(data: &Value) -> String {
    let mut out = format!("filed in {}\n", data["project"].as_str().unwrap_or("?"));
    out.push_str(&super::head(&data["record"]));
    out.push_str(&super::projections(data));
    for upload in data["uploads"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  uploaded {} · {} bytes · {}\n",
            upload["attachment"]["original_name"]
                .as_str()
                .unwrap_or("?"),
            upload["bytes"].as_u64().unwrap_or(0),
            upload["attachment"]["state"].as_str().unwrap_or("?"),
        ));
    }
    out
}

#[cfg(test)]
#[path = "create_tests.rs"]
mod tests;
