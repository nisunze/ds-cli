//! `ds design status` — the project's transformer status rows, headless.
//!
//! This is the read every other Design answer is built from. It lives beside
//! the transformer lifecycle commands because it uses their credential path,
//! their scope flag and their refusals, but it is not one of them: it answers
//! `ds design status`, not `ds design transformer status`, because a status
//! row is the project's own state, not a step in a transformer's lifecycle.

use ds_cli_auth::{TransformerStatusList, TransformerStatusRow};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::{LANE_ARG, TRANSFORMER_ARG};

pub static COMMAND: Command = Command {
    id: "design.status",
    path: &["design", "status"],
    contract: 1,
    summary: "Read the project's transformer status rows without a browser or map.",
    purpose: "\
The read every headless Design answer starts from. Restores the native user \
and reads only that user's audience-fenced selected project through the fixed \
status call. Without --transformer it answers every transformer document; \
with names it answers exactly those that exist, so a shorter list than the \
request is an answer, not a refusal. Rows are returned as the service sent \
them, bounded but not reshaped, so a consumer reads one shape whether it runs \
here or in the application. No project, Desktop descriptor, URL, body or \
action override is accepted, and nothing here falls back to a browser.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, the row count, and one row per \
transformer exactly as the service sent it: process/report/draft/sketch \
metadata, layer counts, uploads, artifacts and retry capabilities where the \
document carries them.",
    examples: &[Example {
        command: "ds design status --transformer TX-1 --output json",
        note: "`.data.transformers[0].process_metadata.status` is the last run's verdict.",
        runnable: false,
    }],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let mut output = super::project_receipt(&headless);
    let rows = status_json(headless.result());
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(rows.as_object().expect("rows are an object").clone());
    Ok(output)
}

/// The rows as they arrived, with the count the caller would otherwise derive.
fn status_json(list: &TransformerStatusList) -> Value {
    json!({
        "count": list.len(),
        "transformers": list.rows().iter().map(TransformerStatusRow::row).collect::<Vec<_>>(),
    })
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} transformers\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["count"].as_u64().unwrap_or(0),
    );
    if let Some(rows) = data["transformers"].as_array() {
        for row in rows {
            let version = row["metadata"]["version"]
                .as_u64()
                .map(|version| format!("v{version}"))
                .unwrap_or_default();
            let line = format!(
                "  {:<32} {:<12} {:<12} {}",
                row["name"].as_str().unwrap_or("?"),
                row["process_metadata"]["status"].as_str().unwrap_or("-"),
                row["report_metadata"]["status"].as_str().unwrap_or("-"),
                version,
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out
}
