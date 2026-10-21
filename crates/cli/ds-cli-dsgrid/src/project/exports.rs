//! `ds dsgrid project exports list|publish|download` — immutable files
//! recorded against one exact project model revision.
//!
//! An export is a derived file — the PLS-CADD `.bak` delivered for a
//! submission, a GIS layer — pinned to the revision's model digest. Publishing
//! one never moves the head and never changes the revision; an export id, once
//! written, cannot be rewritten. Bytes are uploaded and read back through the
//! native owner, and every read is verified against the digest its record
//! declares.
use super::{LANE, LOCAL, PROJECT, SHARED, with_shared};
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;

const FILE_INVALID: Refusal = Refusal {
    code: "export_file_invalid",
    when: "the export file is missing, unreadable, empty or larger than 256 MiB",
    remedy: "name an existing file of 1 byte to 256 MiB",
};
const OUT_INVALID: Refusal = Refusal {
    code: "grid_project_output_invalid",
    when: "the destination exists or cannot be written",
    remedy: "name a fresh local path; an existing file is never overwritten",
};
const ID_INVALID: Refusal = Refusal {
    code: "export_id_invalid",
    when: "an export, output or format id is outside ^[a-z0-9][a-z0-9_-]{1,127}$",
    remedy: "use lowercase letters, digits, - and _ (2 to 128 characters)",
};
const LIST_REFUSALS: &[Refusal] = &with_shared([LOCAL; 1 + SHARED], 1);
const PUBLISH_REFUSALS: &[Refusal] = &{
    let mut r = with_shared([FILE_INVALID; 2 + SHARED], 2);
    r[1] = ID_INVALID;
    r
};
const DOWNLOAD_REFUSALS: &[Refusal] = &{
    let mut r = with_shared([OUT_INVALID; 2 + SHARED], 2);
    r[1] = ID_INVALID;
    r
};
const MODEL: Arg = Arg::value("model", "<id>", "Exact project model ID.").required();
const REVISION: Arg = Arg::value(
    "revision",
    "<id>",
    "Exact revision the export belongs to (dsgrid project versions).",
)
.required();

pub static LIST: Command = Command {
    id: "dsgrid.project.exports.list",
    path: &["dsgrid", "project", "exports", "list"],
    contract: 1,
    summary: "List the immutable exports recorded on one model revision.",
    purpose: "Answer which delivered files a revision carries — a submitted PLS-CADD .bak, a GIS layer — each pinned to the revision's model digest, with its format, byte length and SHA-256, who recorded it and when. Newest first. Read one with dsgrid project exports download.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        REVISION,
        Arg::value("limit", "<1..500>", "Maximum export records returned.").default("50"),
    ],
    output: "exports[] {export_id, model_digest, items[] {output_id, scope, format, artifact {digest, byte_length}}, created_at, created_by}, total, more. No signed locator.",
    examples: &[],
    refusals: LIST_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["delivered bak", "revision exports", "submission file"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static PUBLISH: Command = Command {
    id: "dsgrid.project.exports.publish",
    path: &["dsgrid", "project", "exports", "publish"],
    contract: 1,
    summary: "Record one file as an immutable export of a revision (needs --yes).",
    purpose: "Upload one delivered file — typically the PLS-CADD .bak of a submission — and record it against one exact revision, pinned to the model digest the catalog holds for it. The head does not move and the revision does not change. The export id is immutable: re-running with the same file is idempotent, a different file under the same id is refused. Omit --export to derive a stable id from the output, format and file digest.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        REVISION,
        Arg::value("file", "<path>", "The file to record (1 byte to 256 MiB).").required(),
        Arg::value(
            "output-id",
            "<id>",
            "Output name inside the export, e.g. pls-delivered-workspace.",
        )
        .required(),
        Arg::value("format", "<id>", "Format id, e.g. pls_cadd_bak.").required(),
        Arg::value(
            "export",
            "<id>",
            "Export id; default derived from output, format and file digest.",
        ),
    ],
    output: "status published, model/revision/version, model_digest the export is pinned to, export_id, output_id, format, digest, byte_length, upload_skipped, head_moved=false, verified.",
    examples: &[Example {
        command: "ds dsgrid project exports publish --project <p> --model <m> --revision <rev> --file /work/delivered.bak --output-id pls-delivered-workspace --format pls_cadd_bak --yes",
        note: "Record the delivered .bak against the submitted revision.",
        runnable: false,
    }],
    refusals: PUBLISH_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["attach bak", "deliver bak", "submission file"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static DOWNLOAD: Command = Command {
    id: "dsgrid.project.exports.download",
    path: &["dsgrid", "project", "exports", "download"],
    contract: 1,
    summary: "Download one export file of a revision, digest-verified.",
    purpose: "Read one export output's exact bytes — the .bak delivered with a submission — verify SHA-256 and byte length against its immutable record, and write a new local file. Needs a Server with get_export_artifact; an older one is refused as server_action_unsupported.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        REVISION,
        Arg::value(
            "export",
            "<id>",
            "Export id from dsgrid project exports list.",
        )
        .required(),
        Arg::value("output-id", "<id>", "Output id inside that export.").required(),
        Arg::value("out", "<path>", "Fresh local file path.").required(),
    ],
    output: "model/revision, export_id, output_id, format, verified sha256, byte count and the local path. No signed locator.",
    examples: &[],
    refusals: DOWNLOAD_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["download bak", "delivered bak"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn catalog_id(flag: &str, value: &str) -> Result<(), Failure> {
    if ds_command_kernel::grid_publication::catalog_identifier(value) {
        Ok(())
    } else {
        Err(Failure::invalid(
            "export_id_invalid",
            format!("--{flag} `{value}` is invalid"),
        )
        .remedy(ID_INVALID.remedy))
    }
}

pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i
        .require("limit")?
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=500).contains(n))
        .ok_or_else(|| {
            Failure::invalid("grid_project_output_invalid", "--limit must be 1..500")
                .remedy(LOCAL.remedy)
        })?;
    let mut data = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::ListExports {
            model: i.require("model")?.into(),
            revision: i.require("revision")?.into(),
        },
    )?
    .data;
    let exports = data["exports"].as_array().cloned().unwrap_or_default();
    let total = exports.len();
    let (kept, withheld) = crate::package::take(exports, limit);
    data["exports"] = json!(kept);
    data["total"] = json!(total);
    data["more"] = json!(withheld > 0);
    Ok(data)
}

