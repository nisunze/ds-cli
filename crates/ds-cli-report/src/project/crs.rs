//! Read existing project projection authority without opening report queues or settings.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Execution, Refusal, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::printing::project_crs::Capture;
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "report.project.crs",
    path: &["report", "project", "crs"],
    contract: 1,
    summary: "Capture the named project's projection authority for printing.",
    purpose: "Read only the authenticated project directory and capture normalized ProjectParams with its exact project, lane, user, audience and source digest. Opens no report queue or outbox, touches no project, performs no automatic publication, and reads or mutates no printing template, configuration or seed. A capture records authority; rendering separately refuses projections that are not admitted metre coordinate systems.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[super::LANE_ARG, super::PROJECT_ARG],
    output: "Named project, lane, authenticated project CRS capture and exact ProjectParams source SHA-256.",
    examples: &[],
    refusals: &[
        super::HEADLESS_SIGNED_OUT,
        super::AUTH_CONTEXT_MISMATCH,
        super::NATIVE_PROFILE,
        super::NATIVE_PROFILE_DIGEST,
        super::NOT_FOUND,
        Refusal {
            code: "print_project_crs_missing",
            when: "the exact authenticated project has no captured projection authority",
            remedy: "connect the native account to the selected lane and verify access to the exact project",
        },
        Refusal {
            code: "print_project_crs_invalid",
            when: "existing ProjectParams projection metadata is malformed",
            remedy: "repair the existing project projection authority",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn capture(
    project: &str,
    lane: &str,
    uid: &str,
    audience: &str,
    params: Option<&Value>,
) -> Result<Capture, Failure> {
    let params = params.ok_or_else(|| Failure::invalid("print_project_crs_missing", "The authenticated directory supplies no projection authority for this exact project.").remedy("Connect the native account to the selected lane and verify access to the exact project."))?;
    Capture::new(project, lane, uid, audience, params.clone()).map_err(|error| {
        Failure::invalid("print_project_crs_invalid", error)
            .detail(json!({"project_params_shape": if params.is_null() { "missing" } else if params.is_object() { "object" } else { "non_object" }, "crs_present": params.get("crs").is_some(), "crs_is_object": params.get("crs").is_some_and(Value::is_object), "mode_is_string": params.pointer("/crs/mode").is_some_and(Value::is_string)}))
            .remedy("Repair the project's existing ProjectParams projection authority.")
    })
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let directory = ds_cli_auth::project_directory(lane)?;
    let identity = directory.identity();
    let capture = capture(
        project,
        lane,
        identity.uid(),
        identity.credential_audience_sha256(),
        directory.project_params(project),
    )
    .map_err(|failure| {
        let mut detail = failure.detail_value().cloned().unwrap_or_else(|| json!({}));
        detail["legacy_crs_shape"] = directory
            .legacy_project_crs_shape(project)
            .cloned()
            .unwrap_or(Value::Null);
        failure.detail(detail)
    })?;
    Ok(
        json!({"project": project, "lane": lane, "source_sha256": capture.source_sha256, "capture": capture}),
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "project {} · {} · CRS source {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["source_sha256"].as_str().unwrap_or("?")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_crs_capture_requires_exact_directory_authority() {
        assert!(capture("other", "stable", "user", "audience", None).is_err());
        assert!(capture("project", "stable", "user", "audience", Some(&json!({}))).is_err());
        assert!(
            capture(
                "project",
                "stable",
                "user",
                "audience",
                Some(&json!({"crs":{"mode":3}}))
            )
            .is_err()
        );
        let params = json!({"crs":{"mode":"utm", "utm_zone":35, "utm_hemisphere":"south"}});
        let actual = capture("project", "stable", "user", "audience", Some(&params)).unwrap();
        assert_eq!(actual.project_params, params);
        assert!(
            actual
                .validate_scope("other", "stable", "user", "audience", None)
                .is_err()
        );
    }
}
