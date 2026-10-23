//! Browser file readers as typed application operations.
//!
//! The application admits one declared reader request with its explicit byte
//! parts, runs the `ds-io` reader that native Core also uses, encodes parsed
//! layers as bounded Arrow IPC chunks for transport, and validates the reply.
//! Every format decision (parsing, CRS, admission, provenance collapse) stays
//! in the reader library; no file system, network or clock is touched here.
//!
//! Key order is explicit. This build enables serde_json `preserve_order`; the
//! readers' default build iterates object keys sorted. Every feature, nested
//! property object and style document is sorted before it is encoded or
//! returned, so columns, Arrow chunks and replies equal the readers' default
//! output whatever the map implementation.
use ds_io as io;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Features per Arrow IPC chunk of an imported layer.
pub const LAYER_IMPORT_FEATURES_PER_CHUNK: usize = io::IMPORT_FEATURES_PER_BATCH;
const ARROW_FILE_MAGIC: &[u8] = b"ARROW1";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct ImportMapCenter {
    pub lng: f64,
    pub lat: f64,
}

/// One declared reader. Every reader takes one byte part, except `shapefile`
/// (`.shp`, `.dbf`, `.prj`; an empty part is an absent sidecar). Text readers
/// take UTF-8 bytes; `sketch_features` takes a UTF-8 JSON array of features.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum LayerImportRequest {
    GeojsonLayers {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "wasm", tsify(optional))]
        crs: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "wasm", tsify(optional))]
        center: Option<ImportMapCenter>,
    },
    TransformerBackupLayers {
        name: String,
    },
    KmlLayers {
        name: String,
    },
    KmlInspect {
        name: String,
    },
    KmzLayers {
        name: String,
    },
    KmzInspect {
        name: String,
    },
    ArchiveLayers {
        name: String,
    },
    ArchiveInspect {
        name: String,
    },
    PlsCaddXmlLayers {
        name: String,
    },
    PlsCaddXmlInspect {
        name: String,
    },
    PlsCaddBackupLayers {
        name: String,
        #[serde(default)]
        #[cfg_attr(feature = "wasm", tsify(optional))]
        options: io::PlsCaddImportOptions,
    },
    PlsCaddBackupProbe,
    ShapefileZip {
        name: String,
    },
    Shapefile {
        name: String,
    },
    SourceInspect {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "wasm", tsify(optional))]
        separator: Option<String>,
    },
    SourceLayer {
        name: String,
        input: io::SourceLayerInput,
    },
    SourceLayers {
        name: String,
        input: io::SourceLayerInput,
    },
    SketchFeatures {
        columns: Vec<String>,
        id_prefix: String,
    },
}

/// One parsed layer whose features travel as Arrow IPC chunks beside the
/// reply (`chunk_feature_counts[i]` features in chunk `i`, in order).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct ImportedIpcLayer {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub crs: Option<io::LayerCrsInfo>,
    pub feature_count: usize,
    #[serde(default)]
    pub geometry_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wasm", tsify(optional, type = "Record<string, unknown>"))]
    pub style_document: Option<Value>,
    pub columns: Vec<String>,
    pub chunk_feature_counts: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum LayerImportResponse {
    IpcLayers(Vec<ImportedIpcLayer>),
    Inspection(io::ArchiveInspection),
    Envelope(io::GeoLayerImportEnvelope),
    SourceInspection(io::SourceInspection),
    SourceLayer(io::SourceLayerResult),
    BackupProbe(bool),
    SketchFeatures(io::sketch::SketchFeaturesOutput),
}

/// A reply and the Arrow IPC chunks of its `ipc_layers`, flattened in layer order.
#[derive(Clone, Debug)]
pub struct LayerImportReply {
    pub response: LayerImportResponse,
    pub chunks: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerImportError {
    InvalidRequest(String),
    InvalidResponse(String),
    /// The reader refused the bytes; its own message (often coded) is kept.
    Reader(String),
}
impl std::fmt::Display for LayerImportError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => {
                write!(out, "layer_import_request_invalid: {message}")
            }
            Self::InvalidResponse(message) => {
                write!(out, "layer_import_response_invalid: {message}")
            }
            Self::Reader(message) => out.write_str(message),
        }
    }
}
impl std::error::Error for LayerImportError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplyKind {
    IpcLayers,
    Inspection,
    Envelope,
    SourceInspection,
    SourceLayer,
    BackupProbe,
    SketchFeatures,
}

