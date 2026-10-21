//! `ds pm task delete` — one governed task or milestone subtree deletion.
//!
//! The server records the exact pre-delete graph documents in a durable
//! backup in the same commit. An explicit revision and stable command id
//! let a caller safely retry a lost response through the server's ledger.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Value, json};

use crate::{LANE_ARG, TASK_ARG};

const BASE_REVISION_ARG: Arg = Arg::value(
    "base-revision",
    "<revision>",
    "Exact live plan revision reviewed before deletion; obtain it with `ds pm plan`.",
)
.required();
const COMMAND_ID_ARG: Arg = Arg::value(
    "command-id",
    "<idempotency-id>",
    "Stable id for this deletion; reuse it if a reply is lost (8-128 letters, digits, _ or -).",
)
.required();
const INVALID_COMMAND_ID: Refusal = Refusal {
    code: "invalid_command_id",
    when: "the idempotency id is outside the server's 8-128 character command ledger spelling",
    remedy: "choose one stable id starting with a letter or digit and using only letters, digits, _ or -",
};

pub static COMMAND: Command = Command {
    id: "pm.task.delete",
    path: &["pm", "task", "delete"],
    contract: 1,
    summary: "Delete a task or milestone subtree with a durable server backup.",
    purpose: "Delete the selected task or milestone, its descendants, and attached dependencies and residuals as one governed graph edit. The server atomically captures the exact pre-delete documents in a durable backup and returns its id. An assigned descendant refuses the cascade. Supply the plan revision you reviewed and a stable command id; a changed head is refused, and retrying the same command id after a lost reply replays the same decision. Headless and scoped to the exact --project.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        BASE_REVISION_ARG,
        COMMAND_ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "`project`, `task_id`, `backup_id`, `command_id`, `base_revision`, and the server's complete applied result, including deleted ids, committed revision, warnings and violations.",
    examples: &[Example {
        command: "ds pm task delete --project <exact-id> --task T-0007 --base-revision 42 --command-id delete-20260929-1 --yes --output json",
        note: "Review the task and plan first. Reuse the exact command id for a retry.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<24>(&[
        crate::INVALID_NUMBER,
        INVALID_COMMAND_ID,
        crate::deletion::RESPONSE_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["remove", "recover", "restore"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

// ds-brain's PM command ledger requires this stricter spelling. The native
// transport's generic command-id check is intentionally looser.
fn command_id_valid(id: &str) -> bool {
    (8..=128).contains(&id.len())
        && id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn unreadable(message: &str) -> Failure {
    Failure::internal(crate::deletion::RESPONSE_UNREADABLE.code, message)
        .remedy(crate::deletion::RESPONSE_UNREADABLE.remedy)
}

fn verified_delete(
    project: &str,
    task_id: &str,
    command_id: &str,
    base_revision: i64,
    result: Value,
) -> Result<Value, Failure> {
    let applied = result
        .get("applied")
        .and_then(Value::as_bool)
        .ok_or_else(|| unreadable("delete result has no applied flag"))?;
    if !applied {
        let violations = result.get("violations").cloned().unwrap_or(Value::Null);
        let message = violations
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row["message"].as_str())
            .unwrap_or("The server did not apply this task deletion.");
        return Err(Failure::invalid(crate::PM_REFUSED.code, message)
            .detail(json!({ "task_id": task_id, "violations": violations }))
            .remedy(crate::PM_REFUSED.remedy));
    }

    let deleted = result.get("deleted_task_ids").and_then(Value::as_array);
    if result.get("kind").and_then(Value::as_str) != Some("delete_task")
        || result.get("command_id").and_then(Value::as_str) != Some(command_id)
        || result.get("backup_id").and_then(Value::as_str) != Some(command_id)
        || result
            .get("committed_revision")
            .and_then(Value::as_i64)
            .is_none_or(|revision| revision <= base_revision)
        || deleted.is_none_or(|ids| !ids.iter().any(|id| id.as_str() == Some(task_id)))
        || !result["deleted_dependency_ids"].is_array()
        || !result["deleted_residual_ids"].is_array()
        || (!result["warnings"].is_null() && !result["warnings"].is_array())
    {
        return Err(unreadable(
            "delete result does not match the requested task and command, or lacks its backup and deleted ids",
        ));
    }

    Ok(json!({
        "project": project,
        "task_id": task_id,
        "base_revision": base_revision,
        "command_id": command_id,
        "backup_id": command_id,
        "result": result,
    }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let task_id = inputs.require("task")?;
    let command_id = inputs.require("command-id")?;
    let base_revision = crate::integer(
        inputs.require("base-revision")?,
        "base-revision",
        0,
        i64::MAX,
    )?;
    if !command_id_valid(command_id) {
        return Err(Failure::invalid(
            INVALID_COMMAND_ID.code,
            "`--command-id` must be 8-128 characters, start with a letter or digit, and contain only letters, digits, _ or -",
        )
        .detail(json!({ "field": "command-id" }))
        .remedy(INVALID_COMMAND_ID.remedy));
    }

    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &Action::Commit {
            command_id: command_id.to_owned(),
            base_revision,
            command: json!({ "kind": "delete_task", "delete_task": { "task_id": task_id } }),
        },
    )?;
    let resolved_project = report.project_id().to_owned();
    verified_delete(
        &resolved_project,
        task_id,
        command_id,
        base_revision,
        report.into_result(),
    )
}

pub fn render(data: &Value) -> String {
    let result = &data["result"];
    let mut out = format!(
        "deleted {} in {} · {} tasks, {} dependencies, {} residuals · revision {} · backup {}\n",
        data["task_id"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        result["deleted_task_ids"].as_array().map_or(0, Vec::len),
        result["deleted_dependency_ids"]
            .as_array()
            .map_or(0, Vec::len),
        result["deleted_residual_ids"]
            .as_array()
            .map_or(0, Vec::len),
        result["committed_revision"].as_i64().unwrap_or(0),
        data["backup_id"].as_str().unwrap_or("?"),
    );
    out.push_str(&super::warnings(result));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied() -> Value {
        json!({
            "applied": true,
            "kind": "delete_task",
            "command_id": "delete-1234",
            "backup_id": "delete-1234",
            "committed_revision": 8,
            "deleted_task_ids": ["T-1", "T-2"],
            "deleted_dependency_ids": ["D-1"],
            "deleted_residual_ids": ["R-1"],
            "warnings": null,
        })
    }

    #[test]
    fn requires_exact_backup_and_selected_task_in_applied_result() {
        let receipt = verified_delete("project", "T-1", "delete-1234", 7, applied())
            .expect("matching server result");
        assert_eq!(receipt["backup_id"], "delete-1234");
        assert_eq!(receipt["result"]["deleted_task_ids"], json!(["T-1", "T-2"]));

        let mut wrong_backup = applied();
        wrong_backup["backup_id"] = json!("other-1234");
        assert_eq!(
            verified_delete("project", "T-1", "delete-1234", 7, wrong_backup)
                .expect_err("mismatch")
                .code(),
            crate::deletion::RESPONSE_UNREADABLE.code
        );
        let mut missing_root = applied();
        missing_root["deleted_task_ids"] = json!(["T-2"]);
        assert_eq!(
            verified_delete("project", "T-1", "delete-1234", 7, missing_root)
                .expect_err("missing selected task")
                .code(),
            crate::deletion::RESPONSE_UNREADABLE.code
        );
    }

    #[test]
    fn refusal_and_bad_command_id_do_not_report_success() {
        let refused = verified_delete(
            "project",
            "T-1",
            "delete-1234",
            7,
            json!({ "applied": false, "violations": [{ "message": "assigned descendant" }] }),
        );
        assert_eq!(refused.expect_err("refusal").code(), crate::PM_REFUSED.code);
        for id in ["short", ".delete-1234", "delete.1234", "delete 1234"] {
            assert!(!command_id_valid(id), "{id}");
        }
        assert!(command_id_valid("delete-1234"));
    }
}
