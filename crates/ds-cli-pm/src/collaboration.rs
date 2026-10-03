//! Narrow adapters for existing collaboration index and task comments.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_management::Command as PMCommand;
use serde_json::{Value, json};

const LIMIT: Arg = Arg::value("limit", "<1..100>", "Bound rows per page (default 50).");
const CURSOR: Arg = Arg::value(
    "cursor",
    "<cursor>",
    "Replay the exact next_cursor from the preceding page.",
);
const ID: Arg = Arg::value(
    "id",
    "<comment-id>",
    "Stable comment id; read comments after an uncertain reply before choosing another id.",
);
const BODY: Arg = Arg::value("body", "<text>", "Comment text, 1..4000 UTF-8 bytes.");
const PAGE_LIMIT: Refusal = Refusal {
    code: "pm_page_limit",
    when: "the requested page limit is not an integer from 1 through 100",
    remedy: "pass --limit with an integer from 1 through 100",
};
const UNREADABLE: Refusal = Refusal {
    code: "pm_collaboration_unreadable",
    when: "the collaboration response is incomplete or belongs to another request",
    remedy: "update the server and ds together; do not infer success from missing fields",
};
const INDEX_COMMAND_INVALID: Refusal = Refusal {
    code: "pm_index_command_invalid",
    when: "the public-index adapter received a command outside its closed read type",
    remedy: "update ds and use pm project list; report a repeated adapter refusal",
};

