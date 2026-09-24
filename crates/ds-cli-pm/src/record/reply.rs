//! `ds pm record reply` — answer in a thread; a reply from the owing side
//! settles what was owed.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::Value;

use super::{
    AFFECTS_ARG, BODY_ARG, BODY_FILE_ARG, CATEGORY_ARG, CHANNEL_ARG, DIRECTION_ARG, DOCUMENT_ARG,
    EXTERNAL_NAME_ARG, HAPPENED_AT_ARG, ID_ARG, PARTY_ARG, REFERENCE_ARG, RESPONSE_DUE_ARG,
    RESPONSE_OWED_BY_ARG, SOURCE_ASSET_ARG, SUBJECT_ARG,
};
use crate::LANE_ARG;

const REPLY_TO_ARG: Arg = Arg::value(
    "reply-to",
    "<record-id>",
    "The record this answers. The reply joins its thread and inherits its parties and sensitivity.",
)
.required();

pub static COMMAND: Command = Command {
    id: "pm.record.reply",
    path: &["pm", "record", "reply"],
    contract: 1,
    summary: "Reply in a thread; from the owing side it settles the answer owed.",
    purpose: "\
Files the next exchange of a thread. The reply inherits the parent's thread, \
its parties and its sensitivity, and its channel when none is given. When \
the reply comes from the side that owed the answer — the party named on the \
parent, or the project when the project owed it — the parent's response \
becomes `responded` and every task blocked on it is cleared in the same \
commit. The reply may itself owe an answer (--response-owed-by) and may be \
authored against its own asset (--source-asset). Headless: writes to the \
named project of the signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        REPLY_TO_ARG,
        CATEGORY_ARG.required(),
        DIRECTION_ARG.required(),
        SUBJECT_ARG.required(),
        CHANNEL_ARG,
        BODY_ARG,
        BODY_FILE_ARG,
        HAPPENED_AT_ARG,
        REFERENCE_ARG,
        PARTY_ARG,
        EXTERNAL_NAME_ARG,
        SOURCE_ASSET_ARG,
        RESPONSE_OWED_BY_ARG,
        RESPONSE_DUE_ARG,
        AFFECTS_ARG,
        DOCUMENT_ARG,
        ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "\
The project and the new `record` with its projections, exactly what \
`ds pm record read` answers; `threadTotal` counts the thread it joined.",
    examples: &[Example {
        command: "ds pm record reply --reply-to R-0031 --category response --direction inbound --subject \"Re: Questions on spans\" --party p_acme --source-asset a_2x3y4z5a6b7c --yes --project <exact-id>",
        note: "An inbound reply from the party that owed the answer clears every task blocked on R-0031.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<34>(&[
        crate::RECORD_NOT_FOUND,
        crate::THREAD_MISMATCH,
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
        "respond",
        "letter",
        "email",
        "who owes",
        "ball court",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let data = super::fields(inputs, false)?;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::RecordReply {
            record_id: inputs.require("reply-to")?.to_owned(),
            id: inputs.value("id").map(str::to_owned),
            data,
        },
    )?;
    super::view(report)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "replied in {} · thread of {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["threadTotal"].as_u64().unwrap_or(0),
    );
    out.push_str(&super::head(&data["record"]));
    out.push_str(&super::projections(data));
    out
}
