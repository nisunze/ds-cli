//! Thin offline transport for the exchange owner's archived-transformer preview.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::io::Write;
pub static COMMAND: Command = Command {
    id: "dsgrid.backup.preview",
    path: &["dsgrid", "backup", "preview"],
    contract: 1,
    summary: "Decode an archived transformer into a read-only map preview.",
    purpose: "Verifies native snapshot integrity and origin, or decodes legacy JSON without claiming verified origin. Saves geometry and retained document evidence to a new local JSON. No current project, design edit, restore or publication is involved.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "path",
            "<file>",
            "Downloaded .dsgrid or legacy .firestore.json; at most 64 MiB.",
        )
        .required(),
        Arg::value(
            "format",
            "<dsgrid|json>",
            "Exact archive format from the ledger.",
        )
        .choices(&["dsgrid", "json"])
        .required(),
        Arg::value(
            "project",
            "<id>",
            "Origin project recorded by the deletion event.",
        )
        .required(),
        Arg::value(
            "transformer",
            "<name>",
            "Origin transformer recorded by the deletion event.",
        )
        .required(),
        Arg::value("out", "<file>", "New preview JSON file; never overwritten.").required(),
    ],
    output: "New file, origin verification, feature/layer counts and bounds; persisted_to_project=false. Full geometry stays in --out.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "backup_preview_invalid",
            when: "input is unreadable, oversized, corrupt or disagrees with the recorded origin",
            remedy: "download the exact event archive and use its project, transformer and format",
        },
        Refusal {
            code: "backup_preview_output",
            when: "the output exists or cannot be written",
            remedy: "choose a new writable output path",
        },
    ],
    reference: None,
    availability: || Availability::Available,
};
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let invalid = |e: String| {
        Failure::invalid("backup_preview_invalid", e)
            .remedy("use the exact event archive and recorded origin")
    };
    let path = inputs.require("path")?;
    let meta = std::fs::metadata(path).map_err(|e| invalid(e.to_string()))?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err(invalid("Input exceeds 64 MiB".into()));
    }
    let bytes = std::fs::read(path).map_err(|e| invalid(e.to_string()))?;
    let preview = ds_grid_exchange::backup_preview::evaluate(
        ds_grid_exchange::backup_preview::Request {
            format: inputs.require("format")?.into(),
            project: Some(inputs.require("project")?.into()),
            transformer: Some(inputs.require("transformer")?.into()),
        },
        &bytes,
    )
    .map_err(invalid)?;
    let out = inputs.require("out")?;
    let encoded = serde_json::to_vec(&preview)
        .map_err(|e| Failure::invalid("backup_preview_invalid", e.to_string()))?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| {
            Failure::invalid("backup_preview_output", e.to_string())
                .remedy("choose a new writable output path")
        })?;
    if let Err(error) = file.write_all(&encoded).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(out);
        return Err(Failure::invalid("backup_preview_output", error.to_string())
            .remedy("choose a writable output path"));
    }
    Ok(
        json!({"file":out,"project":preview.project,"transformer":preview.transformer,"origin_verified":preview.origin_verified,"feature_count":preview.feature_count,"layer_count":preview.geometry.layers.len(),"bounds":preview.bounds,"persisted_to_project":false}),
    )
}
pub fn render(value: &Value) -> String {
    format!("{}\n", value)
}
