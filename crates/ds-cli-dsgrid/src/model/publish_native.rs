//! Native file publication, including governed replacement-content import.
use ds_cli_contract::{Inputs, outcome::Failure};
use ds_design_workspace::grid_publication::{self, Request};
use ds_grid_exchange::conversion::{
    BatchMode, ConversionRequest, OutcomeStatus, PlsContainer, PlsVersionIntent, SourceCandidate,
    SourceSet, TargetFormat, execute_conversion, plan_conversion, resolve_pls_project_selection,
};
use ds_grid_exchange::lineage_transfer::{
    ImportAsVersionError, ImportAsVersionRequest, import_package_as_version,
};
use ds_grid_exchange::package::unpack;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn run(inputs: &Inputs, path: &str) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let request = Request {
        project: project.into(),
        model_id: inputs.value("project-model").map(str::to_owned),
        model_kind: inputs.require("kind")?.into(),
        expected_head_revision_id: inputs.value("expected-head").map(str::to_owned),
        display_name: inputs.value("name").map(str::to_owned),
        reason: inputs.value("reason").map(str::to_owned),
    };
    let (bytes, import_receipt, source_receipt) = if inputs.switch("replace-content") {
        let (incoming, source_receipt) = incoming_bytes(inputs, path)?;
        let model = inputs.require("project-model")?;
        let revision = inputs.require("expected-head")?;
        let downloaded = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::Download {
                model: model.into(),
                revision: revision.into(),
            },
        )?;
        let target_bytes = downloaded.bytes.ok_or_else(|| {
            Failure::failed(
                "replace_content_invalid",
                "verified head download returned no bytes",
            )
        })?;
        let incoming_package = unpack(&incoming).map_err(|e| {
            Failure::invalid("replace_content_invalid", format!("incoming package: {e}"))
        })?;
        let target_package = unpack(&target_bytes).map_err(|e| {
            Failure::invalid(
                "replace_content_invalid",
                format!("project head package: {e}"),
            )
        })?;
        let incoming_sha = format!("{:x}", Sha256::digest(&incoming));
        let target_sha = downloaded.data["sha256"].as_str().ok_or_else(|| {
            Failure::failed(
                "replace_content_invalid",
                "verified head download has no digest",
            )
        })?;
        let imported = import_package_as_version(&ImportAsVersionRequest {
            working_bytes: &incoming,
            target_head_bytes: &target_bytes,
            expected_working_sha256: &incoming_sha,
            expected_target_head_sha256: target_sha,
            expected_working_model_id: &incoming_package.manifest.model.model_id,
            expected_target_model_id: &target_package.manifest.model.model_id,
            expected_target_revision: target_package.manifest.model.model_revision,
            libraries: &[],
        })
        .map_err(import_failure)?;
        (
            imported.package_bytes,
            Some(json!(imported.receipt)),
            source_receipt,
        )
    } else {
        if inputs.value("crs").is_some()
            || inputs.value("select-project").is_some()
            || inputs.switch("swap-xy")
        {
            return Err(Failure::invalid(
                "replace_content_target_required",
                "native import options require --replace-content",
            ));
        }
        (crate::package::read_bytes(path)?, None, None)
    };
    let intent = grid_publication::prepare(&request, &bytes).map_err(|message| {
        let failure = match message.split(':').next().unwrap_or("") {
            "publish_expected_head_required" => {
                Failure::invalid("publish_expected_head_required", message)
            }
            "model_too_large" => Failure::invalid("model_too_large", message),
            _ => Failure::invalid("model_invalid", message),
        };
        failure.remedy("inspect the package and exact publication inputs before retrying")
    })?;
    let mut receipt = ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::Publish { intent, bytes },
    )?
    .data;
    if let Some(import) = import_receipt {
        receipt["content_import"] = import;
        receipt["active_model_changed"] = json!(false);
    }
    if let Some(source) = source_receipt {
        receipt["source_conversion"] = source;
    }
    Ok(receipt)
}