impl LayerImportRequest {
    fn parts(&self) -> usize {
        if matches!(self, Self::Shapefile { .. }) {
            3
        } else {
            1
        }
    }
    fn reply_kind(&self) -> ReplyKind {
        match self {
            Self::GeojsonLayers { .. }
            | Self::TransformerBackupLayers { .. }
            | Self::KmlLayers { .. }
            | Self::KmzLayers { .. }
            | Self::ArchiveLayers { .. }
            | Self::PlsCaddXmlLayers { .. }
            | Self::PlsCaddBackupLayers { .. }
            | Self::SourceLayers { .. } => ReplyKind::IpcLayers,
            Self::KmlInspect { .. }
            | Self::KmzInspect { .. }
            | Self::ArchiveInspect { .. }
            | Self::PlsCaddXmlInspect { .. } => ReplyKind::Inspection,
            Self::ShapefileZip { .. } | Self::Shapefile { .. } => ReplyKind::Envelope,
            Self::SourceInspect { .. } => ReplyKind::SourceInspection,
            Self::SourceLayer { .. } => ReplyKind::SourceLayer,
            Self::PlsCaddBackupProbe => ReplyKind::BackupProbe,
            Self::SketchFeatures { .. } => ReplyKind::SketchFeatures,
        }
    }
}

impl LayerImportResponse {
    fn kind(&self) -> ReplyKind {
        match self {
            Self::IpcLayers(_) => ReplyKind::IpcLayers,
            Self::Inspection(_) => ReplyKind::Inspection,
            Self::Envelope(_) => ReplyKind::Envelope,
            Self::SourceInspection(_) => ReplyKind::SourceInspection,
            Self::SourceLayer(_) => ReplyKind::SourceLayer,
            Self::BackupProbe(_) => ReplyKind::BackupProbe,
            Self::SketchFeatures(_) => ReplyKind::SketchFeatures,
        }
    }
}

pub fn validate_request(request: &LayerImportRequest) -> Result<(), LayerImportError> {
    use LayerImportRequest as Request;
    let name = match request {
        Request::PlsCaddBackupProbe => return Ok(()),
        Request::SketchFeatures { id_prefix, columns } => {
            let mut seen = std::collections::BTreeSet::new();
            if let Some(column) = columns.iter().find(|column| !seen.insert(*column)) {
                return Err(LayerImportError::InvalidRequest(format!(
                    "sketch column {column:?} is listed twice"
                )));
            }
            id_prefix
        }
        Request::GeojsonLayers { name, center, .. } => {
            if center.is_some_and(|c| !c.lng.is_finite() || !c.lat.is_finite()) {
                return Err(LayerImportError::InvalidRequest(
                    "map center must be finite".into(),
                ));
            }
            name
        }
        Request::TransformerBackupLayers { name }
        | Request::KmlLayers { name }
        | Request::KmlInspect { name }
        | Request::KmzLayers { name }
        | Request::KmzInspect { name }
        | Request::ArchiveLayers { name }
        | Request::ArchiveInspect { name }
        | Request::PlsCaddXmlLayers { name }
        | Request::PlsCaddXmlInspect { name }
        | Request::PlsCaddBackupLayers { name, .. }
        | Request::ShapefileZip { name }
        | Request::Shapefile { name }
        | Request::SourceInspect { name, .. }
        | Request::SourceLayer { name, .. }
        | Request::SourceLayers { name, .. } => name,
    };
    if name.trim().is_empty() {
        return Err(LayerImportError::InvalidRequest(
            "a source name (or sketch id prefix) is required".into(),
        ));
    }
    Ok(())
}

pub fn decode_request(bytes: &[u8]) -> Result<LayerImportRequest, LayerImportError> {
    let request = serde_json::from_slice(bytes)
        .map_err(|error| LayerImportError::InvalidRequest(error.to_string()))?;
    validate_request(&request)?;
    Ok(request)
}

