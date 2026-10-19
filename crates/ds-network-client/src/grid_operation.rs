//! Closed Grid file operations: stage exact admitted bytes privately, run the
//! captured owner once and admit only its complete digest-bound artifacts.
use super::*;
use ds_network_contracts::{
    grid::MAX_NATIVE_GRID_PACKAGE_BYTES,
    grid_operation::{
        valid_staging_name, GridOperationInputV1, GridOperationV1, NativeGridOperationRequestV1,
        NativeGridOperationResultV1, GRID_OPERATION_REQUEST_SCHEMA_V1,
        GRID_OPERATION_RESULT_ARTIFACT, GRID_OPERATION_RESULT_SCHEMA_V1,
        MAX_GRID_OPERATION_ARTIFACTS, MAX_GRID_OPERATION_ARTIFACT_BYTES, MAX_GRID_OPERATION_INPUTS,
        MAX_GRID_OPERATION_INPUT_BYTES, MAX_GRID_OPERATION_REQUEST_BYTES,
        MAX_GRID_OPERATION_RESULT_BYTES,
    },
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct GridOperationOutcome {
    pub result: NativeGridOperationResultV1,
    /// Every declared artifact, verified against the result's receipts.
    pub artifacts: BTreeMap<String, Vec<u8>>,
    pub identity: EngineIdentity,
    pub executable_sha256: String,
    pub development_override: bool,
}

impl GridOperationOutcome {
    /// Remove one declared artifact's verified bytes.
    pub fn take_artifact(&mut self, name: &str) -> Option<Vec<u8>> {
        self.artifacts.remove(name)
    }
}

fn invalid(message: impl Into<String>) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}

