//! Ordinary exact held GIS export through the existing Report host and Sync seal.
use super::{CliEngine, PublicationQueue, SealedRun, host_failure, seal_run_for_server};
use ds_cli_contract::{Failure, Inputs};
use ds_command_kernel::report_export::{
    self,
    held_export::{self as policy, Request},
};
use ds_report_host::{RunExtras, RunSettings, TransformerReportInputs};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};

/// The held status observation has no saved revision for this transformer.
pub(super) const STATUS_NOT_HELD: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "design_status_not_held",
    when: "this device holds no saved revision of the transformer",
    remedy: "run ds design status --read refresh once online, then retry",
};
fn refusal(r: policy::Refusal) -> Failure {
    Failure::invalid(r.code, r.message).remedy("Preserve the held room; save or refresh through its design owner when its saved revision is unavailable or changed")
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(policy::INVALID, message.into())
}
fn read(inputs: &Inputs) -> Result<Request, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(inputs.require("held-export")?)
        .and_then(|f| f.take(policy::MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| invalid(e.to_string()))?;
    let r = Request::decode(&bytes).map_err(refusal)?;
    if r.project != inputs.require("project")? || r.lane != inputs.require("lane")? {
        return Err(invalid(
            "Held export differs from explicit --project/--lane",
        ));
    }
    Ok(r)
}
fn output(inputs: &Inputs) -> Result<PathBuf, Failure> {
    let out = PathBuf::from(inputs.require("out-dir")?);
    if !out.is_absolute() || std::fs::symlink_metadata(&out).is_ok() {
        return Err(invalid(
            "held-export requires a fresh absolute output directory",
        ));
    }
    Ok(out)
}
pub(super) fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    for arg in super::COMMAND.args {
        if !matches!(arg.name, "held-export" | "project" | "lane" | "out-dir")
            && (inputs.switch(arg.name)
                || !inputs.repeated(arg.name).is_empty()
                || inputs
                    .value(arg.name)
                    .is_some_and(|v| Some(v) != arg.default))
        {
            return Err(invalid(format!(
                "--{} cannot accompany --held-export",
                arg.name
            )));
        }
    }
    output(inputs)?;
    read(inputs)?;
    Ok(())
}
pub(super) fn run(inputs: &Inputs) -> Result<Value, Failure> {
    preflight(inputs)?;
    let r = read(inputs)?;
    // Held-first (DECOUPLING-012): the export reads only what this device
    // holds. The held status observation proves the saved revision and
    // content the room claims; the held configuration proves its receipt.
    // Nothing held is a typed refusal, never a live read. Layer bytes are
    // never substituted for the caller's captured room.
    let identity = ds_cli_auth::headless_identity_for_named_project(&r.lane)?;
    r.authority(
        &r.project,
        identity.lane(),
        identity.uid(),
        identity.credential_audience_sha256(),
    )
    .map_err(refusal)?;
    let (status, _) = ds_cli_auth::read_transformer_status_for_project(
        &r.lane,
        &r.project,
        ds_command_kernel::transformer_status_store::TransformerStatusRead::Held,
    )?;
    let wanted = ds_command_kernel::transformer_status_store::row_identity(&r.transformer);
    let saved = status
        .result()
        .rows()
        .iter()
        .find(|row| ds_command_kernel::transformer_status_store::row_identity(row.name()) == wanted)
        .map(|row| &row.row()["metadata"])
        .filter(|metadata| metadata["version"].is_u64() && metadata["content_digest"].is_string())
        .ok_or_else(|| {
            Failure::unavailable(
                STATUS_NOT_HELD.code,
                format!("This device holds no saved revision of {}", r.transformer),
            )
            .remedy(STATUS_NOT_HELD.remedy)
        })?;
    r.saved_source(saved["version"].as_u64(), saved["content_digest"].as_str())
        .map_err(refusal)?;
    let configuration = ds_cli_auth::config_document::read(
        &r.lane,
        &r.project,
        ds_command_kernel::gateway_exchange::design::ProjectConfigurationRead::Held,
    )?;
    let authoritative = report_export::InputReceipt::from_config(
        &serde_json::to_value(&configuration.document).map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(invalid)?;
    if authoritative != r.receipt {
        return Err(Failure::conflict(
            "report_held_export_configuration_changed",
            "The original project receipt differs from the current admitted configuration; explicitly refresh the configuration and retry",
        ));
    }
    let identity = &identity;
    let current = || {
        let now = ds_cli_auth::headless_identity_for_named_project(&r.lane)?;
        super::require_same_context(identity, &r.project, &now, &r.project).map_err(host_failure)
    };
    current()?;
    let admin = if r.receipt.requires_admin_bounds() {
        let root = ds_report_host::shared_root().map_err(invalid)?;
        let path = ds_report_host::installed_admin_bounds_path(
            &root,
            &r.receipt.reference_semantic_sha256,
        );
        Some(
            ds_report_host::verify_admin_bounds_asset(&path, &r.receipt.reference_semantic_sha256)
                .map_err(host_failure)?,
        )
    } else {
        None
    };
    let queue = PublicationQueue::open(&r.lane, None)?;
    let tokens = r.selection.tokens().map_err(invalid)?;
    let selected: Vec<&str> = tokens.iter().map(String::as_str).collect();
    ds_sync_runtime::reports::admit_selected_replacement(
        &queue.store,
        &queue.fence,
        &r.project,
        &r.transformer,
        &selected,
    )
    .map_err(|error| {
        Failure::conflict("report_pending_outputs_require_sync", error)
            .remedy("Sync the previous report publication, then retry this exact selection")
    })?;
    let engine = CliEngine::capture(&r.lane).map_err(host_failure)?;
    let out = output(inputs)?;
    ds_layer_store::private::create_dir_all(&out).map_err(|error| invalid(error.to_string()))?;
    let staging = out.join("staging");
    let settings = RunSettings {
        project_id: &r.project,
        receipt: &r.receipt,
        admin_bounds: admin.as_ref(),
        staging_root: &staging,
        out_root: &out,
    };
    let report_inputs = TransformerReportInputs {
        transformer: r.transformer.clone(),
        server_version: r.server_version,
        layers: r.layers.clone(),
        content_digest: Some(r.content_digest.clone()),
        transformer_natures: None,
        selection: Some(r.selection.clone()),
        print_context: None,
        sheet: None,
        survey: None,
    };
    let media = r.media_document();
    let run = ds_report_host::run_transformer_report_with(
        &engine,
        &settings,
        &report_inputs,
        &RunExtras {
            media_grant: media.as_deref(),
        },
    )
    .map_err(host_failure)?;
    current()?;
    let (batch_id, client_publish_id, client_run_id, artifacts) =
        match seal_run_for_server(&run, identity.uid(), &r.project, &queue, &current)? {
            SealedRun::Queued(sealed) => (
                sealed.receipt.batch_id,
                sealed.receipt.client_publish_id,
                sealed.receipt.client_run_id,
                sealed.artifacts,
            ),
            SealedRun::AlreadyRecorded { replay_key, .. } => {
                let batch = ds_report_artifacts::list_project_publication_batches_from_root(
                    &queue.root,
                    &r.project,
                )
                .map_err(invalid)?
                .into_iter()
                .find(|b| b.client_publish_id == replay_key && b.owner_uid == r.uid)
                .ok_or_else(|| invalid("Committed replay batch is unavailable"))?;
                (
                    batch.batch_id,
                    batch.client_publish_id,
                    batch.client_run_id,
                    batch.outputs,
                )
            }
        };
    let mut context_warnings: Vec<String> = run
        .failed
        .iter()
        .map(|failure| {
            format!(
                "{}: {} {}",
                failure.output_id, failure.message, failure.remedy
            )
        })
        .collect();
    if let Some(omission) = run.receipt.get("quantity_media_omitted") {
        context_warnings.push(format!(
            "Quantity export omitted {} unrelated media references ({})",
            omission["reference_count"],
            omission["reason"]
                .as_str()
                .unwrap_or("media_not_required_for_quantity")
        ));
    }
    if let Some(omission) = &run.survey_layers_omitted {
        context_warnings.push(omission.message.clone());
    }
    Ok(
        json!({"schema":"ds.report-held-export-result/v1","project":r.project,"lane":r.lane,"clientRunId":client_run_id,"clientPublishId":client_publish_id,"batchId":batch_id,"engineVersion":run.engine.engine_version,"engineBuildManifestSha256":run.engine.build_manifest_sha256,"transformer":r.transformer,"serverVersion":r.server_version,"inputBaseFingerprint":run.receipt["input_base_fingerprint"],"roomContentSha256":run.receipt["room_content_sha256"],"artifacts":artifacts,"warnings":run.warnings,"contextWarnings":context_warnings,"failedFormats":run.failed,"quantityMediaOmitted":run.receipt["quantity_media_omitted"],"surveyLayersOmitted":run.survey_layers_omitted,"publicationQueued":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload() -> Request {
        let capture:policy::Capture=serde_json::from_value(json!({"project":"project-a","transformer":"tx_1","room":{"source":"lv","transformer_name":"tx_1","server_known":true,"local_dirty":false,"server_version":7,"layers":{"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"id":"p1"}}]}}},"receipt":{"schema":1,"country":"Burundi","sheets_json":r#"{"project_settings":[{"parameter":"report_formats","value":"voltage_drop"}]}"#,"sheets_sha256":"15b0aad6b1bb2895d12cfe8c810fbae6c470c3d83fc1a83a080673406a93db8a","reference_semantic_sha256":"b".repeat(64)},"selection":{"schema":"ds.design-output-selection/v1","geospatial":["shp"],"tabular":["xlsx"]},"media_grant":null})).unwrap();
        capture.prepare("canary", "owner", &"a".repeat(64)).unwrap()
    }
    #[test]
    fn held_intake_requires_exact_scope_fresh_scratch_and_no_other_controls() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("held.json");
        let output = root.path().join("output");
        std::fs::write(&input, serde_json::to_vec(&payload()).unwrap()).unwrap();
        let args = vec![
            "--held-export".into(),
            input.to_str().unwrap().into(),
            "--out-dir".into(),
            output.to_str().unwrap().into(),
            "--project".into(),
            "project-a".into(),
            "--lane".into(),
            "canary".into(),
        ];
        let parse = |tokens: &[String]| {
            ds_cli_contract::args::parse(&super::super::COMMAND, tokens).unwrap()
        };
        preflight(&parse(&args)).unwrap();
        assert!(!output.exists());
        for extra in [
            vec!["--dry-run"],
            vec!["--seed"],
            vec!["--held-preview", "other.json"],
            vec!["--transformer", "other"],
            vec!["--selection", "other.json"],
        ] {
            let mut mixed = args.clone();
            mixed.extend(extra.iter().map(|s| s.to_string()));
            assert!(preflight(&parse(&mixed)).is_err(), "{extra:?}");
        }
        let mut wrong = args.clone();
        wrong[5] = "project-b".into();
        assert!(preflight(&parse(&wrong)).is_err());
        let mut r = payload();
        r.local_dirty = true;
        std::fs::write(&input, serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(
            preflight(&parse(&args)).unwrap_err().code(),
            policy::UNSAVED
        );
        std::fs::write(&input, serde_json::to_vec(&payload()).unwrap()).unwrap();
        std::fs::create_dir(&output).unwrap();
        assert!(preflight(&parse(&args)).is_err());
    }
}
