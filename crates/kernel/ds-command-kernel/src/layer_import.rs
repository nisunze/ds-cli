//! Browser file readers as typed application operations.
//!
//! The Kernel admits one declared reader request with its explicit byte
//! parts and names the effect: the governed Network unit's `layerImport`
//! operation runs the reader there and answers the response with the Arrow
//! IPC chunks of its layers beside it. The Kernel validates that reply
//! against the request it admitted. A streaming GeoJSON import is the unit's
//! `grid.stream` entry, admitted and decoded here the same way. Temporary
//! upload grouping plans layers from preview metadata over the shared upload
//! rules. Every format decision (parsing, CRS, admission, provenance
//! collapse) stays with the readers; the vocabulary is
//! `ds_network_contracts::page_data::layer_import`.
use crate::grid_engine::{GridEffect, UNIT};
use ds_network_contracts::grid_unit::{
    decode_reply as decode_grid_reply, GridArg, GridModelMethod, GridOperation, GridRequest,
    GridResult, GridStreamMethod,
};
pub use ds_network_contracts::page_data::layer_import::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn effect(operation: GridOperation, args: Vec<GridArg>) -> Result<GridEffect, LayerImportError> {
    let request = GridRequest::new(operation, args)
        .map_err(|refusal| LayerImportError::InvalidRequest(refusal.message))?;
    Ok(GridEffect {
        unit: UNIT,
        entry: operation.entry(),
        payload: request.encode(),
    })
}

/// The effect for one admitted reader request over its explicit byte parts.
pub fn request_effect(input: &[u8], parts: &[&[u8]]) -> Result<GridEffect, LayerImportError> {
    let request = decode_request(input)?;
    if parts.len() != request.parts() {
        return Err(LayerImportError::InvalidRequest(format!(
            "expected {} byte part(s), received {}",
            request.parts(),
            parts.len()
        )));
    }
    let mut args = vec![GridArg::Bytes(input.to_vec())];
    args.extend(parts.iter().map(|part| GridArg::Bytes(part.to_vec())));
    effect(GridOperation::Model(GridModelMethod::LayerImport), args)
}

fn invalid(message: impl Into<String>) -> LayerImportError {
    LayerImportError::InvalidResponse(message.into())
}

/// The JSON text and byte parts of a unit reply. The owner's refusal keeps
/// its own message; a malformed or crossed reply is a response refusal.
fn reply_parts(
    operation: GridOperation,
    reply: &[u8],
) -> Result<Option<(String, Vec<Vec<u8>>)>, LayerImportError> {
    match decode_grid_reply(operation, reply) {
        Err(malformed) => Err(invalid(malformed.message)),
        Ok(Err(refusal)) => Err(LayerImportError::Reader(refusal.message)),
        Ok(Ok(GridResult::Parts { text, blobs })) => Ok(Some((text, blobs))),
        Ok(Ok(GridResult::Opened)) => Ok(None),
        Ok(Ok(_)) => Err(invalid("the reply does not carry a layer import answer")),
    }
}

/// Decode the unit's reply to `request` and validate it; chunk bytes are
/// checked by length and Arrow file magic only.
pub fn decode_unit_reply(
    request: &LayerImportRequest,
    reply: &[u8],
) -> Result<LayerImportReply, LayerImportError> {
    let (text, chunks) = reply_parts(GridOperation::Model(GridModelMethod::LayerImport), reply)?
        .ok_or_else(|| invalid("the reply does not carry a layer import answer"))?;
    let heads = chunks
        .iter()
        .map(|chunk| chunk_head(chunk))
        .collect::<Vec<_>>();
    let response = decode_response(request, text.as_bytes(), &heads)?;
    Ok(LayerImportReply { response, chunks })
}

// ── Streaming GeoJSON ───────────────────────────────────────────────────

/// One step of the unit's GeoJSON import stream.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamStep<'a> {
    Open(&'a str),
    Push(&'a [u8]),
    Finish,
}

impl StreamStep<'_> {
    fn method(&self) -> GridStreamMethod {
        match self {
            Self::Open(_) => GridStreamMethod::GeoJsonStreamOpen,
            Self::Push(_) => GridStreamMethod::GeoJsonStreamPush,
            Self::Finish => GridStreamMethod::GeoJsonStreamFinish,
        }
    }
}

