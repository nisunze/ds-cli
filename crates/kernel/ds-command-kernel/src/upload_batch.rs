//! Chunk-level upload cleaning over Arrow IPC: clean one staged chunk, scan it
//! for raw values a replacement plan names, or stamp per-source provenance.
//! The browser keeps chunk I/O and scheduling; the `ds-cleaning` owner keeps
//! every cleaning decision. Chunk bytes cross the boundary as bytes, never as
//! feature JSON.
use ds_cleaning::arrow_ipc;
use ds_cleaning::{ColumnPlan, UploadCleaningConflict};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::BTreeMap;

/// Every chunk the owner writes is an Arrow IPC file.
const ARROW_FILE_MAGIC: &[u8] = b"ARROW1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct CleanBatchControl {
    pub file_name: String,
    pub parsed_name: String,
    pub target_key: String,
    /// The parsed layer's full column list; keeps emission order stable
    /// across chunks.
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub column_plan: Option<ColumnPlan>,
    #[serde(default)]
    pub import_key: Option<String>,
    #[serde(default)]
    pub transformer_document_name: Option<String>,
    /// Absolute row of this chunk's first feature, for conflict messages.
    #[serde(default)]
    pub base_row_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct ScanBatchControl {
    pub column_plan: ColumnPlan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct AnnotateBatchControl {
    #[serde(default)]
    pub source_file: Option<String>,
    #[serde(default)]
    pub source_layer: Option<String>,
    /// PLS-CADD structure identity shift for a later file of a batch.
    #[serde(default)]
    pub structure_number_offset: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", content = "input", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub enum UploadBatchRequest {
    Clean(CleanBatchControl),
    ScanReplacementCandidates(ScanBatchControl),
    Annotate(AnnotateBatchControl),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct BatchIpc {
    #[serde(serialize_with = "ipc_out", deserialize_with = "ipc_in")]
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub ipc: Vec<u8>,
    pub feature_count: usize,
}

/// A conflict is an expected answer the operator resolves, not a refusal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub enum CleanBatchOutcome {
    Cleaned(BatchIpc),
    Conflict(UploadCleaningConflict),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct ReplacementCandidates {
    /// column → raw value key → raw value, bounded by the plan, not the rows.
    pub candidates: BTreeMap<String, BTreeMap<String, Value>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "result", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum UploadBatchResponse {
    Clean(CleanBatchOutcome),
    ScanReplacementCandidates(ReplacementCandidates),
    Annotate(BatchIpc),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UploadBatchError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for UploadBatchError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(out, "upload_batch_request_invalid: {message}"),
            Self::InvalidResponse(message) => {
                write!(out, "upload_batch_response_invalid: {message}")
            }
        }
    }
}
impl std::error::Error for UploadBatchError {}

fn ipc_out<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(bytes)
}

/// Bytes from a browser `Uint8Array`, or a number array from JSON.
fn ipc_in<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    struct Bytes;
    impl<'de> serde::de::Visitor<'de> for Bytes {
        type Value = Vec<u8>;
        fn expecting(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {
            out.write_str("Arrow IPC bytes")
        }
        fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
            Ok(bytes.to_vec())
        }
        fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Vec<u8>, E> {
            Ok(bytes)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
            let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(byte) = seq.next_element()? {
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }
    deserializer.deserialize_byte_buf(Bytes)
}

fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.trim().is_empty())
}

pub fn validate_request(request: &UploadBatchRequest) -> Result<(), UploadBatchError> {
    let UploadBatchRequest::Clean(control) = request else {
        return Ok(());
    };
    if control.file_name.trim().is_empty() || control.target_key.trim().is_empty() {
        return Err(UploadBatchError::InvalidRequest(
            "a file and a target layer are required".into(),
        ));
    }
    Ok(())
}

