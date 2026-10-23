//! Full package emission is admitted before the host sees any writable bytes.
use super::*;
use ds_network_contracts::{grid::*, grid_apply_batch::*};

#[derive(Debug)]
pub struct GridApplyBatchOutcome {
    pub metadata: GridOutcome<NativeGridApplyBatchResultV1>,
    pub model: Option<Vec<u8>>,
}
impl CapturedNetworkClient {
    pub fn grid_apply_batch(
        &self,
        request: &NativeGridApplyBatchRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridApplyBatchOutcome, NetworkFailure> {
        let input = serde_json::to_vec(request).map_err(|error| invalid(error.to_string()))?;
        if input.len() > MAX_GRID_APPLY_BATCH_REQUEST_BYTES {
            return Err(invalid("prepared Grid batch request exceeds its bound"));
        }
        let mut metadata = self.grid_artifact::<NativeGridApplyBatchResultV1>(
            "grid-apply-batch",
            NetworkCapability::GridApplyBatchV1,
            Some(&input),
            Some(model),
            scratch,
            cancel,
            "result.json",
            GRID_APPLY_BATCH_RESULT_SCHEMA_V1,
            MAX_NATIVE_GRID_REPORT_BYTES,
            None,
            true,
        )?;
        let result = &metadata.result;
        let projection = &result.projection;
        let ids: Vec<_> = request
            .commands
            .iter()
            .map(|row| row.command_id.as_str())
            .collect();
        let mut operations = std::collections::BTreeMap::<String, usize>::new();
        for row in &request.commands {
            let kind = row.command["command_kind"]
                .as_str()
                .ok_or_else(|| invalid("command kind is absent"))?;
            *operations.entry(kind.into()).or_default() += 1;
        }
        if !crate::same_family(&result.schema, GRID_APPLY_BATCH_RESULT_SCHEMA_V1)
            || result.source.package_sha256 != digest_bytes(model)
            || result.source.package_sha256 != request.source.package_sha256
            || result.source.model_id != request.source.model_id
            || result.source.expected_revision != request.source.expected_revision
            || result.source.engineering_input_root != request.source.engineering_input_root
            || !revision(&result.result_revision)
            || !root(&result.result_engineering_input_root)
            || result.model_schema != 5
            || result.engine_version.is_empty()
            || result.engine_version.len() > 128
            || result.engine_version.chars().any(char::is_control)
            || projection["source"]["model_id"] != result.source.model_id.as_str()
            || projection["source"]["authored_revision"] != result.source.expected_revision
            || !projection["source"]["package_revision"].is_u64()
            || projection["selected_command_ids"] != serde_json::json!(ids)
            || projection["commands"].as_u64() != Some(request.commands.len() as u64)
            || projection["operations"] != serde_json::json!(operations)
            || projection["batch_sha256"] != request.batch_sha256
            || projection["source_sha256"] != format!("sha256:{}", result.source.package_sha256)
            || projection["parent_revision"] != request.source.expected_revision
            || projection["would_apply"] != true
            || !projection["idempotent_replay"].is_boolean()
            || !projection["package_revision_increment"].is_u64()
            || projection["dry_run"] != request.dry_run
            || projection["persisted"] != false
            || projection["resulting_revision"] != result.result_revision
            || !result.journal["history"].is_object()
            || !result.journal["entries"].is_array()
        {
            return Err(invalid(
                "Grid batch facts differ from the frozen source, selected commands or mutation projection",
            ));
        }
        match &request.guard {
            None if projection.get("correction_guard").is_some()
                || projection.get("selected_reviews").is_some()
                || projection.get("changes").is_some() =>
            {
                return Err(invalid("unguarded batch returned a correction receipt"));
            }
            Some(guard) => {
                let reviews: Vec<_> = request
                    .commands
                    .iter()
                    .map(|row| {
                        serde_json::json!({
                            "command_id": row.command_id, "review_ref": row.review_ref,
                            "command_kind": row.command["command_kind"],
                        })
                    })
                    .collect();
                let mut expected = guard.document.clone();
                let expected = expected
                    .as_object_mut()
                    .ok_or_else(|| invalid("guard is not an object"))?;
                expected.remove("source_sha256");
                expected.insert("sha256".into(), serde_json::json!(guard.sha256));
                if projection["correction_guard"] != serde_json::json!(expected)
                    || projection["selected_reviews"] != serde_json::json!(reviews)
                    || !projection["changes"]["tables"].is_array()
                    || !projection["changes"]["added"].is_u64()
                    || !projection["changes"]["removed"].is_u64()
                    || !projection["changes"]["changed"].is_u64()
                {
                    return Err(invalid(
                        "correction guard or ordered review projection differs",
                    ));
                }
            }
            None => {}
        }
        let staging = metadata
            .staging
            .take()
            .ok_or_else(|| invalid("private apply staging was not retained"))?;
        let output = staging.path().join("output");
        let files = fs::read_dir(&output)
            .map_err(|error| invalid(error.to_string()))?
            .count();
        let model = if request.dry_run {
            if !result.artifacts.is_empty() || files != 1 || projection.get("artifact").is_some() {
                return Err(invalid("a preview published writable model bytes"));
            }
            None
        } else {
            if result.artifacts.len() != 1 || files != 2 {
                return Err(invalid("complete apply artifact roster differs"));
            }
            let artifact = &result.artifacts[0];
            if artifact.file != "model.dsgrid"
                || artifact.schema != "ds.grid/v5"
                || artifact.byte_count == 0
                || artifact.byte_count > MAX_NATIVE_GRID_PACKAGE_BYTES
                || !valid_hex(&artifact.sha256, 64)
            {
                return Err(invalid(
                    "apply model artifact has an incompatible fixed identity",
                ));
            }
            let bytes = read_regular(
                &output.join("model.dsgrid"),
                MAX_NATIVE_GRID_PACKAGE_BYTES,
                "network_result_invalid",
            )?;
            if bytes.len() != artifact.byte_count
                || digest_bytes(&bytes) != artifact.sha256
                || projection["artifact"]["byte_len"].as_u64() != Some(bytes.len() as u64)
                || projection["artifact"]["sha256"] != format!("sha256:{}", artifact.sha256)
                || !projection["artifact"]["package_revision"].is_u64()
            {
                return Err(NetworkFailure::new(
                    "network_result_digest_mismatch",
                    "complete model differs from its receipt or result projection",
                ));
            }
            Some(bytes)
        };
        if cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Grid batch completed after host cancellation",
            ));
        }
        Ok(GridApplyBatchOutcome { metadata, model })
    }
}
fn invalid(message: impl Into<String>) -> NetworkFailure {
    NetworkFailure::new("network_result_invalid", message)
}
fn revision(value: &str) -> bool {
    value
        .strip_prefix("rev:")
        .is_some_and(|value| valid_hex(value, 64))
}
fn root(value: &str) -> bool {
    value
        .strip_prefix("fnv1a64:")
        .is_some_and(|value| valid_hex(value, 16))
}
