//! `ds report project scope` — the plan: who participates in a Combined Report run.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use super::grouping::{self, GROUP_BY_ARG, WHERE_ARG};
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
report produces, never a participant, and is refused outright. With \
--group-by/--where it previews, from the tag projection, the archives a \
grouped `combined` would publish. Nothing is \
generated or saved. No project, Desktop descriptor, URL, body or action \
override is accepted.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        GROUP_BY_ARG,
        WHERE_ARG,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "\
Lane and named-project identity/status, the scope `mode`, the participating \
transformers and count, the excluded names with their lifecycle state and \
retirement reason, project-level inventory rows (which are never Combined \
Report inputs), and `combined_ready` (at least one active LV transformer). \
Grouped: `grouping` with its counts, the projection sha256, and each group's \
`path` and `transformers`.",
    examples: &[
        Example {
            command: "ds report project scope --output json --project <exact-id>",
            note: "`.data.excluded` lists what a Combined Report run would leave out, and why.",
            runnable: false,
        },
        Example {
            command: "ds report project scope --group-by city --group-by phase --output json --project <exact-id>",
            note: "`.data.grouping.groups` is one future archive each; `_unassigned` is untagged.",
            runnable: false,
        },
    ],
    refusals: super::SCOPE_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["preview groups"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    if let Some(requested) = grouping::requested(inputs)? {
        let grouped = grouping::resolve(
            inputs.require("lane")?,
            inputs.require("project")?,
            &requested,
        )?;
        let mut output = grouped.receipt.clone();
        output["scope"] = grouped.scope.clone();
        output["scope"]["mode"] = "grouped".into();
        output["grouping"] = grouping::grouping_json(&grouped, true);
        return Ok(output);
    }
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
    let grouping = &data["grouping"];
    if grouping.is_object() {
        out.push_str(&format!(
            "  {} group(s) over {} transformer(s) · {} filtered out · {} with an _unassigned level\n",
            grouping["group_count"].as_u64().unwrap_or(0),
            grouping["transformer_count"].as_u64().unwrap_or(0),
            grouping["filtered_out_count"].as_u64().unwrap_or(0),
            grouping["unassigned_count"].as_u64().unwrap_or(0),
        ));
        for group in grouping["groups"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  group {:<40} {}\n",
                path_line(&group["path"]),
                group["transformer_count"].as_u64().unwrap_or(0),
            ));
        }
    }
    out
}

/// `city=bere / phase=i`, from a rendered group path.
pub(super) fn path_line(path: &Value) -> String {
    let steps: Vec<String> = path
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "{}={}",
                step["key"].as_str().unwrap_or("?"),
                step["value"].as_str().unwrap_or("?")
            )
        })
        .collect();
    if steps.is_empty() {
        "(filtered scope)".to_string()
    } else {
        steps.join(" / ")
    }
}
