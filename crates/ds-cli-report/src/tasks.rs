//! `ds report tasks` — the reporter's own published task contracts.
//!
//! `ds-report task-schemas` emits every registered task with a full JSON
//! Schema for its request. That document is tens of kilobytes, which makes
//! printing it the single most expensive thing this domain could do — and
//! printing it by default would undo the reason `ds` exists.
//!
//! So the same tiering the CLI applies to itself is applied to the engine's
//! schema: an index by default, one task's full schema when named. The
//! schemas are never copied into this repository. They are read from the
//! engine that owns them, at the version actually installed, so they cannot
//! be stale.

use std::ffi::OsString;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DISCOVERY_TIMEOUT, DS_REPORT};

pub static COMMAND: Command = Command {
    id: "report.tasks",
    path: &["report", "tasks"],
    contract: 1,
    summary: "List the reporter's tasks, or one task's request schema.",
    purpose: "\
Reads the task contracts published by the installed engine rather than any \
copy kept here, so what you see is what this machine will accept. By default \
it lists the tasks with their effect and network class. Name one with --task \
to get its complete request schema — that document is large, which is why it \
is never printed unless asked for.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "task",
        "<name>",
        "Return this one task's full request schema.",
    )],
    output: "\
Without --task, one entry per task: name, effect, network class, whether it is \
idempotent or destructive, and its required fields. With --task, that task's \
complete JSON Schema.",
    examples: &[
        Example {
            command: "ds report tasks --output json",
            note: "The index. Cheap.",
            runnable: true,
        },
        Example {
            command: "ds report tasks --task export_transformer_report --output json",
            note: "One full request schema.",
            runnable: true,
        },
    ],
    refusals: &[
        Refusal {
            code: "reporter_engine_missing",
            when: "`ds-report` is not installed next to `ds`",
            remedy: "install the desktop, or set DS_REPORT_BIN to a built ds-report",
        },
        Refusal {
            code: "unknown_task",
            when: "--task names a task this engine does not register",
            remedy: "run `ds report tasks` for the names this engine accepts",
        },
    ],
    reference: Some("docs/reference/report.md"),
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

/// Fetch the engine's published schemas. Shared with `ds report export`,
/// which validates a request against them before writing anything.
pub fn schemas() -> Result<Value, Failure> {
    DS_REPORT.call_json("task-schemas", &[] as &[OsString], DISCOVERY_TIMEOUT)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let document = schemas()?;
    let tasks = document["tasks"].as_array().cloned().unwrap_or_default();

    let Some(wanted) = inputs.value("task") else {
        return Ok(json!({
            "service": document["service"],
            "tasks": tasks.iter().map(|task| json!({
                "name": task["name"],
                "effect": task["effect"],
                "network": task["network"],
                "idempotent": task["idempotent"],
                "destructive": task["destructive"],
                "required": task["request_schema"]["required"],
            })).collect::<Vec<_>>(),
            "more": { "next": "ds report tasks --task <name>" },
        }));
    };

    let found = tasks.iter().find(|task| task["name"] == wanted);
    let Some(task) = found else {
        let known: Vec<&str> = tasks
            .iter()
            .filter_map(|task| task["name"].as_str())
            .collect();
        let mut failure = Failure::invalid(
            "unknown_task",
            format!("this engine does not register a task named `{wanted}`"),
        );
        match ds_cli_contract::args::nearest(wanted, known.iter().copied()) {
            Some(suggestion) => failure = failure.remedy(format!("did you mean `{suggestion}`?")),
            None => {
                failure = failure.remedy("run `ds report tasks` for the names this engine accepts")
            }
        }
        return Err(failure
            .next("ds report tasks")
            .detail(json!({ "tasks": known })));
    };

    Ok(task.clone())
}

pub fn render(data: &Value) -> String {
    // One task's schema, or the index — told apart by which shape came back.
    if let Some(schema) = data.get("request_schema") {
        let mut out = format!(
            "{}\n  effect {}  network {}  idempotent {}\n\nREQUEST FIELDS\n",
            data["name"].as_str().unwrap_or(""),
            data["effect"].as_str().unwrap_or(""),
            data["network"].as_str().unwrap_or(""),
            data["idempotent"],
        );
        let required: Vec<&str> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if let Some(properties) = schema["properties"].as_object() {
            for (name, spec) in properties {
                let mark = if required.contains(&name.as_str()) {
                    "*"
                } else {
                    " "
                };
                out.push_str(&format!(
                    "  {mark} {name:<22} {:<8} {}\n",
                    spec["type"].as_str().unwrap_or("?"),
                    spec["description"].as_str().unwrap_or(""),
                ));
            }
        }
        out.push_str("\n  * required\n");
        return out;
    }

    let mut out = String::new();
    for task in data["tasks"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<36}  {}  network {}\n",
            task["name"].as_str().unwrap_or(""),
            task["effect"].as_str().unwrap_or(""),
            task["network"].as_str().unwrap_or(""),
        ));
    }
    out.push_str("\nnext: ds report tasks --task <name>\n");
    out
}
