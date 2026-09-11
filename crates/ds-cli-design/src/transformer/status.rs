//! `ds design status` — the project's transformer status rows, headless.
//!
//! This is the read every other Design answer is built from. It lives beside
//! the transformer lifecycle commands because it uses their credential path,
//! their scope flag and their refusals, but it is not one of them: it answers
//! `ds design status`, not `ds design transformer status`, because a status
//! row is the project's own state, not a step in a transformer's lifecycle.

use ds_cli_auth::{TransformerStatusList, TransformerStatusRow};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::design_health::{TransformerHealth, summarize, transformer_health};
use serde_json::{Value, json};

use super::{LANE_ARG, TRANSFORMER_ARG};

pub const FINDINGS_ARG: Arg = Arg::switch(
    "findings",
    "List the project's findings as rows: transformer, phase, code, affected count.",
);

pub static COMMAND: Command = Command {
    id: "design.status",
    path: &["design", "status"],
    contract: 1,
    summary: "Read the project's transformer status rows and verdicts, headlessly.",
    purpose: "\
The read every headless Design answer starts from. Restores the native user \
and reads only that user's audience-fenced selected project through the fixed \
status call. Without --transformer it answers every transformer document; \
with names it answers exactly those that exist, so a shorter list than the \
request is an answer, not a refusal. Rows are returned as the service sent \
them, bounded but not reshaped, so a consumer reads one shape whether it runs \
here or in the application. No project, Desktop descriptor, URL, body or \
action override is accepted, and nothing here falls back to a browser. Every \
row also carries the shared kernel's verdict (health) and truth (view, phase \
ownership, latest action, governance, retry, kind, allows); the reference \
document describes each. A headless client holds no browser rooms.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG, FINDINGS_ARG],
    output: "\
Lane and selected-project identity/status, the row count, the project's \
severity summary, and one row per transformer as the service sent it — \
process/report/draft/sketch metadata, layer counts, uploads, artifacts, \
retry capabilities — plus `health`, `view`, `phase_ownership`, `latest_action`, \
`governance`, `retry`, `kind` and `allows`. With --findings, one `findings` row \
per finding.",
    examples: &[
        Example {
            command: "ds design status --transformer TX-1 --output json",
            note: "`.data.transformers[0].health.severity` is the row's verdict.",
            runnable: false,
        },
        Example {
            command: "ds design status --findings --output json",
            note: "`.data.findings` is the project's issue list.",
            runnable: false,
        },
    ],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let mut output = super::project_receipt(&headless);
    let rows = status_json(headless.result(), inputs.switch("findings"));
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(rows.as_object().expect("rows are an object").clone());
    Ok(output)
}

/// The rows as they arrived, each with the verdict the shared kernel reads
/// from it, plus the count and the per-bucket summary the caller would
/// otherwise derive — and derive differently from the application, which is
/// the split this closes.
fn status_json(list: &TransformerStatusList, findings: bool) -> Value {
    let health: Vec<TransformerHealth> = list
        .rows()
        .iter()
        .map(|row| transformer_health(row.row()))
        .collect();
    // The rows' truth — saved/unsaved, locality, lane, presence, version,
    // which run owns the record, the latest action, the governance label, the
    // retry verdict, the row's kind and what it may be a target of — is the
    // shared kernel's answer over the same rows. A headless client holds no
    // browser rooms, so every row is remote and clean here by construction.
    let truth = status_row_truth(list);
    let rows: Vec<Value> = list
        .rows()
        .iter()
        .zip(&health)
        .enumerate()
        .map(|(index, (row, health))| {
            let mut row = row.row().clone();
            if let Some(object) = row.as_object_mut() {
                object.insert(
                    "health".into(),
                    serde_json::to_value(health).unwrap_or(Value::Null),
                );
                if let Some(answer) = truth.get(index) {
                    for key in [
                        "view",
                        "phase_ownership",
                        "latest_action",
                        "governance",
                        "retry",
                        "kind",
                        "allows",
                    ] {
                        object.insert(key.into(), answer[key].clone());
                    }
                }
            }
            row
        })
        .collect();
    let mut out = json!({
        "count": list.len(),
        "summary": summarize(health.iter()),
        "transformers": rows,
    });
    if findings {
        out.as_object_mut().expect("object").insert(
            "findings".into(),
            Value::Array(fleet_findings(list.rows(), &health)),
        );
    }
    out
}

/// One kernel call over the list: the row truth in server order (the kernel
/// keeps the server's first position for every name), keyed back by index.
fn status_row_truth(list: &TransformerStatusList) -> Vec<Value> {
    let rows: Vec<Value> = list.rows().iter().map(|row| row.row().clone()).collect();
    let request = json!({
        "schema": ds_command_kernel::design_status_row::SCHEMA,
        "server_rows": rows,
        "local_room_headers": [],
        "current_user_email": "",
        "now_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    });
    let Ok(input) = serde_json::to_vec(&request) else {
        return Vec::new();
    };
    let Ok(reply) = ds_command_kernel::design_status_row::evaluate(&input) else {
        return Vec::new();
    };
    let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
    let mut by_index: Vec<Value> = vec![Value::Null; list.len()];
    for row in reply["rows"].as_array().into_iter().flatten() {
        let slot = row["server_index"]
            .as_u64()
            .and_then(|index| by_index.get_mut(index as usize));
        if let Some(slot) = slot {
            *slot = row.clone();
        }
    }
    by_index
}

/// Every finding in the project, in row order then finding order — the same
/// list the register's error and warning tables sort and render.
fn fleet_findings(rows: &[TransformerStatusRow], health: &[TransformerHealth]) -> Vec<Value> {
    let mut out = Vec::new();
    for (row, health) in rows.iter().zip(health) {
        for finding in &health.findings {
            let mut finding = serde_json::to_value(finding).unwrap_or(Value::Null);
            if let Some(object) = finding.as_object_mut() {
                object.insert("transformer".into(), Value::String(row.name().to_string()));
            }
            out.push(finding);
        }
    }
    out
}

pub fn render(data: &Value) -> String {
    let summary = &data["summary"];
    let mut out = format!(
        "project {} ({}) · {} · {} transformers · {} processing · {} warning · {} error\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["count"].as_u64().unwrap_or(0),
        summary["processing"].as_u64().unwrap_or(0),
        summary["warnings"].as_u64().unwrap_or(0),
        summary["errors"].as_u64().unwrap_or(0),
    );
    if let Some(rows) = data["findings"].as_array() {
        for finding in rows {
            let line = format!(
                "  {:<32} {:<8} {:<8} {:<28} {}",
                finding["transformer"].as_str().unwrap_or("?"),
                finding["severity"].as_str().unwrap_or("-"),
                finding["phase"].as_str().unwrap_or("-"),
                finding["code"]
                    .as_str()
                    .filter(|code| !code.is_empty())
                    .unwrap_or("-"),
                finding["message"].as_str().unwrap_or(""),
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
        return out;
    }
    if let Some(rows) = data["transformers"].as_array() {
        for row in rows {
            let version = row["metadata"]["version"]
                .as_u64()
                .map(|version| format!("v{version}"))
                .unwrap_or_default();
            let line = format!(
                "  {:<32} {:<9} {:<10} {:<8} {:<12} {:<12} {}",
                row["name"].as_str().unwrap_or("?"),
                row["health"]["severity"].as_str().unwrap_or("-"),
                row["kind"].as_str().unwrap_or("-"),
                row["governance"]["state"].as_str().unwrap_or("-"),
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
