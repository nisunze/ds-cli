//! One application boundary for `design.features.select` on captured room bytes.
//!
//! Acquisition and authorization stay with their current owners. This owner
//! retains explicit source identity, invokes the one `ds_geo` selector, and
//! admits the response against that same captured source. A working-copy base
//! version is provenance, never proof of a persisted revision or a write fence.
use std::{collections::BTreeMap, error::Error, fmt};

pub use ds_geo::feature_selection::{
    FeatureSelectionError, FeatureSelectionResult, FeatureSelector, MAX_PROJECTED_IDS,
};
use ds_geo::feature_selection::{
    MAX_MATCHED_FEATURES, MAX_SAMPLE_FEATURES, MAX_SCANNED_FEATURES, MAX_SELECTION_LAYERS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

/// The source actually held by the caller. Naming a project never authorizes it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DesignFeatureSourceContext {
    /// Saved snapshot metadata came with these exact acquired bytes. Legacy
    /// snapshots may omit either field; no digest is manufactured here.
    TransformerSnapshot {
        ds_project: String,
        transformer: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_digest: Option<String>,
    },
    /// Authored bytes held by the editor/cache. The base is only the version
    /// from which those bytes were edited; it does not describe their digest.
    TransformerWorkingCopy {
        ds_project: String,
        transformer: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_version: Option<u64>,
        dirty: bool,
        server_known: bool,
    },
}

impl DesignFeatureSourceContext {
    fn validate(&self) -> Result<(), DesignFeaturesSelectFault> {
        let (project, transformer, digest) = match self {
            Self::TransformerSnapshot {
                ds_project,
                transformer,
                content_digest,
                ..
            } => (ds_project, transformer, content_digest.as_deref()),
            Self::TransformerWorkingCopy {
                ds_project,
                transformer,
                ..
            } => (ds_project, transformer, None),
        };
        if !crate::execution_context::valid_project(project) {
            return Err(DesignFeaturesSelectFault::new(
                "context_corrupt",
                "feature selection requires one exact captured DS project",
                json!({"field":"source.dsProject"}),
            ));
        }
        // Retained transformer-context reads already accept 160 characters;
        // this is not the narrower rule for creating a new transformer.
        if !retained_name(transformer, 160) || transformer == "__all__" {
            return Err(DesignFeaturesSelectFault::new(
                "auth_input_invalid",
                "feature selection requires one exact captured transformer",
                json!({"field":"source.transformer"}),
            ));
        }
        if digest.is_some_and(|value| !retained_name(value, 256)) {
            return Err(DesignFeaturesSelectFault::new(
                "auth_input_invalid",
                "feature selection source digest violates the retained metadata bound",
                json!({"field":"source.contentDigest"}),
            ));
        }
        Ok(())
    }
}

fn retained_name(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= maximum
        && !value.chars().any(char::is_control)
}

/// Additive application fields are compatible. Unknown selector clauses retain
/// the selector owner's closed admission, so an ignored clause cannot broaden a selection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
#[serde(rename_all = "camelCase")]
pub struct DesignFeaturesSelectRequest {
    pub source: DesignFeatureSourceContext,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub layers: BTreeMap<String, Value>,
    #[serde(default)]
    pub selector: FeatureSelector,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
#[serde(rename_all = "camelCase")]
pub struct DesignFeaturesSelectResponse {
    pub source: DesignFeatureSourceContext,
    pub selection: FeatureSelectionResult,
}

/// Stable native/browser refusal envelope. Hosts may adapt presentation only.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
#[serde(rename_all = "camelCase")]
pub struct DesignFeaturesSelectFault {
    pub code: String,
    pub message: String,
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(type = "unknown"))]
    pub details: Value,
}

impl DesignFeaturesSelectFault {
    fn new(code: impl Into<String>, message: impl Into<String>, details: Value) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details,
        }
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            "{\"code\":\"serialize_failed\",\"message\":\"failed to serialize feature-selection error\",\"details\":{}}".to_string()
        })
    }
    fn invalid_json(error: serde_json::Error) -> Self {
        Self::new(
            "invalid_json",
            "feature-selection input is not the typed application contract",
            json!({"category":category(&error),"line":error.line(),"column":error.column()}),
        )
    }
    fn input_too_large(actual: usize) -> Self {
        Self::new(
            "input_too_large",
            "feature-selection input exceeds the one-shot JSON byte bound",
            json!({"payloadBytes":actual,"maxPayloadBytes":MAX_INPUT_BYTES}),
        )
    }
    fn incompatible(message: &str) -> Self {
        Self::new("selection_response_incompatible", message, json!({}))
    }
}

