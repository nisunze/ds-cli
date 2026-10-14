//! Prepared offline jobs retain their exact producer output subtree.
use super::*;
use ds_network_contracts::{
    design_snapshot::{
        decode_design_snapshot_result, encode_design_snapshot_request,
        DESIGN_SNAPSHOT_OUTPUT_SCHEMA_V1,
    },
    engine::JsonObjectOrder,
    lv::{LvProcessInput, LvProcessOutput},
};
pub type DesignSnapshotEnginePin = NetworkEnginePin;
#[derive(Debug)]
pub struct DesignSnapshotOutcome {
    pub bytes: Vec<u8>,
    pub artifact_bytes: Vec<u8>,
    pub output: LvProcessOutput,
    pub identity: EngineIdentity,
    pub executable_sha256: String,
    pub cancelled: bool,
    pub development_override: bool,
}
fn admit_design_identity(identity: &EngineIdentity) -> Result<(), NetworkFailure> {
    if identity.json_object_order != JsonObjectOrder::Insertion
        || !identity
            .capabilities
            .contains(&NetworkCapability::DesignSnapshotV1)
    {
        return Err(NetworkFailure::new(
            "network_engine_incompatible",
            "Design snapshot requires insertion-order serialization and DesignSnapshotV1",
        ));
    }
    Ok(())
}
pub fn design_snapshot_engine_pin(
    workers: usize,
) -> Result<DesignSnapshotEnginePin, NetworkFailure> {
    let captured = CapturedNetworkClient::capture(workers, &|| false)?;
    admit_design_identity(captured.identity())?;
    Ok(captured.pin())
}
pub fn process_design_snapshot(
    prepared: &LvProcessInput,
    workers: usize,
    scratch: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<DesignSnapshotOutcome, NetworkFailure> {
    let captured = CapturedNetworkClient::capture(workers, cancel)?;
    captured.process_design_snapshot(prepared, scratch, cancel)
}

impl CapturedNetworkClient {
    pub fn process_design_snapshot(
        &self,
        prepared: &LvProcessInput,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<DesignSnapshotOutcome, NetworkFailure> {
        let input = encode_design_snapshot_request(prepared).map_err(NetworkFailure::native)?;
        let engine = &self.engine;
        let identity = &self.identity;
        let executable_sha256 = &self.executable_sha256;
        let development_override = self.development_override;
        admit_design_identity(identity)?;
        let width = identity.runtime.internal_worker_threads;
        let input_sha256 = digest_bytes(&input);
        let metadata = fs::symlink_metadata(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_scratch_invalid",
                "scratch must be an ordinary directory",
            ));
        }
        let run = tempfile::Builder::new()
            .prefix("ds-network-design-")
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
        let output_directory = run.path().join("output");
        create_output(&output_directory)?;
        let args: Vec<OsString> = vec![
            "--request".into(),
            request_path.as_os_str().to_owned(),
            "--out".into(),
            output_directory.as_os_str().to_owned(),
            "--workers".into(),
            width.to_string().into(),
        ];
        let completed = engine
            .call_cancelled("design-snapshot", &args, TASK_TIMEOUT, width, cancel)
            .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
        if completed.truncated || completed.stdout.len() > MAX_IDENTITY_BYTES {
            return Err(NetworkFailure::new(
                "network_receipt_bounded",
                "task exceeded its receipt output bound",
            ));
        }
        if !completed.succeeded() {
            return Err(read_refusal(&completed.stdout, identity)?);
        }
        let receipt: NetworkReceiptV1 = serde_json::from_str(&completed.stdout)
            .map_err(|e| NetworkFailure::new("network_receipt_invalid", e.to_string()))?;
        if receipt.schema != "ds.network.task-result/v1"
            || receipt.input_sha256 != input_sha256
            || !same_identity(&receipt.engine_identity, identity)?
            || receipt.artifact.file != "output.json"
            || receipt.artifact.schema != DESIGN_SNAPSHOT_OUTPUT_SCHEMA_V1
            || receipt.artifact.byte_count == 0
            || receipt.artifact.byte_count > MAX_NATIVE_LV_OUTPUT_BYTES
            || !valid_hex(&receipt.artifact.sha256, 64)
            || receipt.result_input_sha256.is_some()
        {
            return Err(NetworkFailure::new(
                "network_receipt_invalid",
                "receipt disagrees with frozen Design input, identity or fixed artifact",
            ));
        }
        let metadata = fs::symlink_metadata(&output_directory)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "result directory changed",
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(NetworkFailure::new(
                    "network_result_invalid",
                    "result directory became a reparse point",
                ));
            }
        }
        let artifact_bytes = read_regular(
            &output_directory.join("output.json"),
            MAX_NATIVE_LV_OUTPUT_BYTES,
            "network_result_invalid",
        )?;
        if artifact_bytes.len() != receipt.artifact.byte_count
            || digest_bytes(&artifact_bytes) != receipt.artifact.sha256
        {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "result byte count or digest disagrees with receipt",
            ));
        }
        let result = decode_design_snapshot_result(&artifact_bytes, &prepared.transformer_name)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawResult<'a> {
            schema: String,
            transformer_name: String,
            #[serde(borrow)]
            output: &'a serde_json::value::RawValue,
        }
        let raw: RawResult<'_> = serde_json::from_slice(&artifact_bytes)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        if raw.schema != DESIGN_SNAPSHOT_OUTPUT_SCHEMA_V1
            || raw.transformer_name != prepared.transformer_name
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "raw result addresses another schema or transformer",
            ));
        }
        let bytes = raw.output.get().as_bytes().to_vec();
        if digest_file(
            &request_path,
            MAX_NATIVE_LV_INPUT_BYTES as u64,
            "network_input_changed",
        )? != input_sha256
        {
            return Err(NetworkFailure::new(
                "network_input_changed",
                "typed request changed during task",
            ));
        }
        if digest_file(
            engine.executable(),
            MAX_EXECUTABLE_BYTES,
            "network_engine_changed",
        )? != *executable_sha256
        {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "captured executable changed during task",
            ));
        }
        if cancel() || matches!(receipt.outcome, NetworkTaskOutcome::Cancelled) {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Design snapshot task was cancelled",
            ));
        }
        Ok(DesignSnapshotOutcome {
            bytes,
            artifact_bytes,
            output: result.output,
            identity: identity.clone(),
            executable_sha256: executable_sha256.clone(),
            cancelled: false,
            development_override,
        })
    }
}
