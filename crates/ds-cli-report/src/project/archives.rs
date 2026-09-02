//! `ds report project archives` — the published compounded archives.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::LANE_ARG;

pub static COMMAND: Command = Command {
    id: "report.project.archives",
    path: &["report", "project", "archives"],
    contract: 1,
    summary: "List the selected project's published compounded archives.",
    purpose: "\
Restores the native user and reads only its audience-fenced selected \
project's compounded archive registry, newest first, through the fixed list \
call. Each row is the durable record of one `compounded` run: stem, cloud \
locator, a short-lived signed download when the service signed one, scope, \
layout, coverage and errors. No project, Desktop descriptor, URL, body or \
action override is accepted.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG],
    output: "\
Lane and selected-project identity/status, `count`, and `archives`: stem, \
filename, cloud locator, optional signed `download_url`, creation actor and \
time, status, transformer and district counts and names, individual artifact \
coverage, bounded errors, and the recorded archive layout.",
    examples: &[Example {
        command: "ds report project archives --output json",
        note: "`.data.archives[0].download_url` is the newest deliverable while its signature lasts.",
        runnable: false,
    }],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let headless = ds_cli_auth::compounded_report_list(inputs.require("lane")?)?;
    let archives = headless
        .result()
        .iter()
        .map(|archive| {
            json!({
                "stem": archive.stem(),
                "filename": archive.filename(),
                "gcs_path": archive.gcs_path(),
                "download_url": archive.download_url(),
                "created_at": archive.created_at(),
                "created_by": archive.created_by(),
                "status": archive.status(),
                "transformer_count": archive.transformer_count(),
                "transformers": archive.transformers(),
                "district_count": archive.district_count(),
                "districts": archive.districts(),
                "individual_artifact_transformer_count": archive.individual_artifact_transformer_count(),
                "missing_individual_artifact_count": archive.missing_individual_artifact_count(),
                "errors": archive.errors(),
                "archive_layout": archive.archive_layout().map(|layout| json!({
                    "file_level": layout.file_level(),
                    "combine_per_district": layout.combine_per_district(),
                })),
            })
        })
        .collect::<Vec<_>>();
    let mut output = super::project_receipt(&headless);
    output["count"] = json!(archives.len());
    output["archives"] = Value::Array(archives);
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} archive(s)\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["count"].as_u64().unwrap_or(0),
    );
    if let Some(archives) = data["archives"].as_array() {
        for archive in archives {
            out.push_str(&format!(
                "  {:<34} {:<8} {} transformer(s) · {}\n",
                archive["stem"].as_str().unwrap_or("?"),
                archive["status"].as_str().unwrap_or("?"),
                archive["transformer_count"].as_u64().unwrap_or(0),
                archive["created_at"].as_str().unwrap_or("?"),
            ));
        }
    }
    out
}