impl From<FeatureSelectionError> for DesignFeaturesSelectFault {
    fn from(error: FeatureSelectionError) -> Self {
        let details = match &error {
            FeatureSelectionError::TooManyLayers { count }
            | FeatureSelectionError::TooManyWhereFilters { count }
            | FeatureSelectionError::TooManyIds { count }
            | FeatureSelectionError::SampleTooLarge { count }
            | FeatureSelectionError::RegionTooLarge { count }
            | FeatureSelectionError::ScanLimitExceeded { count } => json!({"count":count}),
            FeatureSelectionError::DuplicateLayerFilter { layer }
            | FeatureSelectionError::InvalidFeatureCollection { layer } => json!({"layer":layer}),
            FeatureSelectionError::InvalidWhereValue { key } => json!({"key":key}),
            FeatureSelectionError::DuplicateId { id } => json!({"id":id}),
            FeatureSelectionError::InvalidFeature { layer, index } => {
                json!({"layer":layer,"index":index})
            }
            _ => json!({}),
        };
        Self::new(error.code(), error.to_string(), details)
    }
}
impl fmt::Display for DesignFeaturesSelectFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}] {}", self.code, self.message)
    }
}
impl Error for DesignFeaturesSelectFault {}

fn category(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        serde_json::error::Category::Io => "io",
        serde_json::error::Category::Syntax => "syntax",
        serde_json::error::Category::Data => "data",
        serde_json::error::Category::Eof => "eof",
    }
}

/// The native CLI and browser operation call this exact typed owner.
pub fn select(
    source: DesignFeatureSourceContext,
    layers: &BTreeMap<String, Value>,
    selector: FeatureSelector,
) -> Result<DesignFeaturesSelectResponse, DesignFeaturesSelectFault> {
    source.validate()?;
    let selection = ds_geo::feature_selection::select_geojson_features(layers, selector)?;
    Ok(DesignFeaturesSelectResponse { source, selection })
}

pub fn encode_request(
    request: &DesignFeaturesSelectRequest,
) -> Result<Vec<u8>, DesignFeaturesSelectFault> {
    request.source.validate()?;
    let bytes = serde_json::to_vec(request).map_err(DesignFeaturesSelectFault::invalid_json)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(DesignFeaturesSelectFault::input_too_large(bytes.len()));
    }
    Ok(bytes)
}

/// The worker carries this outcome unchanged. Domain refusals are data; only
/// mechanical host failures use the worker's generic transport error channel.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Selected {
        #[serde(flatten)]
        response: DesignFeaturesSelectResponse,
    },
    Refused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<DesignFeatureSourceContext>,
        refusal: DesignFeaturesSelectFault,
    },
}

pub fn execute(input: &[u8]) -> String {
    let outcome = if input.len() > MAX_INPUT_BYTES {
        Outcome::Refused {
            source: None,
            refusal: DesignFeaturesSelectFault::input_too_large(input.len()),
        }
    } else {
        match serde_json::from_slice::<DesignFeaturesSelectRequest>(input) {
            Err(error) => Outcome::Refused {
                source: None,
                refusal: DesignFeaturesSelectFault::invalid_json(error),
            },
            Ok(request) => {
                let source = request.source.clone();
                match select(request.source, &request.layers, request.selector) {
                    Ok(response) => Outcome::Selected { response },
                    Err(refusal) => Outcome::Refused {
                        source: Some(source),
                        refusal,
                    },
                }
            }
        }
    };
    encode_outcome(outcome)
}

/// Serialize a native typed success using the same outcome as the worker.
/// Serializing only the inner response omits the required status discriminator.
pub fn encode_response(response: DesignFeaturesSelectResponse) -> String {
    encode_outcome(Outcome::Selected { response })
}

fn encode_outcome(outcome: Outcome) -> String {
    serde_json::to_string(&outcome).unwrap_or_else(|_| {
        r#"{"status":"refused","refusal":{"code":"serialize_failed","message":"feature-selection result could not be serialized","details":{}}}"#.to_string()
    })
}

