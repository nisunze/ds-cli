//! Verify complete blank-model bytes from the captured owner before local writes.
use super::*;
use ds_network_contracts::{grid::MAX_NATIVE_GRID_PACKAGE_BYTES, grid_create::*};

#[derive(Debug)]
pub struct GridCreateOutcome {
    pub metadata: GridOutcome<NativeGridCreateResultV1>,
    pub model: Vec<u8>,
}

impl CapturedNetworkClient {
    pub fn grid_create(
        &self,
        request: &NativeGridCreateRequestV1,
        standards: Option<&[u8]>,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridCreateOutcome, NetworkFailure> {
        let input = serde_json::to_vec(request).map_err(|error| invalid(error.to_string()))?;
        if input.len() > MAX_GRID_CREATE_REQUEST_BYTES
            || standards.is_some_and(|bytes| bytes.len() > MAX_GRID_CREATE_STANDARDS_BYTES)
            || standards.map(digest_bytes) != request.standards_sha256
        {
            return Err(NetworkFailure::new(
                "grid_create_request_invalid",
                "blank model input bound or standards digest differs",
            ));
        }
        let mut metadata = self.grid_artifact::<NativeGridCreateResultV1>(
            "grid-create",
            NetworkCapability::GridCreateV1,
            Some(&input),
            Some(standards.unwrap_or_default()),
            scratch,
            cancel,
            "result.json",
            GRID_CREATE_RESULT_SCHEMA_V1,
            MAX_GRID_CREATE_RESULT_BYTES,
            None,
            true,
        )?;
        let result = &metadata.result;
        let artifact = &result.artifact;
        if !crate::same_family(&result.schema, GRID_CREATE_RESULT_SCHEMA_V1)
            || result.engine_version.is_empty()
            || result.engine_version.len() > 128
            || result.engine_version.chars().any(char::is_control)
            || result.model_id.is_empty()
            || request
                .model_id
                .as_ref()
                .is_some_and(|id| id != &result.model_id)
            || result.coordinate_system.is_empty()
            || result.coordinate_system.chars().any(char::is_control)
            || result.model_schema != 5
            || result.package_revision != 0
            || !prefixed_hash(&result.authored_revision, "rev:", 64)
            || !prefixed_hash(&result.snapshot_fingerprint, "fnv1a64:", 16)
            || result.standards_sha256 != request.standards_sha256
            || artifact.file != "model.dsgrid"
            || artifact.schema != "ds.grid/v5"
            || artifact.byte_count == 0
            || artifact.byte_count > MAX_NATIVE_GRID_PACKAGE_BYTES
            || !valid_hex(&artifact.sha256, 64)
        {
            return Err(invalid(
                "blank model facts differ from the captured request or schema",
            ));
        }
        let staging = metadata
            .staging
            .take()
            .ok_or_else(|| invalid("create staging missing"))?;
        let output = staging.path().join("output");
        if fs::read_dir(&output)
            .map_err(|error| invalid(error.to_string()))?
            .count()
            != 2
        {
            return Err(invalid("blank model artifact roster differs"));
        }
        let model = read_regular(
            &output.join("model.dsgrid"),
            MAX_NATIVE_GRID_PACKAGE_BYTES,
            "network_result_invalid",
        )?;
        if model.len() != artifact.byte_count || digest_bytes(&model) != artifact.sha256 {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "blank model bytes differ from their artifact receipt",
            ));
        }
        if cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "blank model creation cancelled",
            ));
        }
        Ok(GridCreateOutcome { metadata, model })
    }
}

fn prefixed_hash(value: &str, prefix: &str, digits: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|hash| valid_hex(hash, digits))
}
fn invalid(message: impl Into<String>) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}
