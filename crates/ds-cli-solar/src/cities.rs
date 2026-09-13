//! Native project Solar city discovery.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;
pub static COMMAND: Command = Command {
    id: "solar.cities",
    path: &["solar", "cities"],
    contract: 1,
    summary: "List the selected project's governed Solar cities headlessly.",
    purpose: "Read existing city ids and display names through the native selected project's Solar service. These exact ids feed capture and preparation. Local workspace city discovery remains solar.project.status.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[Arg::value(
        "lane",
        "<stable|canary>",
        "Native lane; defaults to stable.",
    )],
    output: "Project id, templates with template_id/display_name and metadata, and count. No city input body.",
    examples: &[],
    refusals: ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    reference: Some("docs/reference/solar.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::solar_project_session(i.value("lane").unwrap_or("stable"))?
        .execute(&ds_cli_auth::SolarProjectCommand::ListCities)
}

pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
