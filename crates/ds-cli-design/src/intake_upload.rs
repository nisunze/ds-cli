//! Headless design intake through the shared Rust upload state machine.
use ds_cli_auth::StatusUploadDomain;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Map, Value};
use std::io::Read;

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
        code: "status_upload_invalid",
        when: "a source file or settings document is absent, changes during admission, or exceeds a bound",
        remedy: "pass bounded regular files and a JSON object of scalar process settings",
    },
];

pub static COMMAND: Command = Command {
    id: "design.intake.upload",
    path: &["design", "intake", "upload"],
    contract: 1,
    summary: "Upload and process LV design files without Desktop.",
    purpose: "Names its project with --project (the saved selection is never read), admits every file through the shared Rust upload state machine, obtains project-scoped resumable targets, uploads local bytes, then submits independent one-file processing jobs. Upload admission, phase order, process request shape, result matching, failures and progress are the same kernel used by the browser; this native adapter supplies filesystem and fixed network effects only.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        Arg::repeated(
            "file",
            "<path>",
            "Local .gpkg, .zip, or .geojson source; repeat for multiple independent files.",
        )
        .required(),
        Arg::value(
            "mode",
            "<lv-drafting|sketch-lv|lv-process>",
            "Processing operation.",
        )
        .required()
        .choices(&["lv-drafting", "sketch-lv", "lv-process"]),
        Arg::value(
            "settings",
            "<settings.json>",
            "Optional scalar JSON settings object, used only by lv-process.",
        ),
        Arg::value("lane", "<stable|canary>", "Native user lane.")
            .default("stable")
            .choices(&["stable", "canary"]),
    ],
    output: "Frozen project/lane, terminal upload phase, aggregate progress, and one success or error result per source file.",
    examples: &[Example {
        command: "ds design intake upload --project <id> --file ./T001.zip --mode lv-process --settings ./process-settings.json --yes --output json",
        note: "Runs the complete intake on the named project with no open map or Desktop pairing.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("status_upload_invalid", message)
        .remedy("Pass bounded regular files and a JSON object of scalar process settings")
}

fn read_settings(path: Option<&str>) -> Result<Map<String, Value>, Failure> {
    let Some(path) = path else {
        return Ok(Map::new());
    };
    let file = std::fs::File::open(path)
        .map_err(|error| invalid(format!("Cannot open settings file {path}: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| invalid(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(invalid(
            "Settings must be a regular JSON file no larger than 1 MiB.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(error.to_string()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("Settings are not valid JSON: {error}")))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("Settings JSON must be an object."))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mode = match inputs.require("mode")? {
        "lv-drafting" => StatusUploadDomain::LvDrafting,
        "sketch-lv" => StatusUploadDomain::SketchLv,
        "lv-process" => StatusUploadDomain::LvProcess,
        _ => return Err(invalid("Unknown processing mode.")),
    };
    let settings = read_settings(inputs.value("settings"))?;
    ds_cli_auth::status_upload(
        inputs.require("lane")?,
        inputs.require("project")?,
        inputs.repeated("file"),
        mode,
        &settings,
    )
}

pub fn render(data: &Value) -> String {
    let results = data["results"].as_array().map_or(0, Vec::len);
    let succeeded = data["results"].as_array().map_or(0, |rows| {
        rows.iter().filter(|row| row["ok"] == true).count()
    });
    format!(
        "  {:<14} {}\n  {:<14} {}\n  {:<14} {}\n  {:<14} {}/{}\n",
        "project",
        data["project_name"]
            .as_str()
            .unwrap_or_else(|| data["project"].as_str().unwrap_or("?")),
        "lane",
        data["lane"].as_str().unwrap_or("?"),
        "phase",
        data["phase"].as_str().unwrap_or("?"),
        "succeeded",
        succeeded,
        results,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_is_headless_project_and_global_write() {
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::GlobalWrite);
        assert!(!COMMAND.authority.requires_desktop());
    }

    #[test]
    fn settings_reject_non_object() {
        let path =
            std::env::temp_dir().join(format!("ds-upload-settings-{}.json", std::process::id()));
        std::fs::write(&path, b"[]").unwrap();
        let result = read_settings(path.to_str());
        let _ = std::fs::remove_file(path);
        assert!(result.is_err());
    }
}
