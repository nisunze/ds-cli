//! `ds pm record read` — one record, with its body.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::Value;

use super::RECORD_ARG;
use crate::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "pm.record.read",
    path: &["pm", "record", "read"],
    contract: 1,
    summary: "Read one record: body, who owes the answer, attachments, blockers.",
    purpose: "\
The whole record: what it is, which direction it travelled, what state it \
is in, whether a response is owed — by which party or person, by when, and \
whether it is outstanding, overdue, answered or waived — what it affects, \
what it was authored against, and, projected beside it, its `attachments` \
(the .eml with its parts, screenshots, external links), its registered \
`documents`, the tasks blocked on it and the tasks made from it. The body \
is bounded, and a body that was cut says so. Headless: the named project \
of the signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[RECORD_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "\
`record` with its canonical fields, `responseStatus`, `responseOwnerPartyId` \
or `responseOwnerId`, `daysOverdue`/`daysUntilDue`, `sensitivity`, the \
bounded `body` and related-id collections; `attachments` with \
`attachmentsTotal`, `documents`, `blockedTasks`, `resultingTasks`, \
`threadTotal` and `asOf`.",
    examples: &[Example {
        command: "ds pm record read --record R-0031 --output json --project <exact-id>",
        note: "`.data.record.responseStatus` says whose move it is; `.data.attachments[].assetId` opens with `ds assets`.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<22>(&[crate::RECORD_NOT_FOUND]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "letter",
        "email",
        "rfi",
        "instruction",
        "submission",
        "decision",
        "who owes",
        "ball court",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::RecordRead {
            record_id: inputs.require("record")?.to_owned(),
        },
    )?;
    super::view(report)
}

pub fn render(data: &Value) -> String {
    let record = &data["record"];
    let mut out = super::head(record);
    if let Some(body) = record["body"].as_str().filter(|text| !text.is_empty()) {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
        if record["bodyTruncated"].as_bool().unwrap_or(false) {
            out.push_str("… body cut to its bound\n");
        }
    }
    out.push_str(&super::projections(data));
    let related = record["relatedTaskIds"].as_array().map_or(0, Vec::len);
    if related > 0 {
        out.push_str(&format!(
            "\n{}\n",
            crate::plural(related as u64, "linked task")
        ));
    }
    out
}
