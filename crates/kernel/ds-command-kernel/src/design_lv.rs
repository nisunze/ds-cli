//! The application boundary for the existing `design.lv.process` command.
//!
//! Capture is synchronous: one explicit project, ordered transformer identities
//! and immutable serialized input. Hosts retain authentication and file IO;
//! Network receives its existing project-free request and owns all engineering.
use ds_core_contracts::{CommandEnvelope, CommandRefusal, Failure};
pub use ds_network_contracts::lv::{LvProcessBatchInput, LvProcessBatchOutput};
use ds_network_contracts::lv_processing::{
    lv_batch_output_from_native, native_lv_request_from_batch, NativeLvError, NativeLvResultV1,
    MAX_NATIVE_LV_FEATURES, MAX_NATIVE_LV_INPUT_BYTES, MAX_NATIVE_LV_OUTPUT_BYTES,
    NATIVE_LV_RESULT_SCHEMA_V1,
};
use serde_json::Value;

pub const COMMAND: &str = "design.lv.process";

/// An application request captured from actual authored bytes. This is neither
/// authorization nor a persisted design revision; its private fields cannot be
/// replaced while an asynchronous host call is running.
#[derive(Clone, Debug)]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    wasm_bindgen::prelude::wasm_bindgen
)]
pub struct CapturedLvProcess {
    project: String,
    transformer_names: Vec<String>,
    input: Vec<u8>,
}

fn invalid(message: &'static str) -> CommandRefusal {
    CommandRefusal::from_failure(&Failure::invalid("fast_lv_input_invalid", message))
}
fn incompatible(message: &'static str) -> CommandRefusal {
    CommandRefusal::from_failure(
        &Failure::unavailable("network_result_invalid", message)
            .remedy("Retain the unchanged input and use a compatible Network component."),
    )
}
fn native_fault(error: NativeLvError) -> CommandRefusal {
    CommandRefusal::from_failure(&Failure::invalid(error.code(), error.to_string()))
}

pub fn capture(
    project: &str,
    input: LvProcessBatchInput,
) -> Result<CapturedLvProcess, CommandRefusal> {
    if !crate::execution_context::valid_project(project) {
        return Err(invalid(
            "LV processing requires one exact captured DS project.",
        ));
    }
    if input.jobs.iter().any(|job| job.ds_project != project) {
        return Err(invalid(
            "Every job of a page LV batch belongs to --project.",
        ));
    }
    // The existing owner admits names, uniqueness, settings, geometry and source
    // table bounds. No application adapter reproduces those engineering rules.
    native_lv_request_from_batch(input.clone()).map_err(native_fault)?;
    let bytes =
        serde_json::to_vec(&input).map_err(|_| invalid("The page LV batch cannot be encoded."))?;
    if bytes.len() > MAX_NATIVE_LV_INPUT_BYTES {
        return Err(native_fault(NativeLvError::InputTooLarge {
            actual: bytes.len(),
            maximum: MAX_NATIVE_LV_INPUT_BYTES,
        }));
    }
    Ok(CapturedLvProcess {
        project: project.to_owned(),
        transformer_names: input
            .jobs
            .into_iter()
            .map(|job| job.transformer_name)
            .collect(),
        input: bytes,
    })
}

/// The application executor that runs every LV batch: installed DS Core,
/// which runs the separately released Network engine executable.
pub const CORE_PROCESSOR: &str = "ds-core";
/// The stored process-path marker (`process_metadata.lane`): data other
/// consumers read; never read back here for a decision.
pub const PROCESSING_LANE: &str = "fast";

/// Who executed an LV batch, read from the executor's own receipt (Core's
/// `design.lv.process` command data). Additive receipt fields are ignored; a
/// missing optional member stays absent. No host stamps or infers it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct LvProcessProvenance {
    pub processor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_core_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_sha256: Option<String>,
    #[serde(default)]
    pub development_override: bool,
}

