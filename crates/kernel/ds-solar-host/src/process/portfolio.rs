//! Captured fixed portfolio task over one exact locally closed city batch.
use super::*;
use ds_solar_contracts::artifact_names::{
    PORTFOLIO_CALCULATION_NAME, PORTFOLIO_CONTRIBUTION_NAME, PORTFOLIO_RESULT_NAME,
    PORTFOLIO_RUN_INPUT_NAME, REPORT_INPUT_NAME, portfolio_report_stem,
};
use ds_solar_contracts::portfolio_calculation::PortfolioCityCalculation;
use ds_solar_contracts::portfolio_task::*;
use ds_solar_contracts::{
    ArtifactRole, BatchArtifact, BatchState, PortfolioCityContribution, PortfolioMember,
    RepresentativeGraphArtifactReceipt, SolarArtifactBatch,
};

/// Captured once before queueing. Admission grants no project or source authority;
/// the host must first capture the approved workspace and exact source manifest.
pub struct PortfolioEngine {
    captured: PreparedEngine,
}
impl PortfolioEngine {
    pub fn capture(
        engine: &Path,
        scratch: &Path,
        workers: usize,
        cancel: &CancelToken,
    ) -> Result<Self, ProcessFailure> {
        let captured = PreparedEngine::capture(engine, scratch, workers, cancel)?;
        for schema in [
            PORTFOLIO_TASK_REQUEST_SCHEMA,
            PORTFOLIO_TASK_RESULT_SCHEMA,
            ds_solar_contracts::PORTFOLIO_RUN_INPUT_SCHEMA_VERSION,
            ds_solar_contracts::PORTFOLIO_SCHEMA_VERSION,
            "ds-solar.artifact-batch/v1",
        ] {
            if !captured
                .identity
                .schemas
                .iter()
                .any(|present| present == schema)
            {
                return Err(ProcessFailure::from(
                    "solar_engine_schema_incompatible_for_portfolio".to_string(),
                ));
            }
        }
        let stamp = &captured.completion_build_manifest_sha256;
        if stamp.len() != 64
            || !stamp
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || (!captured.identity.schema.is_empty()
                && stamp != &captured.identity.build_manifest_sha256)
        {
            return Err(ProcessFailure::from(
                "solar_engine_identity_completion_stamp_invalid".to_string(),
            ));
        }
        Ok(Self { captured })
    }

    pub fn identity(&self) -> &EngineIdentity {
        self.captured.identity()
    }

    /// Executes the fixed owner task once, without calculation or report code
    /// linked into the host. The private output remains alive through commit.
    pub fn calculate(
        self,
        request: &PortfolioTaskRequest,
        cancel: &CancelToken,
    ) -> Result<VerifiedPortfolio, ProcessFailure> {
        self.calculate_inner(request, cancel, CALCULATION_DEADLINE)
            .map_err(ProcessFailure::from)
    }

    fn calculate_inner(
        self,
        request: &PortfolioTaskRequest,
        cancel: &CancelToken,
        deadline: Duration,
    ) -> Result<VerifiedPortfolio, String> {
        check_cancel(cancel)?;
        request
            .validate_transport()
            .map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > PORTFOLIO_TASK_MAX_REQUEST_BYTES {
            return Err("solar_portfolio_request_too_large".into());
        }
        let captured = self.captured;
        let input = captured.directory.path().join("request.json");
        files::write(&input, &bytes).map_err(|error| error.to_string())?;
        let out = captured.directory.path().join("out");
        let _budget = budget::acquire(captured.workers, cancel)?;
        // An update during the capacity wait cannot redirect the accepted job.
        verify_executable(&captured.executable, &captured.stamp)?;
        let completion = run_task(
            &captured.executable,
            captured.directory.path(),
            _budget.workers(),
            Task::Portfolio {
                request: &input,
                out: &out,
            },
            cancel,
            deadline,
        );
        verify_executable(&captured.executable, &captured.stamp)?;
        let completion = completion?;
        check_cancel(cancel)?;
        ordinary_directory(&out)?;
        let encoded = files::read(
            &out.join("task-result.json"),
            PORTFOLIO_TASK_MAX_RESULT_BYTES as u64,
        )
        .map_err(|error| format!("solar_task_portfolio_result_invalid: {error}"))?;
        let task: PortfolioTaskResult = serde_json::from_slice(&encoded)
            .map_err(|error| format!("solar_task_portfolio_result_invalid: {error}"))?;
        if task.completion_build_manifest_sha256 != captured.completion_build_manifest_sha256 {
            return Err("solar_task_portfolio_completion_build_stamp_mismatch".into());
        }
        verify_completion(&completion, &task)?;
        verify_portfolio(&task, request, &bytes, &captured.identity, &out, cancel)?;
        let outputs = task
            .outputs
            .iter()
            .cloned()
            .map(|entry| VerifiedPortfolioFile {
                root: out.clone(),
                entry,
            })
            .collect();
        Ok(VerifiedPortfolio {
            _directory: captured.directory,
            task,
            outputs,
        })
    }
}

