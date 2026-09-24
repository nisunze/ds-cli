//! Exact print-reference removal through the same native owner as publication.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;

pub static REMOVE: Command = Command {
    id: "report.artifact.remove",
    path: &["report", "artifact", "remove"],
    contract: 1,
    summary: "Remove one exact published MV, custom or transformer print reference.",
    purpose: "Remove a duplicate or unwanted print from its project report list. Use the exact filename, gs:// locator and SHA-256 from the current print receipt; a replacement is refused. Requires design.delete_combined and an active selected project. Only the print reference is removed: stored bytes, network models and other report formats are retained. Repeat after an ambiguous result is safe. Standalone custom-map Assets instead use assets.classify with status archive; combined report versions use their report-version lifecycle.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        super::project::LANE_ARG,
        Arg::value(
            "scope",
            "<mv|combined|transformer>",
            "Published report slot.",
        )
        .choices(&["mv", "combined", "transformer"])
        .required(),
        Arg::value(
            "transformer",
            "<name>",
            "Exact target: mv_data, combined_transformer, or an individual name.",
        )
        .required(),
        Arg::value(
            "filename",
            "<name>",
            "Exact published filename; never a path.",
        )
        .required(),
        Arg::value(
            "gcs-path",
            "<gs://…>",
            "Exact registered print locator; not an arbitrary delete destination.",
        )
        .required(),
        Arg::value(
            "sha256",
            "<digest>",
            "Exact lowercase SHA-256 from the print receipt.",
        )
        .required(),
    ],
    output: "Project, scope, transformer, file_name, sha256, gcs_path, removed reference count and bytes_deleted=false. Zero is an idempotent already-absent result.",
    examples: &[],
    refusals: super::project::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["remove print", "delete map", "published print"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn remove(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let command = ds_cli_auth::RemoveReportArtifactCommand {
        scope: i.require("scope")?.into(),
        transformer: i.require("transformer")?.into(),
        file_name: i.require("filename")?.into(),
        gcs_path: i.require("gcs-path")?.into(),
        sha256: i.require("sha256")?.into(),
    };
    Ok(ds_cli_auth::remove_report_artifact(i.require("lane")?, &command)?.into_result())
}