impl LvProcessProvenance {
    pub fn from_core_receipt(data: &Value) -> Self {
        let text = |key: &str| {
            data.get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .map(str::to_owned)
        };
        Self {
            processor: CORE_PROCESSOR.into(),
            engine_core_version: text("engine_core_version"),
            executable_sha256: text("executable_sha256"),
            development_override: data
                .get("development_override")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

/// An admitted LV batch with its executor provenance and, per job in input
/// order, the persisted process verdict (`null` for a failed job).
#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct LvProcessRun {
    pub jobs: Vec<ds_network_contracts::lv::LvProcessBatchJobOutput>,
    pub provenance: LvProcessProvenance,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "(Record<string, unknown> | null)[]")
    )]
    pub process_metadata: Vec<Option<serde_json::Map<String, Value>>>,
}

impl LvProcessRun {
    pub fn new(output: LvProcessBatchOutput, provenance: LvProcessProvenance) -> Self {
        let process_metadata = output
            .jobs
            .iter()
            .map(|job| {
                job.output
                    .as_ref()
                    .filter(|_| job.ok)
                    .map(|result| process_metadata(result, &provenance))
            })
            .collect();
        Self {
            jobs: output.jobs,
            provenance,
            process_metadata,
        }
    }
}

/// The persisted verdict of one successful job: `error` when the engine
/// reported an error-severity warning, `warning` when an actionable warning
/// (severity `warning` or unstated) remains, otherwise `success`; per-layer
/// result counts; the settings the engine used and the executor provenance.
/// `drafting_source` stays the retry vocabulary (`ds-web`: drafted from the
/// design room), never the compute owner.
pub fn process_metadata(
    output: &ds_network_contracts::lv::LvProcessOutput,
    provenance: &LvProcessProvenance,
) -> serde_json::Map<String, Value> {
    use serde_json::json;
    let severity = |warning: &&ds_network_contracts::lv::NetworkWarning| {
        warning.severity.trim().to_lowercase()
    };
    let first_error = output.warnings.iter().find(|w| severity(w) == "error");
    let first_warning = output
        .warnings
        .iter()
        .find(|w| matches!(severity(w).as_str(), "" | "warning"));
    let status = match (first_error, first_warning) {
        (Some(_), _) => "error",
        (None, Some(_)) => "warning",
        (None, None) => "success",
    };
    let counts: serde_json::Map<String, Value> = output
        .gdfs
        .iter()
        .map(|(layer, collection)| {
            let count = collection
                .get("features")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            (layer.clone(), json!(count))
        })
        .collect();
    let mut metadata = serde_json::Map::new();
    metadata.insert("status".into(), json!(status));
    metadata.insert("drafting_source".into(), json!("ds-web"));
    metadata.insert("processor".into(), json!(provenance.processor));
    if let Some(version) = &provenance.engine_core_version {
        metadata.insert("engine_core_version".into(), json!(version));
    }
    metadata.insert("lane".into(), json!(PROCESSING_LANE));
    metadata.insert(
        "settings".into(),
        serde_json::to_value(&output.settings_used).unwrap_or(Value::Null),
    );
    metadata.insert("result_counts".into(), Value::Object(counts));
    if !output.warnings.is_empty() {
        metadata.insert(
            "warnings".into(),
            serde_json::to_value(&output.warnings).unwrap_or(Value::Null),
        );
    }
    if let Some(error) = first_error {
        metadata.insert(
            "error".into(),
            serde_json::to_value(error).unwrap_or(Value::Null),
        );
        metadata.insert("error_message".into(), json!(error.message));
    } else if let Some(warning) = first_warning {
        metadata.insert(
            "warning".into(),
            serde_json::to_value(warning).unwrap_or(Value::Null),
        );
    }
    metadata
}

pub fn capture_bytes(project: &str, input: &[u8]) -> Result<CapturedLvProcess, CommandRefusal> {
    if input.len() > MAX_NATIVE_LV_INPUT_BYTES {
        return Err(native_fault(NativeLvError::InputTooLarge {
            actual: input.len(),
            maximum: MAX_NATIVE_LV_INPUT_BYTES,
        }));
    }
    let batch = serde_json::from_slice(input).map_err(|_| {
        invalid("The page LV batch has missing fields or incompatible field types.")
    })?;
    capture(project, batch)
}