/// Exact task receipt and its closed file whitelist. No worker path is exposed.
pub struct VerifiedPortfolio {
    _directory: tempfile::TempDir,
    pub task: PortfolioTaskResult,
    pub outputs: Vec<VerifiedPortfolioFile>,
}
impl VerifiedPortfolio {
    pub fn file(&self, name: &str) -> Option<&VerifiedPortfolioFile> {
        self.outputs.iter().find(|file| file.entry.name == name)
    }
}
pub struct VerifiedPortfolioFile {
    pub entry: ArtifactEntry,
    root: PathBuf,
}
impl VerifiedPortfolioFile {
    pub fn read(&self, cancel: &CancelToken) -> Result<Vec<u8>, ProcessFailure> {
        read_entry(&self.root.join("artifacts"), &self.entry, cancel).map_err(ProcessFailure::from)
    }
    pub fn copy_to(&self, destination: &Path, cancel: &CancelToken) -> Result<(), ProcessFailure> {
        let bytes = self.read(cancel)?;
        check_cancel(cancel).map_err(ProcessFailure::from)?;
        files::write(destination, &bytes).map_err(|error| ProcessFailure::from(error.to_string()))
    }
}

fn verify_completion(completion: &[u8], task: &PortfolioTaskResult) -> Result<(), String> {
    if completion.len() > PORTFOLIO_TASK_MAX_COMPLETION_BYTES {
        return Err("solar_task_portfolio_completion_bound_exceeded".into());
    }
    let value: serde_json::Value = serde_json::from_slice(completion)
        .map_err(|error| format!("solar_task_portfolio_completion_invalid: {error}"))?;
    let artifact_bytes = task
        .outputs
        .iter()
        .try_fold(0_u64, |total, entry| total.checked_add(entry.byte_count))
        .ok_or("solar_task_portfolio_output_bound_exceeded")?;
    if value["status"] != "ok"
        || value["schema_version"] != PORTFOLIO_TASK_RESULT_SCHEMA
        || value["result_file"] != "task-result.json"
        || value["request_sha256"] != task.request_sha256
        || value["cities"].as_u64() != Some(task.result.city_count)
        || value["artifacts"].as_u64() != Some(task.outputs.len() as u64)
        || value["artifact_bytes"].as_u64() != Some(artifact_bytes)
    {
        return Err("solar_task_portfolio_completion_identity_or_request_mismatch".into());
    }
    Ok(())
}

fn read_entry(root: &Path, entry: &ArtifactEntry, cancel: &CancelToken) -> Result<Vec<u8>, String> {
    check_cancel(cancel)?;
    if !portfolio_task_safe_name(&entry.name) || entry.byte_count > PORTFOLIO_TASK_MAX_FILE_BYTES {
        return Err("solar_task_portfolio_artifact_descriptor_invalid".into());
    }
    ordinary_directory(root)?;
    let mut path = root.to_path_buf();
    let parts = entry.name.split('/').collect::<Vec<_>>();
    for part in &parts[..parts.len() - 1] {
        path.push(part);
        ordinary_directory(&path)?;
    }
    path.push(parts[parts.len() - 1]);
    let bytes = files::read(&path, entry.byte_count)
        .map_err(|error| format!("solar_task_portfolio_artifact_invalid: {error}"))?;
    if ArtifactEntry::from_bytes(&entry.name, &entry.media_type, &bytes) != *entry {
        return Err("solar_task_portfolio_artifact_digest_or_size_mismatch".into());
    }
    check_cancel(cancel)?;
    Ok(bytes)
}

