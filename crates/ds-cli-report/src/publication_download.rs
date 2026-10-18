//! Exact, authorized immutable artifact retrieval. No render or queue owner.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "report.publication.download",
    path: &["report", "publication", "download"],
    contract: 1,
    summary: "Download one exact published report output and verify its bytes.",
    purpose: "Reauthorizes the named project and binds the selected immutable head revision and output SHA/size. Downloads through the existing native verified-byte owner. A moved head, changed account, bad ticket or corrupt bytes refuses; no recalculation, queue, fallback or substituted output. Writes a fresh private destination only after verification.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::project::PROJECT_ARG,
        crate::project::LANE_ARG,
        Arg::value("engine", "<engine>", "Exact published engine.")
            .required()
            .choices(&["network_reporter", "solar", "print", "vector_tiler"]),
        Arg::value("operation", "<operation>", "Exact published operation.").required(),
        Arg::value("variant", "<variant>", "Exact published variant.").required(),
        Arg::value("output-id", "<id>", "Exact published output id.").required(),
        Arg::value(
            "head-revision",
            "<positive-integer>",
            "Immutable revision selected from inventory.",
        )
        .required(),
        Arg::value("sha256", "<hex>", "Selected output SHA-256.").required(),
        Arg::value(
            "size-bytes",
            "<positive-integer>",
            "Selected output byte count.",
        )
        .required(),
        Arg::value(
            "out",
            "<absolute-path>",
            "Fresh private output path in an existing directory.",
        )
        .required(),
    ],
    output: "ds.report-download/v1: project, lane, head_revision, output_id, path, sha256, size_bytes, filename, content_type.",
    examples: &[Example {
        command: "ds report publication download --project <id> --engine network_reporter --operation export-<key> --variant default --output-id xlsx --head-revision <revision> --sha256 <sha256> --size-bytes <bytes> --out <absolute-file> --output json",
        note: "Pins the output already selected; never substitutes a newer head.",
        runnable: false,
    }],
    refusals: crate::publication::SHOW.refusals,
    reference: Some("docs/reference/report.md"),
    search: &["artifact", "download", "immutable", "sha256"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("publication_selector_invalid", e.to_string())
}
fn failed(e: impl std::fmt::Display) -> Failure {
    Failure::failed("publication_unreadable", e.to_string())
}
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = i.require("project")?;
    let lane = i.require("lane")?;
    let engine = i.require("engine")?;
    let operation = i.require("operation")?;
    let variant = i.require("variant")?;
    let output_id = i.require("output-id")?;
    let sha = i.require("sha256")?;
    let revision = i
        .require("head-revision")?
        .parse::<u64>()
        .map_err(invalid)?;
    let size = i.require("size-bytes")?.parse::<u64>().map_err(invalid)?;
    let out = Path::new(i.require("out")?);
    if !ds_command_kernel::execution_context::valid_project(project)
        || !ds_command_kernel::compute_artifact_inventory::valid_operation(engine, operation)
        || !identifier(variant)
        || !identifier(output_id)
        || !ds_command_kernel::report_export::exact_lower_sha256(sha)
        || revision == 0
        || size == 0
        || !out.is_absolute()
        || out.exists()
        || !out.parent().is_some_and(Path::is_dir)
    {
        return Err(invalid(
            "Use exact publication pins and a fresh absolute destination.",
        ));
    }
    let before = ds_cli_auth::headless_principal_read_only(lane)?;
    let stage = tempfile::Builder::new()
        .prefix(".ds-report-download-")
        .tempdir_in(out.parent().unwrap())
        .map_err(failed)?;
    let path = stage.path().join("output");
    let head = ds_cli_auth::publication_output_for_project(
        lane,
        project,
        &ds_cli_auth::PublicationOutputSelection {
            engine,
            operation,
            variant,
            output_id,
            head_revision: revision,
            sha256: sha,
            size_bytes: size,
        },
        &path,
    )?;
    let output = head
        .outputs
        .iter()
        .find(|o| o.output_id == output_id)
        .ok_or_else(|| failed("Selected output is absent."))?;
    crate::inventory_copy::copy_verified(
        std::fs::File::open(&path).map_err(failed)?,
        out,
        sha,
        size,
        &|| {
            let after = ds_cli_auth::headless_principal_read_only(lane)?;
            if before.account_uid() != after.account_uid()
                || before.deployment() != after.deployment()
                || before.install_id() != after.install_id()
            {
                return Err(failed("Account changed before output delivery."));
            }
            Ok(())
        },
    )
    .map_err(|error| failed(error.message()))?;
    Ok(
        json!({"schema":"ds.report-download/v1","project":project,"lane":lane,"head_revision":revision,"output_id":output_id,"path":out,"sha256":sha,"size_bytes":size,"filename":ds_command_kernel::compute_artifact_inventory::filename(engine,operation,output),"content_type":output.content_type}),
    )
}

fn identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
