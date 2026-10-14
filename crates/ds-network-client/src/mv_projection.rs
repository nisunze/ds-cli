//! Captured offline MV projection transport; no package decode or CRS algorithm.
use super::*;
use ds_network_contracts::mv_projection::*;

impl CapturedNetworkClient {
    pub fn mv_projection(
        &self,
        request: &MvProjectionRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<MvProjectionResultV1>, NetworkFailure> {
        validate_request(request)
            .map_err(|error| NetworkFailure::new("mv_projection_request_invalid", error))?;
        if request.json_object_order != self.identity.json_object_order {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "MV projection JSON object order differs from captured consumer profile",
            ));
        }
        let input = serde_json::to_vec(request).map_err(|error| {
            NetworkFailure::new("mv_projection_request_invalid", error.to_string())
        })?;
        if input.len() > MAX_MV_PROJECTION_REQUEST_BYTES
            || model.len() > MAX_MV_PROJECTION_PACKAGE_BYTES
        {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "MV projection input exceeds its bound",
            ));
        }
        let mut outcome = self.grid_artifact::<MvProjectionResultV1>(
            "mv-projection",
            NetworkCapability::MvProjectionV1,
            Some(&input),
            Some(model),
            scratch,
            cancel,
            MV_PROJECTION_ARTIFACT,
            MV_PROJECTION_RESULT_SCHEMA_V1,
            MAX_MV_PROJECTION_RESULT_BYTES,
            None,
            true,
        )?;
        let stage = outcome.staging.as_ref().ok_or_else(|| {
            NetworkFailure::new(
                "network_result_invalid",
                "MV projection output inventory is absent",
            )
        })?;
        let entries = fs::read_dir(stage.path().join("output"))
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?
            .take(2)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?;
        if entries.len() != 1 || entries[0].file_name() != MV_PROJECTION_ARTIFACT {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "MV projection returned an unrequested artifact",
            ));
        }
        let result = &outcome.result;
        super::grid::validate_source(&result.source, &request.source, model)?;
        if result.schema != MV_PROJECTION_RESULT_SCHEMA_V1
            || result.json_object_order != request.json_object_order
            || result.revision_id != request.revision_id
            || result.layers != request.layers
            || result.target_crs != "OGC:CRS84"
            || result.source_crs.is_empty()
            || result.source_crs.len() > 128
            || result.source_crs.chars().any(char::is_control)
            || result.projection["type"] != "FeatureCollection"
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "MV projection result differs from captured source, filters or CRS contract",
            ));
        }
        let features = result.projection["features"].as_array().ok_or_else(|| {
            NetworkFailure::new(
                "network_result_invalid",
                "MV projection has no feature array",
            )
        })?;
        if features.len() > MAX_MV_PROJECTION_FEATURES
            || features.iter().any(|feature| {
                feature["type"] != "Feature"
                    || feature["geometry"].is_null()
                    || !feature["properties"]["_layer"]
                        .as_str()
                        .is_some_and(|name| {
                            request.layers.iter().any(|layer| layer.as_str() == name)
                        })
            })
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "MV projection contains unrequested or invalid features",
            ));
        }
        drop(outcome.staging.take());
        Ok(outcome)
    }
}
