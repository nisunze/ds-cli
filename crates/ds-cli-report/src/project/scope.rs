//! `ds report project scope` — the plan: who participates in a Combined Report run.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use super::{LANE_ARG, PROJECT_ARG, TRANSFORMER_ARG};

pub static COMMAND: Command = Command {
    id: "report.project.scope",
    path: &["report", "project", "scope"],
    contract: 1,
    summary: "Show which transformers a Combined Report would include.",
    purpose: "\
Start here. Restores the native user and reads only its audience-fenced \
named project's transformer lifecycle inventory. Without --transformer the \
scope is every active saved transformer, which is exactly what `combined` \
resolves; with names it checks each one, so a retired, deleted or missing \
name is reported before any artifact is produced. A reserved computed \
identity — `collisions`, `combined_transformer` and its aliases — is what a \
report produces, never a participant, and is refused outright. Nothing is \
generated. No project, Desktop descriptor, URL, body or action override is \
accepted.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG, PROJECT_ARG],
    output: "\
Lane and named-project identity/status, the scope `mode`, the participating \
transformers and count, the excluded names with their lifecycle state and \
retirement reason, project-level inventory rows (which are never Combined \
Report inputs), and `combined_ready` (at least one active LV transformer).",
    examples: &[Example {
        command: "ds report project scope --output json --project <exact-id>",
        note: "`.data.excluded` lists what a Combined Report run would leave out, and why.",
        runnable: false,
    }],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs)?;
    let headless = ds_cli_auth::transformer_inventory_for_project(
        inputs.require("lane")?,
        inputs.require("project")?,
        &requested,
    )?;
    let mut output = super::project_receipt(&headless);
    output["scope"] = super::scope_json(&requested, headless.result());
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let scope = &data["scope"];
    let mut out = format!(
        "project {} ({}) · {} · scope {} · {} participating · {} excluded · combined {}\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        scope["mode"].as_str().unwrap_or("?"),
        scope["participating_count"].as_u64().unwrap_or(0),
        scope["excluded_count"].as_u64().unwrap_or(0),
        if scope["combined_ready"].as_bool().unwrap_or(false) {
            "ready"
        } else {
            "not ready"
        },
    );
    if let Some(excluded) = scope["excluded"].as_array() {
        for entry in excluded {
            out.push_str(&format!(
                "  excluded {:<28} {}{}\n",
                entry["name"].as_str().unwrap_or("?"),
                entry["state"].as_str().unwrap_or("?"),
                entry["reason"]
                    .as_str()
                    .map(|r| format!(" · {r}"))
                    .unwrap_or_default(),
            ));
        }
    }
    out
}