/// The effect for one admitted stream step.
pub fn stream_effect(step: &StreamStep) -> Result<GridEffect, LayerImportError> {
    let args = match step {
        StreamStep::Open(name) if name.trim().is_empty() => {
            return Err(LayerImportError::InvalidRequest(
                "a source name is required".into(),
            ))
        }
        StreamStep::Open(name) => vec![GridArg::Text(name.to_string())],
        StreamStep::Push(bytes) => vec![GridArg::Bytes(bytes.to_vec())],
        StreamStep::Finish => Vec::new(),
    };
    effect(GridOperation::Stream(step.method()), args)
}

/// Decode the unit's answer to one stream step: `None` for an open, else the
/// validated progress update and the Arrow chunks the step completed.
pub fn decode_stream_reply(
    method: GridStreamMethod,
    reply: &[u8],
) -> Result<Option<(GeoJsonStreamUpdate, Vec<Vec<u8>>)>, LayerImportError> {
    let Some((text, chunks)) = reply_parts(GridOperation::Stream(method), reply)? else {
        return Ok(None);
    };
    let update = serde_json::from_str(&text).map_err(|error| invalid(error.to_string()))?;
    let heads = chunks
        .iter()
        .map(|chunk| chunk_head(chunk))
        .collect::<Vec<_>>();
    Ok(Some((validate_stream_update(update, &heads)?, chunks)))
}

// ── Temporary upload grouping ───────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum UploadGroupingMode {
    Layer,
    Source,
}

/// One source layer reference; extra fields the host carries are kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct UploadSourceReference {
    pub source_file_name: String,
    pub source_layer_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub feature_count: Option<u64>,
    #[serde(flatten)]
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub extra: Map<String, Value>,
}

/// Where a grouped layer's Arrow chunks are stored by the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct UploadChunkSource {
    pub file_name: String,
    pub parsed_layer: String,
}

/// The geometry families a temporary upload layer is grouped by.
const UPLOAD_GROUP_GEOMETRIES: [&str; 5] = ["Point", "LineString", "Polygon", "Mixed", "Unknown"];

/// Preview metadata of one parsed layer; Arrow chunks stay with the host.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct UploadGroupSourceLayer {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        feature = "wasm",
        tsify(
            optional,
            type = "\"Point\" | \"LineString\" | \"Polygon\" | \"Mixed\" | \"Unknown\""
        )
    )]
    pub geometry_type: Option<String>,
    #[serde(default)]
    pub feature_count: u64,
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(type = "unknown[]"))]
    pub features: Vec<Value>,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub chunk_sources: Option<Vec<UploadChunkSource>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub source_file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub source_layer_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub source_layers: Option<Vec<UploadSourceReference>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct UploadGroupSource {
    pub file_name: String,
    pub layers: Vec<UploadGroupSourceLayer>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct UploadGroupsRequest {
    pub sources: Vec<UploadGroupSource>,
    pub mode: UploadGroupingMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct UploadGroupReference {
    #[serde(flatten)]
    pub reference: UploadSourceReference,
    /// `[source index, layer index, reference index]` of the original.
    pub member: [usize; 3],
}

/// One planned temporary layer: the members it gathers and their provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct UploadGroupLayer {
    pub name: String,
    #[cfg_attr(
        feature = "wasm",
        tsify(type = "\"Point\" | \"LineString\" | \"Polygon\" | \"Mixed\" | \"Unknown\"")
    )]
    pub geometry_type: String,
    pub feature_count: u64,
    #[cfg_attr(feature = "wasm", tsify(type = "GeoJSON.Feature[]"))]
    pub features: Vec<Value>,
    pub columns: Vec<String>,
    /// `[source index, layer index]` of every grouped layer, in order.
    pub members: Vec<[usize; 2]>,
    pub chunk_sources: Vec<UploadChunkSource>,
    pub source_layers: Vec<UploadGroupReference>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub struct UploadGroupsPlan {
    pub groups: Vec<UploadGroupLayer>,
}

