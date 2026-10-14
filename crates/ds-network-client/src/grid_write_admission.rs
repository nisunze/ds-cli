//! Verify the captured package guard's exact source and operation before writes.
use super::*;
use ds_network_contracts::grid_write_admission::*;

impl CapturedNetworkClient {
    pub fn grid_write_admission(
        &self,
        request: &NativeGridWriteAdmissionRequestV1,
        model: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<GridOutcome<NativeGridWriteAdmissionResultV1>, NetworkFailure> {
        let input = serde_json::to_vec(request)
            .map_err(|error| NetworkFailure::new("grid_task_input_invalid", error.to_string()))?;
        if input.len() > MAX_GRID_WRITE_ADMISSION_BYTES {
            return Err(NetworkFailure::new(
                "grid_task_bound_refused",
                "write admission request exceeds its bound",
            ));
        }
        let outcome = self.grid_artifact::<NativeGridWriteAdmissionResultV1>(
            "grid-write-admission",
            NetworkCapability::GridWriteAdmissionV1,
            Some(&input),
            Some(model),
            scratch,
            cancel,
            "admission.json",
            GRID_WRITE_ADMISSION_RESULT_SCHEMA_V1,
            MAX_GRID_WRITE_ADMISSION_BYTES,
            None,
            false,
        )?;
        let result = &outcome.result;
        crate::grid::validate_source(&result.source, &request.source, model)?;
        if matches!(&result.verdict, NativeGridWriteAdmissionVerdictV1::Denied { code, message }
            if code != "composite_model_protected" || message.is_empty())
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "write admission has an invalid denial",
            ));
        }
        if result.schema != GRID_WRITE_ADMISSION_RESULT_SCHEMA_V1
            || result.operation != request.operation
            || result.model_schema != 5
            || result.engine_version.is_empty()
            || result.engine_version.len() > 128
            || result.engine_version.chars().any(char::is_control)
        {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "write admission addresses another schema or operation",
            ));
        }
        Ok(outcome)
    }
}
