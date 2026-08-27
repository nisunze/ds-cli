use crate::engine_failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::open_release;
use ds_grid_model::EntityId;
use serde_json::{Value, json};
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "library.open",
    path: &["library", "open"],
    contract: 1,
    summary: "Open one exact verified release from a local library store.",
    purpose: "Reads artifact id plus immutable version from the verified local store. Missing versions refuse; there is no latest-release fallback.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("store", "<dir>", "Local verified release store.").required(),
        Arg::value("library-id", "<id>", "Exact library id.").required(),
        Arg::value(
            "library-version",
            "<id>",
            "Exact immutable library version.",
        )
        .required(),
    ],
    output: "Verified manifest identity, content root, tables and asset count.",
    examples: &[Example {
        command: "ds library open --store ./cache --library-id standards --library-version v1 --output json",
        note: "Open an exact offline release.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "library_open_failed",
        when: "the exact release is absent or fails verification",
        remedy: "install that exact id/version into the store, then retry",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let store = inputs.require("store")?;
    let artifact_id = EntityId::new(inputs.require("library-id")?)
        .map_err(|error| Failure::invalid("invalid_library_id", error.to_string()))?;
    let version = EntityId::new(inputs.require("library-version")?)
        .map_err(|error| Failure::invalid("invalid_library_version", error.to_string()))?;
    let release = open_release(Path::new(store), &artifact_id, &version)
        .map_err(|error| engine_failure("library_open_failed", error))?;
    let artifact = format!("{store}/{artifact_id}/{version}");
    Ok(json!({
        "store": store,
        "artifact_id": artifact_id,
        "version": version,
        "content_root_digest": release.manifest.content_root_digest,
        "asset_count": release.assets.len(),
        "table_counts": release.snapshot.table_counts(),
        "execution_owner": "ds",
        "deterministic_completion": "the exact locally stored release was verified and decoded without fallback",
        "pls_cadd_ui_handoff": { "required": false, "condition": "Only a native solver/check or explicit visual acceptance requires PLS-CADD.", "artifact": artifact, "digest": release.manifest.content_root_digest, "post_ui_reimport": "Re-import any native-saved workspace as a new authority candidate." },
        "engineer_decision": "Engineer decides whether this exact release is authoritative for the intended project and strength scope."
    }))
}
pub fn render(data: &Value) -> String {
    format!(
        "opened {}/{}\n{}",
        data["artifact_id"],
        data["version"],
        data["content_root_digest"].as_str().unwrap_or("")
    )
}