pub fn encode_request(request: &LayerImportRequest) -> Result<Vec<u8>, LayerImportError> {
    validate_request(request)?;
    serde_json::to_vec(request).map_err(|error| LayerImportError::InvalidRequest(error.to_string()))
}

/// `File.text()` semantics: a leading UTF-8 byte-order mark is dropped and
/// invalid sequences become U+FFFD.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).into_owned()
}

fn optional(part: &[u8]) -> Option<&[u8]> {
    (!part.is_empty()).then_some(part)
}

fn reader<T>(result: Result<T, String>) -> Result<T, LayerImportError> {
    result.map_err(LayerImportError::Reader)
}

/// Sort every JSON object a reply carries (see the module note).
pub fn canonical_envelope(mut envelope: io::GeoLayerImportEnvelope) -> io::GeoLayerImportEnvelope {
    for layer in &mut envelope.layers {
        layer.features.iter_mut().for_each(Value::sort_all_objects);
        if let Some(style) = &mut layer.style_document {
            style.sort_all_objects();
        }
    }
    envelope
}

/// Encode an envelope's layers as Arrow IPC chunks in the upload working-set
/// format (the `ds-cleaning` codec the upload chunks use). Columns are the
/// reader's schema, or every property key seen.
pub fn envelope_to_ipc(
    envelope: io::GeoLayerImportEnvelope,
) -> Result<(Vec<ImportedIpcLayer>, Vec<Vec<u8>>), LayerImportError> {
    let envelope = canonical_envelope(envelope);
    let mut layers = Vec::with_capacity(envelope.layers.len());
    let mut chunks = Vec::new();
    for layer in envelope.layers {
        let schema_columns = layer
            .schema
            .iter()
            .map(|field| field.name.clone())
            .collect::<Vec<_>>();
        let columns = if schema_columns.is_empty() {
            ds_cleaning::arrow_ipc::columns_for_features(&layer.features)
        } else {
            schema_columns
        };
        let mut chunk_feature_counts = Vec::new();
        for features in layer.features.chunks(LAYER_IMPORT_FEATURES_PER_CHUNK) {
            chunks.push(
                ds_cleaning::arrow_ipc::features_to_ipc(features, &columns)
                    .map_err(LayerImportError::Reader)?,
            );
            chunk_feature_counts.push(features.len());
        }
        layers.push(ImportedIpcLayer {
            name: layer.name,
            crs: layer.crs,
            feature_count: layer.features.len(),
            geometry_type: layer.geometry_type,
            style_document: layer.style_document,
            columns,
            chunk_feature_counts,
        });
    }
    Ok((layers, chunks))
}

fn ipc(envelope: io::GeoLayerImportEnvelope) -> Result<LayerImportReply, LayerImportError> {
    let (layers, chunks) = envelope_to_ipc(envelope)?;
    Ok(LayerImportReply {
        response: LayerImportResponse::IpcLayers(layers),
        chunks,
    })
}

fn plain(response: LayerImportResponse) -> Result<LayerImportReply, LayerImportError> {
    Ok(LayerImportReply {
        response,
        chunks: Vec::new(),
    })
}