impl CapturedLvProcess {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn input(&self) -> &[u8] {
        &self.input
    }
    pub fn native_request(&self) -> Result<Vec<u8>, CommandRefusal> {
        let batch = serde_json::from_slice(&self.input)
            .map_err(|_| invalid("The captured page LV batch is unreadable."))?;
        native_lv_request_from_batch(batch).map_err(native_fault)
    }

    /// Native application hosts use the same response constructor as the page
    /// decoder. The native adapter separately verifies executable/artifact identity.
    pub fn response_from_native(
        &self,
        result: NativeLvResultV1,
    ) -> Result<LvProcessBatchOutput, CommandRefusal> {
        if result.schema != NATIVE_LV_RESULT_SCHEMA_V1 {
            return Err(incompatible(
                "The native LV result has an incompatible document schema.",
            ));
        }
        let output = lv_batch_output_from_native(result, &self.project);
        self.validate_response(&output)?;
        Ok(output)
    }

    pub fn decode_response(&self, bytes: &[u8]) -> Result<LvProcessBatchOutput, CommandRefusal> {
        if bytes.len() > MAX_NATIVE_LV_OUTPUT_BYTES {
            return Err(incompatible(
                "The LV response exceeds its bounded result contract.",
            ));
        }
        let output = serde_json::from_slice(bytes).map_err(|_| {
            incompatible("The LV response has missing fields or incompatible field types.")
        })?;
        self.validate_response(&output)?;
        Ok(output)
    }

    /// Core's envelope as one admitted run: the batch result, the executor's
    /// own provenance receipt and each successful job's persisted verdict.
    pub fn decode_core_run(&self, value: Value) -> Result<LvProcessRun, CommandRefusal> {
        let provenance = value
            .get("data")
            .map(LvProcessProvenance::from_core_receipt)
            .unwrap_or_else(|| LvProcessProvenance::from_core_receipt(&Value::Null));
        let output = self.decode_core_response(value)?;
        Ok(LvProcessRun::new(output, provenance))
    }

    pub fn decode_core_response(
        &self,
        value: Value,
    ) -> Result<LvProcessBatchOutput, CommandRefusal> {
        let envelope: CommandEnvelope<Value> = serde_json::from_value(value)
            .map_err(|_| incompatible("Core answered with a missing field, incompatible field type or unknown response status."))?;
        if envelope.command() != COMMAND {
            return Err(incompatible("Core answered another command's LV request."));
        }
        match envelope {
            CommandEnvelope::Error { error, .. } => {
                error.validate().map_err(incompatible)?;
                Err(error)
            }
            CommandEnvelope::Ok { mut files, .. } => {
                let output = files
                    .remove("out")
                    .ok_or_else(|| incompatible("Core answered without the LV result file."))?;
                let bytes = serde_json::to_vec(&output)
                    .map_err(|_| incompatible("Core's LV result cannot be encoded."))?;
                self.decode_response(&bytes)
            }
        }
    }

