//! The server's bounded list of Project Work assignment targets.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

const ROSTER_UNREADABLE: Refusal = Refusal {
    code: "member_roster_unreadable",
    when: "the PM server returned an incomplete or differently shaped assignment roster",
    remedy: "update ds and the server as one release; do not infer assignees from the plan",
};

pub static COMMAND: Command = Command {
    id: "pm.member.list",
    path: &["pm", "member", "list"],
    contract: 1,
    summary: "List the project's eligible Project Work assignees.",
    purpose: "Read the bounded project-side member directory the PM server uses for task assignment. The plan's admin permission grants editing access but does not make a platform administrator a member of every project. A row says whether the account is active and assignable. Use this command before naming an owner or assignment requester.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::LANE_ARG, crate::PROJECT_ARG],
    output: "The exact project id, scanned count, truncation flag and up to 500 members, each with email, roles, active and assignable. A truncated roster is not an exhaustive assignment list.",
    examples: &[Example {
        command: "ds pm member list --project <exact-id> --output json",
        note: "Use .data.members[] where assignable is true; do not infer membership from pm.plan.permissions.admin.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[ROSTER_UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "assignee",
        "project member",
        "member roster",
        "assignment",
        "active people",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let report = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &ds_client_core::project_management::Command::Assignees,
    )?;
    let project_id = report.project_id().to_owned();
    let roster = report.into_result();
    validate_roster(&roster, &project_id)?;
    Ok(roster)
}

fn validate_roster(roster: &Value, project: &str) -> Result<(), Failure> {
    let valid = roster["project_id"].as_str() == Some(project)
        && roster["scanned"].as_u64().is_some()
        && roster["truncated"].as_bool().is_some()
        && roster["members"].as_array().is_some_and(|members| {
            members.len() <= 500
                && members.iter().all(|member| {
                    member["email"]
                        .as_str()
                        .is_some_and(|email| !email.is_empty())
                        && member["roles"]
                            .as_array()
                            .is_some_and(|roles| roles.iter().all(Value::is_string))
                        && member["active"].as_bool().is_some()
                        && member["assignable"].as_bool().is_some()
                })
        });
    if valid {
        Ok(())
    } else {
        Err(
            Failure::invalid(ROSTER_UNREADABLE.code, ROSTER_UNREADABLE.when)
                .remedy(ROSTER_UNREADABLE.remedy),
        )
    }
}

pub fn render(data: &Value) -> String {
    let members = data["members"].as_array();
    let mut out = format!(
        "{} project members in {}{}\n",
        members.map_or(0, Vec::len),
        data["project_id"].as_str().unwrap_or("?"),
        if data["truncated"] == true {
            " (truncated)"
        } else {
            ""
        },
    );
    if let Some(members) = members {
        for member in members {
            let roles = member["roles"]
                .as_array()
                .map(|roles| {
                    roles
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let status = if member["assignable"] == true {
                "assignable"
            } else {
                "unavailable"
            };
            let active = if member["active"] == true {
                "active"
            } else {
                "inactive"
            };
            out.push_str(&format!(
                "  {}  {}  {}  {}\n",
                member["email"].as_str().unwrap_or("?"),
                roles,
                active,
                status
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roster_requires_the_requested_project_and_explicit_eligibility() {
        let good = json!({"project_id":"p","scanned":2,"truncated":false,"members":[
            {"email":"member@example.com","roles":["editor"],"active":true,"assignable":true},
            {"email":"sleeping@example.com","roles":["reader"],"active":false,"assignable":false}
        ]});
        validate_roster(&good, "p").expect("server roster");
        assert!(validate_roster(&good, "other").is_err());
        let mut missing = good;
        missing["members"][0]
            .as_object_mut()
            .unwrap()
            .remove("assignable");
        assert!(validate_roster(&missing, "p").is_err());
    }
}
