//! `ds survey dashboard` — the survey dashboard's numbers, computed here.
//!
//! The dashboard report is a transform of survey rows, so it runs on the edge
//! over the copy this machine already holds (`ds survey entries read` fills
//! it), through the kernel's one owner (`survey_dashboard`). Nothing is
//! fetched and ds-brain computes nothing: a form that is not held, or held
//! under a working-area filter, is named in the answer so a partial count is
//! never read as the project's.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::survey_dashboard;
use ds_command_kernel::time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ds_project_data::survey_hold as store;
use serde_json::{Value, json};

const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");

const NOTHING_HELD: Refusal = Refusal {
    code: "survey_dashboard_nothing_held",
    when: "this machine holds no survey copy for the project",
    remedy: "run `ds survey entries read --project <id> --form <slug>` for each form, then retry",
};
const REQUEST_INVALID: Refusal = Refusal {
    code: "survey_dashboard_request_invalid",
    when: "a held copy is outside the dashboard's bounds or not one form slug",
    remedy: "refresh the named form with `ds survey entries read`, then retry",
};

const REFUSALS: &[Refusal] = &[
    ds_cli_auth::SIGNED_OUT_REFUSAL,
    Refusal {
        code: "project_required",
        when: "--project is absent, blank or untrimmed",
        remedy: "pass one exact ds_project value from ds auth project list",
    },
    Refusal {
        code: "survey_hold_store",
        when: "the core's survey copy could not be read or failed its digest",
        remedy: "check the layer root, then refresh the copy with `ds survey entries read`",
    },
    NOTHING_HELD,
    REQUEST_INVALID,
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.dashboard",
    path: &["survey", "dashboard"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Survey dashboard numbers computed here from the held copy, offline.",
    purpose: "Totals per form, entries per day and per surveyor with their spread, distinct surveyors per day and the administrative breakdown, aggregated on this machine from the survey rows it holds (deleted entries excluded, empty forms omitted). Nothing is fetched and no cloud service aggregates: refresh the copy first with `ds survey entries read`. Each counted form says whether it was held whole or under a working-area filter.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::PROJECT, LANE],
    output: "ds.survey-dashboard/v1 report: totals, activeGrandTotal, byDay, bySurveyor, surveyorStats, activeSurveyorsByDay, byAdmin, generatedAt/By, source edge; plus `held` per form: rows, whole_form, refreshed_at.",
    examples: &[Example {
        command: "ds survey dashboard --project <exact-id> --output json",
        note: "The dashboard over what this machine holds; `ds survey local status` shows how fresh it is.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "survey progress",
        "daily entries",
        "surveyor counts",
        "survey report",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = crate::project_id(inputs.require("project")?)?;
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
            .remedy("check the layer root, then refresh the copy with `ds survey entries read`")
    };
    let root = ds_layer_store::default_root().map_err(unreadable)?;
    let scope = Scope {
        principal: identity.uid().to_owned(),
        project: project.to_owned(),
    };
    let manifests = store::manifests(&root, &scope).map_err(unreadable)?;
    if manifests.is_empty() {
        return Err(Failure::conflict(
            NOTHING_HELD.code,
            format!("this machine holds no survey copy for {project}; nothing was aggregated"),
        )
        .remedy(NOTHING_HELD.remedy)
        .next("ds survey local status"));
    }
    let mut forms = Vec::with_capacity(manifests.len());
    let mut held = serde_json::Map::new();
    for manifest in &manifests {
        let rows = store::rows(&root, &scope, manifest).map_err(unreadable)?;
        held.insert(
            manifest.form.clone(),
            json!({
                "rows": rows.len(),
                "whole_form": manifest.filter.is_whole_form(),
                "refreshed_at": manifest.refreshed_at,
            }),
        );
        forms.push(json!({"form": manifest.form, "rows": rows}));
    }
    let generated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| unreadable(error.to_string()))?;
    let request = json!({
        "forms": forms,
        // The breakdown is drawn only from rows that carry administrative
        // values, which only Rwanda projects' rows do.
        "rwanda": true,
        "generated_at": generated_at,
    });
    let report = survey_dashboard::evaluate(request.to_string().as_bytes()).map_err(|refusal| {
        Failure::invalid(REQUEST_INVALID.code, refusal.message).remedy(REQUEST_INVALID.remedy)
    })?;
    let mut report: Value = serde_json::from_str(&report)
        .map_err(|error| unreadable(format!("the dashboard report is unreadable: {error}")))?;
    report["project"] = json!(project);
    report["lane"] = json!(identity.lane());
    report["held"] = Value::Object(held);
    Ok(report)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} · survey dashboard (computed here) · {} active entries\n",
        data["project"].as_str().unwrap_or("?"),
        data["activeGrandTotal"].as_u64().unwrap_or(0),
    );
    if let Some(totals) = data["totals"].as_object() {
        for (form, counts) in totals {
            let whole = data["held"][form]["whole_form"].as_bool().unwrap_or(true);
            out.push_str(&format!(
                "  {:<36} {:>8}{}\n",
                form,
                counts["active"].as_u64().unwrap_or(0),
                if whole { "" } else { "  (working-area filter)" }
            ));
        }
    }
    out
}
