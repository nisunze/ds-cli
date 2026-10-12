//! One explicit metadata-only task attachment detach through the native PM owner.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_management::Command as PMCommand;
use ds_command_kernel::project_management::task_attachment_detach::Request;
use serde_json::{Value, json};

const ATTACHMENT: Arg = Arg::value(
    "attachment",
    "<attachment-id>",
    "Exact task attachment id reviewed with `ds pm task read`.",
)
.required();
const VERSION: Arg = Arg::value(
    "expected-version",
    "<version>",
    "Exact attachment version reviewed before detaching; a changed version is refused.",
)
.required();
const INVALID: Refusal = Refusal {
    code: "attachment_detach_invalid",
    when: "project, task or attachment reference is malformed, or expected-version is not a positive incrementable integer",
    remedy: "read the exact task and attachment, then supply their exact ids and positive attachment version",
};
pub static COMMAND: Command = Command {
    id: "pm.task.detach",
    path: &["pm", "task", "detach"],
    contract: 1,
    summary: "Detach one task attachment while retaining its file (needs --yes).",
    purpose: "Remove one reviewed attachment's primary task relationship. The server checks the exact project, task, attachment version and creator/admin permission in one transaction. Issued, approved, final and superseded evidence is protected. File bytes, downloads, storage generations and other references remain; task context omits the detached relationship. One explicit write, no speculative read or automatic retry. Headless on the named project.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::TASK_ARG,
        ATTACHMENT,
        VERSION,
        crate::LANE_ARG,
    ],
    output: "Verified project_id, task_id, attachment_id, resulting version, detached=true, changed and retained item. changed=false is an exact server replay, never a storage deletion.",
    examples: &[Example {
        command: "ds pm task detach --project <exact-id> --task T-0007 --attachment <attachment-id> --expected-version 3 --yes --output json",
        note: "Read the task first; review any conflict or lost reply before deciding to repeat the same exact request.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<22>(&[INVALID]),
    reference: Some("docs/reference/pm.md"),
    search: &["unlink", "attachment", "retain", "remove"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn request(inputs: &Inputs) -> Result<Request, Failure> {
    let invalid = |message: &str| Failure::invalid(INVALID.code, message).remedy(INVALID.remedy);
    let expected_version = inputs
        .require("expected-version")?
        .parse::<i64>()
        .map_err(|_| invalid("expected-version must be a positive integer"))?;
    let request = Request {
        task_id: inputs.require("task")?.to_owned(),
        attachment_id: inputs.require("attachment")?.to_owned(),
        expected_version,
    };
    request
        .validate_for(inputs.require("project")?)
        .map_err(invalid)?;
    Ok(request)
}
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let result = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &PMCommand::TaskDetach { request },
    )?
    .into_result();
    Ok(result)
}
pub fn render(data: &Value) -> String {
    format!(
        "detached {} from task {} - version {} - file retained{}\n",
        data["attachment_id"].as_str().unwrap_or("?"),
        data["task_id"].as_str().unwrap_or("?"),
        data["version"].as_i64().unwrap_or(0),
        if data["changed"] == json!(false) {
            " - already detached"
        } else {
            ""
        }
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_version_and_reference_refuse_before_login() {
        for (task, version) in [
            ("task", "0"),
            ("task", "-1"),
            ("task", "many"),
            ("task", "9223372036854775807"),
            ("foreign/task", "3"),
        ] {
            let argv = [
                "--project",
                "project",
                "--task",
                task,
                "--attachment",
                "file",
                "--expected-version",
                version,
            ]
            .map(str::to_owned);
            let inputs = ds_cli_contract::parse(&COMMAND, &argv).unwrap();
            assert_eq!(request(&inputs).unwrap_err().code(), INVALID.code);
        }
    }
}
