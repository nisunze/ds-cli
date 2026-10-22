//! Completion seals and read-only observations through captured host authority.
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct CityPublication {
    pub city_id: String,
    pub operation: String,
    pub variant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Only a fresh reviewed import calls this hook. Inspection never adopts a
/// historical final. The producer is this import build; the input fence is the
/// source calculation's exact report-input bytes, independently of its build.
pub(super) fn city_final_completed(
    app: &AppHandle,
    directory: &Path,
    source: &ds_solar_contracts::SolarArtifactBatch,
    final_batch: &ds_solar_contracts::SolarArtifactBatch,
    city: &str,
    name: &str,
) -> (Option<serde_json::Value>, Option<String>) {
    let Some(provider) = app.publication.as_ref() else {
        return (None, None);
    };
    match city_final_publication(directory, source, final_batch, city, name)
        .and_then(|publication| provider.retain(publication))
    {
        Ok(receipt) => (Some(receipt), None),
        Err(error) => (None, Some(error)),
    }
}

fn city_final_publication(
    directory: &Path,
    source: &ds_solar_contracts::SolarArtifactBatch,
    final_batch: &ds_solar_contracts::SolarArtifactBatch,
    city: &str,
    name: &str,
) -> Result<crate::sync::Publication, String> {
    use ds_solar_contracts::ArtifactRole;
    if final_batch.run.project_id != source.run.project_id
        || final_batch.run.root != source.run.root
        || final_batch.run.run_id != source.run.run_id
        || final_batch.run.cities.len() != 1
        || final_batch.run.cities[0].city_id != city
        || !source.run.cities.contains(&final_batch.run.cities[0])
    {
        return Err("Reviewed final differs from its calculation lineage".into());
    }
    let logical = format!("cities/{city}/{name}");
    let final_file = verified_batch_artifact_at(directory, source, &logical, MAX_DOCUMENT_BYTES)?;
    if final_file.descriptor.role != ArtifactRole::Report
        || !final_batch.artifacts.contains(&final_file.descriptor)
    {
        return Err("Reviewed final is absent from its new closed import".into());
    }
    let input = verified_batch_artifact_at(
        directory,
        source,
        &format!("cities/{city}/{REPORT_INPUT_NAME}"),
        MAX_BATCH_ARTIFACT_BYTES,
    )?;
    if input.descriptor.role != ArtifactRole::ReportInput
        || input.descriptor.media_type != "application/json"
    {
        return Err("Reviewed final has no declared calculation report input".into());
    }
    let variant = name
        .strip_suffix(".md")
        .ok_or("Reviewed final is not Markdown")?;
    let release = format!(
        "{}@{}",
        ds_solar_contracts::version::ENGINE_NAME,
        ds_solar_contracts::version::ENGINE_VERSION
    );
    let publication = crate::sync::close(
        crate::sync::Source {
            project: &source.run.project_id,
            run: &source.run.run_id,
            operation: &format!("report-{city}"),
            variant,
            resource: city,
            release: &release,
            manifest: ds_solar_contracts::EngineIdentity::publication_manifest_sha256(),
            input_base: input
                .descriptor
                .content_digest
                .strip_prefix("sha256:")
                .ok_or("Calculation report input has no digest")?,
        },
        vec![(
            ds_sync_runtime::LocalOutput {
                filename: None,
                paper_size: None,
                presentation: None,
                output_id: variant.into(),
                format: "md".into(),
                content_type: final_file.descriptor.media_type,
                sha256: final_file
                    .descriptor
                    .content_digest
                    .strip_prefix("sha256:")
                    .ok_or("Reviewed final has no digest")?
                    .into(),
                size_bytes: final_file.descriptor.byte_count,
            },
            final_file.bytes,
        )],
    )?;
    ds_command_kernel::sync::declaration::validate(&publication.project, &publication.declaration)?;
    if closed_batch_at(directory)? != *source {
        return Err("Calculation lineage changed before final retention".into());
    }
    Ok(publication)
}

/// Trusted completion hook: only freshly computed, closed native batches enter
/// here. Reading an old batch never calls this function.
pub(super) fn cities_completed(
    app: &AppHandle,
    directory: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    claims: &BTreeMap<String, Result<ds_compute_runtime::SolarSnapshotProvenance, String>>,
    manifest: &str,
) -> Vec<CityPublication> {
    let Some(provider) = app.publication.as_ref() else {
        return Vec::new();
    };
    let mut receipts = Vec::new();
    for city in &batch.run.cities {
        let publications = claims
            .get(&city.city_id)
            .ok_or_else(|| {
                "No captured publication claim exists for this prepared city".to_string()
            })
            .and_then(|claim| claim.as_ref().map_err(Clone::clone))
            .and_then(|claim| city_publications(directory, batch, &city.city_id, claim, manifest));
        match publications {
            Err(error) => receipts.push(CityPublication {
                city_id: city.city_id.clone(),
                operation: format!("calculate-{}", city.city_id),
                variant: "default".into(),
                publication: None,
                error: Some(error),
            }),
            Ok(publications) => {
                for publication in publications {
                    let operation = publication.row.identity.operation.clone();
                    let variant = publication.row.identity.variant.clone();
                    let retained = provider.retain(publication);
                    let (publication, error) = match retained {
                        Ok(receipt) => (Some(receipt), None),
                        Err(error) => (None, Some(error)),
                    };
                    receipts.push(CityPublication {
                        city_id: city.city_id.clone(),
                        operation,
                        variant,
                        publication,
                        error,
                    });
                }
            }
        }
    }
    receipts
}

fn city_publications(
    directory: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    city: &str,
    claim: &ds_compute_runtime::SolarSnapshotProvenance,
    manifest: &str,
) -> Result<Vec<crate::sync::Publication>, String> {
    use ds_solar_contracts::ArtifactRole;
    if claim.project_id != batch.run.project_id
        || claim.root != batch.run.root
        || claim.template_id != city
    {
        return Err("Captured publication claim belongs to another calculation context".into());
    }
    let result_entries = batch
        .artifacts
        .iter()
        .filter(|a| a.city_id.as_deref() == Some(city) && a.role == ArtifactRole::CityResult)
        .collect::<Vec<_>>();
    if result_entries.len() != 1 {
        return Err("City closure must contain exactly one calculation result".into());
    }
    let result_file = verified_batch_artifact_at(
        directory,
        batch,
        &result_entries[0].name,
        MAX_BATCH_ARTIFACT_BYTES,
    )?;
    let result: ds_solar_contracts::SolarCalculationResult =
        serde_json::from_slice(&result_file.bytes)
            .map_err(|_| "The closed calculation result is invalid")?;
    result.validate().map_err(|e| e.to_string())?;
    if !result.engine.build_manifest_sha256.is_empty()
        && result.engine.build_manifest_sha256 != manifest
    {
        return Err("Calculation bytes belong to another producer build".into());
    }
    let member = batch
        .run
        .cities
        .iter()
        .find(|c| c.city_id == city)
        .ok_or("City is absent from its batch")?;
    if result.identity.project_id != claim.project_id
        || result.identity.root != claim.root
        || result.identity.city_id != city
        || result.input_digest != member.input_digest
        || result.result_digest != member.result_digest
    {
        return Err("Calculation result differs from its closed batch".into());
    }
    let report_input = verified_batch_artifact_at(
        directory,
        batch,
        &format!("cities/{city}/{REPORT_INPUT_NAME}"),
        MAX_BATCH_ARTIFACT_BYTES,
    )?;
    if report_input.descriptor.role != ArtifactRole::ReportInput {
        return Err("Calculation has no declared report input".into());
    }
    let input_sha = report_input
        .descriptor
        .content_digest
        .strip_prefix("sha256:")
        .ok_or("Report input has no exact digest")?
        .to_owned();
    let output = |id: String, format: &str, file: VerifiedBatchArtifact| -> Result<_, String> {
        Ok((
            ds_sync_runtime::LocalOutput {
                filename: None,
                paper_size: None,
                presentation: None,
                output_id: id,
                format: format.into(),
                content_type: file.descriptor.media_type,
                sha256: file
                    .descriptor
                    .content_digest
                    .strip_prefix("sha256:")
                    .ok_or("Publication output has no exact digest")?
                    .into(),
                size_bytes: file.descriptor.byte_count,
            },
            file.bytes,
        ))
    };
    let release = format!("{}@{}", result.engine.name, result.engine.version);
    let calculate = format!("calculate-{city}");
    let report = format!("report-{city}");
    let close = |operation: &str, variant: &str, base: &str, outputs| {
        crate::sync::close(
            crate::sync::Source {
                project: &batch.run.project_id,
                run: &batch.run.run_id,
                operation,
                variant,
                resource: city,
                release: &release,
                manifest,
                input_base: base,
            },
            outputs,
        )
    };
    let mut calculation = close(
        &calculate,
        "default",
        &claim.input_base_fingerprint,
        vec![output("report-input".into(), "json", report_input)?],
    )?;
    calculation.declaration["source_snapshot_sha256"] = claim.source_snapshot_sha256.clone().into();
    calculation.declaration["snapshot_receipt_id"] = claim.snapshot_receipt_id.clone().into();
    ds_command_kernel::sync::declaration::validate(&calculation.project, &calculation.declaration)?;
    let mut publications = vec![calculation];
    for draft in batch
        .artifacts
        .iter()
        .filter(|a| a.city_id.as_deref() == Some(city) && a.role == ArtifactRole::Report)
    {
        let name = draft
            .name
            .strip_prefix(&format!("cities/{city}/"))
            .ok_or("Report draft has no city namespace")?;
        let variant = match name {
            PARITY_DRAFT_NAME
            | APD_DRAFT_EN_NAME
            | NETWORK_DRAFT_NAME
            | NETWORK_DRAFT_EN_NAME
            | PLANT_DRAFT_NAME
            | PLANT_DRAFT_EN_NAME
            | FINANCIAL_DRAFT_NAME
            | FINANCIAL_DRAFT_EN_NAME => name
                .strip_suffix(".md")
                .ok_or("Report draft has no Markdown extension")?,
            _ => return Err("Report draft is outside the native publication vocabulary".into()),
        };
        let mut outputs = vec![output(
            variant.into(),
            "md",
            verified_batch_artifact_at(directory, batch, &draft.name, MAX_DOCUMENT_BYTES)?,
        )?];
        for chart in batch
            .artifacts
            .iter()
            .filter(|a| a.city_id.as_deref() == Some(city) && a.role == ArtifactRole::Chart)
        {
            let name = chart
                .name
                .strip_prefix(&format!("cities/{city}/"))
                .and_then(|n| n.strip_suffix(".png"))
                .ok_or("Chart is outside its city namespace")?;
            let id = std::iter::once("plot")
                .chain(name.split('/').filter(|p| *p != "plots"))
                .collect::<Vec<_>>()
                .join("-");
            outputs.push(output(
                id,
                "png",
                verified_batch_artifact_at(
                    directory,
                    batch,
                    &chart.name,
                    MAX_BATCH_ARTIFACT_BYTES,
                )?,
            )?);
        }
        let publication = close(&report, variant, &input_sha, outputs)?;
        ds_command_kernel::sync::declaration::validate(
            &publication.project,
            &publication.declaration,
        )?;
        publications.push(publication);
    }
    if closed_batch_at(directory)? != *batch {
        return Err("City batch changed before publication".into());
    }
    Ok(publications)
}
pub fn portfolio(
    app: AppHandle,
    command: PortfolioBatchCommand,
) -> Result<serde_json::Value, String> {
    handoff(app, command, None)
}

/// Called only by the native computation after its immutable batch closes.
pub(crate) fn completed(
    app: AppHandle,
    command: PortfolioBatchCommand,
    manifest: &str,
) -> Result<serde_json::Value, String> {
    handoff(app, command, Some(manifest))
}
fn handoff(
    app: AppHandle,
    command: PortfolioBatchCommand,
    completion: Option<&str>,
) -> Result<serde_json::Value, String> {
    let provider = app
        .publication
        .as_ref()
        .ok_or("This host has no captured portfolio publication authority")?;
    let directory = portfolio_run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&directory)?;
    require_batch_selector(&batch, &command.root, &command.run_id)?;
    require_expected_portfolio_batch(
        &batch,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    let plan = ds_solar_contracts::portfolio_publication::plan(&batch)
        .map_err(|error| error.to_string())?;
    portfolio_batch_inventory_from(&directory, &batch)?;
    let mut result = None;
    let mut outputs = Vec::new();
    for declaration in plan {
        let artifact =
            verified_batch_artifact_at(&directory, &batch, &declaration.name, 16 * 1024 * 1024)?;
        if declaration.output_id == "portfolio-result" {
            result = Some(artifact.bytes);
        } else {
            outputs.push(crate::PortfolioPublicationFile {
                declaration,
                bytes: artifact.bytes,
            });
        }
    }
    let bytes = result.ok_or("The portfolio closure has no result")?;
    let result: ds_solar_contracts::SolarPortfolioResult = serde_json::from_slice(&bytes)
        .map_err(|_| "The sealed portfolio result is invalid JSON")?;
    result.validate().map_err(|error| error.to_string())?;
    let input_file = verified_batch_artifact_at(
        &directory,
        &batch,
        "portfolio-run-input.json",
        MAX_DOCUMENT_BYTES,
    )?;
    if input_file.descriptor.role != ds_solar_contracts::ArtifactRole::PortfolioRunInput
        || input_file.descriptor.media_type != "application/json"
    {
        return Err("The portfolio closure has no declared input authority".into());
    }
    let input: PortfolioRunInput = serde_json::from_slice(&input_file.bytes)
        .map_err(|_| "The sealed portfolio input is invalid JSON")?;
    input.validate().map_err(|error| error.to_string())?;
    if input.identity.root != result.root
        || input.identity.project_id != result.project_id
        || input.identity.portfolio_id != result.portfolio_id
        || input.identity.membership_revision != result.membership_revision
        || input.run_id != result.run_id
        || input.input_digest != result.input_digest
    {
        return Err("The portfolio result differs from its sealed input authority".into());
    }
    if result.root != batch.run.root
        || result.project_id != batch.run.project_id
        || result.run_id != batch.run.run_id
        || result.cities.len() != batch.run.cities.len()
        || !result
            .cities
            .iter()
            .zip(&batch.run.cities)
            .all(|(result, member)| {
                result.city_id == member.city_id
                    && result.input_digest == member.input_digest
                    && result.result_digest == member.result_digest
            })
    {
        return Err("The portfolio result differs from its selected closure".into());
    }
    let after = closed_batch_at(&directory)?;
    if after != batch {
        return Err("The portfolio closure changed before publication".into());
    }
    portfolio_batch_inventory_from(&directory, &after)?;
    let manifest = match completion {
        Some(manifest) => manifest.to_owned(),
        None => observed_completion_manifest(&directory, &batch, &input, &result)?,
    };
    let publication = crate::sync::portfolio_with_manifest(&result, bytes, outputs, &manifest)?;
    let key = publication.row.client_publish_id.clone();
    let output_count = publication.row.outputs.len();
    let retained = if completion.is_some() {
        provider.retain(publication)?
    } else {
        provider.observe(&publication)?
    };
    if retained["project_id"] != result.project_id
        || retained["requested_publish_id"] != key
        || serde_json::from_value::<ds_command_kernel::sync_store::ArtifactState>(
            retained["state"].clone(),
        )
        .is_err()
    {
        return Err("The portfolio retention receipt differs from its selected closure".into());
    }
    let receipt = serde_json::json!({"project_id":result.project_id,"portfolio_id":result.portfolio_id,"membership_revision":result.membership_revision,
        "source_run_id":result.run_id,"publication_run_id":retained["client_publish_id"],"output_count":output_count,"publication":retained});
    Ok(
        serde_json::json!({"run_id":command.run_id,"batch_id":batch.batch_id,"batch_digest":batch.batch_digest,"receipt":receipt}),
    )
}

/// An on-disk hint supports lookup only. Observation cannot retain/adopt bytes
/// or grant release authority; the captured provider must already hold that seal.
fn observed_completion_manifest(
    directory: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    input: &PortfolioRunInput,
    result: &ds_solar_contracts::SolarPortfolioResult,
) -> Result<String, String> {
    let path = directory.join("engine-completion.json");
    if !path.exists() {
        return Ok(result.engine.build_manifest_sha256.clone());
    }
    let bytes = read_bounded_regular_file(&path, "engine completion", 32 << 10)?;
    let hint: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid engine completion")?;
    let engine: ds_solar_contracts::EngineIdentity =
        serde_json::from_value(hint["engine"].clone()).map_err(|_| "Invalid completion engine")?;
    let mut result_engine = result.engine.clone();
    if engine.schema.is_empty() && result_engine.schemas.is_empty() {
        result_engine.schemas = engine.schemas.clone();
    }
    let manifest = hint["build_manifest_sha256"]
        .as_str()
        .ok_or("Completion build stamp is absent")?;
    if hint["schema"] != "ds-solar.native-engine-completion/v1"
        || hint["batch_id"] != batch.batch_id
        || hint["batch_digest"] != batch.batch_digest
        || hint["input_digest"] != input.input_digest
        || hint["result_digest"] != result.portfolio_digest
        || engine != result_engine
        || manifest.len() != 64
        || !manifest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || (!engine.build_manifest_sha256.is_empty() && engine.build_manifest_sha256 != manifest)
    {
        return Err("Engine completion belongs to another closed portfolio".into());
    }
    Ok(manifest.to_owned())
}