/// Run one admitted reader over its explicit byte parts.
pub fn execute(
    request: &LayerImportRequest,
    parts: &[&[u8]],
) -> Result<LayerImportReply, LayerImportError> {
    use LayerImportRequest as Request;
    use LayerImportResponse as Response;
    validate_request(request)?;
    if parts.len() != request.parts() {
        return Err(LayerImportError::InvalidRequest(format!(
            "expected {} byte part(s), received {}",
            request.parts(),
            parts.len()
        )));
    }
    let bytes = parts[0];
    match request {
        Request::GeojsonLayers { name, crs, center } => {
            ipc(reader(io::geojson_import(io::GeoJsonImportInput {
                text: io::GeoTextImportInput {
                    name: name.clone(),
                    text: text(bytes),
                },
                crs: crs.clone(),
                center: center.map(|c| io::GeoJsonMapCenter {
                    lng: c.lng,
                    lat: c.lat,
                }),
            }))?)
        }
        Request::TransformerBackupLayers { name } => ipc(reader(
            io::transformer_backup_json_to_layer(io::GeoTextImportInput {
                name: name.clone(),
                text: text(bytes),
            }),
        )?),
        Request::KmlLayers { name } => {
            ipc(reader(io::kml_text_to_layer(io::GeoTextImportInput {
                name: name.clone(),
                text: text(bytes),
            }))?)
        }
        Request::KmlInspect { name } => plain(Response::Inspection(reader(
            io::kml_text_inspect_entry(io::GeoTextImportInput {
                name: name.clone(),
                text: text(bytes),
            }),
        )?)),
        Request::KmzLayers { name } => ipc(reader(io::kmz_bytes_to_layer(name, bytes))?),
        Request::KmzInspect { name } => plain(Response::Inspection(reader(
            io::kmz_bytes_inspect_entry(name, bytes),
        )?)),
        Request::ArchiveLayers { name } => ipc(io::upload_provenance::collapse(
            reader(io::archive_bytes_to_layer(name, bytes))?,
            name,
        )),
        Request::ArchiveInspect { name } => plain(Response::Inspection(
            io::upload_provenance::collapse_inspection(
                reader(io::archive_bytes_inspect(name, bytes))?,
                name,
            ),
        )),
        Request::PlsCaddXmlLayers { name } => ipc(reader(io::pls_cadd_xml_to_layer(
            io::PlsCaddXmlImportInput {
                name: name.clone(),
                bytes: bytes.to_vec(),
                input_projection: None,
            },
        ))?),
        Request::PlsCaddXmlInspect { name } => plain(Response::Inspection(reader(
            io::pls_cadd_xml_inspect(name, bytes),
        )?)),
        Request::PlsCaddBackupLayers { name, options } => ipc(reader(
            io::pls_cadd_backup_to_layers_with(name, bytes, *options),
        )?),
        Request::PlsCaddBackupProbe => plain(Response::BackupProbe(
            io::pls_cadd_is_backup_container(bytes),
        )),
        Request::ShapefileZip { name } => plain(Response::Envelope(canonical_envelope(reader(
            io::shp_zip_bytes_to_layer(name, bytes),
        )?))),
        Request::Shapefile { name } => plain(Response::Envelope(canonical_envelope(reader(
            io::shp_dbf_prj_bytes_to_layer(name, bytes, optional(parts[1]), optional(parts[2])),
        )?))),
        Request::SourceInspect { name, separator } => {
            plain(Response::SourceInspection(reader(io::source_inspect(
                name,
                bytes,
                io::SourceInspectInput {
                    separator: separator.clone(),
                },
            ))?))
        }
        Request::SourceLayer { name, input } => {
            let mut result = reader(io::source_to_layer(name, bytes, input.clone()))?;
            result.envelope = canonical_envelope(result.envelope);
            plain(Response::SourceLayer(result))
        }
        Request::SourceLayers { name, input } => ipc(io::upload_provenance::collapse(
            reader(io::source_to_layer(name, bytes, input.clone()))?.envelope,
            name,
        )),
        Request::SketchFeatures { columns, id_prefix } => {
            let mut features: Vec<Value> = serde_json::from_slice(bytes).map_err(|error| {
                LayerImportError::InvalidRequest(format!("sketch features: {error}"))
            })?;
            // Object-valued properties stringify in sorted key order.
            features.iter_mut().for_each(Value::sort_all_objects);
            plain(Response::SketchFeatures(
                io::sketch::sketch_features_from_layer(io::sketch::SketchFeaturesInput {
                    features,
                    columns: columns.clone(),
                    id_prefix: id_prefix.clone(),
                }),
            ))
        }
    }
}

fn invalid(message: impl Into<String>) -> LayerImportError {
    LayerImportError::InvalidResponse(message.into())
}