/// The derived export id: stable for one output, format and file, so a retry
/// of the same publication lands on the same immutable record.
fn default_export(output: &str, format: &str, digest: &str) -> String {
    let key = format!("{output}\n{format}\n{digest}");
    format!("x-{}", &format!("{:x}", Sha256::digest(key))[..24])
}

pub fn publish(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let output = i.require("output-id")?;
    let format = i.require("format")?;
    catalog_id("output-id", output)?;
    catalog_id("format", format)?;
    let path = i.require("file")?;
    let bytes = read_export(path)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let export = match i.value("export") {
        Some(export) => {
            catalog_id("export", export)?;
            export.to_owned()
        }
        None => default_export(output, format, &digest),
    };
    let mut data = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::PublishExport {
            model: i.require("model")?.into(),
            revision: i.require("revision")?.into(),
            export,
            output: output.into(),
            format: format.into(),
            bytes,
        },
    )?
    .data;
    data["file"] = json!(path);
    Ok(data)
}

/// The native owner's export bound.
pub const MAX_EXPORT_BYTES: u64 = 256 * 1024 * 1024;

/// Read one export file, sized before it is read.
pub(crate) fn read_export(path: &str) -> Result<Vec<u8>, Failure> {
    let refuse = |why: String| {
        Failure::invalid("export_file_invalid", format!("{path}: {why}"))
            .remedy(FILE_INVALID.remedy)
    };
    let length = std::fs::metadata(path)
        .map_err(|e| refuse(e.to_string()))?
        .len();
    if length == 0 || length > MAX_EXPORT_BYTES {
        return Err(refuse(format!("holds {length} bytes")));
    }
    std::fs::read(path).map_err(|e| refuse(e.to_string()))
}

pub fn download(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let export = i.require("export")?;
    let output = i.require("output-id")?;
    catalog_id("export", export)?;
    catalog_id("output-id", output)?;
    let out = std::path::Path::new(i.require("out")?);
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(out_failure("destination already exists"));
    }
    let mut receipt = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::DownloadExport {
            model: i.require("model")?.into(),
            revision: i.require("revision")?.into(),
            export: export.into(),
            output: output.into(),
        },
    )?;
    let bytes = receipt
        .bytes
        .take()
        .ok_or_else(|| out_failure("verified owner returned no bytes"))?;
    write_new(out, &bytes)?;
    receipt.data["out"] = json!(out);
    Ok(receipt.data)
}

fn out_failure(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("grid_project_output_invalid", e.to_string()).remedy(OUT_INVALID.remedy)
}

/// Stage beside the destination, sync, then persist without clobbering.
pub(crate) fn write_new(out: &std::path::Path, bytes: &[u8]) -> Result<(), Failure> {
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(out_failure)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(out_failure)?;
    staged.write_all(bytes).map_err(out_failure)?;
    staged.as_file().sync_all().map_err(out_failure)?;
    staged.persist_noclobber(out).map_err(out_failure)?;
    Ok(())
}

pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_derived_export_id_is_stable_and_in_the_catalog_grammar() {
        let a = default_export("pls-delivered-workspace", "pls_cadd_bak", &"a".repeat(64));
        assert_eq!(
            a,
            default_export("pls-delivered-workspace", "pls_cadd_bak", &"a".repeat(64))
        );
        assert_ne!(
            a,
            default_export("pls-delivered-workspace", "pls_cadd_bak", &"b".repeat(64))
        );
        assert!(ds_command_kernel::grid_publication::catalog_identifier(&a));
    }
}
