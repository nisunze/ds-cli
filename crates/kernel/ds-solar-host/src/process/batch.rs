//! Captured fixed batch task; artifact bytes remain private until verified.
use super::*;
use base64::Engine;
use ds_solar_contracts::prepared_batch::*;
use ds_solar_contracts::{ArtifactRole, BatchArtifact, BatchCity, BatchState};

/// Receives exact attributed owner events incrementally. The callback must
/// return promptly, like the shared ProgressSink; no intermediate event queue.
pub trait BatchProgressSink: Send + Sync {
    fn emit(&self, event: PreparedBatchProgress);
}
impl<F: Fn(PreparedBatchProgress) + Send + Sync> BatchProgressSink for F {
    fn emit(&self, event: PreparedBatchProgress) {
        self(event);
    }
}

/// Largest explicitly declared local batch task timeout. Existing callers retain
/// the two-hour default; the offline CLI declares a four-hour task contract.
pub const MAX_BATCH_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// The same immutable executable capture/resource fence as PreparedEngine.
pub struct BatchEngine {
    captured: PreparedEngine,
}
impl BatchEngine {
    pub fn capture(
        engine: &Path,
        scratch: &Path,
        workers: usize,
        cancel: &CancelToken,
    ) -> Result<Self, ProcessFailure> {
        let captured = PreparedEngine::capture(engine, scratch, workers, cancel)?;
        for schema in [
            PREPARED_BATCH_REQUEST_SCHEMA,
            PREPARED_BATCH_RESULT_SCHEMA,
            PREPARED_BATCH_PROGRESS_SCHEMA,
            "ds-solar.artifact-batch/v1",
        ] {
            if !captured
                .identity
                .schemas
                .iter()
                .any(|present| present == schema)
            {
                return Err(ProcessFailure::from(
                    "solar_engine_schema_incompatible_for_batch".to_string(),
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
    /// Pure fixed-task admission, including exact serialized request size. Hosts
    /// can refuse invalid local input before selecting or probing an executable.
    pub fn validate_request(request: &PreparedBatchRequest) -> Result<(), ProcessFailure> {
        write_request(request, std::io::sink(), BATCH_MAX_REQUEST_BYTES)
            .map_err(ProcessFailure::from)
    }
    pub fn identity(&self) -> &EngineIdentity {
        self.captured.identity()
    }
    /// One capture runs once. CPU limits apply to the process and all nested
    /// city/chart workers; requested concurrency cannot raise the caller budget.
    pub fn calculate(
        self,
        request: &PreparedBatchRequest,
        cancel: &CancelToken,
        progress: Option<&dyn BatchProgressSink>,
    ) -> Result<VerifiedBatch, ProcessFailure> {
        self.calculate_with_timeout(request, cancel, progress, CALCULATION_DEADLINE)
    }
    /// A positive, bounded caller deadline covers admission, worker-budget wait,
    /// execution and result verification. No late queued worker is launched.
    pub fn calculate_with_timeout(
        self,
        request: &PreparedBatchRequest,
        cancel: &CancelToken,
        progress: Option<&dyn BatchProgressSink>,
        timeout: Duration,
    ) -> Result<VerifiedBatch, ProcessFailure> {
        self.calculate_inner(request, cancel, progress, timeout)
            .map_err(ProcessFailure::from)
    }
    fn calculate_inner(
        self,
        request: &PreparedBatchRequest,
        cancel: &CancelToken,
        progress: Option<&dyn BatchProgressSink>,
        deadline: Duration,
    ) -> Result<VerifiedBatch, String> {
        check_cancel(cancel)?;
        if deadline.is_zero() || deadline > MAX_BATCH_TIMEOUT {
            return Err("solar_process_deadline_invalid: batch timeout must be positive and at most four hours".into());
        }
        let deadline = Instant::now()
            .checked_add(deadline)
            .ok_or("solar_process_deadline_invalid")?;
        let mut bytes = Vec::new();
        write_request(request, &mut bytes, BATCH_MAX_REQUEST_BYTES)?;
        let captured = self.captured;
        let input = captured.directory.path().join("request.json");
        files::write(&input, &bytes).map_err(|error| error.to_string())?;
        let out = captured.directory.path().join("out");
        let context = ProgressContext::new(
            request,
            &bytes,
            &captured.identity,
            &captured.completion_build_manifest_sha256,
            progress,
        )?;
        let _budget = budget::acquire_until(captured.workers, cancel, deadline)?;
        verify_executable(&captured.executable, &captured.stamp)?;
        let result = run_task_observed(
            &captured.executable,
            captured.directory.path(),
            _budget.workers(),
            Task::Batch {
                request: &input,
                out: &out,
            },
            cancel,
            remaining(deadline)?,
            Some(&context),
        );
        // Check even a refused task, after child/readers are reaped and before
        // any receipt/file is accepted. Keep the original error if unchanged.
        verify_executable(&captured.executable, &captured.stamp)?;
        result?;
        check_cancel(cancel)?;
        ordinary_directory(&out)?;
        let bytes_result =
            files::read(&out.join("task-result.json"), BATCH_MAX_RESULT_BYTES as u64)
                .map_err(|error| format!("solar_task_batch_result_invalid: {error}"))?;
        let task: PreparedBatchTaskResult = serde_json::from_slice(&bytes_result)
            .map_err(|error| format!("solar_task_batch_result_invalid: {error}"))?;
        if task.completion_build_manifest_sha256 != captured.completion_build_manifest_sha256 {
            return Err("solar_task_batch_completion_build_stamp_mismatch".into());
        }
        remaining(deadline)?;
        verify_batch(&task, request, &bytes, &captured.identity, &out, cancel)?;
        remaining(deadline)?;
        let outputs = task
            .outputs
            .into_iter()
            .map(|entry| VerifiedBatchFile {
                path: out.join("artifacts").join(&entry.name),
                root: out.clone(),
                entry,
            })
            .collect();
        check_cancel(cancel)?;
        Ok(VerifiedBatch {
            _directory: captured.directory,
            engine: task.engine,
            request_sha256: task.request_sha256,
            completion_build_manifest_sha256: task.completion_build_manifest_sha256,
            output: task.output,
            outputs,
        })
    }
}

fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| "solar_process_deadline_exceeded".into())
}

/// Holds the job's private directory through the host's exact-byte commit.
/// The host may read only the closed whitelist, never worker-supplied paths.
pub struct VerifiedBatch {
    _directory: tempfile::TempDir,
    pub engine: EngineIdentity,
    pub request_sha256: String,
    pub completion_build_manifest_sha256: String,
    pub output: PreparedBatchOutput,
    pub outputs: Vec<VerifiedBatchFile>,
}
impl VerifiedBatch {
    pub fn file(&self, name: &str) -> Option<&VerifiedBatchFile> {
        self.outputs.iter().find(|file| file.entry.name == name)
    }
}
pub struct VerifiedBatchFile {
    pub entry: ArtifactEntry,
    path: PathBuf,
    root: PathBuf,
}
impl VerifiedBatchFile {
    /// Reverify after the worker is gone and immediately before consumption.
    pub fn read(&self, cancel: &CancelToken) -> Result<Vec<u8>, ProcessFailure> {
        let path = artifact_path(&self.root, &self.entry.name).map_err(ProcessFailure::from)?;
        if path != self.path {
            return Err(ProcessFailure::from(
                "solar_task_batch_whitelist_entry_changed".to_string(),
            ));
        }
        read_entry(&path, &self.entry, cancel).map_err(ProcessFailure::from)
    }
    pub fn copy_to(&self, destination: &Path, cancel: &CancelToken) -> Result<(), ProcessFailure> {
        let bytes = self.read(cancel)?;
        check_cancel(cancel).map_err(ProcessFailure::from)?;
        files::write(destination, &bytes).map_err(|error| ProcessFailure::from(error.to_string()))
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= BATCH_MAX_LOGICAL_NAME_BYTES
        && !name.contains(['\\', ':', '\0'])
        && name.split('/').count() <= BATCH_MAX_PATH_DEPTH
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}
fn digest_valid(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
// One serializer enforces the same bound for pure admission and actual task
// bytes. Counting into a sink avoids allocating a second request-sized buffer.
fn write_request(
    request: &PreparedBatchRequest,
    output: impl std::io::Write,
    limit: u64,
) -> Result<(), String> {
    admit_request(request)?;
    struct BoundedWriter<W> {
        output: W,
        written: u64,
        limit: u64,
    }
    impl<W: std::io::Write> std::io::Write for BoundedWriter<W> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let next = self
                .written
                .checked_add(bytes.len() as u64)
                .filter(|next| *next <= self.limit)
                .ok_or_else(|| std::io::Error::other("solar_batch_request_too_large"))?;
            self.output.write_all(bytes)?;
            self.written = next;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.output.flush()
        }
    }
    serde_json::to_writer(
        BoundedWriter {
            output,
            written: 0,
            limit,
        },
        request,
    )
    .map_err(|error| error.to_string())
}

fn admit_request(request: &PreparedBatchRequest) -> Result<(), String> {
    let bounded_id = |value: &str| !value.trim().is_empty() && value.len() <= BATCH_MAX_ID_BYTES;
    if request.schema_version != PREPARED_BATCH_REQUEST_SCHEMA
        || request.prepared.is_empty()
        || request.prepared.len() > BATCH_MAX_CITIES
        || !bounded_id(&request.project_id)
        || !bounded_id(&request.root)
        || request
            .options
            .run_id
            .as_deref()
            .is_some_and(|id| !bounded_id(id))
        || request.options.report_drafts.len() > BATCH_MAX_DRAFTS
        || request
            .options
            .report_drafts
            .iter()
            .any(|draft| draft.len() > BATCH_MAX_ID_BYTES)
        || request.options.input_maps.len() > BATCH_MAX_CITIES
    {
        return Err("solar_task_batch_request_invalid".into());
    }
    let mut cities = std::collections::BTreeSet::new();
    for prepared in &request.prepared {
        let city = &prepared.city.identity;
        if !bounded_id(&city.city_id)
            || city.city_id.contains(['/', '\\', ':', '\0'])
            || matches!(city.city_id.as_str(), "." | "..")
            || !cities.insert(city.city_id.as_str())
            || city.project_id != request.project_id
            || city.root != request.root
        {
            return Err("solar_task_batch_request_identity_invalid".into());
        }
        // Invalid sealed inputs remain an ordered typed city failure. Admission
        // does not convert a partial batch into a blanket refusal or recompute.
    }
    let mut maps = 0usize;
    let mut bytes = 0u64;
    for (city, entries) in &request.options.input_maps {
        if !bounded_id(city) {
            return Err("solar_task_batch_map_identity_invalid".into());
        }
        maps = maps
            .checked_add(entries.len())
            .ok_or("solar_task_batch_map_bound_exceeded")?;
        if maps > BATCH_MAX_OUTPUT_FILES {
            return Err("solar_task_batch_map_bound_exceeded".into());
        }
        for (uri, map) in entries {
            if uri.is_empty()
                || uri.len() > BATCH_MAX_URI_BYTES
                || !safe_name(&map.entry.name)
                || map.entry.byte_count > BATCH_MAX_FILE_BYTES
                || !digest_valid(&map.entry.content_digest)
                || map.body_base64.len() as u64 > BATCH_MAX_FILE_BYTES.div_ceil(3) * 4
            {
                return Err("solar_task_batch_map_descriptor_invalid".into());
            }
            let body = base64::engine::general_purpose::STANDARD
                .decode(&map.body_base64)
                .map_err(|error| format!("solar_task_batch_map_invalid: {error}"))?;
            bytes = bytes
                .checked_add(body.len() as u64)
                .ok_or("solar_task_batch_map_bound_exceeded")?;
            if bytes > BATCH_MAX_OUTPUT_BYTES
                || ArtifactEntry::from_bytes(&map.entry.name, &map.entry.media_type, &body)
                    != map.entry
            {
                return Err("solar_task_batch_map_digest_or_size_mismatch".into());
            }
        }
    }
    Ok(())
}

pub(super) struct ProgressContext<'a> {
    request: &'a PreparedBatchRequest,
    request_sha256: String,
    engine_sha256: String,
    sink: Option<&'a dyn BatchProgressSink>,
}
impl<'a> ProgressContext<'a> {
    fn new(
        request: &'a PreparedBatchRequest,
        bytes: &[u8],
        engine: &EngineIdentity,
        completion_stamp: &str,
        sink: Option<&'a dyn BatchProgressSink>,
    ) -> Result<Self, String> {
        Ok(Self {
            request,
            request_sha256: sha256_hex(bytes),
            engine_sha256: sha256_hex(
                &serde_json::to_vec(&ds_solar_contracts::engine_probe::EngineBuildProbe {
                    identity: engine.clone(),
                    completion_build_manifest_sha256: completion_stamp.into(),
                })
                .map_err(|error| error.to_string())?,
            ),
            sink,
        })
    }
}

pub(super) fn read_progress(
    mut stream: impl Read,
    context: &ProgressContext<'_>,
    cancel: &CancelToken,
) -> Result<Vec<u8>, String> {
    let mut buffer = [0; 8192];
    let mut line = Vec::new();
    let mut total = 0usize;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut completion = None;
    loop {
        let n = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n)
            .ok_or("solar_task_batch_progress_bound_exceeded")?;
        if total > BATCH_MAX_PROGRESS_BYTES + BATCH_MAX_COMPLETION_BYTES {
            return Err("solar_task_batch_progress_bound_exceeded".into());
        }
        for byte in &buffer[..n] {
            line.push(*byte);
            if line.len() > BATCH_MAX_PROGRESS_LINE_BYTES {
                return Err("solar_task_batch_progress_line_bound_exceeded".into());
            }
            if *byte != b'\n' {
                continue;
            }
            if completion.is_some() {
                return Err("solar_task_batch_progress_after_completion".into());
            }
            let value: serde_json::Value = serde_json::from_slice(&line)
                .map_err(|error| format!("solar_task_batch_progress_invalid: {error}"))?;
            if value
                .get("schema_version")
                .and_then(|schema| schema.as_str())
                == Some(PREPARED_BATCH_PROGRESS_SCHEMA)
            {
                let event: PreparedBatchProgress = serde_json::from_value(value)
                    .map_err(|error| format!("solar_task_batch_progress_invalid: {error}"))?;
                encode_batch_progress_line(&event)
                    .map_err(|error| format!("solar_task_batch_progress_invalid: {error}"))?;
                let prepared = context
                    .request
                    .prepared
                    .iter()
                    .find(|prepared| prepared.city.identity.city_id == event.city_id)
                    .ok_or("solar_task_batch_progress_city_mismatch")?;
                if event.engine_sha256 != context.engine_sha256
                    || event.request_sha256 != context.request_sha256
                    || event.project_id != context.request.project_id
                    || event.root != context.request.root
                    || event.run_id
                        != context
                            .request
                            .options
                            .run_id
                            .as_deref()
                            .unwrap_or("unattributed")
                    || event.prepared_input_digest != prepared.input_digest
                {
                    return Err("solar_task_batch_progress_identity_or_request_mismatch".into());
                }
                let count = counts.entry(event.city_id.clone()).or_default();
                *count += 1;
                if *count > BATCH_MAX_PROGRESS_EVENTS_PER_CITY {
                    return Err("solar_task_batch_progress_event_bound_exceeded".into());
                }
                check_cancel(cancel)?;
                if let Some(sink) = context.sink {
                    sink.emit(event);
                }
                check_cancel(cancel)?;
            } else {
                if line.len() > BATCH_MAX_COMPLETION_BYTES
                    || !matches!(value["status"].as_str(), Some("ok" | "error"))
                {
                    return Err("solar_task_batch_completion_invalid".into());
                }
                if value["status"] == "ok"
                    && (value["schema_version"] != PREPARED_BATCH_RESULT_SCHEMA
                        || value["request_sha256"] != context.request_sha256)
                {
                    return Err("solar_task_batch_completion_identity_or_request_mismatch".into());
                }
                completion = Some(line.clone());
            }
            line.clear();
        }
    }
    if !line.is_empty() {
        return Err("solar_task_batch_progress_line_incomplete".into());
    }
    completion.ok_or_else(|| "solar_task_batch_completion_missing".into())
}

fn artifact_path(out: &Path, name: &str) -> Result<PathBuf, String> {
    if !safe_name(name) {
        return Err("solar_task_batch_artifact_name_invalid".into());
    }
    let mut path = out.join("artifacts");
    ordinary_directory(&path)?;
    let parts = Path::new(name).components().collect::<Vec<_>>();
    for part in &parts[..parts.len() - 1] {
        path.push(part.as_os_str());
        ordinary_directory(&path)?;
    }
    path.push(parts.last().expect("safe nonempty name").as_os_str());
    Ok(path)
}
fn read_entry(path: &Path, entry: &ArtifactEntry, cancel: &CancelToken) -> Result<Vec<u8>, String> {
    check_cancel(cancel)?;
    let bytes = files::read(path, BATCH_MAX_FILE_BYTES)
        .map_err(|error| format!("solar_task_batch_artifact_invalid: {error}"))?;
    if ArtifactEntry::from_bytes(&entry.name, &entry.media_type, &bytes) != *entry {
        return Err("solar_task_batch_artifact_digest_or_size_mismatch".into());
    }
    check_cancel(cancel)?;
    Ok(bytes)
}

fn verify_batch(
    task: &PreparedBatchTaskResult,
    request: &PreparedBatchRequest,
    request_bytes: &[u8],
    engine: &EngineIdentity,
    out: &Path,
    cancel: &CancelToken,
) -> Result<(), String> {
    check_cancel(cancel)?;
    let prepared_input_digests = request
        .prepared
        .iter()
        .map(|prepared| BatchPreparedInputDigest {
            city_id: prepared.city.identity.city_id.clone(),
            input_digest: prepared.input_digest.clone(),
        })
        .collect::<Vec<_>>();
    let batch = &task.output.batch;
    if task.schema_version != PREPARED_BATCH_RESULT_SCHEMA
        || task.engine != *engine
        || task.request_sha256 != sha256_hex(request_bytes)
        || task.prepared_input_digests != prepared_input_digests
        || batch.run.project_id != request.project_id
        || batch.run.root != request.root
        || batch.run.run_id != request.options.run_id.as_deref().unwrap_or("unattributed")
        || task.output.cities.len() != request.prepared.len()
        || task.output.portfolio.is_some()
        || task.output.portfolio_document.is_some()
        || batch.state != BatchState::LocalReady
    {
        return Err("solar_task_batch_identity_or_request_mismatch".into());
    }
    batch
        .validate()
        .map_err(|error| format!("solar_task_batch_digest_invalid: {error}"))?;
    if task.outputs.len() > BATCH_MAX_OUTPUT_FILES {
        return Err("solar_task_batch_output_bound_exceeded".into());
    }
    let mut declared = BTreeMap::new();
    let mut total = 0u64;
    for entry in &task.outputs {
        if !safe_name(&entry.name)
            || entry.byte_count > BATCH_MAX_FILE_BYTES
            || !digest_valid(&entry.content_digest)
            || declared.insert(entry.name.clone(), entry).is_some()
        {
            return Err("solar_task_batch_artifact_descriptor_invalid".into());
        }
        total = total
            .checked_add(entry.byte_count)
            .ok_or("solar_task_batch_output_bound_exceeded")?;
        if total > BATCH_MAX_OUTPUT_BYTES {
            return Err("solar_task_batch_output_bound_exceeded".into());
        }
        read_entry(&artifact_path(out, &entry.name)?, entry, cancel)?;
    }
    // Enumerate after reap; undeclared files and worker links are refused too.
    verify_inventory(out, &declared)?;
    let mut expected_cities = Vec::new();
    let mut semantic = Vec::new();
    let mut frames = Vec::new();
    for (city, prepared) in task.output.cities.iter().zip(&request.prepared) {
        check_cancel(cancel)?;
        if city.city_id != prepared.city.identity.city_id {
            return Err("solar_task_batch_city_order_mismatch".into());
        }
        let Ok(outcome) = &city.outcome else {
            if city.portfolio_projection.is_some() {
                return Err("solar_task_batch_failed_city_projection_invalid".into());
            }
            // A failed prepared admission must retain the owner's exact typed error.
            if let Err(expected) = ds_solar_contracts::require_prepared(prepared)
                && city.outcome.as_ref().err() != Some(&expected)
            {
                return Err("solar_task_batch_prepared_failure_mismatch".into());
            }
            continue;
        };
        if prepared.validate().is_err() {
            return Err("solar_task_batch_unprepared_success_invalid".into());
        }
        let result = &outcome.result;
        let mut result_engine = result.engine.clone();
        if engine.schema.is_empty() && result_engine.schemas.is_empty() {
            result_engine.schemas = engine.schemas.clone();
        }
        if result_engine != *engine
            || result.identity != prepared.city.identity
            || result.input_digest != prepared.input_digest
            || result.weather_digest != prepared.weather.content_digest
            || result.run_id.as_deref()
                != Some(batch_city_run_id(&batch.run.run_id, &city.city_id).as_str())
            || outcome.artifacts.is_some() != request.options.render_charts
            || outcome.report_model.is_some() != request.options.render_charts
            || (!request.options.generate_documents && !outcome.documents.is_empty())
        {
            return Err("solar_task_batch_city_identity_or_options_mismatch".into());
        }
        result
            .validate()
            .map_err(|error| format!("solar_task_batch_city_digest_invalid: {error}"))?;
        expected_cities.push(BatchCity {
            city_id: city.city_id.clone(),
            input_digest: result.input_digest.clone(),
            result_digest: result.result_digest.clone(),
        });
        let prefix = format!("cities/{}/", city.city_id);
        let entry = declared
            .get(&format!("{prefix}result.json"))
            .ok_or("solar_task_batch_result_artifact_missing")?;
        let value: ds_solar_contracts::SolarCalculationResult = serde_json::from_slice(
            &read_entry(&artifact_path(out, &entry.name)?, entry, cancel)?,
        )
        .map_err(|error| format!("solar_task_batch_city_json_invalid: {error}"))?;
        if serde_json::to_value(value).map_err(|error| error.to_string())?
            != serde_json::to_value(result).map_err(|error| error.to_string())?
        {
            return Err("solar_task_batch_city_result_artifact_mismatch".into());
        }
        semantic.push(BatchArtifact::from_entry(
            entry,
            ArtifactRole::CityResult,
            Some(&city.city_id),
        ));
        let mut record = |entry: &ArtifactEntry, role| -> Result<(), String> {
            if !entry.name.starts_with(&prefix) || declared.get(&entry.name).copied() != Some(entry)
            {
                return Err("solar_task_batch_outcome_artifact_mismatch".into());
            }
            semantic.push(BatchArtifact::from_entry(entry, role, Some(&city.city_id)));
            Ok(())
        };
        if outcome.report_input.name != format!("{prefix}report_input.json")
            || outcome.daily_load_profile_frame.name
                != format!("{prefix}base-daily-load-profile.json")
        {
            return Err("solar_task_batch_outcome_artifact_name_mismatch".into());
        }
        record(&outcome.report_input, ArtifactRole::ReportInput)?;
        if declared
            .get(&outcome.daily_load_profile_frame.name)
            .copied()
            != Some(&outcome.daily_load_profile_frame)
        {
            return Err("solar_task_batch_daily_frame_missing".into());
        }
        frames.push(outcome.daily_load_profile_frame.clone());
        let daily = read_json(out, &outcome.daily_load_profile_frame, cancel)?;
        let expected_daily = serde_json::json!({ "schema_version": "ds-solar.daily-load-profile/v1", "kind": "frame", "rows": 24,
            "columns": { "hour": {"values": (0..24).collect::<Vec<_>>()}, "load_w": {"values": outcome.daily_load_profile.load_w},
            "load_w_actual": {"values": outcome.daily_load_profile.load_w_actual} }});
        if daily != expected_daily
            || outcome
                .daily_load_profile
                .load_w
                .iter()
                .chain(&outcome.daily_load_profile.load_w_actual)
                .any(|value| !value.is_finite())
        {
            return Err("solar_task_batch_daily_frame_mismatch".into());
        }
        let report = read_json(out, &outcome.report_input, cancel)?;
        for (field, value) in [
            ("project_id", request.project_id.as_str()),
            ("root", request.root.as_str()),
            ("city_id", city.city_id.as_str()),
            ("pipeline_run_id", result.run_id.as_deref().unwrap_or("")),
            ("input_digest", result.input_digest.as_str()),
            ("result_digest", result.result_digest.as_str()),
        ] {
            if report["_meta"][field].as_str() != Some(value) {
                return Err("solar_task_batch_report_input_identity_mismatch".into());
            }
        }
        for (field, expected) in [
            ("root", request.root.as_str()),
            ("context", city.city_id.as_str()),
        ] {
            if report["_config"]["paths"]["_meta"][field].as_str() != Some(expected) {
                return Err("solar_task_batch_report_input_path_identity_mismatch".into());
            }
        }
        if let Some(model) = &outcome.report_model {
            if model.name != format!("{prefix}report-model.json") {
                return Err("solar_task_batch_report_model_name_mismatch".into());
            }
            record(model, ArtifactRole::CityReportModel)?;
            let value = read_json(out, model, cancel)?;
            if value["result_digest"] != result.result_digest
                || value["result_schema_version"] != result.schema_version
                || value["city"] != result.identity.display_name
                || value["language"] != prepared.city.run_options.language
            {
                return Err("solar_task_batch_report_model_identity_mismatch".into());
            }
        }
        if let (Some(model), Some(artifacts)) = (&outcome.report_model, &outcome.artifacts) {
            let model = read_json(out, model, cancel)?;
            let chart_names = model["charts"]
                .as_array()
                .ok_or("solar_task_batch_report_model_charts_invalid")?
                .iter()
                .map(|chart| {
                    chart["name"]
                        .as_str()
                        .map(|name| format!("{prefix}{name}.png"))
                        .ok_or("solar_task_batch_report_model_chart_name_invalid")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if artifacts
                .charts
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>()
                != chart_names
                || artifacts
                    .charts
                    .iter()
                    .any(|entry| entry.media_type != "image/png")
            {
                return Err("solar_task_batch_chart_order_or_media_mismatch".into());
            }
        }
        let draft_intents = if request.options.report_drafts.is_empty() {
            vec!["apd".to_string()]
        } else {
            request.options.report_drafts.clone()
        };
        let expected_documents = if request.options.generate_documents {
            draft_intents
                .iter()
                .filter_map(|intent| {
                    ds_solar_contracts::artifact_names::report_draft_name(
                        intent,
                        &prepared.city.run_options.language,
                    )
                })
                .map(|name| format!("{prefix}{name}"))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if outcome
            .documents
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>()
            != expected_documents
        {
            return Err("solar_task_batch_requested_documents_mismatch".into());
        }
        if outcome
            .documents
            .iter()
            .any(|entry| entry.media_type != "text/markdown")
            || [&outcome.report_input, &outcome.daily_load_profile_frame]
                .iter()
                .any(|entry| entry.media_type != "application/json")
        {
            return Err("solar_task_batch_outcome_media_type_mismatch".into());
        }
        let mut expected_maps = BTreeMap::new();
        if let Some(maps) = request.options.input_maps.get(&city.city_id) {
            for map in maps.values() {
                let mut entry = map.entry.clone();
                entry.name = format!("{prefix}{}", entry.name);
                expected_maps.insert(entry.name.clone(), entry);
            }
        }
        if outcome.input_maps != expected_maps.into_values().collect::<Vec<_>>() {
            return Err("solar_task_batch_requested_maps_mismatch".into());
        }
        let mut manifested = Vec::new();
        for entry in &outcome.documents {
            record(entry, ArtifactRole::Report)?;
            manifested.push(entry.clone());
        }
        for entry in &outcome.input_maps {
            record(entry, ArtifactRole::Export)?;
            manifested.push(entry.clone());
        }
        if let Some(artifacts) = &outcome.artifacts {
            if artifacts.manifest.schema_version
                != ds_solar_contracts::version::MANIFEST_SCHEMA_VERSION
            {
                return Err("solar_task_batch_chart_manifest_schema_invalid".into());
            }
            let mut charts = artifacts.charts.clone();
            charts.sort_by(|a, b| a.name.cmp(&b.name));
            if charts != artifacts.manifest.artifacts {
                return Err("solar_task_batch_chart_manifest_mismatch".into());
            }
            for entry in &charts {
                record(entry, ArtifactRole::Chart)?;
                manifested.push(entry.clone());
            }
        }
        for entry in &mut manifested {
            entry.name = entry
                .name
                .strip_prefix(&prefix)
                .ok_or("solar_task_batch_manifest_name_invalid")?
                .to_string();
        }
        manifested.sort_by(|a, b| a.name.cmp(&b.name));
        if result.artifacts.schema_version != ds_solar_contracts::version::MANIFEST_SCHEMA_VERSION
            || result.artifacts.artifacts != manifested
        {
            return Err("solar_task_batch_result_manifest_mismatch".into());
        }
        let projection = city
            .portfolio_projection
            .as_ref()
            .ok_or("solar_task_batch_success_projection_missing")?;
        if let Ok(projection) = projection {
            if projection.contribution.name != format!("{prefix}portfolio-contribution.json")
                || projection.calculation.name != format!("{prefix}portfolio-calculation.json")
            {
                return Err("solar_task_batch_projection_name_mismatch".into());
            }
            record(
                &projection.contribution,
                ArtifactRole::PortfolioContribution,
            )?;
            record(&projection.calculation, ArtifactRole::PortfolioCalculation)?;
            let contribution: ds_solar_contracts::PortfolioCityContribution =
                serde_json::from_value(read_json(out, &projection.contribution, cancel)?)
                    .map_err(|error| format!("solar_task_batch_contribution_invalid: {error}"))?;
            contribution
                .validate()
                .map_err(|error| format!("solar_task_batch_contribution_invalid: {error}"))?;
            if contribution.identity != result.identity
                || contribution.city_run_id != result.run_id.as_deref().unwrap_or("")
                || contribution.engine != result.engine
                || contribution.input_digest != result.input_digest
                || contribution.result_digest != result.result_digest
            {
                return Err("solar_task_batch_contribution_identity_mismatch".into());
            }
            let calculation: ds_solar_contracts::portfolio_calculation::PortfolioCityCalculation =
                serde_json::from_value(read_json(out, &projection.calculation, cancel)?).map_err(
                    |error| format!("solar_task_batch_calculation_projection_invalid: {error}"),
                )?;
            calculation.validate_for(&contribution).map_err(|error| {
                format!("solar_task_batch_calculation_projection_invalid: {error}")
            })?;
            let expected =
                ds_solar_contracts::portfolio_calculation::PortfolioCityCalculation::close(
                    &contribution,
                    &report,
                    &outcome.report_input.content_digest,
                )
                .map_err(|error| {
                    format!("solar_task_batch_calculation_projection_invalid: {error}")
                })?;
            if calculation != expected {
                return Err("solar_task_batch_calculation_projection_source_mismatch".into());
            }
        }
    }
    semantic.sort_by(|a, b| a.name.cmp(&b.name));
    if batch.run.cities != expected_cities
        || batch.artifacts != semantic
        || task.outputs.len() != semantic.len() + frames.len()
    {
        return Err("solar_task_batch_semantic_manifest_mismatch".into());
    }
    check_cancel(cancel)
}
fn read_json(
    out: &Path,
    entry: &ArtifactEntry,
    cancel: &CancelToken,
) -> Result<serde_json::Value, String> {
    serde_json::from_slice(&read_entry(
        &artifact_path(out, &entry.name)?,
        entry,
        cancel,
    )?)
    .map_err(|error| format!("solar_task_batch_artifact_json_invalid: {error}"))
}
pub(super) fn verify_inventory(
    out: &Path,
    declared: &BTreeMap<String, &ArtifactEntry>,
) -> Result<(), String> {
    let mut pending = vec![(out.to_path_buf(), String::new())];
    let mut found = std::collections::BTreeSet::new();
    let mut nodes = 0usize;
    while let Some((directory, logical)) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            nodes += 1;
            if nodes > BATCH_MAX_OUTPUT_FILES * 2 + 16 {
                return Err("solar_task_batch_inventory_bound_exceeded".into());
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "solar_task_batch_inventory_name_invalid")?;
            let relative = if logical.is_empty() {
                name
            } else {
                format!("{logical}/{name}")
            };
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("solar_task_batch_inventory_symlink_invalid".into());
            }
            if metadata.is_dir() {
                if relative != "artifacts"
                    && (!relative.starts_with("artifacts/")
                        || relative.split('/').count() > BATCH_MAX_PATH_DEPTH + 1)
                {
                    return Err("solar_task_batch_inventory_directory_invalid".into());
                }
                pending.push((entry.path(), relative));
            } else if metadata.is_file() {
                if relative == "task-result.json" {
                    continue;
                }
                let name = relative
                    .strip_prefix("artifacts/")
                    .ok_or("solar_task_batch_inventory_undeclared_file")?;
                if !declared.contains_key(name) || !found.insert(name.to_string()) {
                    return Err("solar_task_batch_inventory_undeclared_file".into());
                }
            } else {
                return Err("solar_task_batch_inventory_nonregular".into());
            }
        }
    }
    if found.len() != declared.len() {
        return Err("solar_task_batch_inventory_missing_file".into());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