fn validate_ipc_layers(
    layers: &[ImportedIpcLayer],
    chunk_heads: &[(usize, [u8; 6], [u8; 6])],
) -> Result<(), LayerImportError> {
    let mut next = 0usize;
    for layer in layers {
        let mut seen = std::collections::BTreeSet::new();
        if let Some(column) = layer.columns.iter().find(|column| !seen.insert(*column)) {
            return Err(invalid(format!(
                "layer {:?} lists column {column:?} twice",
                layer.name
            )));
        }
        let mut total = 0usize;
        for count in &layer.chunk_feature_counts {
            if *count == 0 || *count > LAYER_IMPORT_FEATURES_PER_CHUNK {
                return Err(invalid(format!(
                    "layer {:?} has a chunk of {count} features",
                    layer.name
                )));
            }
            total += count;
        }
        if total != layer.feature_count {
            return Err(invalid(format!(
                "layer {:?} claims {} features but its chunks hold {total}",
                layer.name, layer.feature_count
            )));
        }
        next += layer.chunk_feature_counts.len();
    }
    if next != chunk_heads.len() {
        return Err(invalid(format!(
            "the reply names {next} Arrow IPC chunk(s) but carries {}",
            chunk_heads.len()
        )));
    }
    for (index, (length, head, tail)) in chunk_heads.iter().enumerate() {
        if *length < 2 * ARROW_FILE_MAGIC.len()
            || head != ARROW_FILE_MAGIC
            || tail != ARROW_FILE_MAGIC
        {
            return Err(invalid(format!("chunk {index} is not an Arrow IPC file")));
        }
    }
    Ok(())
}

fn validate_envelope(envelope: &io::GeoLayerImportEnvelope) -> Result<(), LayerImportError> {
    for layer in &envelope.layers {
        if let Some(index) = layer
            .features
            .iter()
            .position(|feature| feature.get("type").and_then(Value::as_str) != Some("Feature"))
        {
            return Err(invalid(format!(
                "layer {:?} feature {index} is not a GeoJSON Feature",
                layer.name
            )));
        }
    }
    Ok(())
}

/// The first and last six bytes of a chunk, which is all validation reads.
pub fn chunk_head(bytes: &[u8]) -> (usize, [u8; 6], [u8; 6]) {
    let mut head = [0u8; 6];
    let mut tail = [0u8; 6];
    if bytes.len() >= 6 {
        head.copy_from_slice(&bytes[..6]);
        tail.copy_from_slice(&bytes[bytes.len() - 6..]);
    }
    (bytes.len(), head, tail)
}

/// Validate a reply against the request that produced it. Chunks are checked
/// by length and Arrow file magic only; their bytes are never copied.
pub fn validate_response(
    request: &LayerImportRequest,
    response: LayerImportResponse,
    chunk_heads: &[(usize, [u8; 6], [u8; 6])],
) -> Result<LayerImportResponse, LayerImportError> {
    validate_request(request)?;
    if response.kind() != request.reply_kind() {
        return Err(invalid("reply kind does not match the request"));
    }
    match &response {
        LayerImportResponse::IpcLayers(layers) => validate_ipc_layers(layers, chunk_heads)?,
        _ if !chunk_heads.is_empty() => {
            return Err(invalid("only layer replies carry Arrow IPC chunks"));
        }
        LayerImportResponse::Envelope(envelope) => validate_envelope(envelope)?,
        LayerImportResponse::SourceLayer(result) => validate_envelope(&result.envelope)?,
        LayerImportResponse::SketchFeatures(output) => {
            if let Some(feature) = output.features.iter().find(|f| f.geometry.is_null()) {
                return Err(invalid(format!(
                    "sketch feature {:?} has no geometry",
                    feature.id
                )));
            }
        }
        LayerImportResponse::Inspection(_)
        | LayerImportResponse::SourceInspection(_)
        | LayerImportResponse::BackupProbe(_) => {}
    }
    Ok(response)
}

pub fn decode_response(
    request: &LayerImportRequest,
    bytes: &[u8],
    chunk_heads: &[(usize, [u8; 6], [u8; 6])],
) -> Result<LayerImportResponse, LayerImportError> {
    validate_response(
        request,
        serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?,
        chunk_heads,
    )
}

// ── Streaming GeoJSON ───────────────────────────────────────────────────