pub fn execute(
    ipc: &[u8],
    request: &UploadBatchRequest,
) -> Result<UploadBatchResponse, UploadBatchError> {
    validate_request(request)?;
    let invalid = |message: String| UploadBatchError::InvalidRequest(message);
    // Plan objects key-sorted, as the Network engine parsed them.
    let request = &crate::network_json_order::sorted(request.clone())
        .map_err(|error| invalid(error.to_string()))?;
    Ok(match request {
        UploadBatchRequest::Clean(control) => UploadBatchResponse::Clean(
            match arrow_ipc::clean_feature_batch_ipc(
                &control.file_name,
                &control.parsed_name,
                &control.target_key,
                control.columns.clone(),
                ipc,
                control.column_plan.clone(),
                present(&control.import_key),
                present(&control.transformer_document_name),
                control.base_row_index,
            ) {
                Ok((ipc, feature_count)) => {
                    CleanBatchOutcome::Cleaned(BatchIpc { ipc, feature_count })
                }
                Err(mut conflict) => {
                    conflict.details.sort_keys();
                    CleanBatchOutcome::Conflict(conflict)
                }
            },
        ),
        UploadBatchRequest::ScanReplacementCandidates(control) => {
            UploadBatchResponse::ScanReplacementCandidates(ReplacementCandidates {
                candidates: arrow_ipc::replacement_candidates_ipc(ipc, &control.column_plan)
                    .map_err(invalid)?,
            })
        }
        UploadBatchRequest::Annotate(control) => {
            let (ipc, feature_count) = arrow_ipc::annotate_feature_batch_ipc(
                ipc,
                present(&control.source_file),
                present(&control.source_layer),
                control.structure_number_offset,
            )
            .map_err(invalid)?;
            UploadBatchResponse::Annotate(BatchIpc { ipc, feature_count })
        }
    })
}

fn validate_ipc(batch: &BatchIpc) -> Result<(), UploadBatchError> {
    if !batch.ipc.starts_with(ARROW_FILE_MAGIC) || !batch.ipc.ends_with(ARROW_FILE_MAGIC) {
        return Err(UploadBatchError::InvalidResponse(
            "the reply chunk is not an Arrow IPC file".into(),
        ));
    }
    Ok(())
}

pub fn validate_response(
    request: &UploadBatchRequest,
    response: UploadBatchResponse,
) -> Result<UploadBatchResponse, UploadBatchError> {
    validate_request(request)?;
    match (request, &response) {
        (UploadBatchRequest::Clean(_), UploadBatchResponse::Clean(outcome)) => match outcome {
            CleanBatchOutcome::Cleaned(batch) => validate_ipc(batch),
            CleanBatchOutcome::Conflict(conflict)
                if conflict.code.trim().is_empty() || conflict.message.trim().is_empty() =>
            {
                Err(UploadBatchError::InvalidResponse(
                    "a conflict needs a code and a message".into(),
                ))
            }
            CleanBatchOutcome::Conflict(_) => Ok(()),
        },
        (
            UploadBatchRequest::ScanReplacementCandidates(control),
            UploadBatchResponse::ScanReplacementCandidates(reply),
        ) => {
            if reply
                .candidates
                .keys()
                .any(|column| !control.column_plan.value_replacements.contains_key(column))
            {
                return Err(UploadBatchError::InvalidResponse(
                    "a candidate column is not in the replacement plan".into(),
                ));
            }
            Ok(())
        }
        (UploadBatchRequest::Annotate(_), UploadBatchResponse::Annotate(batch)) => {
            validate_ipc(batch)
        }
        _ => Err(UploadBatchError::InvalidResponse(
            "reply operation does not match the request".into(),
        )),
    }?;
    Ok(response)
}

pub fn decode_response(
    request: &UploadBatchRequest,
    bytes: &[u8],
) -> Result<UploadBatchResponse, UploadBatchError> {
    validate_response(
        request,
        serde_json::from_slice(bytes)
            .map_err(|error| UploadBatchError::InvalidResponse(error.to_string()))?,
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refuse(error: UploadBatchError) -> JsError {
        JsError::new(&error.to_string())
    }
    fn request(input: tsify::Ts<UploadBatchRequest>) -> Result<UploadBatchRequest, JsError> {
        input
            .to_rust()
            .map_err(|error| refuse(UploadBatchError::InvalidRequest(error.to_string())))
    }

    #[wasm_bindgen(js_name = uploadBatchIpc)]
    pub fn run(
        ipc: &[u8],
        input: tsify::Ts<UploadBatchRequest>,
    ) -> Result<tsify::Ts<UploadBatchResponse>, JsError> {
        execute(ipc, &request(input)?)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(UploadBatchError::InvalidResponse(error.to_string())))
    }

    #[wasm_bindgen(js_name = decodeUploadBatch)]
    pub fn decode(
        input: tsify::Ts<UploadBatchRequest>,
        reply: tsify::Ts<UploadBatchResponse>,
    ) -> Result<tsify::Ts<UploadBatchResponse>, JsError> {
        let request = request(input)?;
        let reply = reply
            .to_rust()
            .map_err(|error| refuse(UploadBatchError::InvalidResponse(error.to_string())))?;
        validate_response(&request, reply)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(UploadBatchError::InvalidResponse(error.to_string())))
    }
}

#[cfg(test)]
#[path = "upload_batch_tests.rs"]
mod tests;
