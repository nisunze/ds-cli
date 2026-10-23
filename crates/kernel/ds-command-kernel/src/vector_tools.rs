//! Vector tools — the one application owner of the vector tool catalogue,
//! request admission and runs that browser and desktop windows use.
//!
//! Every call is a closed typed operation: a tool is one of the runnable
//! [`VectorToolId`]s (catalogue roadmap entries are described, never
//! dispatched), and layer data crosses only as Arrow IPC bytes. Geometry
//! algorithms stay with their library owners: `ds-geo-lite` primitives,
//! `ds-geo` for the IPC/WKB layer wire and outlier statistics, and `ds-io`
//! for sketch assembly. Hosts own files, rendering, terrain acquisition and
//! transport; nothing here reads a file, the network or the clock.
//!
//! JSON this module emits keeps sorted object keys, so a catalogue, a receipt
//! or an exported GeoJSON file reads byte for byte the same on every host.

mod admission;
mod collisions;
mod descriptor;
mod runner;

pub use descriptor::Refusal;
pub use ds_geo::analysis::{GeometryOutlierFinding, GeometryOutlierOutput, GeometryOutlierSummary};
pub use ds_io::sketch::{SketchFeature, SketchFeaturesOutput};

use descriptor::refuse;
use ds_geo::layer::{ipc, Layer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The closed set of tools a run may dispatch. Each serializes as its
/// catalogue command; the descriptor id is accepted as an alias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorToolId {
    #[serde(rename = "buffer", alias = "geoprocessing::Buffer")]
    Buffer,
    #[serde(rename = "sample", alias = "sampling::Points along geometry")]
    Sample,
    #[serde(rename = "intersect", alias = "overlay::Line intersections")]
    Intersect,
    #[serde(rename = "measure", alias = "geometry::Add geometry attributes")]
    Measure,
    #[serde(rename = "outliers", alias = "analysis::Outlier geometries")]
    Outliers,
    #[serde(
        rename = "random-points-area",
        alias = "sampling::Random points in area"
    )]
    RandomPointsArea,
    #[serde(rename = "collisions", alias = "analysis::Collisions")]
    Collisions,
}

impl VectorToolId {
    pub const ALL: [Self; 7] = [
        Self::Buffer,
        Self::Sample,
        Self::Intersect,
        Self::Measure,
        Self::Outliers,
        Self::RandomPointsArea,
        Self::Collisions,
    ];
    /// The catalogue command this tool runs.
    pub const fn command(self) -> &'static str {
        match self {
            Self::Buffer => "buffer",
            Self::Sample => "sample",
            Self::Intersect => "intersect",
            Self::Measure => "measure",
            Self::Outliers => "outliers",
            Self::RandomPointsArea => "random-points-area",
            Self::Collisions => "collisions",
        }
    }
}

/// Raw bytes: a browser `Uint8Array`, or a number array in JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Bytes;
            fn expecting(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {
                out.write_str("bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(bytes.to_vec()))
            }
            fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(bytes))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Bytes, A::Error> {
                let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 20));
                while let Some(byte) = seq.next_element()? {
                    bytes.push(byte);
                }
                Ok(Bytes(bytes))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
    }
}

/// A layer a request names. Data is bound beside the request as IPC; inline
/// GeoJSON is admitted only by [`prepare`], the file-import edge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorLayerReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "unknown"))]
    pub geojson: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub layer: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorProjection {
    Inline,
    Complete,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(missing_as_null))]
pub struct VectorOutputOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<VectorProjection>,
}

/// The portable request a tool's `input_schema` describes. Parameters are the
/// tool's own; its schema supplies their defaults, ranges and refusals.
/// Fields are declared in key order so JSON copies read like the catalogue.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub against: Option<VectorLayerReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<VectorOutputOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub parameters: Option<Map<String, Value>>,
    pub source: VectorLayerReference,
}

/// A request document exactly as authored: JSON text, at most 32 MiB.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorToolPrepareInput {
    pub document: String,
    pub tool: VectorToolId,
}

/// A normalized request plus the IPC of any inline GeoJSON it imported.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolPrepared {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub against_ipc: Option<Bytes>,
    pub request: VectorToolRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub source_ipc: Option<Bytes>,
}

/// One run over bound IPC chunks. `dry_run` previews without a result layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolRunInput {
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array[]"))]
    pub against: Vec<Bytes>,
    #[serde(default)]
    pub dry_run: bool,
    pub request: VectorToolRequest,
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array[]"))]
    pub source: Vec<Bytes>,
    pub tool: VectorToolId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorValueType {
    String,
    Number,
    Integer,
    Boolean,
}