fn entry(artifact: &BatchArtifact) -> ArtifactEntry {
    ArtifactEntry {
        name: artifact.name.clone(),
        media_type: artifact.media_type.clone(),
        byte_count: artifact.byte_count,
        content_digest: artifact.content_digest.clone(),
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes)
        .map_err(|error| format!("solar_task_portfolio_artifact_json_invalid: {error}"))
}

fn verify_portfolio(
    task: &PortfolioTaskResult,
    request: &PortfolioTaskRequest,
    request_bytes: &[u8],
    engine: &EngineIdentity,
    out: &Path,
    cancel: &CancelToken,
) -> Result<(), String> {
    check_cancel(cancel)?;
    let mut result_engine = task.result.engine.clone();
    if engine.schema.is_empty() && result_engine.schemas.is_empty() {
        result_engine.schemas = engine.schemas.clone();
    }
    if task.schema_version != PORTFOLIO_TASK_RESULT_SCHEMA
        || task.engine != *engine
        || result_engine != *engine
        || task.request_sha256 != sha256_hex(request_bytes)
        || task.input.identity != request.identity
        || task.input.run_id != request.run_id
        || task.input.representative_strategy != request.representative_strategy
        || task.input.language != request.language
        || task.input.document_options != request.documents
        || task.result.project_id != request.identity.project_id
        || task.result.root != request.identity.root
        || task.result.portfolio_id != request.identity.portfolio_id
        || task.result.portfolio_name != request.identity.portfolio_name
        || task.result.membership_revision != request.identity.membership_revision
        || task.result.run_id != request.run_id
        || task.result.input_digest != task.input.input_digest
        || task.result.assumptions != task.input.assumptions
        || task.batch.state != BatchState::LocalReady
        || task.batch.run.project_id != request.identity.project_id
        || task.batch.run.root != request.identity.root
        || task.batch.run.run_id != request.run_id
    {
        return Err("solar_task_portfolio_identity_or_request_mismatch".into());
    }
    task.input
        .validate()
        .map_err(|error| format!("solar_task_portfolio_input_invalid: {error}"))?;
    task.result
        .validate()
        .map_err(|error| format!("solar_task_portfolio_result_invalid: {error}"))?;
    task.batch
        .validate()
        .map_err(|error| format!("solar_task_portfolio_batch_invalid: {error}"))?;
    if request
        .assertions
        .currency
        .as_deref()
        .is_some_and(|value| value != task.input.assumptions.currency.as_str())
        || request
            .assertions
            .project_years
            .is_some_and(|value| value != task.input.assumptions.project_years)
        || request
            .assertions
            .discount_rate
            .is_some_and(|value| value.to_bits() != task.input.assumptions.discount_rate.to_bits())
    {
        return Err("solar_task_portfolio_assertion_mismatch".into());
    }
    let source = Path::new(&request.source.directory);
    if !source.is_absolute()
        || fs::canonicalize(source).map_err(|error| error.to_string())? != source
    {
        return Err("solar_task_portfolio_source_directory_invalid".into());
    }
    let source_batch: SolarArtifactBatch =
        decode(&read_entry(source, &request.source.manifest, cancel)?)?;
    if source_batch != request.source.batch
        || task.source.manifest != request.source.manifest
        || task.source.source_run_id != source_batch.run.run_id
        || task.source.source_batch_id != source_batch.batch_id
        || task.source.source_batch_digest != source_batch.batch_digest
    {
        return Err("solar_task_portfolio_source_identity_mismatch".into());
    }
    let mut consumed = BTreeMap::from([(
        request.source.manifest.name.clone(),
        request.source.manifest.clone(),
    )]);
    let mut source_cities = Vec::new();
    let mut members = Vec::new();
    let mut contributions = Vec::new();
    for city in &request.cities {
        let source_city = source_batch
            .run
            .cities
            .iter()
            .find(|member| &member.city_id == city)
            .ok_or("solar_task_portfolio_source_member_missing")?;
        source_cities.push(source_city.clone());
        let mut selected = |name: &str, role| -> Result<(ArtifactEntry, Vec<u8>), String> {
            let name = format!("cities/{city}/{name}");
            let artifact = source_batch
                .artifacts
                .iter()
                .find(|artifact| artifact.name == name)
                .ok_or("solar_task_portfolio_source_artifact_missing")?;
            if artifact.role != role
                || artifact.city_id.as_deref() != Some(city)
                || artifact.media_type != "application/json"
            {
                return Err("solar_task_portfolio_source_artifact_role_mismatch".into());
            }
            let entry = entry(artifact);
            let bytes = read_entry(source, &entry, cancel)?;
            consumed.insert(entry.name.clone(), entry.clone());
            Ok((entry, bytes))
        };
        let (_, contribution_bytes) = selected(
            PORTFOLIO_CONTRIBUTION_NAME,
            ArtifactRole::PortfolioContribution,
        )?;
        let (_, calculation_bytes) = selected(
            PORTFOLIO_CALCULATION_NAME,
            ArtifactRole::PortfolioCalculation,
        )?;
        let (report_entry, report_bytes) = selected(REPORT_INPUT_NAME, ArtifactRole::ReportInput)?;
        let contribution: PortfolioCityContribution = decode(&contribution_bytes)?;
        let calculation: PortfolioCityCalculation = decode(&calculation_bytes)?;
        calculation
            .validate_for(&contribution)
            .map_err(|error| error.to_string())?;
        let rebuilt = PortfolioCityCalculation::close(
            &contribution,
            &decode(&report_bytes)?,
            &report_entry.content_digest,
        )
        .map_err(|error| error.to_string())?;
        if contribution.identity.city_id != *city
            || contribution.identity.project_id != request.identity.project_id
            || contribution.identity.root != request.identity.root
            || contribution.input_digest != source_city.input_digest
            || contribution.result_digest != source_city.result_digest
            || rebuilt != calculation
        {
            return Err("solar_task_portfolio_source_projection_mismatch".into());
        }
        members.push(
            PortfolioMember::bind(
                &contribution,
                calculation.projection_digest,
                &source_batch.batch_id,
                &source_batch.batch_digest,
            )
            .map_err(|error| error.to_string())?,
        );
        contributions.push(contribution);
    }
    if task.source.cities != source_cities
        || task.input.members != members
        || task.batch.run.cities != source_cities
        || task.result.cities.len() != members.len()
        || task
            .result
            .cities
            .iter()
            .zip(&members)
            .any(|(city, member)| {
                city.city_id != member.city_id
                    || city.city_run_id != member.city_run_id
                    || city.input_digest != member.input_digest
                    || city.result_digest != member.result_digest
                    || city.contribution_digest != member.contribution_digest
                    || city.calculation_projection_digest != member.calculation_projection_digest
            })
    {
        return Err("solar_task_portfolio_membership_or_projection_mismatch".into());
    }
    task.input
        .ordered_contributions(&contributions)
        .map_err(|error| error.to_string())?;
    verify_graph_sources(task, request, &source_batch, source, &mut consumed, cancel)?;
    verify_files(task, out, cancel)?;
    // Reopen every consumed source at the final handoff; unrelated cities/charts
    // stay unopened and no freshness or acquisition API is available here.
    for entry in consumed.values() {
        read_entry(source, entry, cancel)?;
    }
    check_cancel(cancel)
}

