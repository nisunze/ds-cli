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
    "Requested folder level; district/sector folders need resolved administrative values.",
)
.default("transformer")
.choices(&["transformer", "sector", "district", "root"]);
const COMBINE_PER_GROUP_ARG: Arg = Arg::switch(
    "combine-per-group",
    "Also file one combined set for each first-level applied report group.",
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
service for one compounded archive over its audience-fenced selected \
project: it resolves the scope, composes the combined sets and publishes one \
ZIP with a registry row. District and sector folders come from the project's \
applied `report_archive` grouping, not from this request. Retired \
transformers are never in scope. Blocks until the service answers (up to ten \
minutes). No project, URL, body or action override is accepted.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        FILE_LEVEL_ARG,
        COMBINE_PER_GROUP_ARG,
        FORCE_ARG,
        LANE_ARG,
    ],
    output: "\
Lane and selected-project identity/status, the requested scope, and \
`archive_layout` — the layout asked for, never the tree achieved: \
unresolved administrative values collapse to `_unassigned/`, and `ds report \
project archives` confirms the tree built. Then the receipt: status, \
`prefix`, cloud locators, individual coverage, missing individuals with \
causes, bounded errors and registry-write failure.",
    examples: &[Example {
        command: "ds report project compounded --file-level sector --yes --output json",
        note: "`ds report project archives` then confirms the foldering built.",
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
    let combine_per_group = inputs.switch("combine-per-group");
    let force = inputs.switch("force");
    let request = CompoundedReportRequest::new(transformers, file_level, combine_per_group, force);
    let headless = ds_cli_auth::compounded_report(inputs.require("lane")?, &request)?;
    let receipt = headless.result();
    let mut output = super::project_receipt(&headless);
    let fields = json!({
        "scope": {
            "mode": if request.transformers().is_empty() { "all_active" } else { "explicit" },
            "requested": request.transformers().names(),
        },
        "archive_layout": archive_layout(file_level.token(), combine_per_group),
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
            .map(|cause| json!({
                "transformer": cause.transformer(),
                "code": cause.code(),
                "detail": cause.detail(),
            }))
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

/// The requested layout, in the report layer's own words.
///
/// `combine_per_group` is the current name for the choice the registry has
/// always recorded as `combine_per_district`; both are reported so a caller
/// reading a fresh receipt and one reading an old registry row see the same
/// archive described the same way.
fn archive_layout(file_level: &str, combine_per_group: bool) -> Value {
    let mut layout = json!({
        "file_level": file_level,
        "combine_per_group": combine_per_group,
    });
    let vocabulary = super::archive_layout_vocabulary(Some(file_level), None, combine_per_group);
    layout.as_object_mut().expect("layout is an object").extend(
        vocabulary
            .as_object()
            .expect("vocabulary is an object")
            .clone(),
    );
    layout
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
                cause["transformer"].as_str().unwrap_or("?"),
                cause["detail"]
                    .as_str()
                    .or_else(|| cause["code"].as_str())
                    .unwrap_or("?"),
            ));
        }
    }
    if data["registry_write_failed"].as_bool().unwrap_or(false) {
        out.push_str("  registry row not written; future runs will not see this archive\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `archive_layout` key is pinned by the contract, so the honesty has
    /// to live in the prose beside it: the receipt reports what was asked
    /// for, and only the registry says what was built.
    #[test]
    fn the_descriptor_calls_archive_layout_a_request_not_a_tree() {
        assert!(
            COMMAND
                .output
                .contains("the layout asked for, never the tree achieved"),
            "{}",
            COMMAND.output
        );
        assert!(
            COMMAND.output.contains("`_unassigned/`"),
            "{}",
            COMMAND.output
        );
        assert!(
            COMMAND.output.contains("ds report project archives"),
            "{}",
            COMMAND.output
        );
        assert!(
            COMMAND
                .purpose
                .contains("applied `report_archive` grouping"),
            "{}",
            COMMAND.purpose
        );
        assert!(
            FILE_LEVEL_ARG.summary.starts_with("Requested folder level"),
            "{}",
            FILE_LEVEL_ARG.summary
        );
    }
}