/// Progress of a streaming GeoJSON import after one pushed byte slice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(missing_as_null))]
pub struct GeoJsonStreamUpdate {
    pub name: String,
    pub feature_count: usize,
    #[serde(default)]
    pub geometry_type: Option<String>,
    pub columns: Vec<String>,
    pub done: bool,
    /// Features in each Arrow IPC chunk emitted by this step, in order.
    pub chunk_feature_counts: Vec<usize>,
}

/// A streaming GeoJSON import held by its host: bytes in, bounded chunks out.
pub struct GeoJsonStreamImport {
    stream: io::GeoJsonFeatureStream,
}

impl GeoJsonStreamImport {
    pub fn new(name: &str) -> Result<Self, LayerImportError> {
        if name.trim().is_empty() {
            return Err(LayerImportError::InvalidRequest(
                "a source name is required".into(),
            ));
        }
        Ok(Self {
            stream: io::GeoJsonFeatureStream::new(name),
        })
    }

    fn step(
        &self,
        batches: Vec<io::GeoJsonFeatureBatch>,
    ) -> Result<(GeoJsonStreamUpdate, Vec<Vec<u8>>), LayerImportError> {
        let mut chunks = Vec::with_capacity(batches.len());
        let mut chunk_feature_counts = Vec::with_capacity(batches.len());
        for batch in batches {
            chunks.push(
                ds_cleaning::arrow_ipc::features_to_ipc(&batch.features, &batch.columns)
                    .map_err(LayerImportError::Reader)?,
            );
            chunk_feature_counts.push(batch.features.len());
        }
        Ok((
            GeoJsonStreamUpdate {
                name: self.stream.name().to_string(),
                feature_count: self.stream.feature_count(),
                geometry_type: self.stream.geometry_type().map(str::to_string),
                columns: self.stream.columns().to_vec(),
                done: self.stream.done(),
                chunk_feature_counts,
            },
            chunks,
        ))
    }

    pub fn push(
        &mut self,
        bytes: &[u8],
    ) -> Result<(GeoJsonStreamUpdate, Vec<Vec<u8>>), LayerImportError> {
        let batches = self.stream.push(bytes).map_err(LayerImportError::Reader)?;
        self.step(batches)
    }

    pub fn finish(&mut self) -> Result<(GeoJsonStreamUpdate, Vec<Vec<u8>>), LayerImportError> {
        let batches = self.stream.finish().map_err(LayerImportError::Reader)?;
        self.step(batches)
    }
}

pub fn validate_stream_update(
    update: GeoJsonStreamUpdate,
    chunk_heads: &[(usize, [u8; 6], [u8; 6])],
) -> Result<GeoJsonStreamUpdate, LayerImportError> {
    let emitted: usize = update.chunk_feature_counts.iter().sum();
    if emitted > update.feature_count {
        return Err(invalid(
            "a stream step emitted more features than it has read",
        ));
    }
    validate_ipc_layers(
        &[ImportedIpcLayer {
            name: update.name.clone(),
            crs: None,
            feature_count: emitted,
            geometry_type: None,
            style_document: None,
            columns: update.columns.clone(),
            chunk_feature_counts: update.chunk_feature_counts.clone(),
        }],
        chunk_heads,
    )?;
    Ok(update)
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
        .and_then(serde_json::from_value::<ds_cleaning::upload_groups::Input>)
        .map_err(|error| LayerImportError::InvalidRequest(error.to_string()))?;
    let mut planned = ds_cleaning::upload_groups::plan(input);
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
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(typescript_custom_section)]
    const TS_REPLY: &'static str = r#"