/// Plan temporary layers from parsed upload previews (the same grouping the
/// parsed IPC upload sources use).
pub fn temporary_upload_groups(
    request: &UploadGroupsRequest,
) -> Result<Vec<UploadGroupLayer>, LayerImportError> {
    let input = serde_json::to_value(request)
        .and_then(serde_json::from_value::<ds_upload_rules::upload_groups::Input>)
        .map_err(|error| LayerImportError::InvalidRequest(error.to_string()))?;
    let mut planned = ds_upload_rules::upload_groups::plan(input);
    // Stamped preview features keep sorted keys, as in every reader reply.
    planned.sort_all_objects();
    validate_upload_groups(
        request,
        serde_json::from_value(planned).map_err(|e| invalid(e.to_string()))?,
    )
}

pub fn validate_upload_groups(
    request: &UploadGroupsRequest,
    groups: Vec<UploadGroupLayer>,
) -> Result<Vec<UploadGroupLayer>, LayerImportError> {
    let layer = |source: usize, index: usize| {
        request
            .sources
            .get(source)
            .and_then(|s| s.layers.get(index))
    };
    for group in &groups {
        if !UPLOAD_GROUP_GEOMETRIES.contains(&group.geometry_type.as_str()) {
            return Err(invalid(format!(
                "group {:?} has unknown geometry {:?}",
                group.name, group.geometry_type
            )));
        }
        if group.members.is_empty() {
            return Err(invalid(format!("group {:?} has no members", group.name)));
        }
        if let Some([s, l]) = group.members.iter().find(|[s, l]| layer(*s, *l).is_none()) {
            return Err(invalid(format!(
                "group {:?} names missing member [{s}, {l}]",
                group.name
            )));
        }
        if let Some(reference) = group
            .source_layers
            .iter()
            .find(|r| layer(r.member[0], r.member[1]).is_none())
        {
            return Err(invalid(format!(
                "group {:?} names a missing source reference {:?}",
                group.name, reference.member
            )));
        }
    }
    Ok(groups)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use crate::grid_engine::browser::effect_value;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(typescript_custom_section)]
    const TS_REPLY: &'static str = r#"
