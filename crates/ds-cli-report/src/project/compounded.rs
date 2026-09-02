//! `ds report project compounded` — publish one compounded archive in the
//! background against the CLI-selected project.

use ds_cli_auth::{CompoundedReportRequest, ReportFileLevel};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::{LANE_ARG, TRANSFORMER_ARG};

const FILE_LEVEL_ARG: Arg = Arg::value(
    "file-level",
    "<transformer|sector|district|root>",
    "Folder level for individual transformer artifacts inside the archive.",
)
.default("transformer")
.choices(&["transformer", "sector", "district", "root"]);
const COMBINE_PER_DISTRICT_ARG: Arg = Arg::switch(
    "combine-per-district",
    "Also file one district-scoped combined set under each district folder.",
);
const FORCE_ARG: Arg = Arg::switch(
    "force",
    "Regenerate every individual artifact instead of reusing fresh ones.",
);

pub static COMMAND: Command = Command {
    id: "report.project.compounded",
    path: &["report", "project", "compounded"],
    contract: 1,
    summary: "Publish one compounded report archive in the background (needs --yes).",
    purpose: "\
After CLI confirmation, restores the native user and asks the governed report \
service to produce one compounded archive for only its audience-fenced \
selected project. The service resolves the exact scope (every active saved \
transformer, or the names given), reuses fresh individual artifacts, \
regenerates the rest, composes the combined sets, and publishes one ZIP with \
a registry row. Retired transformers are never in scope. Blocks until the \
service answers (up to ten minutes). No map, Desktop descriptor, project, \
URL, body or action override is accepted.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        FILE_LEVEL_ARG,
        COMBINE_PER_DISTRICT_ARG,
        FORCE_ARG,
        LANE_ARG,
    ],
    output: "\
Lane and selected-project identity/status, the requested scope and layout, \
then the receipt: `status` (success or partial), archive `prefix`, cloud \
locators, individual coverage, missing individuals with typed causes, bounded \
errors, and registry-write failure.",
    examples: &[Example {
        command: "ds report project compounded --file-level sector --combine-per-district --yes --output json",
        note: "Read `.data.prefix` and `.data.archives`; `ds report project archives` lists the signed download afterwards.",
        runnable: false,
    }],
    refusals: super::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = super::transformer_set(inputs)?;
    let file_level = ReportFileLevel::parse(inputs.require("file-level")?)
        .expect("the command parser enforces the file-level choices");
    let combine_per_district = inputs.switch("combine-per-district");
    let force = inputs.switch("force");
    let request =
        CompoundedReportRequest::new(transformers, file_level, combine_per_district, force);
    let headless = ds_cli_auth::compounded_report(inputs.require("lane")?, &request)?;
    let receipt = headless.result();
    let mut output = super::project_receipt(&headless);
    let fields = json!({
        "scope": {
            "mode": if request.transformers().is_empty() { "all_active" } else { "explicit" },
            "requested": request.transformers().names(),
        },
        "archive_layout": {
            "file_level": file_level.token(),
            "combine_per_district": combine_per_district,
        },
        "force": force,
        "status": receipt.status().token(),
        "prefix": receipt.prefix(),
        "archives": receipt.archive_paths(),
        "cached": receipt.cached(),
        "individual_artifact_transformer_count": receipt.individual_artifact_transformer_count(),
        "missing_individual_artifact_count": receipt.missing_individual_artifact_count(),
        "missing_individual_artifacts": receipt.missing_individual_artifacts(),
        "missing_individual_artifact_causes": receipt
            .missing_individual_artifact_causes()
            .iter()
            .map(|(name, cause)| json!({"name": name, "cause": cause}))
            .collect::<Vec<_>>(),
        "errors": receipt.errors(),
        "registry_write_failed": receipt.registry_write_failed(),
        "registry_write_error": receipt.registry_write_error(),
    });
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} · archive {} · {} individual artifact(s), {} missing\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["status"].as_str().unwrap_or("?"),
        data["prefix"].as_str().unwrap_or("?"),
        data["individual_artifact_transformer_count"]
            .as_u64()
            .unwrap_or(0),
        data["missing_individual_artifact_count"]
            .as_u64()
            .unwrap_or(0),
    );
    if let Some(archives) = data["archives"].as_array() {
        for archive in archives {
            out.push_str(&format!("  {}\n", archive.as_str().unwrap_or("?")));
        }
    }
    if let Some(causes) = data["missing_individual_artifact_causes"].as_array() {
        for cause in causes {
            out.push_str(&format!(
                "  missing {:<28} {}\n",
                cause["name"].as_str().unwrap_or("?"),
                cause["cause"].as_str().unwrap_or("?"),
            ));
        }
    }
    if data["registry_write_failed"].as_bool().unwrap_or(false) {
        out.push_str("  registry row not written; future runs will not see this archive\n");
    }
    out
}