/** A reader reply; `chunks` are the Arrow IPC chunks of `ipc_layers`, in layer order. */
export interface LayerImportReply { response: LayerImportResponse; chunks: Uint8Array[] }
/** One streaming GeoJSON step and the Arrow IPC chunks it emitted. */
export interface GeoJsonStreamStep { update: GeoJsonStreamUpdate; chunks: Uint8Array[] }
"#;

    fn js_error(error: LayerImportError) -> JsError {
        JsError::new(&error.to_string())
    }
    fn request_error(error: impl std::fmt::Display) -> JsError {
        js_error(LayerImportError::InvalidRequest(error.to_string()))
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

    fn heads(chunks: &[js_sys::Uint8Array]) -> Vec<(usize, [u8; 6], [u8; 6])> {
        chunks
            .iter()
            .map(|chunk| {
                let length = chunk.length() as usize;
                let mut head = [0u8; 6];
                let mut tail = [0u8; 6];
                if length >= 6 {
                    chunk.subarray(0, 6).copy_to(&mut head);
                    chunk
                        .subarray((length - 6) as u32, length as u32)
                        .copy_to(&mut tail);
                }
                (length, head, tail)
            })
            .collect()
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

    #[wasm_bindgen(js_name = encodeLayerImport)]
    pub fn encode(request: tsify::Ts<LayerImportRequest>) -> Result<Vec<u8>, JsError> {
        let request = from_js(&request.js_value(), LayerImportError::InvalidRequest)?;
        encode_request(&request).map_err(js_error)
    }

    /// Run one declared reader over its byte parts (a worker operation).
    #[wasm_bindgen(js_name = layerImport, unchecked_return_type = "LayerImportReply")]
    pub fn run(input: &[u8], parts: Vec<js_sys::Uint8Array>) -> Result<JsValue, JsError> {
        let request = decode_request(input).map_err(js_error)?;
        let parts = parts
            .iter()
            .map(js_sys::Uint8Array::to_vec)
            .collect::<Vec<_>>();
        let parts = parts.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let reply = execute(&request, &parts).map_err(js_error)?;
        pair(
            "response",
            to_js(&reply.response)?,
            js_chunks(reply.chunks).into(),
        )
    }

    /// Validate a reader reply against its request; chunk bytes stay in place.
    #[wasm_bindgen(js_name = decodeLayerImport, unchecked_return_type = "LayerImportReply")]
    pub fn decode(
        input: &[u8],
        response: tsify::Ts<LayerImportResponse>,
        chunks: Vec<js_sys::Uint8Array>,
    ) -> Result<JsValue, JsError> {
        let request = decode_request(input).map_err(js_error)?;
        let response = from_js(&response.js_value(), LayerImportError::InvalidResponse)?;
        let response = validate_response(&request, response, &heads(&chunks)).map_err(js_error)?;
        let array: js_sys::Array = chunks.into_iter().map(JsValue::from).collect();
        pair("response", to_js(&response)?, array.into())
    }

    /// A streaming GeoJSON import held by the page; push bytes, then finish.
    #[wasm_bindgen(js_name = GeoJsonStreamImport)]
    pub struct GeoJsonStreamImportHandle(GeoJsonStreamImport);

    #[wasm_bindgen(js_class = GeoJsonStreamImport)]
    impl GeoJsonStreamImportHandle {
        #[wasm_bindgen(constructor)]
        pub fn new(name: &str) -> Result<GeoJsonStreamImportHandle, JsError> {
            GeoJsonStreamImport::new(name)
                .map(GeoJsonStreamImportHandle)
                .map_err(js_error)
        }
        #[wasm_bindgen(unchecked_return_type = "GeoJsonStreamStep")]
        pub fn push(&mut self, bytes: &[u8]) -> Result<JsValue, JsError> {
            let (update, chunks) = self.0.push(bytes).map_err(js_error)?;
            pair("update", to_js(&update)?, js_chunks(chunks).into())
        }
        #[wasm_bindgen(unchecked_return_type = "GeoJsonStreamStep")]
        pub fn finish(&mut self) -> Result<JsValue, JsError> {
            let (update, chunks) = self.0.finish().map_err(js_error)?;
            pair("update", to_js(&update)?, js_chunks(chunks).into())
        }
    }

    #[wasm_bindgen(js_name = decodeGeoJsonStreamStep, unchecked_return_type = "GeoJsonStreamStep")]
    pub fn decode_stream(
        update: tsify::Ts<GeoJsonStreamUpdate>,
        chunks: Vec<js_sys::Uint8Array>,
    ) -> Result<JsValue, JsError> {
        let update = from_js(&update.js_value(), LayerImportError::InvalidResponse)?;
        let update = validate_stream_update(update, &heads(&chunks)).map_err(js_error)?;
        let array: js_sys::Array = chunks.into_iter().map(JsValue::from).collect();
        pair("update", to_js(&update)?, array.into())
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
