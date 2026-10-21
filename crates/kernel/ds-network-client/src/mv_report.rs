//! Captured report projection transport; no model decode or staking calculation.
use super::*;
use ds_network_contracts::mv_report::*;
use serde_json::Value;

impl CapturedNetworkClient {
    pub fn mv_report(
        &self,
        request: &MvReportRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<MvReportResultV1>, NetworkFailure> {
        validate_request(request)
            .map_err(|error| NetworkFailure::new("mv_report_request_invalid", error))?;
        if request.json_object_order != self.identity.json_object_order {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "MV report JSON object order differs from captured consumer profile",
            ));
        }
        let input = serde_json::to_vec(request)
            .map_err(|error| NetworkFailure::new("mv_report_request_invalid", error.to_string()))?;
        if input.len() > MAX_MV_REPORT_REQUEST_BYTES || model.len() > MAX_MV_REPORT_PACKAGE_BYTES {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "MV report input exceeds its bound",
            ));
        }
        let mut outcome = self.grid_artifact::<MvReportResultV1>(
            "mv-report",
            NetworkCapability::MvReportV1,
            Some(&input),
            Some(model),
            scratch,
            cancel,
            MV_REPORT_ARTIFACT,
            MV_REPORT_RESULT_SCHEMA_V1,
            MAX_MV_REPORT_RESULT_BYTES,
            None,
            true,
        )?;
        let stage = outcome.staging.as_ref().ok_or_else(|| {
            NetworkFailure::new(
                "network_result_invalid",
                "MV report output inventory is absent",
            )
        })?;
        let entries = fs::read_dir(stage.path().join("output"))
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?
            .take(2)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| NetworkFailure::new("network_result_invalid", error.to_string()))?;
        if entries.len() != 1 || entries[0].file_name() != MV_REPORT_ARTIFACT {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "MV report returned an unrequested artifact",
            ));
        }
        let result = &outcome.result;
        super::grid::validate_source(&result.source, &request.source, model)?;
        if result.schema != MV_REPORT_RESULT_SCHEMA_V1
            || result.json_object_order != request.json_object_order
            || result.source_crs.is_empty()
            || result.source_crs.len() > 128
            || result.source_crs.chars().any(char::is_control)
            || result.projection["type"] != "FeatureCollection"
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "MV report result differs from captured source or contract",
            ));
        }
        let features = result.projection["features"].as_array().ok_or_else(|| {
            NetworkFailure::new("network_result_invalid", "MV report has no feature array")
        })?;
        let table = result.staking_table.as_object().ok_or_else(|| {
            NetworkFailure::new("network_result_invalid", "MV report has no staking table")
        })?;
        let columns = table
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                NetworkFailure::new("network_result_invalid", "MV report has no staking columns")
            })?;
        let rows = table.get("rows").and_then(Value::as_array).ok_or_else(|| {
            NetworkFailure::new("network_result_invalid", "MV report has no staking rows")
        })?;
        if features.len() > MAX_MV_REPORT_FEATURES
            || features.iter().any(|feature| {
                feature["type"] != "Feature" || feature["properties"]["_layer"].as_str().is_none()
            })
            || rows.len() > MAX_MV_REPORT_ROWS
            || columns.len() > 4096
            || columns.iter().any(|column| {
                column.as_array().is_none_or(|pair| {
                    pair.len() != 2 || pair.iter().any(|value| !value.is_string())
                })
            })
            || rows.iter().any(|row| {
                row.as_array()
                    .is_none_or(|values| values.len() != columns.len())
            })
            || table
                .get("sum_row")
                .and_then(Value::as_array)
                .is_none_or(|sum| sum.len() != columns.len())
            || table
                .get("warnings")
                .and_then(Value::as_array)
                .is_none_or(|warnings| warnings.iter().any(|value| !value.is_string()))
            || table
                .get("sheet_name")
                .is_none_or(|name| !name.is_null() && !name.is_string())
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "MV report contains invalid features or staking wire shape",
            ));
        }
        drop(outcome.staging.take());
        Ok(outcome)
    }
}
