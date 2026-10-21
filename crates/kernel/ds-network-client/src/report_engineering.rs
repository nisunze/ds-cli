//! Fixed offline report-engineering task; rendering and transport stay outside.
use super::*;
use ds_network_contracts::report_engineering::*;

#[derive(Debug)]
pub struct ReportEngineeringOutcome {
    pub bytes: Vec<u8>,
    pub receipt: NetworkReceiptV1,
    pub result: ReportEngineeringResultV1,
    pub identity: EngineIdentity,
    pub executable_sha256: String,
    pub development_override: bool,
}

impl CapturedNetworkClient {
    pub fn report_engineering(
        &self,
        request: &ReportEngineeringRequestV1,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ReportEngineeringOutcome, NetworkFailure> {
        validate_report_engineering_request(request)
            .map_err(|e| NetworkFailure::new("report_engineering_invalid", e))?;
        if !self
            .identity
            .capabilities
            .contains(&NetworkCapability::ReportEngineeringV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "selected Network engine has no ReportEngineeringV1 capability",
            ));
        }
        if request.sizing.is_some()
            && !self
                .identity
                .capabilities
                .contains(&NetworkCapability::ReportSizingV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "selected Network engine has no ReportSizingV1 capability",
            ));
        }
        let input = serde_json::to_vec(request)
            .map_err(|e| NetworkFailure::new("report_engineering_invalid", e.to_string()))?;
        if request.feeders.is_some()
            && !self
                .identity
                .capabilities
                .contains(&NetworkCapability::ReportFeederSelectionV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "selected Network engine has no ReportFeederSelectionV1 capability",
            ));
        }
        if input.len() > MAX_REPORT_ENGINEERING_REQUEST_BYTES {
            return Err(NetworkFailure::new(
                "report_engineering_invalid",
                "report request exceeds its admitted input bound",
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
            .prefix("ds-network-report-")
            .tempdir_in(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700))
                .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        }
        let request_path = run.path().join("request.json");
        write_request(&request_path, &input)?;
        let out = run.path().join("output");
        create_output(&out)?;
        let width = self.identity.runtime.internal_worker_threads;
        let args: Vec<OsString> = vec![
            "--request".into(),
            request_path.as_os_str().to_owned(),
            "--out".into(),
            out.as_os_str().to_owned(),
            "--workers".into(),
            width.to_string().into(),
        ];
        let completed = self
            .engine
            .call_cancelled("report-engineering", &args, TASK_TIMEOUT, width, cancel)
            .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
        if completed.truncated || completed.stdout.len() > MAX_IDENTITY_BYTES {
            return Err(NetworkFailure::new(
                "network_receipt_bounded",
                "report task exceeded its terminal receipt bound",
            ));
        }
        if !completed.succeeded() {
            return Err(read_refusal(&completed.stdout, &self.identity)?);
        }
        let receipt: NetworkReceiptV1 = serde_json::from_str(&completed.stdout)
            .map_err(|e| NetworkFailure::new("network_receipt_invalid", e.to_string()))?;
        let input_sha256 = digest_bytes(&input);
        if receipt.schema != "ds.network.task-result/v1"
            || receipt.input_sha256 != input_sha256
            || !same_identity(&receipt.engine_identity, &self.identity)?
            || receipt.result_input_sha256.is_some()
            || receipt.artifact.file != "engineering.json"
            || receipt.artifact.schema != REPORT_ENGINEERING_RESULT_SCHEMA_V1
            || receipt.artifact.byte_count == 0
            || receipt.artifact.byte_count > MAX_REPORT_ENGINEERING_RESULT_BYTES
            || !valid_hex(&receipt.artifact.sha256, 64)
        {
            return Err(NetworkFailure::new(
                "network_receipt_invalid",
                "receipt disagrees with captured report input, engine or fixed artifact",
            ));
        }
        let output_metadata = fs::symlink_metadata(&out)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        if !output_metadata.is_dir() || output_metadata.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "report output directory changed",
            ));
        }
        let outputs = fs::read_dir(&out)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        if outputs.len() != 1 || outputs[0].file_name() != "engineering.json" {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "report output contains undeclared artifacts",
            ));
        }
        let bytes = read_regular(
            &out.join("engineering.json"),
            MAX_REPORT_ENGINEERING_RESULT_BYTES,
            "network_result_invalid",
        )?;
        if bytes.len() != receipt.artifact.byte_count
            || digest_bytes(&bytes) != receipt.artifact.sha256
        {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "report bytes disagree with the owner receipt",
            ));
        }
        let result = decode_report_engineering_result(&bytes, request)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e))?;
        if digest_file(
            &request_path,
            MAX_REPORT_ENGINEERING_REQUEST_BYTES as u64,
            "network_input_changed",
        )? != input_sha256
        {
            return Err(NetworkFailure::new(
                "network_input_changed",
                "captured report request changed during execution",
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
                "captured Network executable changed during report execution",
            ));
        }
        if cancel() || matches!(receipt.outcome, NetworkTaskOutcome::Cancelled) {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "report engineering was cancelled",
            ));
        }
        Ok(ReportEngineeringOutcome {
            bytes,
            receipt,
            result,
            identity: self.identity.clone(),
            executable_sha256: self.executable_sha256.clone(),
            development_override: self.development_override,
        })
    }
}