fn incoming_bytes(inputs: &Inputs, path: &str) -> Result<(Vec<u8>, Option<Value>), Failure> {
    let extension = Path::new(path)
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("dsgrid") {
        if inputs.value("crs").is_some()
            || inputs.value("select-project").is_some()
            || inputs.switch("swap-xy")
        {
            return Err(Failure::invalid(
                "replace_content_target_required",
                "PLS backup options cannot be used with .dsgrid",
            ));
        }
        return Ok((crate::package::read_bytes(path)?, None));
    }
    if !extension.eq_ignore_ascii_case("bak") {
        return Err(Failure::invalid(
            "unsupported_model_source",
            "replacement source must be .dsgrid or .bak",
        ));
    }
    let crs = inputs.value("crs").ok_or_else(|| {
        Failure::invalid(
            "backup_crs_required",
            "PLS-CADD backup import needs a declared projected CRS",
        )
    })?;
    let bytes = crate::package::read_bytes(path)?;
    let name = Path::new(path)
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or_else(|| {
            Failure::invalid(
                "unsupported_model_source",
                "PLS-CADD backup needs a UTF-8 filename",
            )
        })?;
    let sources = SourceSet::new(vec![SourceCandidate::file(name, bytes)]);
    let project = inputs
        .value("select-project")
        .map(|leaf| {
            resolve_pls_project_selection(&sources, leaf)
                .map_err(|e| Failure::invalid("backup_selection_invalid", format!("{leaf}: {e}")))
        })
        .transpose()?;
    let request = ConversionRequest {
        sources,
        batch_mode: BatchMode::Separate,
        target: TargetFormat::Dsgrid,
        pls_version_intent: PlsVersionIntent::ConvertTo16_81,
        pls_container: PlsContainer::Folder,
        combine: None,
        declared_crs: Some(crs.into()),
        expected_location: None,
        swap_xy: inputs.switch("swap-xy"),
        selection: Vec::new(),
        pls_project: project,
    };
    let plan = plan_conversion(&request);
    if !plan.blockers.is_empty() || !plan.losses.is_empty() {
        return Err(Failure::invalid(
            "backup_conversion_blocked",
            "backup conversion plan has blockers or losses",
        )
        .detail(
            json!({"blockers": plan.blockers, "warnings": plan.warnings, "losses": plan.losses}),
        ));
    }
    let result = execute_conversion(&plan, &request.sources)
        .map_err(|e| Failure::invalid("backup_conversion_failed", e.to_string()))?;
    if result.status != OutcomeStatus::Completed
        || result.outputs.len() != 1
        || !result.outputs[0].relative_path.ends_with(".dsgrid")
    {
        return Err(Failure::invalid(
            "backup_conversion_failed",
            "backup did not yield exactly one .dsgrid",
        )
        .detail(json!({"sources": result.per_source})));
    }
    let bytes = result.outputs[0].bytes.clone();
    let package_sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok((
        bytes,
        Some(json!({
            "input": path,
            "plan_id": plan.plan_id,
            "source_digests": plan.pinned_digests,
            "warnings": plan.warnings,
            "losses": plan.losses,
            "package_sha256": package_sha256,
            "per_source": result.per_source,
        })),
    ))
}

fn import_failure(error: ImportAsVersionError) -> Failure {
    use ds_grid_exchange::lineage_transfer::LineageTransferError::*;
    let message = error.to_string();
    match error {
        WorkingDigestMismatch | TargetDigestMismatch => {
            Failure::invalid("replace_content_invalid", message)
        }
        WorkingIdentityMismatch | TargetIdentityMismatch => {
            Failure::invalid("replace_content_invalid", message)
        }
        RevisionOverflow => Failure::invalid("replace_content_invalid", message),
        WorkingPackage(_) | TargetPackage(_) | ResultPackage(_) | ContentChanged => {
            Failure::invalid("replace_content_invalid", message)
        }
    }
}