/// A field a tool adds to its output, with its unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorToolField {
    pub name: String,
    #[serde(default)]
    pub nullable: bool,
    #[serde(rename = "type")]
    pub value_type: VectorValueType,
    pub unit: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorLayerFieldType {
    Text,
    Number,
    Integer,
    Boolean,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorLayerField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: VectorLayerFieldType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorGeometryFamily {
    Point,
    LineString,
    Polygon,
}

/// Bounded GeoJSON rendering, or the existing vector-tile upload pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorRenderMode {
    Geojson,
    VectorTiles,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorOutputLayer {
    pub fields: Vec<VectorLayerField>,
    pub geometry_type: VectorGeometryFamily,
    pub name: String,
    pub render_mode: VectorRenderMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorToolCounts {
    pub processed: u64,
    pub produced: u64,
    pub source_features: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolPreview {
    pub counts: VectorToolCounts,
    pub fields_added: Vec<VectorToolField>,
    /// At most five output features, as GeoJSON.
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>[]"))]
    pub sample: Vec<Value>,
    pub warnings: Vec<String>,
}

/// A source feature an answer leaves out, and the named reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorSkippedFeature {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub index: u64,
    pub reason: String,
}

/// The random-points summary a window shows beside its result.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct RandomPointsAreaStats {
    pub area_count: u64,
    pub buffered_count: u64,
    pub elevation_gaps: u64,
    pub elevation_sampled: bool,
    pub point_count: u64,
    pub skipped_no_buffer: u64,
    pub source_feature_count: u64,
}

/// The control receipt of one run. Layer data never rides here: `result` is
/// always null and the layer travels beside it as IPC.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolReceipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub against_features: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffered_count: Option<u64>,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_gaps: Option<u64>,
    /// Measure: the listed per-feature rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>[]"))]
    pub features: Option<Vec<Value>>,
    pub fields_added: Vec<VectorToolField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_ends: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_part_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub operation: VectorToolId,
    pub output_layer: VectorOutputLayer,
    pub preview: VectorToolPreview,
    pub processed: u64,
    pub produced: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_m: Option<f64>,
    /// The tool's report: [`GeometryOutlierOutput`] for outliers, the ranked
    /// region summary for collisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "unknown"))]
    pub report: Option<Value>,
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(type = "null"))]
    pub result: (),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<BTreeMap<String, u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_features: Option<Vec<VectorSkippedFeature>>,
    pub source_features: u64,
    pub tool_id: String,
    /// Measure: totals over every eligible feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub totals: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_stats: Option<RandomPointsAreaStats>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolRun {
    pub receipt: VectorToolReceipt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub result_ipc: Option<Bytes>,
}

/// IPC chunks of one layer, bounded together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorLayerChunks {
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array[]"))]
    pub chunks: Vec<Bytes>,
}

/// One layer as Arrow IPC.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorLayerIpc {
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub layer: Bytes,
}

/// Point positions as `[lng, lat, lng, lat, …]` for host terrain acquisition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorPointCoordinates {
    pub xy: Vec<f64>,
}

/// A GeoJSON FeatureCollection file, UTF-8.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorGeojsonFile {
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub bytes: Bytes,
}

/// Render a result layer as sketch features with exactly these columns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorSketchInput {
    pub columns: Vec<String>,
    pub id_prefix: String,
    #[cfg_attr(feature = "wasm", tsify(type = "Uint8Array"))]
    pub layer: Bytes,
}

