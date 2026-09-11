//! Where this project's design data lives — Firestore, or the mirrored
//! combined store — read natively from the project's own Settings.
//!
//! One client answer. The browser used to derive it from four parameter
//! spellings in TypeScript and `ds` could not answer it at all; both now ask
//! `ds_command_kernel::design_config`. ds-brain's Go twin is NOT this decision
//! restated: it is the grant-side fence, evaluated where the caller cannot
//! reach, and it stays.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::ProjectConfigurationChange as Change;
use ds_command_kernel::design_config::{self, Request};
use serde_json::Value;

const LANE: Arg = Arg::value("lane", "<stable|canary>", "Deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);

const REFUSALS: &[Refusal] = &[
    ds_cli_report::project::NATIVE_PROFILE,
    ds_cli_report::project::NATIVE_PROFILE_DIGEST,
    ds_cli_report::project::NATIVE_PROFILE_UNSAFE,
    ds_cli_report::project::HEADLESS_SIGNED_OUT,
    ds_cli_report::project::HEADLESS_NO_PROJECT,
    ds_cli_report::project::PROJECT_CONTEXT_STALE,
    ds_cli_report::project::NATIVE_STATE_UNSAFE,
    ds_cli_report::project::NATIVE_STATE_UNAVAILABLE,
    ds_cli_report::project::NATIVE_STATE_PROTECTION,
    ds_cli_report::project::NATIVE_STATE_ROOT,
    ds_cli_report::project::NATIVE_STATE_CONFLICT,
    ds_cli_report::project::NATIVE_CLEANUP,
    Refusal {
        code: "design_data_lane_unreadable",
        when: "The project's Settings document has no readable shape",
        remedy: "Read the project's Settings with `ds design config sheets` and repair the project_settings sheet",
    },
];

pub static COMMAND: Command = Command {
    id: "design.data.lane",
    path: &["design", "data", "lane"],
    contract: 1,
    summary: "Where this project's design data is read from.",
    purpose: "Reads the selected project's fresh Settings through the native user client and asks the shared kernel which design-data path the project declares: Firestore, or the mirrored combined store. The explicit parameter wins; the legacy spellings answer only when it is absent; an undeclared project is Firestore. The parameter that answered and why are named, so a surprising lane is traceable to the row that set it.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE],
    output: "{project, path: firestore|mirrored, source: explicit|legacy|default, parameter, reason_key}.",
    examples: &[Example {
        command: "ds design data lane --output json",
        note: "`.data.path` decides which store a combined read should trust.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let current = ds_cli_auth::settings_configuration(i.require("lane")?, Change::ReadSettings)?;
    let mut result = design_config::apply(Request::DesignDataPath {
        document: current.document,
    })
    .map_err(|e| {
        Failure::invalid("design_data_lane_unreadable", e.to_string())
            .remedy("Repair the project_settings sheet with `ds design config`")
    })?;
    result["project"] = current.summary["project"].clone();
    Ok(result)
}

pub fn render(data: &Value) -> String {
    format!(
        "  {:<18} {}\n  {:<18} {}\n  {:<18} {}\n  {:<18} {}\n",
        "path",
        data["path"].as_str().unwrap_or("?"),
        "declared by",
        data["source"].as_str().unwrap_or("?"),
        "parameter",
        data["parameter"].as_str().unwrap_or(""),
        "why",
        data["reason_key"].as_str().unwrap_or("")
    )
}
