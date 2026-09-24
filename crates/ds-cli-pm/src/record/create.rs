//! `ds pm record create` — file one exchange with an outside party.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::Value;

use super::{
    AFFECTS_ARG, BODY_ARG, BODY_FILE_ARG, CATEGORY_ARG, CHANNEL_ARG, DIRECTION_ARG, DOCUMENT_ARG,
    EXTERNAL_NAME_ARG, HAPPENED_AT_ARG, ID_ARG, MEETING_NOTE_ARG, PARTY_ARG, REFERENCE_ARG,
    RESPONSE_DUE_ARG, RESPONSE_OWED_BY_ARG, SOURCE_ASSET_ARG, SOURCE_MESSAGE_ARG, SUBJECT_ARG,
};
use crate::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "pm.record.create",
    path: &["pm", "record", "create"],
    contract: 1,
    summary: "File a letter, email, call, meeting or transmittal as a record.",
    purpose: "\
Files one exchange with an outside party so it can be threaded, owed and \
blocked on. A record is always authored against something — the ingested \
.eml or screenshot (--source-asset), the in-app message (--source-message) \
or the meeting note (--meeting-note); an answer in a thread is `ds pm record \
reply`. Name the parties by id and say who owes the next answer with \
--response-owed-by: a party, or a member by email. A submission or \
transmittal carries at least one registered --document. Sensitivity is the \
strictest of the assets named. The same --id twice is refused, not \
duplicated. Headless; no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
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
        LANE_ARG,
    ],
    output: "\
The project and `record` — id, thread, category, channel, direction, subject, \
`responseStatus`, who owes the answer and by when, sensitivity, what it was \
authored against — plus `attachments`, `documents`, `blockedTasks`, \
`resultingTasks` and `threadTotal`, exactly what `ds pm record read` answers.",
    examples: &[Example {
        command: "ds pm record create --category review --channel email --direction inbound --subject \"MV plan and profile — not approved\" --happened-at 2026-09-14 --party p_acme --source-asset a_1x2y3z4a5b6c --affects scope,schedule --yes",
        note: "The .eml was ingested first with `ds assets ingest`; read .data.record.id to reply or to make a task --from-record.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<33>(&[
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
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "letter",
        "email",
        "mail",
        "file a letter",
        "log a call",
        "minutes",
        "meeting",
        "rfi",
        "instruction",
        "submission",
        "transmittal",
        "review",
        "who owes",
        "ball in court",
        "response due",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let data = super::fields(inputs, true)?;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        &Action::RecordCreate {
            id: inputs.value("id").map(str::to_owned),
            data,
        },
    )?;
    super::view(report)
}

pub fn render(data: &Value) -> String {
    let mut out = format!("filed in {}\n", data["project"].as_str().unwrap_or("?"));
    out.push_str(&super::head(&data["record"]));
    out.push_str(&super::projections(data));
    out
}
