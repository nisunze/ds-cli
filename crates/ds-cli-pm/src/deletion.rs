//! Durable Project Work deletion backups. The server owns their contents,
//! ordering, integrity checks and exact-head restore.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Map, Value, json};

use crate::{LANE_ARG, PROJECT_ARG};

const BACKUP_ARG: Arg = Arg::value(
    "backup",
    "<backup-id>",
    "Deletion backup id from `ds pm deletion inventory` or the delete result.",
)
.required();
const LIMIT_ARG: Arg = Arg::value("limit", "<count>", "Backups in one page (1-100).").default("50");
const CURSOR_ARG: Arg = Arg::value(
    "cursor",
    "<backup-id>",
    "Continue after the previous page's next_cursor, when it is nonempty.",
);
const ITEM_KEY_ARG: Arg = Arg::value(
    "item-key",
    "<snapshot-key>",
    "Read one immutable snapshot named by summary.entries[].key.",
);
const BASE_REVISION_ARG: Arg = Arg::value(
    "base-revision",
    "<revision>",
    "Exact live plan revision reviewed before restore; obtain it with `ds pm plan`.",
)
.required();
const COMMAND_ID_ARG: Arg = Arg::value(
    "command-id",
    "<idempotency-id>",
    "Stable id for this restore; reuse it if a reply is lost (8-128 letters, digits, _ or -).",
)
.required();

pub const RESPONSE_UNREADABLE: Refusal = Refusal {
    code: "deletion_response_unreadable",
    when: "the server answered a deletion command with an incomplete or inconsistent result",
    remedy: "report the backup id and response; do not infer that a restore committed",
};

