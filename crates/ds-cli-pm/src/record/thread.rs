//! `ds pm record thread` — the whole exchange in time order.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::Value;

use super::RECORD_ARG;
use crate::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "pm.record.thread",
    path: &["pm", "record", "thread"],
    contract: 1,
    summary: "Read a whole thread in time order, with attachments and blockers.",
    purpose: "\
Every record sharing one thread — name any record of it — in the order the \
exchanges happened, each with its response status, who owes the answer, its \
attachments and registered documents, the tasks blocked on it and the tasks \
made from it. A record the signed-in user may not read has no row; the total \
still counts it. Headless: the selected project of the signed-in native \
credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[RECORD_ARG, LANE_ARG],
    output: "\
The project, `threadId`, `total`, `truncated`, `asOf` and `records` — each \
the view `ds pm record read` answers — in happened-at order.",
    examples: &[Example {
        command: "ds pm record thread --record R-0031 --output json",
        note: "Read .data.records[].record.responseStatus to see whose move it is.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<23>(&[
        crate::RECORD_NOT_FOUND,
        crate::BOUND_EXCEEDED,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "thread",
        "conversation",
        "history",
        "exchange",
        "letter",
        "email",
        "who owes",
        "ball in court",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        &Action::RecordThread {
            record_id: inputs.require("record")?.to_owned(),
        },
    )?;
    let project = report.project_id().to_owned();
    crate::data(
        &ds_command_kernel::project_management::correspondence::thread_view(
            &project,
            &report.into_result(),
        ),
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "thread {} in {} · {} of {}\n",
        data["threadId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["records"].as_array().map_or(0, Vec::len),
        data["total"].as_u64().unwrap_or(0),
    );
    if let Some(rows) = data["records"].as_array() {
        for view in rows {
            out.push_str(&super::head(&view["record"]));
            out.push_str(&super::projections(view));
        }
    }
    out
}