/// Admit a result only for the source captured before dispatch. Deserialize
/// tolerates additive fields; missing/changed required fields and impossible
/// counts are explicit incompatibilities, never partial success/defaults.
pub fn decode_response(
    output: &str,
    expected_source: &DesignFeatureSourceContext,
) -> Result<DesignFeaturesSelectResponse, DesignFeaturesSelectFault> {
    expected_source.validate()?;
    if output.len() > MAX_RESPONSE_BYTES {
        return Err(DesignFeaturesSelectFault::incompatible(
            "feature-selection response exceeds the bounded result contract",
        ));
    }
    let outcome: Outcome = serde_json::from_str(output).map_err(|error| {
        DesignFeaturesSelectFault::new(
            "selection_response_incompatible",
            "feature-selection response has an unsupported status, missing required fields or incompatible types",
            json!({"category":category(&error),"line":error.line(),"column":error.column()}),
        )
    })?;
    let result = match outcome {
        Outcome::Selected { response } => response,
        Outcome::Refused { source, refusal } => {
            if source
                .as_ref()
                .is_some_and(|source| source != expected_source)
            {
                return Err(DesignFeaturesSelectFault::incompatible(
                    "feature-selection refusal belongs to another captured source",
                ));
            }
            return Err(refusal);
        }
    };
    if &result.source != expected_source {
        return Err(DesignFeaturesSelectFault::incompatible(
            "feature-selection response belongs to another captured source",
        ));
    }
    let selection = &result.selection;
    let r = &selection.receipt;
    let sum = r
        .matched_by_layer
        .values()
        .try_fold(0usize, |sum, count| sum.checked_add(*count));
    let mut matched_counts = BTreeMap::new();
    for matched in &selection.matches {
        *matched_counts
            .entry(matched.layer.clone())
            .or_insert(0usize) += 1;
    }
    let projected_ids: Vec<_> = selection
        .matches
        .iter()
        .filter_map(|m| m.id.clone())
        .take(MAX_PROJECTED_IDS)
        .collect();
    let identified = selection.matches.iter().filter(|m| m.id.is_some()).count();
    use ds_geo::feature_selection::{
        FeatureSelectionGeometryOmissionReason as Omission,
        FeatureSelectionSampleGeometryState as GeometryState,
    };
    let inconsistent_sample = selection.sample.iter().enumerate().any(|(index, sample)| {
        let Some(matched) = selection.matches.get(index) else {
            return true;
        };
        sample.layer != matched.layer
            || sample.index != matched.index
            || sample.id != matched.id
            || match sample.geometry_state {
                GeometryState::Included => {
                    sample.geometry.is_none() || sample.geometry_omission_reason.is_some()
                }
                GeometryState::Omitted => {
                    sample.geometry.is_some() || sample.geometry_omission_reason.is_none()
                }
            }
    });
    let oversize_samples = selection
        .sample
        .iter()
        .filter(|sample| sample.geometry_omission_reason == Some(Omission::Oversize))
        .count();
    let samples_with_geometry = selection
        .sample
        .iter()
        .filter(|sample| sample.geometry.is_some())
        .count();
    if r.selected_layers.len() > MAX_SELECTION_LAYERS
        || r.scanned_features > MAX_SCANNED_FEATURES
        || r.matched_features > MAX_MATCHED_FEATURES
        || r.matched_features != selection.matches.len()
        || r.matched_features > r.scanned_features
        || sum != Some(r.matched_features)
        || r.missing_identity_features > r.scanned_features
        || r.matched_by_layer != matched_counts
        || r.identified_matches != identified
        || selection.ids != projected_ids
        || inconsistent_sample
        || r.sample_geometry_oversize_omissions != oversize_samples
        || r.identified_matches > r.matched_features
        || selection.ids.len() > MAX_PROJECTED_IDS
        || r.projected_id_count != selection.ids.len()
        || selection.ids.len() > r.identified_matches
        || r.projected_ids_truncated != (r.identified_matches > selection.ids.len())
        || r.sample_count != selection.sample.len()
        || r.sample_count > MAX_SAMPLE_FEATURES
        || r.sample_count > r.matched_features
        || r.sample_with_geometry != samples_with_geometry
        || r.sample_without_geometry != r.sample_count - samples_with_geometry
        || r.sample_geometry_oversize_omissions > r.sample_without_geometry
        || selection
            .matches
            .iter()
            .any(|m| !r.selected_layers.contains(&m.layer) || m.index >= r.scanned_features)
    {
        return Err(DesignFeaturesSelectFault::incompatible(
            "feature-selection response violates required selection counts or bounds",
        ));
    }
    Ok(result)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub mod browser {
    use super::*;
    use tsify::{Ts, Tsify};
    use wasm_bindgen::prelude::*;
    fn js(error: DesignFeaturesSelectFault) -> JsValue {
        JsError::new(&error.to_json()).into()
    }
    fn invalid_js(error: tsify::Error) -> JsValue {
        js(DesignFeaturesSelectFault::new(
            "invalid_json",
            "feature-selection value is not the typed application contract",
            json!({"category":"data","detail":error.to_string()}),
        ))
    }
    #[wasm_bindgen(js_name = designFeaturesSelect)]
    pub fn execute_wasm(input: &[u8]) -> String {
        execute(input)
    }
    #[wasm_bindgen(js_name = encodeDesignFeaturesSelect)]
    pub fn encode_wasm(request: Ts<DesignFeaturesSelectRequest>) -> Result<Vec<u8>, JsValue> {
        encode_request(&request.to_rust().map_err(invalid_js)?).map_err(js)
    }
    #[wasm_bindgen(js_name = decodeDesignFeaturesSelect)]
    pub fn decode_wasm(
        output: &str,
        expected_source: Ts<DesignFeatureSourceContext>,
    ) -> Result<Ts<DesignFeaturesSelectResponse>, JsValue> {
        decode_response(output, &expected_source.to_rust().map_err(invalid_js)?)
            .map_err(js)?
            .into_ts()
            .map_err(|error| {
                js(DesignFeaturesSelectFault::new(
                    "serialize_failed",
                    "feature-selection result could not be serialized",
                    json!({"category":"data","detail":error.to_string()}),
                ))
            })
    }
}

#[cfg(test)]
#[path = "design_features_tests.rs"]
mod tests;