    fn validate_response(&self, output: &LvProcessBatchOutput) -> Result<(), CommandRefusal> {
        if output.jobs.len() != self.transformer_names.len() {
            return Err(incompatible(
                "The LV result job count differs from the captured input.",
            ));
        }
        for (job, name) in output.jobs.iter().zip(&self.transformer_names) {
            if job.ds_project != self.project || &job.transformer_name != name {
                return Err(incompatible(
                    "The LV result project or transformer order differs from the captured input.",
                ));
            }
            if job.ok {
                let result = job
                    .output
                    .as_ref()
                    .ok_or_else(|| incompatible("A successful LV job has no output."))?;
                if job.error.is_some() || job.error_code.is_some() || result.gdfs.is_empty() {
                    return Err(incompatible(
                        "A successful LV job has inconsistent outcome fields.",
                    ));
                }
                for layer in result.gdfs.values() {
                    let rows = layer
                        .get("features")
                        .and_then(Value::as_array)
                        .filter(|_| {
                            layer.get("type").and_then(Value::as_str) == Some("FeatureCollection")
                        })
                        .ok_or_else(|| {
                            incompatible("An LV result layer is not a GeoJSON FeatureCollection.")
                        })?;
                    if rows.iter().any(|row| {
                        row.get("type").and_then(Value::as_str) != Some("Feature")
                            || row
                                .get("properties")
                                .is_some_and(|p| !p.is_null() && !p.is_object())
                    }) {
                        return Err(incompatible(
                            "The LV result feature count or row shape is incompatible.",
                        ));
                    }
                }
                if result.warnings.iter().any(|warning| {
                    warning.code.is_empty()
                        || warning.severity.is_empty()
                        || warning.message.is_empty()
                }) {
                    return Err(incompatible(
                        "The LV result warnings violate their field bounds.",
                    ));
                }
            } else if job.output.is_some()
                || job.error.as_ref().is_none_or(|e| e.is_empty())
                || job.error_code.as_ref().is_some_and(|e| e.is_empty())
                || job.differential.is_some()
            {
                return Err(incompatible(
                    "A failed LV job has missing or inconsistent outcome fields.",
                ));
            }
            if job.differential.as_ref().is_some_and(|plan| {
                !matches!(plan.scope.as_str(), "full" | "feeders")
                    || plan.reason_key.is_empty()
                    || plan.reason_key.len() > 128
                    || plan.selected_count > MAX_NATIVE_LV_FEATURES
                    || plan.frozen_count > MAX_NATIVE_LV_FEATURES
                    || plan.feeders.len() > MAX_NATIVE_LV_FEATURES
            }) {
                return Err(incompatible(
                    "The LV result differential plan violates its field bounds.",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub mod browser {
    use super::*;
    use crate::capabilities::exports::{run_captured_lv, CapabilityHost};
    use wasm_bindgen::prelude::*;

    pub(crate) fn failure(error: CommandRefusal) -> JsValue {
        let value = js_sys::Error::new(&error.message);
        value.set_name("CapabilityRefusal");
        use serde::Serialize;
        if let Ok(fields) =
            error.serialize(&tsify::serde_wasm_bindgen::Serializer::json_compatible())
        {
            js_sys::Object::assign(value.as_ref(), fields.unchecked_ref());
        }
        value.into()
    }

    #[wasm_bindgen(js_name = captureLvProcess)]
    pub fn capture_wasm(
        project: String,
        input: tsify::Ts<LvProcessBatchInput>,
    ) -> Result<CapturedLvProcess, JsValue> {
        let input = input.to_rust().map_err(|_| {
            failure(invalid(
                "The page LV batch has missing fields or incompatible field types.",
            ))
        })?;
        capture(&project, input).map_err(failure)
    }

    #[wasm_bindgen]
    impl CapturedLvProcess {
        /// Consume this immutable request once. The same helper serves the
        /// capabilityRun LV branch; no direct-engine fallback is available.
        #[wasm_bindgen(js_name = run)]
        pub async fn run_wasm(
            self,
            host: CapabilityHost,
        ) -> Result<tsify::Ts<LvProcessRun>, JsValue> {
            let output = run_captured_lv(&host, &self).await?;
            use tsify::Tsify;
            output.into_ts().map_err(|_| {
                failure(incompatible(
                    "The admitted LV response cannot cross the browser boundary.",
                ))
            })
        }

        #[wasm_bindgen(js_name = decodeResponse)]
        pub fn decode_wasm(
            &self,
            #[wasm_bindgen(unchecked_param_type = "CommandEnvelope<unknown>")] value: tsify::Ts<
                CommandEnvelope<Value>,
            >,
        ) -> Result<tsify::Ts<LvProcessBatchOutput>, JsValue> {
            let value = value.to_rust().map_err(|_| {
                failure(incompatible("Core's LV response has incompatible fields."))
            })?;
            let value = serde_json::to_value(value)
                .map_err(|_| failure(incompatible("Core's LV response cannot be encoded.")))?;
            use tsify::Tsify;
            self.decode_core_response(value)
                .map_err(failure)?
                .into_ts()
                .map_err(|_| {
                    failure(incompatible(
                        "The admitted LV response cannot cross the browser boundary.",
                    ))
                })
        }
    }
}

#[cfg(test)]
#[path = "design_lv_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "design_lv_run_tests.rs"]
mod run_tests;
