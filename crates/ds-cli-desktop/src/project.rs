//! Visible-project discovery and exact active-project switching.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};

const TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_LIMIT: &str = "50";

pub const LIST_OP: BridgeOp = BridgeOp {
    operation: "project.list",
    arguments: &["status", "query", "limit"],
};

pub const SWITCH_OP: BridgeOp = BridgeOp {
    operation: "project.switch",
    arguments: &["project"],
};

pub const BRIDGE_OPS: &[&BridgeOp] = &[&LIST_OP, &SWITCH_OP];

const PROJECT_NOT_VISIBLE: Refusal = Refusal {
    code: "project_not_visible",
    when: "the exact project id is not visible to the signed-in desktop user",
    remedy: "run `ds desktop project list --query <text>` and use an exact returned id",
};

const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "a project id or query is empty, untrimmed, or longer than its bound",
    remedy: "use the exact bounded text returned by project discovery",
};

const COMMON_REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    ops::INVALID_NUMBER,
    INVALID_TEXT,
];

pub static LIST_COMMAND: Command = Command {
    id: "desktop.project.list",
    path: &["desktop", "project", "list"],
    contract: 1,
    summary: "List projects visible to the signed-in desktop user.",
    purpose: "\
Returns a bounded project picker projection from the paired application's own \
signed-in repository. Use its exact `project` ids with `desktop project switch`; \
the CLI never invents an id from a display name.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "status",
            "<active|testing|archived|all>",
            "Lifecycle bucket to list.",
        )
        .choices(&["active", "testing", "archived", "all"])
        .default("all"),
        Arg::value(
            "query",
            "<text>",
            "Match project id, name, display name, or location; at most 160 characters.",
        ),
        Arg::value("limit", "<n>", "Return at most this many matches; 1..500.")
            .default(DEFAULT_LIMIT),
        DESCRIPTOR_ARG,
    ],
    output: "\
The current `activeProject`, lifecycle bucket, match count, and bounded \
`projects`; each carries its exact id, display name, location, role, and status. \
`more.omitted` reports truncation.",
    examples: &[Example {
        command: "ds desktop project list --status testing --query survey --output json",
        note: "Use one exact .data.projects[].project value when switching.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/desktop.project.md"),
    availability: ops::paired_availability,
};

pub static SWITCH_COMMAND: Command = Command {
    id: "desktop.project.switch",
    path: &["desktop", "project", "switch"],
    contract: 1,
    summary: "Switch the paired desktop to one exact visible project.",
    purpose: "\
Requests one exact active-project context change through the paired application. \
The app verifies that the signed-in user can see the project, performs its normal \
project switch, and keeps project-scoped local rooms under their own project keys.",
    chapter: Chapter::Project,
    effect: Effect::LocalUi,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<id>",
            "Exact project id returned by `desktop project list`.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "\
`changed`, `previousProject`, the resulting `activeProject`, and the selected \
project's bounded summary. No project data is written.",
    examples: &[Example {
        command: "ds desktop project switch --project arjgpydw_survey_test --output json",
        note: "Follow with `ds desktop status` before project-scoped work.",
        runnable: false,
    }],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        PROJECT_NOT_VISIBLE,
        INVALID_TEXT,
    ],
    reference: Some("docs/reference/desktop.project.md"),
    availability: ops::paired_availability,
};

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = ops::integer(inputs.require("limit")?, "limit", 1, 500)?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    let mut arguments = Map::from_iter([
        (
            "status".to_string(),
            Value::String(inputs.require("status")?.to_string()),
        ),
        ("limit".to_string(), Value::Number(limit.into())),
    ]);
    if let Some(query) = inputs.value("query") {
        let query = bounded_text(query, "query", 160)?;
        arguments.insert("query".to_string(), Value::String(query.to_string()));
    }
    ops::invoke(&descriptor, &LIST_OP, Value::Object(arguments), TIMEOUT)
        .map_err(ops::classify_signed_out)
}

pub fn switch(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = bounded_text(inputs.require("project")?, "project", 160)?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &SWITCH_OP,
        json!({ "project": project }),
        TIMEOUT,
    )
    .map_err(classify_project_switch)
}

fn bounded_text<'a>(value: &'a str, flag: &str, max: usize) -> Result<&'a str, Failure> {
    if value.is_empty() || value.trim() != value || value.chars().count() > max {
        return Err(Failure::invalid(
            "invalid_text",
            format!("`--{flag}` must be non-empty, trimmed, and at most {max} characters"),
        ));
    }
    Ok(value)
}

fn classify_project_switch(failure: Failure) -> Failure {
    let failure = ops::classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if detail.contains("not visible to the signed-in user") {
        return Failure::invalid("project_not_visible", "the project is not visible")
            .remedy(PROJECT_NOT_VISIBLE.remedy)
            .next("ds desktop project list --status all");
    }
    failure
}

pub fn render_list(data: &Value) -> String {
    let mut out = format!(
        "active project  {}\n{} visible project matches\n",
        data["activeProject"].as_str().unwrap_or("none"),
        data["matched"].as_u64().unwrap_or(0),
    );
    for project in data["projects"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<34} {:<9} {}\n",
            project["project"].as_str().unwrap_or(""),
            project["status"].as_str().unwrap_or(""),
            project["name"].as_str().unwrap_or(""),
        ));
    }
    if let Some(omitted) = data["more"]["omitted"].as_u64().filter(|value| *value > 0) {
        out.push_str(&format!("  {omitted} more omitted\n"));
    }
    out
}

pub fn render_switch(data: &Value) -> String {
    if data["changed"].as_bool().unwrap_or(false) {
        format!(
            "project switched  {} -> {}\n",
            data["previousProject"].as_str().unwrap_or("none"),
            data["activeProject"].as_str().unwrap_or("none"),
        )
    } else {
        format!(
            "project already active  {}\n",
            data["activeProject"].as_str().unwrap_or("none")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_switch_declare_only_their_exact_bridge_arguments() {
        assert_eq!(LIST_OP.arguments, ["status", "query", "limit"]);
        assert_eq!(SWITCH_OP.arguments, ["project"]);
    }

    #[test]
    fn project_text_is_bounded_and_trimmed() {
        assert_eq!(
            bounded_text("project-1", "project", 160).unwrap(),
            "project-1"
        );
        assert!(bounded_text(" project-1", "project", 160).is_err());
        assert!(bounded_text("", "project", 160).is_err());
    }
}
