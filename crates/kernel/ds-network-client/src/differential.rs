//! Captured, offline LV differential planning; Network retains every decision.
use super::*;
use ds_network_contracts::differential::*;

impl CapturedNetworkClient {
    pub fn lv_differential_plan(
        &self,
        request: &DifferentialPlanRequest,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeDifferentialPlanResultV1>, NetworkFailure> {
        let input = serde_json::to_vec(request)
            .map_err(|error| NetworkFailure::new("network_request_invalid", error.to_string()))?;
        self.lv_differential_plan_bytes(&input, scratch, cancel)
    }

    /// Preserve the admitted JSON representation. Numeric normalization can
    /// expand a valid 64 MiB document; the transport must not narrow it.
    pub fn lv_differential_plan_bytes(
        &self,
        request: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeDifferentialPlanResultV1>, NetworkFailure> {
        if request.len() > MAX_DIFFERENTIAL_PLAN_REQUEST_BYTES - 4096 {
            return Err(NetworkFailure::new(
                "network_request_invalid",
                "differential request exceeds its transport bound",
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(request)
            .map_err(|error| NetworkFailure::new("network_request_invalid", error.to_string()))?;
        serde_json::from_value::<DifferentialPlanRequest>(value)
            .map_err(|error| NetworkFailure::new("network_request_invalid", error.to_string()))?;
        #[derive(serde::Serialize)]
        struct Envelope<'a> {
            schema: &'static str,
            request: &'a serde_json::value::RawValue,
        }
        let text = std::str::from_utf8(request)
            .map_err(|error| NetworkFailure::new("network_request_invalid", error.to_string()))?;
        let raw = serde_json::value::RawValue::from_string(text.to_owned())
            .map_err(|error| NetworkFailure::new("network_request_invalid", error.to_string()))?;
        let input = serde_json::to_vec(&Envelope {
            schema: DIFFERENTIAL_PLAN_REQUEST_SCHEMA_V1,
            request: &raw,
        })
        .map_err(|error| NetworkFailure::new("network_request_invalid", error.to_string()))?;
        if input.len() > MAX_DIFFERENTIAL_PLAN_REQUEST_BYTES {
            return Err(NetworkFailure::new(
                "network_request_invalid",
                "differential wrapper exceeds its transport bound",
            ));
        }
        let outcome = self.grid_artifact::<NativeDifferentialPlanResultV1>(
            "lv-differential-plan",
            NetworkCapability::LvDifferentialPlanV1,
            Some(&input),
            None,
            scratch,
            cancel,
            "plan.json",
            DIFFERENTIAL_PLAN_RESULT_SCHEMA_V1,
            MAX_DIFFERENTIAL_PLAN_RESULT_BYTES,
            None,
            false,
        )?;
        let plan = &outcome.result.plan;
        let shape = match plan.scope.as_str() {
            "feeders" => plan.reason_key == "differential_narrowed" && !plan.feeders.is_empty(),
            "full" => {
                plan.feeders.is_empty()
                    && plan.selected_count == 0
                    && plan.frozen_count == 0
                    && matches!(
                        plan.reason_key.as_str(),
                        "differential_full_mv_session"
                            | "differential_full_toggle_off"
                            | "differential_full_edit_context_reloaded"
                            | "differential_full_commit_forced"
                            | "differential_full_no_changes"
                            | "differential_full_blocking_diagnostics"
                            | "differential_full_changes_unmapped"
                            | "differential_full_no_feeders"
                    )
            }
            _ => false,
        };
        if outcome.result.schema != DIFFERENTIAL_PLAN_RESULT_SCHEMA_V1 || !shape {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "differential plan has an invalid schema or vocabulary",
            ));
        }
        Ok(outcome)
    }
}
