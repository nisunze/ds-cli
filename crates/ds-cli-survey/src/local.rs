//! What this machine holds of a project's survey data, answered offline.
//!
//! The core keeps one copy per account, project and form (the kernel's
//! `survey::hold`, bytes in `ds_project_data::survey_hold`): its filter, rows,
//! refresh time and cursor, and the photos held beside it. This command reads
//! that and the working-area form choice, and never goes online, so an agent
//! can decide whether a read will be local before it makes one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::time::OffsetDateTime;
use ds_project_data::survey_hold as store;
use serde_json::{Value, json};

const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

const REFUSALS: &[Refusal] = &[
    ds_cli_auth::SIGNED_OUT_REFUSAL,
    Refusal {
        code: "project_required",
        when: "--project is absent, blank or untrimmed",
        remedy: "pass one exact ds_project value from ds auth project list",
    },
    Refusal {
        code: "survey_hold_store",
        when: "the core's survey copy or the working-area choice could not be read",
        remedy: "check the layer root, then retry",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
];

pub static STATUS_COMMAND: Command = Command {
    id: "survey.local.status",
    path: &["survey", "local", "status"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "What survey data this machine holds for a project, offline.",
    purpose: "Use before reading or reporting survey data: each form held here with the filter it was read under, its rows, when it was refreshed and its cursor, the photos and thumbnails held, and the forms the working area loads. Nothing is fetched. `ds survey entries read` fills and refreshes the copy; `ds survey photo fetch` fills the photos.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::PROJECT, LANE],
    output: "Per form: held or not, filter and whether it is the whole form, rows, refreshed time and age, cursor, last read, photos and thumbnails held; the working-area choice; where held photos live.",
    examples: &[Example {
        command: "ds survey local status --project <exact-id> --output json",
        note: "Which forms a read would answer locally, and how fresh each is.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &["held survey", "local survey", "survey cache"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = crate::text(inputs.require("project")?, "project", 256)?;
    let lane = inputs.require("lane")?;
    let identity =
        ds_cli_auth::probe_headless_identity_for_named_project(lane)?.ok_or_else(|| {
            Failure::unauthorized(
                "headless_signed_out",
                "this machine has no signed-in account on this lane",
            )
            .remedy("run `ds account connect`")
        })?;
    let unreadable = |message: String| {
        Failure::unavailable("survey_hold_store", message)
            .remedy("check the layer root, then retry")
    };
    let root = ds_layer_store::default_root().map_err(unreadable)?;
    let chosen = ds_layer_store::working_area_forms::read(identity.lane(), identity.uid(), project)
        .map_err(unreadable)?;
    let scope = Scope {
        principal: identity.uid().to_owned(),
        project: project.to_owned(),
    };
    let forms = chosen.clone().unwrap_or_default();
    let mut report =
        store::status(&root, &scope, &forms, OffsetDateTime::now_utc()).map_err(unreadable)?;
    report["lane"] = json!(identity.lane());
    report["working_area"] = json!({"chosen": chosen.is_some(), "forms": chosen});
    report["media_dir"] = json!(store::media_dir(&root, &scope));
    Ok(report)
}

pub fn render(data: &Value) -> String {
    let project = data["project"].as_str().unwrap_or("project");
    let mut text = format!(
        "{project}: {} forms held, {} rows\n",
        data["held_forms"].as_u64().unwrap_or(0),
        data["held_rows"].as_u64().unwrap_or(0)
    );
    for form in data["forms"].as_array().into_iter().flatten() {
        let name = form["form"].as_str().unwrap_or("?");
        let photos = format!(
            "{} photos, {} thumbnails",
            form["photos"]["originals"].as_u64().unwrap_or(0),
            form["photos"]["thumbnails"].as_u64().unwrap_or(0)
        );
        if form["held"].as_bool() == Some(true) {
            let scope = if form["whole_form"].as_bool() == Some(true) {
                "whole form"
            } else {
                "filtered"
            };
            text.push_str(&format!(
                "  {name}  {} rows ({scope})  refreshed {}s ago  {photos}\n",
                form["rows"].as_u64().unwrap_or(0),
                form["age_seconds"].as_i64().unwrap_or(0),
            ));
        } else {
            text.push_str(&format!("  {name}  not held  {photos}\n"));
        }
    }
    if data["working_area"]["chosen"].as_bool() != Some(true) {
        text.push_str("  the working area has no form choice on this machine\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_says_what_is_held_and_what_is_not() {
        let text = render(&json!({"project":"p1","held_forms":1,"held_rows":91,
            "forms":[{"form":"transformer_survey","held":true,"whole_form":true,"rows":91,
                      "age_seconds":42,"photos":{"originals":103,"thumbnails":103}},
                     {"form":"lv_poles","held":false,"photos":{"originals":0,"thumbnails":0}}],
            "working_area":{"chosen":false,"forms":null}}));
        assert!(text.contains("p1: 1 forms held, 91 rows"));
        assert!(text.contains("transformer_survey  91 rows (whole form)  refreshed 42s ago  103 photos, 103 thumbnails"));
        assert!(text.contains("lv_poles  not held"));
        assert!(text.contains("no form choice"));
    }
}
