//! Native file host for the kernel's closed immutable exported-design import.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_client_core::transformer_history_import::{Import, Prepared, Source};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};
const PROJECT: Arg = Arg::value(
    "project",
    "<project-id>",
    "Exact project for the prepared baseline; saved selection unused.",
)
.required();
const fn cmd(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
    effect: Effect,
    authority: Authority,
    requires: Requires,
) -> Command {
    Command {
        id,
        path,
        summary,
        args,
        effect,
        authority,
        requires,
        contract: 1,
        chapter: Chapter::Design,
        execution: Execution::Sync,
        purpose: "Import a declared archival exported design as immutable v0. Six native GeoPackage layers retain original fid, source properties and dimensions; no runtime Survey identity is invented. Explicit source record, asset/version/member hashes and current transformer source fence are mandatory. Import never changes working GDFS or overwrites existing history. Source scope exported_design is read-only and cannot be promoted into runtime identity by this command. Import before runtime baseline bootstrap; conflicting v0 refuses.",
        output: "Verified archival pins, native feature counts and new prepared artifact, or exact source-pinned exported-design v0 acknowledgement. No working design save, engineering approval, PM issuance or runtime restore.",
        examples: &[],
        refusals: &[Refusal {
            code: "history_import_invalid",
            when: "Closed source/request, native geometry/fid bounds, exact file hash, project scope or server acknowledgement differs",
            remedy: "Read the exact source and pinned receipt; do not rewrite IDs, geometry, provenance or immutable history to force acceptance",
        }],
        reference: Some("docs/reference/design.md"),
        search: &["historical baseline", "April import"],
        availability: match authority {
            Authority::HeadlessProject => ds_cli_auth::native_availability,
            _ => available,
        },
    }
}
fn available() -> Availability {
    Availability::Available
}
pub static PREPARE:Command=cmd("design.version.import-prepare",&["design","version","import-prepare"],"Prepare a pinned native GeoPackage as read-only exported-design v0.",&[PROJECT,Arg::value("object","<transformer-id>","Exact existing transformer identity.").required(),Arg::value("path","<source.gpkg>","Closed original source file; never modified.").required(),Arg::value("source","<source-ref.json>","Closed Source fields record_id,asset_id,asset_version,asset_sha256,archive_member,member_sha256,historical_period.").required(),Arg::value("expected-source","<RFC3339>","Exact current transformer source revision fence.").required(),Arg::value("reason","<text>","Archival import reason.").required(),Arg::value("milestone","<text>","Optional historical milestone label."),Arg::value("idempotency-key","<key>","Stable exact-request replay key.").required(),Arg::value("out","<new.json>","New absolute prepared file; never overwrites.").required()],Effect::LocalFileWrite,Authority::None,Requires::Server);
pub static PUBLISH: Command = cmd(
    "design.version.import",
    &["design", "version", "import"],
    "Create immutable source-pinned exported-design v0 (needs --yes).",
    &[
        PROJECT,
        Arg::value(
            "file",
            "<prepared.json>",
            "Exact native prepared input; project must match --project.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
    Authority::HeadlessProject,
    Requires::Server,
);
fn bad(e: impl ToString) -> Failure {
    Failure::invalid("history_import_invalid", e.to_string())
}
fn read(path: &str, bound: u64) -> Result<Vec<u8>, Failure> {
    let metadata = std::fs::metadata(path).map_err(bad)?;
    if !metadata.is_file() || metadata.len() > bound {
        return Err(bad("source is not a bounded regular file"));
    }
    std::fs::read(path).map_err(bad)
}
pub fn prepare(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let source: Source = serde_json::from_slice(&read(i.require("source")?, 8192)?).map_err(bad)?;
    let result = ds_io::read_geopackage_layers(
        Path::new(i.require("path")?),
        &ds_command_kernel::transformer_history_import::LAYERS.map(str::to_owned),
        20000,
        64 * 1024 * 1024,
    )
    .map_err(bad)?;
    if result.source_sha256 != source.member_sha256 {
        return Err(bad(
            "original GeoPackage digest differs from pinned archive member",
        ));
    }
    let prepared = Prepared {
        schema: "ds.lv-history-import/v1".into(),
        project_id: i.require("project")?.into(),
        history_import: Import {
            transformer_id: i.require("object")?.into(),
            version_number: 0,
            expected_source_revision: i.require("expected-source")?.into(),
            reason: i.require("reason")?.into(),
            milestone: i.value("milestone").map(str::to_owned),
            idempotency_key: i.require("idempotency-key")?.into(),
            source,
            layers: result.layers,
        },
    };
    prepared.validate().map_err(bad)?;
    let bytes = serde_json::to_vec(&prepared).map_err(bad)?;
    let out = Path::new(i.require("out")?);
    if !out.is_absolute() {
        return Err(bad("output must be a new absolute path"));
    }
    let parent = out.parent().ok_or_else(|| bad("output parent absent"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(bad)?;
    file.write_all(&bytes).map_err(bad)?;
    file.as_file().sync_all().map_err(bad)?;
    file.persist_noclobber(out).map_err(bad)?;
    Ok(
        json!({"project":prepared.project_id,"transformer":prepared.history_import.transformer_id,"source_modified":false,"published":false,"snapshot_scope":"exported_design","feature_counts":result.feature_counts,"source_sha256":result.source_sha256,"artifact":{"path":out,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}}),
    )
}
pub fn publish(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let prepared: Prepared =
        serde_json::from_slice(&read(i.require("file")?, 17 * 1024 * 1024)?).map_err(bad)?;
    prepared.validate().map_err(bad)?;
    if prepared.project_id != i.require("project")? {
        return Err(bad("prepared project differs from explicit --project"));
    }
    ds_cli_auth::transformer_history_import_for_project(
        i.require("lane")?,
        &prepared.project_id,
        &prepared.history_import,
    )
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