/// Every vector tool operation a host may run, off or on the UI thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "input", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum VectorToolOperation {
    Prepare(VectorToolPrepareInput),
    Run(Box<VectorToolRunInput>),
    MergeLayers(VectorLayerChunks),
    PointCoordinates(VectorLayerIpc),
    ExportGeojson(VectorLayerIpc),
    SketchFeatures(VectorSketchInput),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "result", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum VectorToolResponse {
    Prepare(VectorToolPrepared),
    Run(VectorToolRun),
    MergeLayers(VectorLayerIpc),
    PointCoordinates(VectorPointCoordinates),
    ExportGeojson(VectorGeojsonFile),
    SketchFeatures(SketchFeaturesOutput),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum VectorToolStatus {
    Available,
    Roadmap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolExample {
    pub request: VectorToolRequest,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct VectorToolRefusalNote {
    pub code: String,
    pub remedy: String,
    pub when: String,
}

/// One catalogue entry: the complete contract of a tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolDescriptor {
    pub category: String,
    pub category_name: String,
    pub command: String,
    pub description: String,
    pub examples: Vec<VectorToolExample>,
    pub id: String,
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub input_schema: Value,
    pub name: String,
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, unknown>"))]
    pub output_schema: Value,
    pub refusals: Vec<VectorToolRefusalNote>,
    pub status: VectorToolStatus,
    pub summary: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct VectorToolCatalogue {
    pub tools: Vec<VectorToolDescriptor>,
}

fn request_invalid(message: impl std::fmt::Display) -> Refusal {
    refuse("vector_request_invalid", message.to_string())
}

fn response_invalid(message: impl std::fmt::Display) -> Refusal {
    Refusal {
        code: "vector_response_invalid",
        message: message.to_string(),
        remedy: "The reply does not match the vector tool contract; select a matching Kernel.",
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&json!(self).to_string())
    }
}

impl std::error::Error for Refusal {}

/// The full catalogue, roadmap entries included.
pub fn catalogue() -> Result<VectorToolCatalogue, Refusal> {
    static TOOLS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let tools = TOOLS.get_or_init(|| {
        let mut tools =
            descriptor::describe(None).expect("the checked-in catalogue describes itself");
        tools.sort_all_objects();
        tools
    });
    serde_json::from_value(json!({ "tools": tools })).map_err(response_invalid)
}

/// Admit a catalogue id or command as one of the runnable tools. Unknown and
/// roadmap tools are refused by name; nothing outside the closed set runs.
pub fn tool_id(selector: &str) -> Result<VectorToolId, Refusal> {
    let tool = descriptor::tool(selector)?;
    if !descriptor::available(&tool.command) {
        return Err(refuse(
            "vector_tool_roadmap",
            format!("{} has a schema but no executable runner yet.", tool.name),
        ));
    }
    serde_json::from_value(json!(tool.command)).map_err(response_invalid)
}

/// Bind bounded IPC chunks into one typed layer.
fn read_chunks(chunks: &[Bytes]) -> Result<Arc<Layer>, Refusal> {
    let mut layer = Layer::default();
    let mut bytes = 0;
    for chunk in chunks {
        bytes += chunk.0.len();
        if bytes > ipc::MAX_BYTES {
            return Err(refuse(
                "vector_data_bound_exceeded",
                "IPC input exceeds 32 MiB; split the layer",
            ));
        }
        let decoded =
            ipc::decode(&chunk.0).map_err(|error| refuse("vector_document_malformed", error))?;
        if layer.features.len() + decoded.features.len() > ipc::MAX_ROWS {
            return Err(refuse(
                "vector_data_bound_exceeded",
                "IPC input exceeds 20000 rows; split the layer",
            ));
        }
        layer.features.extend(decoded.features);
    }
    Ok(Arc::new(sorted(layer)))
}

fn read_layer(bytes: &Bytes) -> Result<Layer, Refusal> {
    ipc::decode(&bytes.0)
        .map(sorted)
        .map_err(|error| refuse("vector_document_malformed", error))
}

/// Object-valued properties keep sorted keys, so a property re-encoded as IPC
/// JSON text reads the same whatever key order its source document used.
fn sorted(mut layer: Layer) -> Layer {
    for feature in &mut layer.features {
        feature.properties.sort_keys();
        feature
            .properties
            .values_mut()
            .for_each(Value::sort_all_objects);
        if let Some(id) = feature.id.as_mut() {
            id.sort_all_objects();
        }
    }
    layer
}

fn encode(layer: &Layer) -> Result<Bytes, Refusal> {
    ipc::encode(layer)
        .map(Bytes)
        .map_err(|error| refuse("vector_document_malformed", error))
}

/// The import edge: admit an authored document, convert any inline GeoJSON
/// to IPC once, and return the normalized request without a data twin.
pub fn prepare(input: &VectorToolPrepareInput) -> Result<VectorToolPrepared, Refusal> {
    if input.document.len() > ipc::MAX_BYTES {
        return Err(refuse(
            "vector_data_bound_exceeded",
            "import exceeds 32 MiB",
        ));
    }
    let id = input.tool.command();
    let mut request: Value = serde_json::from_str(&input.document).map_err(|error| Refusal {
        code: "vector_request_invalid",
        message: error.to_string(),
        remedy: "Use the descriptor input_schema.",
    })?;
    request.sort_all_objects();
    let mut prepared = VectorToolPrepared::default();
    if ["source", "against"]
        .iter()
        .any(|port| request[*port].get("geojson").is_some())
    {
        request = admission::prepare(id, request)?;
        for port in ["source", "against"] {
            let Some(value) = request[port].get("geojson") else {
                continue;
            };
            let layer = Layer::import_geojson(value)
                .map_err(|error| refuse("vector_document_malformed", error))?;
            let bytes = encode(&layer)?;
            if port == "source" {
                prepared.source_ipc = Some(bytes);
            } else {
                prepared.against_ipc = Some(bytes);
            }
            let reference = request[port]
                .as_object_mut()
                .expect("normalized port is an object");
            reference.remove("geojson");
            if !reference.contains_key("file") {
                reference.insert("file".into(), json!(format!("{port}.arrow")));
            }
        }
    }
    admission::reject_json_data(&request)?;
    let mut normalized = admission::prepare(id, request)?;
    normalized.sort_all_objects();
    prepared.request = serde_json::from_value(normalized).map_err(request_invalid)?;
    Ok(prepared)
}

/// The binary data-plane runner. JSON carries only the control receipt.
pub fn run(input: &VectorToolRunInput) -> Result<VectorToolRun, Refusal> {
    let request = serde_json::to_value(&input.request).map_err(request_invalid)?;
    admission::reject_json_data(&request)?;
    let source = read_chunks(&input.source)?;
    let against = if input.against.is_empty() {
        None
    } else {
        Some(read_chunks(&input.against)?)
    };
    let result = runner::run(
        input.tool.command(),
        request,
        source,
        against,
        runner::RunOptions {
            dry_run: input.dry_run,
        },
    )?;
    let result_ipc = result.layer.map(|layer| encode(&layer)).transpose()?;
    let mut metadata = result.metadata;
    metadata.sort_all_objects();
    let receipt = serde_json::from_value(metadata).map_err(response_invalid)?;
    Ok(VectorToolRun {
        receipt,
        result_ipc,
    })
}

pub fn execute(operation: &VectorToolOperation) -> Result<VectorToolResponse, Refusal> {
    use VectorToolOperation as Operation;
    use VectorToolResponse as Response;
    let response = match operation {
        Operation::Prepare(input) => Response::Prepare(prepare(input)?),
        Operation::Run(input) => Response::Run(run(input)?),
        Operation::MergeLayers(input) => Response::MergeLayers(VectorLayerIpc {
            layer: encode(&*read_chunks(&input.chunks)?)?,
        }),
        Operation::PointCoordinates(input) => {
            let layer = read_layer(&input.layer)?;
            Response::PointCoordinates(VectorPointCoordinates {
                xy: layer
                    .features
                    .iter()
                    .filter_map(|feature| feature.geometry.as_ref())
                    .flat_map(|geometry| geometry.points().into_iter().flatten())
                    .collect(),
            })
        }
        Operation::ExportGeojson(input) => {
            let mut collection = read_layer(&input.layer)?.export_geojson();
            collection.sort_all_objects();
            Response::ExportGeojson(VectorGeojsonFile {
                bytes: Bytes(serde_json::to_vec(&collection).map_err(response_invalid)?),
            })
        }
        Operation::SketchFeatures(input) => {
            let layer = read_layer(&input.layer)?;
            Response::SketchFeatures(ds_io::sketch::sketch_features_from_layer(
                ds_io::sketch::SketchFeaturesInput {
                    features: layer
                        .features
                        .iter()
                        .map(|feature| {
                            let mut value = feature.export_geojson();
                            value.sort_all_objects();
                            value
                        })
                        .collect(),
                    columns: input.columns.clone(),
                    id_prefix: input.id_prefix.clone(),
                },
            ))
        }
    };
    validate_response(operation, &response)?;
    Ok(response)
}

/// An Arrow IPC file opens and closes with its magic; anything else is not a layer.
fn is_ipc_file(bytes: &Bytes) -> bool {
    bytes.0.len() >= 12 && bytes.0.starts_with(b"ARROW1") && bytes.0.ends_with(b"ARROW1")
}

/// A reply must answer the operation asked, in the declared shape.
pub fn validate_response(
    operation: &VectorToolOperation,
    response: &VectorToolResponse,
) -> Result<(), Refusal> {
    use VectorToolOperation as Operation;
    use VectorToolResponse as Response;
    match (operation, response) {
        (Operation::Prepare(_), Response::Prepare(prepared)) => {
            let ports = [
                Some(&prepared.request.source),
                prepared.request.against.as_ref(),
            ];
            if ports.iter().flatten().any(|port| port.geojson.is_some()) {
                return Err(response_invalid(
                    "a prepared request still carries inline GeoJSON",
                ));
            }
            for (bytes, port) in [
                (&prepared.source_ipc, Some(&prepared.request.source)),
                (&prepared.against_ipc, prepared.request.against.as_ref()),
            ] {
                if let Some(bytes) = bytes {
                    if !is_ipc_file(bytes) || port.and_then(|port| port.file.as_ref()).is_none() {
                        return Err(response_invalid(
                            "imported IPC is not bound to a named file",
                        ));
                    }
                }
            }
            Ok(())
        }
        (Operation::Run(input), Response::Run(run)) => {
            let receipt = &run.receipt;
            if receipt.operation != input.tool || receipt.dry_run != input.dry_run {
                return Err(response_invalid("the receipt answers another tool or mode"));
            }
            if input.tool == VectorToolId::RandomPointsArea && receipt.ui_stats.is_none() {
                return Err(response_invalid(
                    "a random-points receipt carries its summary",
                ));
            }
            let layer_expected = !input.dry_run && input.tool != VectorToolId::Collisions;
            match &run.result_ipc {
                Some(bytes) if layer_expected && is_ipc_file(bytes) => {}
                None if !layer_expected => {}
                _ => {
                    return Err(response_invalid(
                        "the result layer does not match the run mode",
                    ))
                }
            }
            let counts = receipt.preview.counts;
            if counts.processed != receipt.processed
                || counts.produced != receipt.produced
                || counts.source_features != receipt.source_features
                || receipt.preview.sample.len() > 5
            {
                return Err(response_invalid("the preview disagrees with the receipt"));
            }
            if input.tool == VectorToolId::Outliers {
                let report = receipt.report.clone().unwrap_or(Value::Null);
                serde_json::from_value::<GeometryOutlierOutput>(report)
                    .map_err(response_invalid)?;
            }
            Ok(())
        }
        (Operation::MergeLayers(_), Response::MergeLayers(layer)) => is_ipc_file(&layer.layer)
            .then_some(())
            .ok_or_else(|| response_invalid("the merged layer is not Arrow IPC")),
        (Operation::PointCoordinates(_), Response::PointCoordinates(points)) => {
            if points.xy.len() % 2 == 0 {
                Ok(())
            } else {
                Err(response_invalid(
                    "point coordinates must be longitude/latitude pairs",
                ))
            }
        }
        (Operation::ExportGeojson(_), Response::ExportGeojson(file)) => {
            if file.bytes.0.first() == Some(&b'{') {
                Ok(())
            } else {
                Err(response_invalid("the export is not a GeoJSON document"))
            }
        }
        (Operation::SketchFeatures(input), Response::SketchFeatures(output)) => {
            let columns: std::collections::BTreeSet<_> = input.columns.iter().collect();
            if output.features.iter().all(|feature| {
                feature
                    .properties
                    .keys()
                    .collect::<std::collections::BTreeSet<_>>()
                    == columns
            }) {
                Ok(())
            } else {
                Err(response_invalid(
                    "sketch features do not carry exactly the requested columns",
                ))
            }
        }
        _ => Err(response_invalid("the reply answers another operation")),
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refusal(error: Refusal) -> JsError {
        JsError::new(&error.to_string())
    }

    #[wasm_bindgen(js_name = vectorToolCatalogue)]
    pub fn catalogue() -> Result<tsify::Ts<VectorToolCatalogue>, JsError> {
        super::catalogue()
            .map_err(refusal)?
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }

    #[wasm_bindgen(js_name = vectorToolId)]
    pub fn tool(selector: &str) -> Result<tsify::Ts<VectorToolId>, JsError> {
        tool_id(selector)
            .map_err(refusal)?
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }

    #[wasm_bindgen(js_name = vectorTool)]
    pub fn run(
        operation: tsify::Ts<VectorToolOperation>,
    ) -> Result<tsify::Ts<VectorToolResponse>, JsError> {
        let operation = operation
            .to_rust()
            .map_err(|error| refusal(request_invalid(error)))?;
        execute(&operation)
            .map_err(refusal)?
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }

    #[wasm_bindgen(js_name = decodeVectorTool)]
    pub fn decode(
        operation: tsify::Ts<VectorToolOperation>,
        response: tsify::Ts<VectorToolResponse>,
    ) -> Result<tsify::Ts<VectorToolResponse>, JsError> {
        let operation = operation
            .to_rust()
            .map_err(|error| refusal(request_invalid(error)))?;
        let response = response
            .to_rust()
            .map_err(|error| refusal(response_invalid(error)))?;
        validate_response(&operation, &response).map_err(refusal)?;
        response
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }
}

#[cfg(test)]
#[path = "vector_tools_tests.rs"]
mod tests;
