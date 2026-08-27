use crate::{engine_failure, read, write_new};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::{
    ArtifactKind, LibraryReleaseOptions, bundle_digest, dsgrid, pack_library, read_library_manifest,
};
use ds_grid_model::EntityId;
use serde_json::{Value, json};
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "library.pack",
    path: &["library", "pack"],
    contract: 1,
    summary: "Pack one verified .dsgrid snapshot into an immutable library release.",
    purpose: "Uses the linked authoritative package/library codecs. The source must be an embedded-only .dsgrid; this command does not resolve or flatten another pinned library and never emits PLS-CADD assets.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "Verified source .dsgrid.").required(),
        Arg::value("out", "<path>", "New .dsgrid-library output path.").required(),
        Arg::value("library-id", "<id>", "Stable library id.").required(),
        Arg::value("library-version", "<id>", "Immutable library version.").required(),
        Arg::value("kind", "<kind>", "asset_bundle, asset_overlay or template.")
            .default("asset_bundle")
            .choices(&["asset_bundle", "asset_overlay", "template"]),
    ],
    output: "Written path, exact bundle/content-root digests, byte length and immutable identity.",
    examples: &[Example {
        command: "ds library pack --model ./standards.dsgrid --out ./standards.dsgrid-library --library-id standards --library-version v1 --output json",
        note: "Create one local immutable release.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "library_pack_failed",
            when: "the source/package/release is invalid",
            remedy: "validate the source model and resolve every resource closure issue",
        },
        Refusal {
            code: "output_exists",
            when: "the output path already exists",
            remedy: "choose a new immutable output path",
        },
    ],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let model = inputs.require("model")?;
    let out = inputs.require("out")?;
    let (package, _) = dsgrid::ingest(&read(model)?)
        .map_err(|error| engine_failure("library_pack_failed", error))?;
    let kind = match inputs.value("kind").unwrap_or("asset_bundle") {
        "asset_bundle" => ArtifactKind::AssetBundle,
        "asset_overlay" => ArtifactKind::AssetOverlay,
        "template" => ArtifactKind::Template,
        _ => unreachable!("choices enforced"),
    };
    let bytes = pack_library(
        &package.snapshot,
        &LibraryReleaseOptions {
            kind,
            artifact_id: EntityId::new(inputs.require("library-id")?)
                .map_err(|error| Failure::invalid("invalid_library_id", error.to_string()))?,
            revision_id: EntityId::new(inputs.require("library-version")?)
                .map_err(|error| Failure::invalid("invalid_library_version", error.to_string()))?,
            parent_revision_ids: Vec::new(),
            dependency_pins: Vec::new(),
            assets: package.assets,
        },
    )
    .map_err(|error| engine_failure("library_pack_failed", error))?;
    write_new(Path::new(out), &bytes)?;
    let manifest = read_library_manifest(&bytes)
        .map_err(|error| engine_failure("library_pack_failed", error))?;
    let digest = bundle_digest(&bytes);
    Ok(json!({
        "written": out,
        "artifact_id": manifest.artifact_id,
        "version": manifest.revision_id,
        "content_root_digest": manifest.content_root_digest,
        "bundle_digest": digest,
        "byte_length": bytes.len(),
        "execution_owner": "ds",
        "deterministic_completion": "the verified DS Grid snapshot was packed into one new immutable release",
        "pls_cadd_ui_handoff": { "required": false, "condition": "Packing never opens PLS-CADD; only a later native solver/check or visual acceptance requires it.", "artifact": out, "digest": digest, "post_ui_reimport": "Re-import any native-saved workspace as a new authority candidate." },
        "engineer_decision": "Engineer approves release adoption and certification scope; packing does not certify engineering adequacy."
    }))
}
pub fn render(data: &Value) -> String {
    format!(
        "packed {}/{} -> {}\n{}",
        data["artifact_id"],
        data["version"],
        data["written"].as_str().unwrap_or(""),
        data["bundle_digest"].as_str().unwrap_or("")
    )
}