pub static PROJECTS: Command = Command {
    id: "pm.project.list",
    path: &["pm", "project", "list"],
    contract: 1,
    summary: "List authenticated collaboration projects outside member selection.",
    purpose: "Read the existing public Project Work index. Returns bounded status/cursor pages; eligible membership still governs assignments and notices. Never selects a project.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::LANE_ARG,
        Arg::value(
            "status",
            "<active|archived|testing>",
            "Project status (default active).",
        ),
        LIMIT,
        CURSOR,
    ],
    output: "projects, status and next_cursor; an empty cursor ends this status page stream.",
    examples: &[Example {
        command: "ds pm project list --status active --limit 50 --output json",
        note: "Follow next_cursor with --cursor; this does not select a project.",
        runnable: true,
    }],
    refusals: &crate::read_refusals::<19>(&[PAGE_LIMIT, UNREADABLE, INDEX_COMMAND_INVALID]),
    reference: Some("docs/reference/pm.md"),
    search: &["public projects", "project discovery"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static COMMENT: Command = Command {
    id: "pm.task.comment",
    path: &["pm", "task", "comment"],
    contract: 1,
    summary: "Add an ordinary comment to an existing Project Work task.",
    purpose: "Calls the existing authenticated create_comment action with an explicit project/task and stable id. Does not change review state. After uncertain delivery, read comments by the same id; duplicate creation is refused without a second comment.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::TASK_ARG,
        ID.required(),
        BODY.required(),
        crate::LANE_ARG,
    ],
    output: "project and comment receipt with server id, version and fields; no chat delivery claim.",
    examples: &[Example {
        command: "ds pm task comment --project <exact-id> --task <task-id> --id <comment-id> --body \"Please check the crossing\" --yes --output json",
        note: "Read the same comment id after an uncertain reply; ordinary comments leave review state unchanged.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<22>(&[UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &["task comment", "collaboration"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static COMMENTS: Command = Command {
    id: "pm.task.comments",
    path: &["pm", "task", "comments"],
    contract: 1,
    summary: "Read a bounded page of comments on one task.",
    purpose: "Read the existing task-scoped context comments and their record cursor. Follow next_cursor to find older comments. Never infer delivery from task existence.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::TASK_ARG,
        LIMIT,
        CURSOR,
        crate::LANE_ARG,
    ],
    output: "project, task, comments, truncated and next_cursor (pm_comments).",
    examples: &[Example {
        command: "ds pm task comments --project <exact-id> --task <task-id> --limit 50 --output json",
        note: "Follow next_cursor with --cursor to find older comments.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<18>(&[PAGE_LIMIT, UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &["task comments", "comment history"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn limit(inputs: &Inputs) -> Result<i64, Failure> {
    let value = inputs
        .value("limit")
        .unwrap_or("50")
        .parse::<i64>()
        .map_err(|_| {
            Failure::invalid(PAGE_LIMIT.code, "limit is an integer from 1 to 100")
                .remedy(PAGE_LIMIT.remedy)
        })?;
    if !(1..=100).contains(&value) {
        return Err(
            Failure::invalid(PAGE_LIMIT.code, "limit is from 1 to 100").remedy(PAGE_LIMIT.remedy)
        );
    }
    Ok(value)
}
fn unreadable() -> Failure {
    Failure::invalid(
        UNREADABLE.code,
        "the collaboration response is incomplete or belongs to a different request",
    )
    .remedy(UNREADABLE.remedy)
}
fn validate_projects(result: &Value, status: &str, limit: i64) -> Result<(), Failure> {
    if result["status"].as_str() != Some(status)
        || result["next_cursor"]
            .as_str()
            .is_none_or(|value| value.len() > 256)
        || !result["projects"].as_array().is_some_and(|rows| {
            rows.len() <= limit as usize
                && rows.iter().all(|row| {
                    row["id"].as_str().is_some_and(|id| !id.is_empty())
                        && row["name"].is_string()
                        && row["lifecycle_state"].as_str() == Some(status)
                })
        })
    {
        return Err(unreadable());
    }
    Ok(())
}
pub fn projects(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let status = inputs.value("status").unwrap_or("active");
    let limit = limit(inputs)?;
    let result = ds_cli_auth::project_management_public_projects(
        inputs.value("lane").unwrap_or("stable"),
        &PMCommand::PublicProjects {
            status: status.to_owned(),
            cursor: inputs.value("cursor").unwrap_or("").to_owned(),
            limit,
        },
    )?;
    validate_projects(&result, status, limit)?;
    Ok(result)
}

pub fn comment(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let result = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &PMCommand::TaskCommentCreate {
            task_id: inputs.require("task")?.to_owned(),
            id: inputs.require("id")?.to_owned(),
            body: inputs.require("body")?.to_owned(),
        },
    )?
    .into_result();
    let item = &result["item"];
    if item["id"].as_str() != inputs.value("id")
        || item["version"].as_u64().is_none_or(|version| version == 0)
        || item["data"]["task_id"].as_str() != inputs.value("task")
    {
        return Err(unreadable());
    }
    Ok(json!({"project":project,"comment":item}))
}
pub fn comments(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let task = inputs.require("task")?;
    let limit = limit(inputs)?;
    let result = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &PMCommand::TaskComments {
            task_id: task.to_owned(),
            cursor: inputs.value("cursor").unwrap_or("").to_owned(),
            limit,
        },
    )?
    .into_result();
    if result["project_id"].as_str() != Some(project)
        || result["task_id"].as_str() != Some(task)
        || result["comments"]
            .as_array()
            .is_none_or(|rows| rows.len() > limit as usize)
        || !result["truncated_by_collection"]["pm_comments"].is_boolean()
    {
        return Err(unreadable());
    }
    Ok(
        json!({"project":project,"task":task,"comments":result["comments"],"truncated":result["truncated_by_collection"]["pm_comments"],"next_cursor":result["next_cursors"]["pm_comments"]}),
    )
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collaboration_commands_declare_their_response_and_page_refusals() {
        for command in [&PROJECTS, &COMMENT, &COMMENTS] {
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code == unreadable().code()
                        && refusal.remedy == UNREADABLE.remedy)
            );
        }
        for command in [&PROJECTS, &COMMENTS] {
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code == PAGE_LIMIT.code
                        && refusal.remedy == PAGE_LIMIT.remedy)
            );
        }
    }
    #[test]
    fn public_project_page_must_match_status_and_bounds() {
        let page = json!({"status":"active","next_cursor":"next","projects":[{"id":"p","name":"Project","lifecycle_state":"active"}]});
        validate_projects(&page, "active", 1).unwrap();
        assert!(validate_projects(&page, "archived", 1).is_err());
        assert!(validate_projects(&page, "active", 0).is_err());
        assert!(validate_projects(&json!({"projects":[]}), "active", 50).is_err());
    }
}