impl CapturedNetworkClient {
    /// Run one closed operation. `inputs` are `(caller label, bytes)` in the
    /// caller's exact order; labels never name host paths.
    pub fn grid_operation(
        &self,
        call: &GridOperationV1,
        model: Option<&[u8]>,
        inputs: &[(String, Vec<u8>)],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOperationOutcome, NetworkFailure> {
        // An older or narrower engine refuses before anything is staged.
        if !self.identity.capabilities.contains(&call.capability()) {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network does not admit this Grid file operation",
            ));
        }
        if call.reads_package() != model.is_some() {
            return Err(NetworkFailure::new(
                "grid_task_input_invalid",
                "the staged package does not match the operation's declared input",
            ));
        }
        if model.is_some_and(|bytes| bytes.len() > MAX_NATIVE_GRID_PACKAGE_BYTES) {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "opaque Grid package exceeds its contract bound",
            ));
        }
        let total = inputs
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .fold(0u64, u64::saturating_add);
        if inputs.len() > MAX_GRID_OPERATION_INPUTS || total > MAX_GRID_OPERATION_INPUT_BYTES {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "staged operation inputs exceed their contract bound",
            ));
        }
        let request = NativeGridOperationRequestV1 {
            schema: GRID_OPERATION_REQUEST_SCHEMA_V1.into(),
            call: call.clone(),
            package_sha256: model.map(digest_bytes),
            inputs: inputs
                .iter()
                .enumerate()
                .map(|(index, (name, bytes))| GridOperationInputV1 {
                    name: name.clone(),
                    file: format!("input-{index:04}"),
                    sha256: digest_bytes(bytes),
                    byte_count: bytes.len() as u64,
                })
                .collect(),
        };
        let request_bytes = serde_json::to_vec(&request)
            .map_err(|e| NetworkFailure::new("network_request_invalid", e.to_string()))?;
        if request_bytes.len() > MAX_GRID_OPERATION_REQUEST_BYTES {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "Grid operation request exceeds its contract bound",
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
            .prefix("ds-network-op-")
            .tempdir_in(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700))
                .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        }
        let request_path = run.path().join("request.json");
        write_request(&request_path, &request_bytes)?;
        let width = self.identity.runtime.internal_worker_threads;
        let output = run.path().join("output");
        create_output(&output)?;
        let mut args: Vec<OsString> = vec![
            "--request".into(),
            request_path.as_os_str().to_owned(),
            "--out".into(),
            output.as_os_str().to_owned(),
            "--workers".into(),
            width.to_string().into(),
        ];
        let model_path = run.path().join("model.dsgrid");
        if let Some(model) = model {
            write_request(&model_path, model)?;
            args.extend(["--model".into(), model_path.as_os_str().to_owned()]);
        }
        let inputs_dir = run.path().join("inputs");
        if !inputs.is_empty() {
            create_output(&inputs_dir)?;
            for (staged, (_, bytes)) in request.inputs.iter().zip(inputs) {
                write_request(&inputs_dir.join(&staged.file), bytes)?;
            }
            args.extend(["--inputs".into(), inputs_dir.as_os_str().to_owned()]);
        }
        let completed = self
            .engine
            .call_cancelled("grid-operation", &args, TASK_TIMEOUT, width, cancel)
            .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
        if completed.truncated || completed.stdout.len() > MAX_IDENTITY_BYTES {
            return Err(NetworkFailure::new(
                "network_receipt_bounded",
                "Grid operation exceeded its terminal receipt bound",
            ));
        }
        if !completed.succeeded() {
            return Err(read_refusal(&completed.stdout, &self.identity)?);
        }
        let input_sha256 = digest_bytes(&request_bytes);
        let receipt: NetworkReceiptV1 = serde_json::from_str(&completed.stdout)
            .map_err(|e| NetworkFailure::new("network_receipt_invalid", e.to_string()))?;
        if receipt.schema != "ds.network.task-result/v1"
            || receipt.input_sha256 != input_sha256
            || !same_identity(&receipt.engine_identity, &self.identity)?
            || receipt.artifact.file != GRID_OPERATION_RESULT_ARTIFACT
            || receipt.artifact.schema != GRID_OPERATION_RESULT_SCHEMA_V1
            || receipt.artifact.byte_count == 0
            || receipt.artifact.byte_count > MAX_GRID_OPERATION_RESULT_BYTES
            || !valid_hex(&receipt.artifact.sha256, 64)
            || receipt.result_input_sha256.is_some()
        {
            return Err(NetworkFailure::new(
                "network_receipt_invalid",
                "Grid operation receipt differs from frozen input, identity or fixed artifact",
            ));
        }
        if matches!(receipt.outcome, NetworkTaskOutcome::Cancelled) || cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Grid operation was cancelled",
            ));
        }
        let metadata = fs::symlink_metadata(&output).map_err(|e| invalid(e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("result directory changed"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(invalid("result directory became a reparse point"));
            }
        }
        let bytes = read_regular(
            &output.join(GRID_OPERATION_RESULT_ARTIFACT),
            MAX_GRID_OPERATION_RESULT_BYTES,
            "network_result_invalid",
        )?;
        if bytes.len() != receipt.artifact.byte_count
            || digest_bytes(&bytes) != receipt.artifact.sha256
        {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "Grid operation result differs from its receipt",
            ));
        }
        let result: NativeGridOperationResultV1 =
            serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        if result.schema != GRID_OPERATION_RESULT_SCHEMA_V1
            || result.operation != call.id()
            || result.package_sha256 != request.package_sha256
            || result.engine_version.is_empty()
            || result.engine_version.len() > 128
            || result.engine_version.chars().any(char::is_control)
            || result.artifacts.len() > MAX_GRID_OPERATION_ARTIFACTS
        {
            return Err(invalid(
                "Grid operation result addresses another operation, package or bound",
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        names.insert(GRID_OPERATION_RESULT_ARTIFACT.to_owned());
        let mut artifacts = BTreeMap::new();
        for artifact in &result.artifacts {
            if !valid_staging_name(&artifact.file)
                || !names.insert(artifact.file.clone())
                || artifact.byte_count > MAX_GRID_OPERATION_ARTIFACT_BYTES
                || !valid_hex(&artifact.sha256, 64)
            {
                return Err(invalid("Grid operation declared an invalid artifact"));
            }
            let bytes = read_regular(
                &output.join(&artifact.file),
                MAX_GRID_OPERATION_ARTIFACT_BYTES,
                "network_result_invalid",
            )?;
            if bytes.len() != artifact.byte_count || digest_bytes(&bytes) != artifact.sha256 {
                return Err(NetworkFailure::new(
                    "network_result_digest_mismatch",
                    "a Grid operation artifact differs from its declared digest",
                ));
            }
            artifacts.insert(artifact.file.clone(), bytes);
        }
        let present = fs::read_dir(&output)
            .map_err(|e| invalid(e.to_string()))?
            .map(|entry| {
                entry
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .map_err(|e| invalid(e.to_string()))
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        if present != names {
            return Err(invalid("Grid operation output holds undeclared files"));
        }
        if digest_file(
            &request_path,
            MAX_GRID_OPERATION_REQUEST_BYTES as u64,
            "network_input_changed",
        )? != input_sha256
            || model.is_some_and(|model| {
                digest_file(
                    &model_path,
                    MAX_NATIVE_GRID_PACKAGE_BYTES as u64,
                    "network_input_changed",
                )
                .map_or(true, |digest| digest != digest_bytes(model))
            })
        {
            return Err(NetworkFailure::new(
                "network_input_changed",
                "staged Grid operation inputs changed during the task",
            ));
        }
        for staged in &request.inputs {
            if digest_file(
                &inputs_dir.join(&staged.file),
                staged.byte_count,
                "network_input_changed",
            )? != staged.sha256
            {
                return Err(NetworkFailure::new(
                    "network_input_changed",
                    "a staged Grid operation input changed during the task",
                ));
            }
        }
        if digest_file(
            self.engine.executable(),
            MAX_EXECUTABLE_BYTES,
            "network_engine_changed",
        )? != self.executable_sha256
        {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "captured executable changed during the Grid operation",
            ));
        }
        if cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Grid operation completed after host cancellation",
            ));
        }
        Ok(GridOperationOutcome {
            result,
            artifacts,
            identity: self.identity.clone(),
            executable_sha256: self.executable_sha256.clone(),
            development_override: self.development_override,
        })
    }
}