pub static INVENTORY: Command = Command {
    id: "pm.deletion.inventory",
    path: &["pm", "deletion", "inventory"],
    contract: 1,
    summary: "List durable deletion backups for one project, newest first.",
    purpose: "List backups the server saved when Project Work tasks were deleted. Each row names the deleted roots and captured graph entries. Page with the opaque next_cursor. This is a server read under the signed-in identity and the exact --project; it does not inspect local files.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LIMIT_ARG, CURSOR_ARG, LANE_ARG, PROJECT_ARG],
    output: "`project`, `backups` (id, roots, entries, delete and restore metadata), and `next_cursor` for the next page.",
    examples: &[Example {
        command: "ds pm deletion inventory --project <exact-id> --output json",
        note: "Use one backup id with `ds pm deletion read` before restoring it.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<23>(&[crate::INVALID_NUMBER, RESPONSE_UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "delete",
        "deleted task",
        "backup",
        "recover",
        "restore",
        "undo",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static READ: Command = Command {
    id: "pm.deletion.read",
    path: &["pm", "deletion", "read"],
    contract: 1,
    summary: "Inspect one deletion backup or one captured graph document.",
    purpose: "Read the server's immutable deletion backup summary and, when --item-key is given, one captured document. The summary lists every entry and SHA-256 so a restore can be reviewed before it is issued.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[BACKUP_ARG, ITEM_KEY_ARG, LANE_ARG, PROJECT_ARG],
    output: "`project` and `backup`: the complete `summary`, plus `entry` and `document` when --item-key is given.",
    examples: &[Example {
        command: "ds pm deletion read --project <exact-id> --backup <backup-id> --output json",
        note: "Inspect summary.roots and summary.entries before a restore.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<22>(&[RESPONSE_UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &["delete", "deleted task", "recover", "restore", "snapshot"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static RESTORE: Command = Command {
    id: "pm.deletion.restore",
    path: &["pm", "deletion", "restore"],
    contract: 1,
    summary: "Restore a deletion backup against one exact live plan revision.",
    purpose: "Ask the server to verify every captured snapshot and atomically restore its tasks and relationships. Supply the plan revision you reviewed; a changed head is refused. Supply a stable command id so retrying after a lost response replays the same decision. The server checks membership, schedule-editor permission, backup integrity and live id conflicts.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        BACKUP_ARG,
        BASE_REVISION_ARG,
        COMMAND_ID_ARG,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "`project`, `backup_id`, `command_id`, `base_revision`, and the server's complete applied result including `committed_revision`, patches and warnings.",
    examples: &[Example {
        command: "ds pm deletion restore --project <exact-id> --backup <backup-id> --base-revision 42 --command-id restore-20260929-1 --yes --output json",
        note: "Review the backup and `ds pm plan` first. Reuse the same command id for a retry.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<26>(&[
        crate::INVALID_NUMBER,
        crate::BACKUP_INCOMPLETE,
        crate::BACKUP_ALREADY_RESTORED,
        crate::RESTORE_ID_CONFLICT,
        RESPONSE_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["delete", "deleted task", "recover", "undo"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn response_object(value: Value, field: &str) -> Result<Map<String, Value>, Failure> {
    let map = value.as_object().ok_or_else(|| {
        Failure::internal(
            RESPONSE_UNREADABLE.code,
            "the deletion response is not an object",
        )
        .remedy(RESPONSE_UNREADABLE.remedy)
    })?;
    if map.get(field).is_none_or(Value::is_null) {
        return Err(Failure::internal(
            RESPONSE_UNREADABLE.code,
            format!("the deletion response has no {field}"),
        )
        .remedy(RESPONSE_UNREADABLE.remedy));
    }
    Ok(map.clone())
}

fn with_project(project: &str, value: Value, field: &str) -> Result<Value, Failure> {
    let mut map = response_object(value, field)?;
    map.insert("project".into(), json!(project));
    Ok(Value::Object(map))
}

pub fn inventory(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = inputs
        .value("limit")
        .map(|raw| crate::integer(raw, "limit", 1, 100))
        .transpose()?
        .unwrap_or(50);
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::DeletionInventory {
            limit: Some(limit),
            cursor: inputs.value("cursor").map(str::to_owned),
        },
    )?;
    let project = report.project_id().to_owned();
    let result = with_project(&project, report.into_result(), "backups")?;
    if !result["backups"].is_array() || !result["next_cursor"].is_string() {
        return Err(Failure::internal(
            RESPONSE_UNREADABLE.code,
            "the backup inventory is missing its page rows or cursor",
        )
        .remedy(RESPONSE_UNREADABLE.remedy));
    }
    Ok(result)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let backup_id = inputs.require("backup")?;
    let item_key = inputs.value("item-key");
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::DeletionRead {
            backup_id: backup_id.to_owned(),
            item_key: item_key.map(str::to_owned),
        },
    )?;
    let project = report.project_id().to_owned();
    let result = with_project(&project, report.into_result(), "backup")?;
    if !result["backup"]["summary"].is_object()
        || result["backup"]["summary"]["id"].as_str() != Some(backup_id)
        || item_key.is_some_and(|key| {
            result["backup"]["entry"]["key"].as_str() != Some(key)
                || !result["backup"]["document"].is_object()
        })
    {
        return Err(Failure::internal(
            RESPONSE_UNREADABLE.code,
            "the backup read does not match the request",
        )
        .remedy(RESPONSE_UNREADABLE.remedy));
    }
    Ok(result)
}

fn verified_restore(
    project: &str,
    backup_id: &str,
    command_id: &str,
    base_revision: i64,
    result: Value,
) -> Result<Value, Failure> {
    let result = response_object(result, "applied")?;
    let applied = result
        .get("applied")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            Failure::internal(
                RESPONSE_UNREADABLE.code,
                "restore result has no applied flag",
            )
            .remedy(RESPONSE_UNREADABLE.remedy)
        })?;
    if !applied {
        let violations = result.get("violations").cloned().unwrap_or(Value::Null);
        let message = violations
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row["message"].as_str())
            .unwrap_or("The server did not apply this deletion restore.");
        return Err(Failure::invalid(crate::PM_REFUSED.code, message)
            .detail(json!({ "backup_id": backup_id, "violations": violations }))
            .remedy(crate::PM_REFUSED.remedy));
    }
    if result.get("backup_id").and_then(Value::as_str) != Some(backup_id)
        || result.get("command_id").and_then(Value::as_str) != Some(command_id)
        || result
            .get("committed_revision")
            .and_then(Value::as_i64)
            .is_none()
    {
        return Err(Failure::internal(
            RESPONSE_UNREADABLE.code,
            "restore result does not match the requested backup and command",
        )
        .remedy(RESPONSE_UNREADABLE.remedy));
    }
    Ok(json!({
        "project": project,
        "backup_id": backup_id,
        "command_id": command_id,
        "base_revision": base_revision,
        "result": result,
    }))
}

pub fn restore(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let backup_id = inputs.require("backup")?;
    let command_id = inputs.require("command-id")?;
    let base_revision = crate::integer(
        inputs.require("base-revision")?,
        "base-revision",
        0,
        i64::MAX,
    )?;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &Action::DeletionRestore {
            backup_id: backup_id.to_owned(),
            command_id: command_id.to_owned(),
            base_revision,
        },
    )?;
    let project = report.project_id().to_owned();
    verified_restore(
        &project,
        backup_id,
        command_id,
        base_revision,
        report.into_result(),
    )
}

pub fn render_inventory(data: &Value) -> String {
    let backups = data["backups"].as_array();
    let mut out = format!(
        "{} in {}\n",
        crate::plural(backups.map_or(0, Vec::len) as u64, "deletion backup"),
        data["project"].as_str().unwrap_or("?"),
    );
    if let Some(backups) = backups {
        for backup in backups {
            out.push_str(&format!(
                "  {} · {} roots · {} entries · revision {}{}\n",
                backup["id"].as_str().unwrap_or("?"),
                backup["roots"].as_array().map_or(0, Vec::len),
                backup["entries"].as_array().map_or(0, Vec::len),
                backup["delete_revision"].as_i64().unwrap_or(0),
                if backup["restored_at"].as_i64().unwrap_or(0) > 0 {
                    " · restored"
                } else {
                    ""
                }
            ));
        }
    }
    if let Some(cursor) = data["next_cursor"]
        .as_str()
        .filter(|cursor| !cursor.is_empty())
    {
        out.push_str(&format!("next cursor: {cursor}\n"));
    }
    out
}

pub fn render_read(data: &Value) -> String {
    let summary = &data["backup"]["summary"];
    let mut out = format!(
        "backup {} in {} · {} roots · {} entries · deleted at revision {}\n",
        summary["id"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        summary["roots"].as_array().map_or(0, Vec::len),
        summary["entries"].as_array().map_or(0, Vec::len),
        summary["delete_revision"].as_i64().unwrap_or(0),
    );
    if let Some(entries) = summary["entries"].as_array() {
        for entry in entries {
            out.push_str(&format!(
                "  {} {} · key {} · sha256 {}\n",
                entry["collection"].as_str().unwrap_or("?"),
                entry["id"].as_str().unwrap_or("?"),
                entry["key"].as_str().unwrap_or("?"),
                entry["sha256"].as_str().unwrap_or("?"),
            ));
        }
    }
    if summary["restored_at"].as_i64().unwrap_or(0) > 0 {
        out.push_str(&format!(
            "restored by command {}\n",
            summary["restore_command_id"].as_str().unwrap_or("?"),
        ));
    }
    if data["backup"].get("document").is_some() {
        out.push_str("captured document available in --output json\n");
    }
    out
}

pub fn render_restore(data: &Value) -> String {
    format!(
        "restored backup {} in {} · revision {} · command {}\n",
        data["backup_id"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["result"]["committed_revision"].as_i64().unwrap_or(0),
        data["command_id"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_only_reports_applied_for_matching_server_result() {
        let good = json!({
            "applied": true,
            "backup_id": "delete-1234",
            "command_id": "restore-1234",
            "committed_revision": 8,
            "warnings": [],
        });
        let receipt = verified_restore("project", "delete-1234", "restore-1234", 7, good.clone())
            .expect("matching server result");
        assert_eq!(receipt["result"]["committed_revision"], 8);
        let wrong_backup = verified_restore("project", "other-1234", "restore-1234", 7, good);
        assert_eq!(
            wrong_backup.expect_err("mismatch").code(),
            RESPONSE_UNREADABLE.code
        );
        let refused = verified_restore(
            "project",
            "delete-1234",
            "restore-1234",
            7,
            json!({"applied": false, "violations": [{"message": "head moved"}]}),
        );
        assert_eq!(refused.expect_err("refusal").code(), crate::PM_REFUSED.code);
    }
}