fn verify_graph_sources(
    task: &PortfolioTaskResult,
    request: &PortfolioTaskRequest,
    batch: &SolarArtifactBatch,
    source: &Path,
    consumed: &mut BTreeMap<String, ArtifactEntry>,
    cancel: &CancelToken,
) -> Result<(), String> {
    let graphs = task
        .result
        .sections
        .get("representative_graphs")
        .ok_or("solar_task_portfolio_graph_section_missing")?;
    let rows = graphs["graphs"]
        .as_array()
        .ok_or("solar_task_portfolio_graph_receipts_invalid")?;
    if rows.len() > 64 || graphs["graph_count"].as_u64() != Some(rows.len() as u64) {
        return Err("solar_task_portfolio_graph_receipt_count_invalid".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        let receipt: RepresentativeGraphArtifactReceipt =
            serde_json::from_value(row["url"].clone())
                .map_err(|error| format!("solar_task_portfolio_graph_receipt_invalid: {error}"))?;
        receipt.validate().map_err(|error| error.to_string())?;
        let artifact = batch
            .artifacts
            .iter()
            .find(|artifact| artifact.name == receipt.name)
            .ok_or("solar_task_portfolio_graph_source_missing")?;
        if receipt.source_batch_id != batch.batch_id
            || receipt.source_batch_digest != batch.batch_digest
            || row["city_id"].as_str() != Some(receipt.city_id.as_str())
            || !request.cities.contains(&receipt.city_id)
            || !names.insert(receipt.name.clone())
            || artifact.role != receipt.role
            || artifact.city_id.as_deref() != Some(receipt.city_id.as_str())
            || artifact.media_type != receipt.media_type
            || artifact.byte_count != receipt.byte_count
            || artifact.content_digest != receipt.content_digest
        {
            return Err("solar_task_portfolio_graph_source_identity_mismatch".into());
        }
        let entry = entry(artifact);
        read_entry(source, &entry, cancel)?;
        consumed.insert(entry.name.clone(), entry);
    }
    Ok(())
}

fn verify_files(
    task: &PortfolioTaskResult,
    out: &Path,
    cancel: &CancelToken,
) -> Result<(), String> {
    if task.outputs.len() > PORTFOLIO_TASK_MAX_OUTPUT_FILES {
        return Err("solar_task_portfolio_output_bound_exceeded".into());
    }
    let mut declared = BTreeMap::new();
    let mut total = 0_u64;
    for entry in &task.outputs {
        total = total
            .checked_add(entry.byte_count)
            .ok_or("solar_task_portfolio_output_bound_exceeded")?;
        if total > PORTFOLIO_TASK_MAX_OUTPUT_BYTES
            || !portfolio_task_safe_name(&entry.name)
            || declared.insert(entry.name.clone(), entry).is_some()
        {
            return Err("solar_task_portfolio_output_descriptor_invalid".into());
        }
        read_entry(&out.join("artifacts"), entry, cancel)?;
    }
    let mut expected = Vec::new();
    for (artifact, name, value, role) in [
        (
            &task.input_artifact,
            PORTFOLIO_RUN_INPUT_NAME,
            serde_json::to_value(&task.input),
            ArtifactRole::PortfolioRunInput,
        ),
        (
            &task.result_artifact,
            PORTFOLIO_RESULT_NAME,
            serde_json::to_value(&task.result),
            ArtifactRole::PortfolioResult,
        ),
    ] {
        if artifact.name != name
            || artifact.media_type != "application/json"
            || declared.get(name).copied() != Some(artifact)
            || decode::<serde_json::Value>(&read_entry(&out.join("artifacts"), artifact, cancel)?)?
                != value.map_err(|error| error.to_string())?
        {
            return Err("solar_task_portfolio_output_json_mismatch".into());
        }
        expected.push(BatchArtifact::from_entry(artifact, role, None));
    }
    let document_names =
        task.input
            .document_options
            .report_intents
            .iter()
            .flat_map(|intent| {
                task.input.document_options.formats.iter().map(move |_| {
                    format!("{}.md", portfolio_report_stem(*intent, task.input.language))
                })
            })
            .collect::<Vec<_>>();
    if task
        .documents
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>()
        != document_names
    {
        return Err("solar_task_portfolio_requested_document_order_mismatch".into());
    }
    for (artifacts, role, media) in [
        (&task.documents, ArtifactRole::Report, "text/markdown"),
        (&task.charts, ArtifactRole::PortfolioChart, "image/png"),
    ] {
        for entry in artifacts {
            if entry.media_type != media || declared.get(&entry.name).copied() != Some(entry) {
                return Err("solar_task_portfolio_output_role_mismatch".into());
            }
            expected.push(BatchArtifact::from_entry(entry, role, None));
        }
    }
    expected.sort_by(|left, right| left.name.cmp(&right.name));
    let ordered_outputs = std::iter::once(&task.input_artifact)
        .chain(std::iter::once(&task.result_artifact))
        .chain(&task.documents)
        .chain(&task.charts)
        .chain(std::iter::once(&task.batch_manifest))
        .cloned()
        .collect::<Vec<_>>();
    if task.batch.artifacts != expected
        || task.outputs.len() != expected.len() + 1
        || task.outputs != ordered_outputs
        || task.batch_manifest.name != "batch.json"
        || task.batch_manifest.media_type != "application/json"
        || declared.get("batch.json").copied() != Some(&task.batch_manifest)
        || decode::<SolarArtifactBatch>(&read_entry(
            &out.join("artifacts"),
            &task.batch_manifest,
            cancel,
        )?)? != task.batch
    {
        return Err("solar_task_portfolio_output_manifest_mismatch".into());
    }
    // Reuse the existing bounded post-reap inventory walk, then the stricter
    // portfolio file/depth limits already enforced while the child ran.
    batch::verify_inventory(out, &declared)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
