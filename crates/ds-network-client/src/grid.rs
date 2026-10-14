//! Offline Grid artifact transport. Engineering and package decoding stay in Network.
use super::*;
use ds_network_contracts::grid::*;
use serde::de::DeserializeOwned;
type ProgressObserver<'a> = &'a dyn Fn(&str);

#[derive(Debug)]
pub struct GridOutcome<T> {
    pub bytes: Vec<u8>,
    pub result: T,
    pub receipt: NetworkReceiptV1,
    pub identity: EngineIdentity,
    pub executable_sha256: String,
    pub development_override: bool,
    /// Bounded owning progress lines; these never enter the result digest.
    pub progress: String,
    pub(crate) staging: Option<tempfile::TempDir>,
}

impl CapturedNetworkClient {
    pub fn grid_inspect(
        &self,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeGridInspectionV1>, NetworkFailure> {
        let outcome = self.grid_artifact::<NativeGridInspectionV1>(
            "grid-inspect",
            NetworkCapability::GridInspectV1,
            None,
            Some(model),
            scratch,
            cancel,
            "inspection.json",
            "ds.grid.native-inspection/v1",
            MAX_NATIVE_GRID_REQUEST_BYTES,
            None,
            false,
        )?;
        validate_source(
            &outcome.result.source,
            &NativeGridQuerySourcePin {
                package_sha256: digest_bytes(model),
                expected_revision: None,
                engineering_input_root: None,
            },
            model,
        )?;
        if outcome.result.schema != "ds.grid.native-inspection/v1"
            || outcome.result.model_schema == 0
        {
            return Err(invalid_result("inspection has an incompatible schema"));
        }
        Ok(outcome)
    }

    pub fn grid_query(
        &self,
        request: &NativeGridQueryRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeGridQueryResultV1>, NetworkFailure> {
        self.grid_query_observed(request, model, scratch, cancel, &|_| {})
    }

    /// Forward the owner's validated progress frames during execution.
    pub fn grid_query_observed(
        &self,
        request: &NativeGridQueryRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
        progress: &dyn Fn(&str),
    ) -> Result<GridOutcome<NativeGridQueryResultV1>, NetworkFailure> {
        let input = encode(request, MAX_NATIVE_GRID_QUERY_REQUEST_BYTES)?;
        let outcome = self.grid_artifact::<NativeGridQueryResultV1>(
            "grid-query",
            NetworkCapability::GridQueryV1,
            Some(&input),
            Some(model),
            scratch,
            cancel,
            "query.json",
            NATIVE_GRID_QUERY_RESULT_SCHEMA_V1,
            MAX_NATIVE_GRID_REPORT_BYTES,
            Some(progress),
            false,
        )?;
        let result = &outcome.result;
        validate_source(&result.source, &request.source, model)?;
        if result.schema != NATIVE_GRID_QUERY_RESULT_SCHEMA_V1
            || result
                .operation
                .get("operation_id")
                .and_then(serde_json::Value::as_str)
                != Some(request.operation_id.as_str())
            || !bounded_version(&result.engine_version)
        {
            return Err(invalid_result(
                "query result addresses another schema or operation",
            ));
        }
        Ok(outcome)
    }

    pub fn grid_manifest(
        &self,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeGridManifestInspectionV1>, NetworkFailure> {
        let outcome = self.grid_artifact::<NativeGridManifestInspectionV1>(
            "grid-manifest",
            NetworkCapability::GridManifestV1,
            None,
            Some(model),
            scratch,
            cancel,
            "manifest.json",
            NATIVE_GRID_MANIFEST_SCHEMA_V1,
            MAX_NATIVE_GRID_MANIFEST_BYTES,
            None,
            false,
        )?;
        if outcome.result.schema != NATIVE_GRID_MANIFEST_SCHEMA_V1
            || outcome.result.package_sha256 != digest_bytes(model)
            || outcome.result.decoded
        {
            return Err(invalid_result(
                "manifest result differs from the opaque package or decode contract",
            ));
        }
        Ok(outcome)
    }

    pub fn grid_publication(
        &self,
        request: &NativeGridPublicationRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeGridPublicationFactsV1>, NetworkFailure> {
        let input = encode(request, MAX_NATIVE_GRID_REQUEST_BYTES)?;
        let outcome = self.grid_artifact::<NativeGridPublicationFactsV1>(
            "grid-publication",
            NetworkCapability::GridPublicationV1,
            Some(&input),
            Some(model),
            scratch,
            cancel,
            "publication.json",
            NATIVE_GRID_PUBLICATION_RESULT_SCHEMA_V1,
            MAX_NATIVE_GRID_REPORT_BYTES,
            None,
            false,
        )?;
        let result = &outcome.result;
        validate_source(&result.source, &request.source, model)?;
        if result.schema != NATIVE_GRID_PUBLICATION_RESULT_SCHEMA_V1
            || result
                .source_revision_id
                .strip_prefix("rev:")
                .is_none_or(|s| !valid_hex(s, 64))
            || result.model_schema == 0
            || !bounded_version(&result.engine_version)
        {
            return Err(invalid_result(
                "publication facts have an incompatible schema or revision",
            ));
        }
        // source_revision_id may hash library/cache package bytes instead of
        // authored geometry. The Network owner decides which identity applies.
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn grid_artifact<T: DeserializeOwned>(
        &self,
        task: &'static str,
        capability: NetworkCapability,
        request: Option<&[u8]>,
        model: Option<&[u8]>,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
        artifact_name: &'static str,
        artifact_schema: &'static str,
        maximum: usize,
        progress: Option<ProgressObserver<'_>>,
        retain_staging: bool,
    ) -> Result<GridOutcome<T>, NetworkFailure> {
        if !self.identity.capabilities.contains(&capability) {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network does not admit this Grid capability",
            ));
        }
        if model.is_some_and(|bytes| bytes.len() > MAX_NATIVE_GRID_PACKAGE_BYTES) {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "opaque Grid package exceeds its contract bound",
            ));
        }
        let metadata = fs::symlink_metadata(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_scratch_invalid",
                "scratch must be an ordinary directory",
            ));
        }
        let run = tempfile::Builder::new()
            .prefix("ds-network-grid-")
            .tempdir_in(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700))
                .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        }
        let model_path = run.path().join(if task == "grid-create" {
            "standards.dsgrid-template"
        } else {
            "model.dsgrid"
        });
        if let Some(model) = model {
            write_request(&model_path, model)?;
        }
        let output = run.path().join("output");
        create_output(&output)?;
        let width = self.identity.runtime.internal_worker_threads;
        let mut args: Vec<OsString> = vec![
            "--out".into(),
            output.as_os_str().to_owned(),
            "--workers".into(),
            width.to_string().into(),
        ];
        if model.is_some() {
            args.extend([
                if task == "grid-create" {
                    "--standards"
                } else {
                    "--model"
                }
                .into(),
                model_path.as_os_str().to_owned(),
            ]);
        }
        let request_path = run.path().join("request.json");
        if let Some(input) = request {
            write_request(&request_path, input)?;
            args.extend(["--request".into(), request_path.as_os_str().to_owned()]);
        }
        let input_sha256 = digest_bytes(
            request
                .or(model)
                .ok_or_else(|| invalid_result("task has no captured input"))?,
        );
        let pending = std::cell::RefCell::new(Vec::<u8>::new());
        let invalid_progress = std::cell::Cell::new(false);
        let observe = |chunk: &[u8]| {
            let Some(progress) = progress else { return };
            let mut pending = pending.borrow_mut();
            pending.extend_from_slice(chunk);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let bytes = pending.drain(..=end).collect::<Vec<_>>();
                if bytes.len() > 64 * 1024 {
                    invalid_progress.set(true);
                    continue;
                }
                let line = std::str::from_utf8(&bytes);
                let event = line
                    .ok()
                    .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok());
                if event.as_ref().is_some_and(|event| {
                    event["schema"] == "ds.network.grid-progress/v1"
                        && event["input_sha256"] == input_sha256
                        && event["progress"].is_object()
                }) {
                    progress(line.expect("validated UTF-8"));
                } else {
                    invalid_progress.set(true);
                }
            }
            if pending.len() > 64 * 1024 {
                pending.clear();
                invalid_progress.set(true);
            }
        };
        let completed = self
            .engine
            .call_cancelled_observed(task, &args, TASK_TIMEOUT, width, cancel, &observe)
            .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
        if completed.truncated || completed.stdout.len() > MAX_IDENTITY_BYTES {
            return Err(NetworkFailure::new(
                "network_receipt_bounded",
                "Grid task exceeded its terminal receipt bound",
            ));
        }
        if !completed.succeeded() {
            return Err(read_refusal(&completed.stdout, &self.identity)?);
        }
        if progress.is_some() && (invalid_progress.get() || !pending.borrow().is_empty()) {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "Grid progress did not match the captured request",
            ));
        }
        let receipt: NetworkReceiptV1 = serde_json::from_str(&completed.stdout)
            .map_err(|e| NetworkFailure::new("network_receipt_invalid", e.to_string()))?;
        if receipt.schema != "ds.network.task-result/v1"
            || receipt.input_sha256 != input_sha256
            || !same_identity(&receipt.engine_identity, &self.identity)?
            || receipt.artifact.file != artifact_name
            || receipt.artifact.schema != artifact_schema
            || receipt.artifact.byte_count == 0
            || receipt.artifact.byte_count > maximum
            || !valid_hex(&receipt.artifact.sha256, 64)
            || receipt.result_input_sha256.is_some()
        {
            return Err(NetworkFailure::new(
                "network_receipt_invalid",
                "Grid receipt differs from frozen input, identity or fixed artifact",
            ));
        }
        if matches!(receipt.outcome, NetworkTaskOutcome::Cancelled) || cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Grid task was cancelled",
            ));
        }
        let metadata = fs::symlink_metadata(&output).map_err(|e| invalid_result(e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid_result("result directory changed"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(invalid_result("result directory became a reparse point"));
            }
        }
        let bytes = read_regular(
            &output.join(artifact_name),
            maximum,
            "network_result_invalid",
        )?;
        if bytes.len() != receipt.artifact.byte_count
            || digest_bytes(&bytes) != receipt.artifact.sha256
        {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "Grid artifact byte count or digest differs from receipt",
            ));
        }
        let result = serde_json::from_slice(&bytes).map_err(|e| invalid_result(e.to_string()))?;
        if let Some(model) = model {
            if digest_file(
                &model_path,
                MAX_NATIVE_GRID_PACKAGE_BYTES as u64,
                "network_input_changed",
            )? != digest_bytes(model)
            {
                return Err(NetworkFailure::new(
                    "network_input_changed",
                    "staged Grid inputs changed during task",
                ));
            }
        }
        let request_bound = match task {
            "mv-projection" => ds_network_contracts::mv_projection::MAX_MV_PROJECTION_REQUEST_BYTES,
            "mv-report" => ds_network_contracts::mv_report::MAX_MV_REPORT_REQUEST_BYTES,
            "grid-apply" => ds_network_contracts::grid_apply::MAX_GRID_APPLY_REQUEST_BYTES,
            "grid-catalog" => ds_network_contracts::grid_catalog::MAX_GRID_CATALOG_REQUEST_BYTES,
            "grid-create" => ds_network_contracts::grid_create::MAX_GRID_CREATE_REQUEST_BYTES,
            _ => MAX_NATIVE_GRID_QUERY_REQUEST_BYTES,
        };
        if request.is_some()
            && digest_file(&request_path, request_bound as u64, "network_input_changed")?
                != input_sha256
        {
            return Err(NetworkFailure::new(
                "network_input_changed",
                "staged Grid request changed during task",
            ));
        }
        if digest_file(
            self.engine.executable(),
            MAX_EXECUTABLE_BYTES,
            "network_engine_changed",
        )? != self.executable_sha256
        {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "captured executable changed during Grid task",
            ));
        }
        // Cancellation can arrive during result admission; never import it as success.
        if cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Grid task completed after host cancellation",
            ));
        }
        Ok(GridOutcome {
            bytes,
            result,
            receipt,
            identity: self.identity.clone(),
            executable_sha256: self.executable_sha256.clone(),
            development_override: self.development_override,
            progress: completed.stderr,
            staging: retain_staging.then_some(run),
        })
    }
}

