//! Upload cleaning application operations: apply a column plan to parsed
//! upload layers, preview one column, validate replacement candidates, stamp
//! upload provenance, resolve a layer's column vocabulary and merge an upload
//! into an existing layer by import key. Algorithms stay with their library
//! owners (`ds-cleaning`, `ds-network-contracts`); this module owns the
//! typed request, admission bound, refusal vocabulary and reply decoder.
use ds_cleaning::layer_columns::{known_columns_for_layer, upload_mapping_columns_for_layer};
use ds_cleaning::{
    ColumnPlan, LvMvDomainPlan, UploadCleaningConflict, UploadCleaningInput, UploadCleaningOutput,
    UploadColumnValuePreview, UploadColumnValuePreviewInput,
};
use ds_network_contracts::import_key_merge::{
    resolve_import_key_merge, ImportKeyMergeRequest, ImportKeyMergeResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Whole-layer JSON cleaning materializes every feature at once; larger
/// uploads stage Arrow chunks and use the batch operation instead.
pub const APPLY_MAX_FEATURES: usize = 50_000;
pub const APPLY_MAX_REQUEST_BYTES: usize = 50 * 1024 * 1024;
pub const APPLY_TOO_LARGE_CODE: &str = "APPLY_UPLOAD_CLEANING_TOO_LARGE";
const MERGE_MODES: [&str; 3] = ["append", "replace_key", "replace_all"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct UploadApplyInput {
    #[serde(flatten)]
    pub upload: UploadCleaningInput,
    /// Project configuration sheets: known columns, header aliases and value
    /// domains. Absent means only the standard-agnostic domains apply.
    #[serde(default, alias = "config_dfs")]
    #[cfg_attr(feature = "wasm", tsify(optional, type = "Record<string, unknown>"))]
    pub config_dfs: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct ReplacementCandidatesInput {
    pub file_name: String,
    pub parsed_name: String,
    pub target_key: String,
    #[serde(default)]
    pub columns: Vec<String>,
    pub column_plan: ColumnPlan,
    /// Raw values per column observed across every scanned chunk.
    pub candidates: BTreeMap<String, Vec<Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct StampFeaturesInput {
    pub features: Vec<Value>,
    pub file: String,
    pub layer: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum LayerColumnVocabulary {
    /// The attribute table's visible columns, in declaration order.
    Known,
    /// Known columns plus the platform's public outputs, for header mapping.
    UploadMapping,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct LayerColumnsInput {
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub config_dfs: Map<String, Value>,
    pub layer: String,
    pub vocabulary: LayerColumnVocabulary,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", content = "input", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub enum UploadCleaningRequest {
    Apply(UploadApplyInput),
    PreviewColumn(UploadColumnValuePreviewInput),
    ValidateReplacementCandidates(ReplacementCandidatesInput),
    StampFeatures(StampFeaturesInput),
    LayerColumns(LayerColumnsInput),
    ImportKeyMerge(ImportKeyMergeRequest),
}

/// A conflict is an expected answer the operator resolves, not a refusal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub enum UploadCleaningOutcome {
    Cleaned(UploadCleaningOutput),
    Conflict(UploadCleaningConflict),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct ReplacementCandidatesVerdict {
    pub conflict: Option<UploadCleaningConflict>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct StampedFeatures {
    pub features: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct LayerColumns {
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "result", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum UploadCleaningResponse {
    Apply(UploadCleaningOutcome),
    PreviewColumn(UploadColumnValuePreview),
    ValidateReplacementCandidates(ReplacementCandidatesVerdict),
    StampFeatures(StampedFeatures),
    LayerColumns(LayerColumns),
    ImportKeyMerge(ImportKeyMergeResult),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UploadCleaningError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for UploadCleaningError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => {
                write!(out, "upload_cleaning_request_invalid: {message}")
            }
            Self::InvalidResponse(message) => {
                write!(out, "upload_cleaning_response_invalid: {message}")
            }
        }
    }
}
impl std::error::Error for UploadCleaningError {}

fn invalid_request(message: impl Into<String>) -> UploadCleaningError {
    UploadCleaningError::InvalidRequest(message.into())
}
fn invalid_response(message: impl Into<String>) -> UploadCleaningError {
    UploadCleaningError::InvalidResponse(message.into())
}

pub fn validate_request(request: &UploadCleaningRequest) -> Result<(), UploadCleaningError> {
    match request {
        UploadCleaningRequest::PreviewColumn(input) if input.column.trim().is_empty() => {
            Err(invalid_request("a column is required"))
        }
        UploadCleaningRequest::ValidateReplacementCandidates(input)
            if input.target_key.trim().is_empty() =>
        {
            Err(invalid_request("a target layer is required"))
        }
        UploadCleaningRequest::StampFeatures(input)
            if input.features.iter().any(|feature| !feature.is_object()) =>
        {
            Err(invalid_request(
                "every stamped feature must be a JSON object",
            ))
        }
        UploadCleaningRequest::LayerColumns(input) if input.layer.trim().is_empty() => {
            Err(invalid_request("a layer is required"))
        }
        UploadCleaningRequest::ImportKeyMerge(input) if input.layer.trim().is_empty() => {
            Err(invalid_request("a target layer is required"))
        }
        _ => Ok(()),
    }
}

/// Decode with every JSON object key-sorted, as the Network engine parsed it.
pub fn decode_request(bytes: &[u8]) -> Result<UploadCleaningRequest, UploadCleaningError> {
    let request = crate::network_json_order::decode_sorted(bytes)
        .map_err(|error| invalid_request(error.to_string()))?;
    validate_request(&request)?;
    Ok(request)
}

/// Run one request from its owning wire bytes; the byte length is the
/// whole-layer admission measure.
pub fn execute_bytes(bytes: &[u8]) -> Result<UploadCleaningResponse, UploadCleaningError> {
    let request = decode_request(bytes)?;
    execute_measured(request, bytes.len())
}

pub fn execute(
    request: UploadCleaningRequest,
) -> Result<UploadCleaningResponse, UploadCleaningError> {
    let bytes = serde_json::to_vec(&request).map_err(|error| invalid_request(error.to_string()))?;
    execute_bytes(&bytes)
}

fn execute_measured(
    request: UploadCleaningRequest,
    request_bytes: usize,
) -> Result<UploadCleaningResponse, UploadCleaningError> {
    use UploadCleaningRequest as Request;
    use UploadCleaningResponse as Response;
    let response = match request {
        Request::Apply(input) => Response::Apply(apply(input, request_bytes)),
        Request::PreviewColumn(input) => {
            Response::PreviewColumn(ds_cleaning::preview_upload_column_values(input))
        }
        Request::ValidateReplacementCandidates(input) => {
            let conflict = ds_cleaning::validate_value_replacement_candidates(
                &input.file_name,
                &input.parsed_name,
                &input.target_key,
                input.columns,
                Some(&input.column_plan),
                &input.candidates,
            )
            .err();
            Response::ValidateReplacementCandidates(ReplacementCandidatesVerdict { conflict })
        }
        Request::StampFeatures(input) => {
            let mut features = input.features;
            for feature in &mut features {
                ds_cleaning::upload_groups::stamp(feature, &input.file, &input.layer);
            }
            Response::StampFeatures(StampedFeatures { features })
        }
        Request::LayerColumns(input) => {
            let columns = match input.vocabulary {
                LayerColumnVocabulary::Known => {
                    known_columns_for_layer(&input.config_dfs, &input.layer)
                }
                LayerColumnVocabulary::UploadMapping => {
                    upload_mapping_columns_for_layer(&input.config_dfs, &input.layer)
                }
            };
            Response::LayerColumns(LayerColumns { columns })
        }
        Request::ImportKeyMerge(input) => Response::ImportKeyMerge(resolve_import_key_merge(input)),
    };
    // Replies carry objects in the key order the engine emitted.
    crate::network_json_order::sorted(response).map_err(|error| invalid_response(error.to_string()))
}

fn apply(input: UploadApplyInput, request_bytes: usize) -> UploadCleaningOutcome {
    let layer_count = input.upload.inspection.layers.len();
    if request_bytes > APPLY_MAX_REQUEST_BYTES {
        return UploadCleaningOutcome::Conflict(too_large(
            None,
            None,
            request_bytes,
            "payload_bytes",
        ));
    }
    let feature_count: usize = input
        .upload
        .inspection
        .layers
        .iter()
        .map(|layer| layer.features.len())
        .sum();
    if feature_count > APPLY_MAX_FEATURES {
        return UploadCleaningOutcome::Conflict(too_large(
            Some(feature_count),
            Some(layer_count),
            request_bytes,
            "feature_count",
        ));
    }
    let plan = LvMvDomainPlan::new(input.config_dfs.unwrap_or_default());
    match ds_cleaning::apply_upload_cleaning_with_domain_plan(input.upload, &plan) {
        Ok(output) => UploadCleaningOutcome::Cleaned(output),
        Err(conflict) => UploadCleaningOutcome::Conflict(conflict),
    }
}

fn too_large(
    feature_count: Option<usize>,
    layer_count: Option<usize>,
    request_bytes: usize,
    reason: &str,
) -> UploadCleaningConflict {
    let details = json!({
        "entrypoint": "upload_cleaning.apply",
        "featureCount": feature_count,
        "layerCount": layer_count,
        "payloadBytes": request_bytes,
        "maxFeatures": APPLY_MAX_FEATURES,
        "maxPayloadBytes": APPLY_MAX_REQUEST_BYTES,
        "recommendedPath": "upload_batch.clean",
        "reason": reason,
    });
    UploadCleaningConflict {
        code: APPLY_TOO_LARGE_CODE.to_string(),
        message: String::from(
            "Upload cleaning payload is too large for the legacy JSON path. \
             Use the streaming Arrow IPC upload path instead.",
        ),
        details: match details {
            Value::Object(details) => details,
            _ => Map::new(),
        },
    }
}

fn validate_conflict(conflict: &UploadCleaningConflict) -> Result<(), UploadCleaningError> {
    if conflict.code.trim().is_empty() || conflict.message.trim().is_empty() {
        return Err(invalid_response("a conflict needs a code and a message"));
    }
    Ok(())
}

fn validate_cleaned(
    input: &UploadApplyInput,
    output: &UploadCleaningOutput,
) -> Result<(), UploadCleaningError> {
    let targets: BTreeSet<&str> = input
        .upload
        .mapping
        .iter()
        .map(|item| item.target_key.as_str())
        .collect();
    let parsed: BTreeSet<&str> = input
        .upload
        .inspection
        .layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect();
    for (target, layer) in &output.layers {
        if !targets.contains(target.as_str()) {
            return Err(invalid_response(format!(
                "layer {target} was not requested"
            )));
        }
        let features = layer
            .get("features")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response(format!("layer {target} has no feature array")))?;
        if output.feature_counts.get(target) != Some(&features.len()) {
            return Err(invalid_response(format!(
                "layer {target} miscounts its features"
            )));
        }
    }
    if output
        .feature_counts
        .keys()
        .any(|key| !output.layers.contains_key(key))
    {
        return Err(invalid_response("a feature count names no layer"));
    }
    if output.source_layers.iter().any(|(target, source)| {
        !output.layers.contains_key(target) || !parsed.contains(source.as_str())
    }) {
        return Err(invalid_response(
            "a source layer does not belong to this upload",
        ));
    }
    Ok(())
}

fn validate_preview(
    input: &UploadColumnValuePreviewInput,
    preview: &UploadColumnValuePreview,
) -> Result<(), UploadCleaningError> {
    if preview.column != input.column {
        return Err(invalid_response("answer belongs to a different column"));
    }
    let total = preview.total;
    if preview.missing > total
        || preview.changed > total
        || preview.unique_raw > total
        || preview.unique_cleaned > total
        || preview.samples.len() > total
        || preview.samples.len() > input.sample_limit.max(1)
    {
        return Err(invalid_response("inconsistent value counts"));
    }
    Ok(())
}

fn validate_merge(
    input: &ImportKeyMergeRequest,
    result: &ImportKeyMergeResult,
) -> Result<(), UploadCleaningError> {
    if !MERGE_MODES.contains(&result.mode.as_str()) {
        return Err(invalid_response(format!(
            "unknown merge mode {}",
            result.mode
        )));
    }
    if result.error.is_some() && result.merged != input.existing {
        return Err(invalid_response(
            "a refused merge must return the existing rows unchanged",
        ));
    }
    if result.stats.unkeyed_count > input.incoming.len() {
        return Err(invalid_response("more unkeyed rows than incoming rows"));
    }
    Ok(())
}

pub fn validate_response(
    request: &UploadCleaningRequest,
    response: UploadCleaningResponse,
) -> Result<UploadCleaningResponse, UploadCleaningError> {
    use UploadCleaningRequest as Request;
    use UploadCleaningResponse as Response;
    validate_request(request)?;
    match (request, &response) {
        (Request::Apply(input), Response::Apply(outcome)) => match outcome {
            UploadCleaningOutcome::Cleaned(output) => validate_cleaned(input, output),
            UploadCleaningOutcome::Conflict(conflict) => validate_conflict(conflict),
        },
        (Request::PreviewColumn(input), Response::PreviewColumn(preview)) => {
            validate_preview(input, preview)
        }
        (
            Request::ValidateReplacementCandidates(_),
            Response::ValidateReplacementCandidates(verdict),
        ) => verdict.conflict.as_ref().map_or(Ok(()), validate_conflict),
        (Request::StampFeatures(input), Response::StampFeatures(stamped)) => {
            if stamped.features.len() != input.features.len() {
                return Err(invalid_response("stamping changed the feature count"));
            }
            let unstamped = stamped.features.iter().any(|feature| {
                ["_ds_source_file", "_ds_source_layer"]
                    .iter()
                    .any(|key| feature["properties"].get(key).is_none_or(Value::is_null))
            });
            if unstamped {
                return Err(invalid_response("a feature carries no upload provenance"));
            }
            Ok(())
        }
        (Request::LayerColumns(_), Response::LayerColumns(columns)) => {
            let mut seen = BTreeSet::new();
            if columns
                .columns
                .iter()
                .any(|column| column.trim().is_empty() || !seen.insert(column.as_str()))
            {
                return Err(invalid_response("columns must be named and unique"));
            }
            Ok(())
        }
        (Request::ImportKeyMerge(input), Response::ImportKeyMerge(result)) => {
            validate_merge(input, result)
        }
        _ => Err(invalid_response(
            "reply operation does not match the request",
        )),
    }?;
    Ok(response)
}

pub fn decode_response(
    request: &UploadCleaningRequest,
    bytes: &[u8],
) -> Result<UploadCleaningResponse, UploadCleaningError> {
    validate_response(
        request,
        serde_json::from_slice(bytes).map_err(|error| invalid_response(error.to_string()))?,
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refuse(error: UploadCleaningError) -> JsError {
        JsError::new(&error.to_string())
    }

    #[wasm_bindgen(js_name = uploadCleaning)]
    pub fn run(input: &[u8]) -> Result<tsify::Ts<UploadCleaningResponse>, JsError> {
        execute_bytes(input)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(invalid_response(error.to_string())))
    }

    #[wasm_bindgen(js_name = decodeUploadCleaning)]
    pub fn decode(
        input: &[u8],
        reply: tsify::Ts<UploadCleaningResponse>,
    ) -> Result<tsify::Ts<UploadCleaningResponse>, JsError> {
        let request = decode_request(input).map_err(refuse)?;
        let reply = reply
            .to_rust()
            .map_err(|error| refuse(invalid_response(error.to_string())))?;
        validate_response(&request, reply)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(invalid_response(error.to_string())))
    }
}

#[cfg(test)]
#[path = "upload_cleaning_tests.rs"]
mod tests;
