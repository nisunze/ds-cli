//! `ds work task read` — one work item, with everything hanging off it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, TASK_ARG};

pub static COMMAND: Command = Command {
    id: "work.task.read",
    path: &["work", "task", "read"],
    contract: 1,
    summary: "Read one work item with its dependencies and open residuals.",
    purpose: "\
The whole canonical task: schedule, delivery, review and closeout state, who \
holds it, who has been asked to take it, what it depends on, what is still \
outstanding against it, and the records that reference it. Read this before \
any write — the update, assign and respond commands all act on what is here.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TASK_ARG, DESCRIPTOR_ARG],
    output: "\
`task` with the canonical fields, `dependencies`, `residuals` (open first), \
`episodes`, `records` referencing it, and a `*Total` for each bounded related \
collection; plus the project's `permissions`, graph `revision`, and `link`.",
    examples: &[Example {
        command: "ds work task read --task T-0007 --output json",
        note: "`.data.task.assignmentOpen` tells you whether respond is available.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::WORK_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TASK_READ,
        json!({ "task": inputs.require("task")? }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
}

pub fn render(data: &Value) -> String {
    let task = &data["task"];
    let mut out = format!(
        "{} {}\n  {} · review {} · closeout {} · {}% · {}\n",
        task["wbs"].as_str().unwrap_or("—"),
        task["title"].as_str().unwrap_or("?"),
        task["delivery"].as_str().unwrap_or("—"),
        task["review"].as_str().unwrap_or("—"),
        task["closeout"].as_str().unwrap_or("—"),
        task["progress"].as_u64().unwrap_or(0),
        task["responsible"].as_str().unwrap_or("unassigned"),
    );
    if let Some(schedule) = task["start"].as_str() {
        out.push_str(&format!(
            "  {schedule} → {}\n",
            task["finish"].as_str().unwrap_or("—")
        ));
    }
    if let Some(asked) = task["requested"].as_array().filter(|list| !list.is_empty()) {
        let people: Vec<&str> = asked.iter().filter_map(Value::as_str).collect();
        out.push_str(&format!("  asked: {}\n", people.join(", ")));
    }
    for (label, key) in [
        ("depends on", "dependencies"),
        ("residuals", "residuals"),
        ("records", "records"),
    ] {
        let total_key = match key {
            "dependencies" => "dependencyTotal",
            "residuals" => "residualTotal",
            "records" => "recordTotal",
            _ => key,
        };
        let count = data[total_key]
            .as_u64()
            .unwrap_or_else(|| data[key].as_array().map_or(0, Vec::len) as u64);
        if count > 0 {
            out.push_str(&format!("  {label}: {count}\n"));
        }
    }
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}