fn encode<T: serde::Serialize>(request: &T, maximum: usize) -> Result<Vec<u8>, NetworkFailure> {
    let bytes = serde_json::to_vec(request)
        .map_err(|e| NetworkFailure::new("network_request_invalid", e.to_string()))?;
    if bytes.len() > maximum {
        return Err(NetworkFailure::new(
            "grid_task_bound_refused",
            "Grid request exceeds its contract bound",
        ));
    }
    Ok(bytes)
}
fn invalid_result(message: impl Into<String>) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}
fn bounded_version(version: &str) -> bool {
    !version.is_empty() && version.len() <= 128 && !version.chars().any(char::is_control)
}
pub(crate) fn validate_source(
    source: &NativeGridSourcePin,
    requested: &NativeGridQuerySourcePin,
    model: &[u8],
) -> Result<(), NetworkFailure> {
    if source.package_sha256 != digest_bytes(model)
        || source.package_sha256 != requested.package_sha256
        || source
            .expected_revision
            .strip_prefix("rev:")
            .is_none_or(|s| !valid_hex(s, 64))
        || source
            .engineering_input_root
            .strip_prefix("fnv1a64:")
            .is_none_or(|s| !valid_hex(s, 16))
        || requested
            .expected_revision
            .as_ref()
            .is_some_and(|s| s != &source.expected_revision)
        || requested
            .engineering_input_root
            .as_ref()
            .is_some_and(|s| s != &source.engineering_input_root)
    {
        return Err(invalid_result(
            "Grid result source differs from frozen package or explicit pins",
        ));
    }
    Ok(())
}
