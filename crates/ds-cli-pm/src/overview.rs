//! Headless project overview; the canonical kernel projects all four lists.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs, outcome::Failure};
use ds_client_core::project_management::Command as PMCommand;
use ds_command_kernel::project_management::overview::{MAX_ROWS, Options};
use serde_json::Value;

const LIMIT:Arg=Arg::value("limit","<count>","Rows per task, milestone, record and project-note list (1-100); counts state exact or lower_bound.").default("10");
const CURSOR: Arg = Arg::value(
    "records-cursor",
    "<cursor>",
    "Explicit next_cursors.pm_records from a previous overview; no automatic page walk.",
);
pub static COMMAND: Command = Command {
    id: "pm.overview",
    path: &["pm", "overview"],
    contract: 1,
    summary: "Read tasks, milestones, records and project notes together.",
    purpose: "Read a named project's work overview, including inbox tasks and project notes without map geometry. One captured native authority reads get_graph and one bounded get_context page; the kernel returns four independently bounded lists with exact graph/schedule revisions, stable ids, record versions and revision labels. Graph and context are separate reads; no atomic snapshot is claimed. Truncated or cursor-paged record/note counts are lower bounds. Project notes are project records, never personal notes. No polling, speculative reads or automatic pagination.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::PROJECT_ARG, LIMIT, CURSOR, crate::LANE_ARG],
    output: "project_id, graph_revision, schedule_revision, read_consistency, optional context_graph_revision, source_authority, tasks/milestones/records/project_notes each with count/count_kind/returned/truncated/rows, context_returned_records, context_records_cursor, collection truncation flags and next_cursors. Bodies, geometry, private notes and attachments are not returned.",
    examples: &[Example {
        command: "ds pm overview --project <exact-id> --limit 10 --output json",
        note: "Use the returned record cursor only when you deliberately want another context page; counts may be lower bounds.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[crate::INVALID_NUMBER]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "project overview",
        "all work",
        "project notes",
        "inbox",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn options(inputs: &Inputs) -> Result<Options, Failure> {
    let options = Options {
        limit: crate::integer(inputs.value("limit").unwrap_or("10"), "limit", 1, MAX_ROWS)?,
        records_cursor: inputs.value("records-cursor").map(str::to_owned),
    };
    options.validate().map_err(|message| {
        Failure::invalid(crate::INVALID_NUMBER.code, message)
            .remedy("use 1..100 rows and an exact returned records cursor")
    })?;
    Ok(options)
}
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let options = options(inputs)?;
    let report = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &PMCommand::Overview { options },
    )?;
    Ok(report.into_result())
}
pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} - graph {} / schedule {} - {}\n",
        data["project_id"].as_str().unwrap_or("?"),
        data["graph_revision"].as_i64().unwrap_or(0),
        data["schedule_revision"].as_i64().unwrap_or(0),
        data["read_consistency"].as_str().unwrap_or("unknown")
    );
    for (key, label) in [
        ("tasks", "Tasks"),
        ("milestones", "Milestones"),
        ("records", "Records"),
        ("project_notes", "Project notes"),
    ] {
        let list = &data[key];
        out.push_str(&format!(
            "{label}: {} ({}) - {} shown\n",
            list["count"].as_u64().unwrap_or(0),
            list["count_kind"].as_str().unwrap_or("unknown"),
            list["returned"].as_u64().unwrap_or(0)
        ));
        for row in list["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {}  {}\n",
                row["id"].as_str().unwrap_or("?"),
                row["title"]
                    .as_str()
                    .or(row["subject"].as_str())
                    .unwrap_or("")
            ));
        }
    }
    if let Some(cursor) = data["next_cursors"]["pm_records"].as_str() {
        out.push_str(&format!("Next records cursor: {cursor}\n"));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_options_fail_before_authentication() {
        for limit in ["0", "101", "many"] {
            let args = ["--project", "p", "--limit", limit].map(str::to_owned);
            let inputs = ds_cli_contract::parse(&COMMAND, &args).unwrap();
            assert!(options(&inputs).is_err());
        }
        assert_eq!(
            Options::default().limit,
            ds_command_kernel::project_management::overview::DEFAULT_ROWS
        );
    }
}