/** A reader reply; `chunks` are the Arrow IPC chunks of `ipc_layers`, in layer order. */
export interface LayerImportReply { response: LayerImportResponse; chunks: Uint8Array[] }
/** One streaming GeoJSON step and the Arrow IPC chunks it emitted. */
export interface GeoJsonStreamStep { update: GeoJsonStreamUpdate; chunks: Uint8Array[] }
/** The Network unit effect a page-data request names; the host runs it on that unit's entry. */
export interface PageDataEffect { unit: 'network'; entry: 'grid.model' | 'grid.stream'; payload: Uint8Array }
"#;

    fn js_error(error: LayerImportError) -> JsError {
        JsError::new(&error.to_string())
    }
    fn response_error(error: impl std::fmt::Display) -> JsError {
        js_error(LayerImportError::InvalidResponse(error.to_string()))
    }

    /// Objects cross with JSON semantics in both directions, as the previous
    /// JSON wire did: an `undefined` optional field is absent, and a number is
    /// a JavaScript double (an integer beyond 2^53 rounds, never refuses).
    fn from_js<T: serde::de::DeserializeOwned>(
        value: &JsValue,
        error: fn(String) -> LayerImportError,
    ) -> Result<T, JsError> {
        let text: String = js_sys::JSON::stringify(value)
            .map_err(|_| js_error(error("not a JSON value".into())))?
            .into();
        serde_json::from_str(&text).map_err(|e| js_error(error(e.to_string())))
    }
    fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
        let text = serde_json::to_string(value).map_err(response_error)?;
        js_sys::JSON::parse(&text).map_err(|_| response_error("could not encode the reply"))
    }
    fn js_chunks(chunks: Vec<Vec<u8>>) -> js_sys::Array {
        chunks
            .into_iter()
            .map(|bytes| JsValue::from(js_sys::Uint8Array::from(bytes.as_slice())))
            .collect()
    }
    fn pair(key: &str, value: JsValue, chunks: JsValue) -> Result<JsValue, JsError> {
        let out = js_sys::Object::new();
        js_sys::Reflect::set(&out, &JsValue::from_str(key), &value)
            .and_then(|_| js_sys::Reflect::set(&out, &JsValue::from_str("chunks"), &chunks))
            .map_err(|_| response_error("could not assemble the reply"))?;
        Ok(out.into())
    }
    fn effect(effect: Result<GridEffect, LayerImportError>) -> Result<JsValue, JsError> {
        effect_value(&effect.map_err(js_error)?)
            .map_err(|_| response_error("could not assemble the effect"))
    }

    #[wasm_bindgen(js_name = encodeLayerImport)]
    pub fn encode(request: tsify::Ts<LayerImportRequest>) -> Result<Vec<u8>, JsError> {
        let request = from_js(&request.js_value(), LayerImportError::InvalidRequest)?;
        encode_request(&request).map_err(js_error)
    }

    /// The Network unit effect for one declared reader over its byte parts.
    #[wasm_bindgen(js_name = layerImportEffect, unchecked_return_type = "PageDataEffect")]
    pub fn request(input: &[u8], parts: Vec<js_sys::Uint8Array>) -> Result<JsValue, JsError> {
        let parts = parts
            .iter()
            .map(js_sys::Uint8Array::to_vec)
            .collect::<Vec<_>>();
        let parts = parts.iter().map(Vec::as_slice).collect::<Vec<_>>();
        effect(request_effect(input, &parts))
    }

    /// Decode and validate the unit's reply to a reader request.
    #[wasm_bindgen(js_name = decodeLayerImportReply, unchecked_return_type = "LayerImportReply")]
    pub fn decode(input: &[u8], reply: &[u8]) -> Result<JsValue, JsError> {
        let request = decode_request(input).map_err(js_error)?;
        let reply = decode_unit_reply(&request, reply).map_err(js_error)?;
        pair(
            "response",
            to_js(&reply.response)?,
            js_chunks(reply.chunks).into(),
        )
    }

    /// The Network unit effect for one GeoJSON stream step: `open` with the
    /// source name, `push` with bytes, or `finish`.
    #[wasm_bindgen(js_name = geoJsonStreamEffect, unchecked_return_type = "PageDataEffect")]
    pub fn stream(
        #[wasm_bindgen(unchecked_param_type = "'open' | 'push' | 'finish'")] kind: &str,
        #[wasm_bindgen(unchecked_param_type = "string | Uint8Array | undefined")] payload: JsValue,
    ) -> Result<JsValue, JsError> {
        let invalid = |message: &str| js_error(LayerImportError::InvalidRequest(message.into()));
        let name;
        let bytes;
        let step = match kind {
            "open" => {
                name = payload
                    .as_string()
                    .ok_or_else(|| invalid("a stream opens with its source name"))?;
                StreamStep::Open(&name)
            }
            "push" if payload.is_instance_of::<js_sys::Uint8Array>() => {
                bytes = js_sys::Uint8Array::new(&payload).to_vec();
                StreamStep::Push(&bytes)
            }
            "push" => return Err(invalid("a stream push carries bytes")),
            "finish" => StreamStep::Finish,
            _ => return Err(invalid("a stream step is open, push or finish")),
        };
        effect(stream_effect(&step))
    }

    /// Decode the unit's answer to one stream step: `null` for an open.
    #[wasm_bindgen(js_name = decodeGeoJsonStreamReply, unchecked_return_type = "GeoJsonStreamStep | null")]
    pub fn decode_stream(
        #[wasm_bindgen(unchecked_param_type = "'open' | 'push' | 'finish'")] kind: &str,
        reply: &[u8],
    ) -> Result<JsValue, JsError> {
        let method = match kind {
            "open" => GridStreamMethod::GeoJsonStreamOpen,
            "push" => GridStreamMethod::GeoJsonStreamPush,
            "finish" => GridStreamMethod::GeoJsonStreamFinish,
            _ => {
                return Err(js_error(LayerImportError::InvalidRequest(
                    "a stream step is open, push or finish".into(),
                )))
            }
        };
        match decode_stream_reply(method, reply).map_err(js_error)? {
            None => Ok(JsValue::NULL),
            Some((update, chunks)) => pair("update", to_js(&update)?, js_chunks(chunks).into()),
        }
    }

    #[wasm_bindgen(js_name = temporaryUploadGroups, unchecked_return_type = "UploadGroupsPlan")]
    pub fn upload_groups(request: tsify::Ts<UploadGroupsRequest>) -> Result<JsValue, JsError> {
        let request: UploadGroupsRequest =
            from_js(&request.js_value(), LayerImportError::InvalidRequest)?;
        to_js(&UploadGroupsPlan {
            groups: temporary_upload_groups(&request).map_err(js_error)?,
        })
    }
}

#[cfg(test)]
#[path = "layer_import_tests.rs"]
mod tests;
