//! Intentional discovery from the same captured executable used by Grid tasks.
use super::*;
use ds_network_contracts::grid_catalog::*;

impl CapturedNetworkClient {
    pub fn grid_catalog(
        &self,
        request: &NativeGridCatalogRequestV1,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeGridCatalogResultV1>, NetworkFailure> {
        let input = serde_json::to_vec(request)
            .map_err(|e| NetworkFailure::new("grid_catalog_request_invalid", e.to_string()))?;
        if input.len() > MAX_GRID_CATALOG_REQUEST_BYTES {
            return Err(NetworkFailure::new(
                "grid_catalog_request_invalid",
                "catalog request exceeds its transport bound",
            ));
        }
        let outcome = self.grid_artifact::<NativeGridCatalogResultV1>(
            "grid-catalog",
            NetworkCapability::GridCatalogV1,
            Some(&input),
            None,
            scratch,
            cancel,
            "catalog.json",
            GRID_CATALOG_RESULT_SCHEMA_V1,
            MAX_GRID_CATALOG_RESULT_BYTES,
            None,
            false,
        )?;
        let result = &outcome.result;
        let data = &result.data;
        if result.schema != GRID_CATALOG_RESULT_SCHEMA_V1
            || result.kind != request.kind
            || result.engine_version.is_empty()
            || result.engine_version.len() > 128
            || result.engine_version.chars().any(char::is_control)
            || data["engine"].as_str() != Some(result.engine_version.as_str())
            || data["kind"].as_str() != Some(request.kind.as_str())
            || match &request.id {
                Some(id) => {
                    identifier(&data["descriptor"]) != Some(id.as_str())
                        || data.get("entries").is_some()
                }
                None => !data["entries"].is_array() || data.get("descriptor").is_some(),
            }
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "catalog differs from the captured kind or descriptor",
            ));
        }
        Ok(outcome)
    }
}

fn identifier(entry: &serde_json::Value) -> Option<&str> {
    ["operation_id", "command_id", "projection_id", "id"]
        .into_iter()
        .find_map(|key| entry[key].as_str())
}
