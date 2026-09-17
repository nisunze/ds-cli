//! One authenticated acquisition, with Solar-owned request and cache semantics.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "solar.reference.acquire",
    path: &["solar", "reference", "acquire"],
    contract: 2,
    summary: "Acquire verified weather and PV reference data for a headless city.",
    purpose: "Prepare a local captured city on a fresh server. The Solar owner derives the site's complete equipment request and checks its cache. A cache miss fetches one authenticated reference bundle for the selected project, verifies all four artifacts, and checks site/equipment before storing and reading it back. No provider URL or credential is accepted. This prepares inputs; it does not calculate or finalize reports.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "workspace",
            "<dir>",
            "Private Solar workspace containing captured cities.",
        )
        .required(),
        Arg::value("city", "<id>", "One exact local city.").required(),
        crate::portfolio_headless::PROJECT,
        Arg::value("cache", "<dir>", "Destination verified reference cache.").required(),
        Arg::value("lane", "<stable|canary>", "Native authentication lane.")
            .default("stable")
            .choices(&["stable", "canary"]),
    ],
    output: "Project, city, input digest, readiness, cache location and acquisition receipt. No bytes, token or signed URL.",
    examples: &[],
    refusals: crate::seed::REFERENCE_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let workspace = i.require("workspace")?;
    let city = i.require("city")?;
    let cache = i.require("cache")?;
    let mut plan = crate::project::invoke(
        json!({"operation":"reference_plan","workspace":workspace,"city":city,"cache":cache}),
    )?;
    if plan["project"] != i.require("project")? || plan["city"] != city {
        return Err(Failure::invalid(
            "solar_reference_scope_mismatch",
            "local Solar city does not belong to the explicit native project",
        )
        .remedy("pass the workspace's exact --project without changing selection"));
    }
    if plan["ready"] == true {
        plan.as_object_mut().map(|o| o.remove("request"));
        plan["acquired"] = json!(false);
        return Ok(plan);
    }
    let mut session = ds_cli_auth::solar_project_session_for_project(
        i.value("lane").unwrap_or("stable"),
        i.require("project")?,
    )?;
    let request = serde_json::from_value(plan["request"].clone()).map_err(|_| {
        Failure::failed(
            "solar_reference_contract_mismatch",
            "Solar owner returned an invalid reference request",
        )
        .remedy("install matching ds and Solar releases")
    })?;
    let bundle = session.execute(&ds_cli_auth::SolarProjectCommand::Reference { request })?;
    if let Some(error) = bundle.get("reference_error") {
        return Err(Failure::failed(
            "solar_reference_refused",
            error["message"]
                .as_str()
                .unwrap_or("Solar reference producer refused acquisition"),
        )
        .remedy("inspect the producer refusal and city equipment/site settings before retrying")
        .detail(error.clone()));
    }

    crate::project::invoke(
        json!({"operation":"reference_import","workspace":workspace,"city":city,"cache":cache,"expected":plan["input_digest"],"bundle":bundle}),
    )
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
