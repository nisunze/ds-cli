//! One transformer report run, decided once for every native host.
//!
//! The desktop shell (`ds-web/src-tauri/src/network_reporter.rs`), the `ds`
//! executable and a headless server all drive the same engine —
//! `ds-report export-transformer-report` — from the same governed inputs: the
//! project's server-minted Network Reporter input receipt, one exact
//! transformer room and the project's output policy. What those inputs mean
//! is decided here, without IO: which formats to ask for, what the input-base
//! fingerprint and the room digest are, the exact request document the engine
//! reads, whether the engine's result answers the request, and what a run's
//! receipt says. Hosts stage bytes, start the engine, verify files and hold
//! the clock.
//!
//! Every recipe here is byte-identical to the one the desktop shell and
//! ds-brain already apply, so a server-produced receipt carries the same
//! `input_base_fingerprint` and `room_content_sha256` the publication contract
//! admits against.

pub mod jcs;
pub mod proof;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::printing::table_rows::PrintWarning;
use crate::printing::ArtifactPresentation;
use crate::report::{publication_state, PublicationState, LOCAL_ONLY_SCHEMA};
use crate::report_formats::{self, DesignOutputSelection, Output};

/// The wire schema of the receipt ds-brain mints beside a project's
/// configuration (`network_reporter_input_receipt.go`).
pub const INPUT_RECEIPT_SCHEMA: u32 = 1;
/// The member of the `/config/{project}` document that carries it.
pub const INPUT_RECEIPT_MEMBER: &str = "network_reporter_input_receipt";
/// The engine task this module plans and verifies.
pub const REPORT_TASK: &str = "export_transformer_report";
/// The engine subcommand a host names for that task.
pub const REPORT_SUBCOMMAND: &str = "export-transformer-report";
/// The publication engine identity every receipt carries.
pub const ENGINE: &str = "network_reporter";
/// The publication variant of an individual transformer report.
pub const VARIANT: &str = "default";
/// The input shape a native host stages: plain GeoJSON layers and the cached
/// `{groups, sheets, models}` configuration.
pub const INPUT_SHAPE: &str = "plain_local";
/// The publication batch schema of a release engine's run.
pub const PUBLICATION_SCHEMA: u32 = 3;
/// The most semantic request bytes (room plus configuration) one run stages.
pub const MAX_SEMANTIC_REQUEST_BYTES: u64 = 64 * 1024 * 1024;
/// The most bytes a result document may occupy.
pub const MAX_RESULT_BYTES: u64 = 1024 * 1024;
/// The largest artifact a receipt may describe; a streamed publication never
/// materializes it, so this is the transfer bound, not a preview bound.
pub const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// The most print warnings one result may carry.
pub const MAX_PRINT_WARNINGS: usize = 2048;
/// The most transformers one batch may run.
pub const MAX_BATCH_TRANSFORMERS: usize = 2000;
/// The most engine processes a batch may keep resident at once.
pub const MAX_RESIDENT_LIMIT: usize = 64;
/// The resident bound a host uses when the operator names none — the same
/// four the browser batch runner defaulted to.
pub const DEFAULT_RESIDENT_LIMIT: usize = 4;
pub const RUN_RECEIPT_SCHEMA: &str = "ds.report-run/v1";
pub const BATCH_RECEIPT_SCHEMA: &str = "ds.report-batch/v1";
/// The receipt file a host writes beside a run's artifacts.
pub const RUN_RECEIPT_FILE: &str = "report-run.json";
/// The receipt file a host writes at the root of a batch's output directory.
pub const BATCH_RECEIPT_FILE: &str = "report-batch.json";
/// The wire schema of the print-context document a host stages beside a
/// transformer's room and the engine reads digest-bound
/// (`ds-network-reporter` `load_print_context`).
pub const PRINT_CONTEXT_SCHEMA: &str = "ds.print-context/v1";
/// The most context layers one print-context document may carry — the same
/// bound a print layout may request.
pub const MAX_PRINT_CONTEXT_LAYERS: usize = 64;
/// The most features one print-context document may carry, over every layer.
pub const MAX_PRINT_CONTEXT_FEATURES: usize = 100_000;
/// The most bytes one print-context document may occupy: the engine's bound.
pub const MAX_PRINT_CONTEXT_BYTES: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Identities and bounds
// ---------------------------------------------------------------------------

pub fn exact_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A publishable project identity: what ds-brain's compute registry accepts.
pub fn valid_project_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// A publishable transformer identity: ds-brain's canonical reference name,
/// already canonical — the same predicate the desktop shell applies before it
/// stages a room.
pub fn valid_transformer_identity(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 121
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return false;
    }
    let core = value.trim_matches('_');
    if core.is_empty() {
        return false;
    }
    let canonical = match core {
        "all_transformers" | "combined_transformers" | "combined_transformer" => {
            "combined_transformer".to_string()
        }
        _ if core.as_bytes()[0].is_ascii_digit() => format!("_{core}"),
        _ => core.to_string(),
    };
    value == canonical
}

/// A transformer an individual report may be run for: publishable, and not
/// one of the names a report produces or the project holds as a document.
pub fn reportable_transformer(value: &str) -> Result<(), String> {
    if !valid_transformer_identity(value) {
        return Err(format!(
            "transformer {value:?} is not a publishable canonical identity"
        ));
    }
    if crate::design::is_special_row(value) {
        return Err(format!(
            "transformer {value:?} is a reserved identity, never an individual report"
        ));
    }
    Ok(())
}

fn valid_semantic_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

fn decode_sha256(value: &str) -> Result<[u8; 32], String> {
    if !exact_lower_sha256(value) {
        return Err("Network Reporter input digest is invalid".to_string());
    }
    let mut decoded = [0_u8; 32];
    for (index, output) in decoded.iter_mut().enumerate() {
        let pair = &value.as_bytes()[index * 2..index * 2 + 2];
        let nibble = |byte: u8| -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => unreachable!("exact_lower_sha256 already validated the byte"),
            }
        };
        *output = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Ok(decoded)
}

/// `network_reporter/input-base/v2`: the exact server-owned input base of one
/// transformer export, as ds-brain fingerprints it for `work/open` admission
/// and the desktop shell records it. Variable-length strings are uint32
/// big-endian length-prefixed, the version is uint64 big-endian, and both
/// digests contribute their decoded bytes.
pub fn input_base_fingerprint(
    transformer: &str,
    server_version: i64,
    sheets_sha256: &str,
    country: &str,
    reference_semantic_sha256: &str,
) -> Result<String, String> {
    if !valid_transformer_identity(transformer) || server_version < 0 {
        return Err("Network Reporter transformer input version is invalid".to_string());
    }
    let transformer_len = u32::try_from(transformer.len())
        .map_err(|_| "Network Reporter transformer identity is too long".to_string())?;
    let country_len = u32::try_from(country.len())
        .map_err(|_| "Network Reporter country is too long".to_string())?;
    let sheets_digest = decode_sha256(sheets_sha256)?;
    let reference_digest = decode_sha256(reference_semantic_sha256)?;
    let mut preimage =
        Vec::with_capacity(31 + 4 + transformer.len() + 8 + 32 + 4 + country.len() + 32);
    preimage.extend_from_slice(b"network_reporter/input-base/v2\0");
    preimage.extend_from_slice(&transformer_len.to_be_bytes());
    preimage.extend_from_slice(transformer.as_bytes());
    preimage.extend_from_slice(&(server_version as u64).to_be_bytes());
    preimage.extend_from_slice(&sheets_digest);
    preimage.extend_from_slice(&country_len.to_be_bytes());
    preimage.extend_from_slice(country.as_bytes());
    preimage.extend_from_slice(&reference_digest);
    Ok(format!("{:x}", Sha256::digest(preimage)))
}

// ---------------------------------------------------------------------------
// The project's reporter input receipt
// ---------------------------------------------------------------------------

/// The server-minted project-wide portion of a report's input base: the
/// reporting country, the exact settings sheets the report policy reads (as
/// the bytes ds-brain serialized, never re-encoded) and the semantic identity
/// of the Rwanda administrative reference the artifacts are stamped with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReceipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_print_recipe: Option<proof::Provenance>,
    pub schema: u32,
    pub country: String,
    pub sheets_json: String,
    pub sheets_sha256: String,
    pub reference_semantic_sha256: String,
}

impl InputReceipt {
    /// Read the receipt out of one `/config/{project}` document — the same
    /// member the browser reads from its cached copy — and prove it.
    pub fn from_config(document: &Value) -> Result<Self, String> {
        let value = document.get(INPUT_RECEIPT_MEMBER).ok_or_else(|| {
            format!("the project configuration carries no `{INPUT_RECEIPT_MEMBER}`; refresh the project configuration and retry")
        })?;
        let receipt: Self = serde_json::from_value(value.clone()).map_err(|error| {
            format!("the Network Reporter input receipt is unreadable: {error}")
        })?;
        receipt.validate()?;
        Ok(receipt)
    }

    /// The receipt is well-formed and its sheets bytes are the ones it names.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INPUT_RECEIPT_SCHEMA
            || !valid_semantic_text(&self.country, 80)
            || self.sheets_json.is_empty()
            || self.sheets_json.len() as u64 > MAX_SEMANTIC_REQUEST_BYTES
            || !exact_lower_sha256(&self.sheets_sha256)
            || !exact_lower_sha256(&self.reference_semantic_sha256)
        {
            return Err("Network Reporter input receipt is invalid".to_string());
        }
        if format!("{:x}", Sha256::digest(self.sheets_json.as_bytes())) != self.sheets_sha256 {
            return Err(
                "Network Reporter sheets_json does not match its server receipt".to_string(),
            );
        }
        let sheets: Value = serde_json::from_str(&self.sheets_json)
            .map_err(|error| format!("Network Reporter sheets_json is invalid: {error}"))?;
        if !sheets.as_object().is_some_and(|object| !object.is_empty()) {
            return Err("Network Reporter sheets_json must be a non-empty JSON object".to_string());
        }
        Ok(())
    }

    /// A Rwanda project is stamped with the administrative reference; every
    /// other country scrubs those columns and must not stage the asset.
    pub fn requires_admin_bounds(&self) -> bool {
        self.country.trim().eq_ignore_ascii_case("Rwanda")
    }

    pub fn sheets(&self) -> Result<Value, String> {
        serde_json::from_str(&self.sheets_json)
            .map_err(|error| format!("Network Reporter sheets_json is invalid: {error}"))
    }

    /// The exact `{groups, sheets, models}` configuration document the engine
    /// reads under `plain_local`, spliced around the server's own sheets bytes
    /// so a Go spelling (an int64 above 2^53, `-0`, an escaped separator)
    /// survives unchanged.
    pub fn network_config_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        const PREFIX: &[u8] = b"{\"groups\":{},\"sheets\":";
        const SUFFIX: &[u8] = b",\"models\":{}}";
        let capacity = PREFIX
            .len()
            .checked_add(self.sheets_json.len())
            .and_then(|size| size.checked_add(SUFFIX.len()))
            .ok_or_else(|| "Network Reporter config size overflowed".to_string())?;
        let mut config = Vec::with_capacity(capacity);
        config.extend_from_slice(PREFIX);
        config.extend_from_slice(self.sheets_json.as_bytes());
        config.extend_from_slice(SUFFIX);
        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// The room
// ---------------------------------------------------------------------------

/// What one room holds, proven before it is staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RoomEvidence {
    pub layers: usize,
    pub features: usize,
}

fn validate_feature_collection(name: &str, collection: &Value) -> Result<usize, String> {
    if !valid_semantic_text(name, 256) {
        return Err("Network Reporter layer names must be canonical non-empty text".to_string());
    }
    let object = collection
        .as_object()
        .ok_or_else(|| format!("Network Reporter layer {name:?} must be a GeoJSON collection"))?;
    if object.len() != 2
        || object.get("type").and_then(Value::as_str) != Some("FeatureCollection")
        || !object.contains_key("features")
    {
        return Err(format!(
            "Network Reporter layer {name:?} must contain only type=FeatureCollection and features"
        ));
    }
    let features = object
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Network Reporter layer {name:?} features must be an array"))?;
    for (index, feature) in features.iter().enumerate() {
        if feature.get("type").and_then(Value::as_str) != Some("Feature") {
            return Err(format!(
                "Network Reporter layer {name:?} feature[{index}] is not a GeoJSON Feature"
            ));
        }
    }
    Ok(features.len())
}

/// The room is the exact `{layers: {name: FeatureCollection}}` projection the
/// engine consumes, with at least one feature somewhere.
pub fn validate_layers(layers: &BTreeMap<String, Value>) -> Result<RoomEvidence, String> {
    if layers.is_empty() {
        return Err("Network Reporter export requires local transformer layers".to_string());
    }
    let features = layers
        .iter()
        .try_fold(0_usize, |total, (name, collection)| {
            let count = validate_feature_collection(name, collection)?;
            total
                .checked_add(count)
                .ok_or_else(|| "Network Reporter feature count overflowed".to_string())
        })?;
    if features == 0 {
        return Err("Network Reporter export has no local features".to_string());
    }
    Ok(RoomEvidence {
        layers: layers.len(),
        features,
    })
}

/// The exact transformer document a host stages: `{"layers": …}`, the
/// desktop room projection, in the layer map's own order.
pub fn transformer_document_bytes(layers: &BTreeMap<String, Value>) -> Result<Vec<u8>, String> {
    #[derive(Serialize)]
    struct PlainTransformerDocument<'a> {
        layers: &'a BTreeMap<String, Value>,
    }
    serde_json::to_vec(&PlainTransformerDocument { layers })
        .map_err(|error| format!("could not encode local transformer layers: {error}"))
}

// ---------------------------------------------------------------------------
// The print context
// ---------------------------------------------------------------------------

/// Lowercase hex SHA-256 of `bytes` — the digest form every receipt and
/// engine request in this module carries.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The exact `ds.print-context/v1` document a host stages beside a
/// transformer's room, and that the engine reads digest-bound.
///
/// One recipe for every host — the desktop shell assembled this document
/// itself until now — so a desktop and a server given the same coverage,
/// sources and layers stage byte-identical assets under one digest:
/// `{"schema","coverage","sources","layers"}` in that order, layers in the
/// map's own order, compact serialization.
///
/// Validation mirrors what the engine would refuse, so a bad document is
/// refused where its layers were gathered rather than after an engine
/// started: `coverage` is the area the layers were read for (a Polygon or
/// MultiPolygon); every layer id is an identifier and its collection exactly
/// `{type: FeatureCollection, features}` of Features with object properties
/// and an object or null geometry; no layer is empty — the engine drops an
/// empty collection silently, and a host names what it could not supply in
/// the run receipt instead; every `sources` entry is an object naming a
/// `layer` of the document; and the whole stays inside the feature and byte
/// bounds the engine enforces.
pub fn print_context_document(
    coverage: &Value,
    sources: &Value,
    layers: &BTreeMap<String, Value>,
) -> Result<Vec<u8>, String> {
    crate::reference_cache::validate_area(coverage)
        .map_err(|error| format!("print context coverage: {error}"))?;
    if layers.is_empty() {
        return Err("print context has no layers".to_string());
    }
    if layers.len() > MAX_PRINT_CONTEXT_LAYERS {
        return Err(format!(
            "print context carries {} layers, over the {MAX_PRINT_CONTEXT_LAYERS}-layer bound",
            layers.len()
        ));
    }
    let mut features = 0_usize;
    for (id, collection) in layers {
        if !crate::printing::identifier(id) {
            return Err(format!(
                "print context layer id {id:?} is not an identifier"
            ));
        }
        let object = collection.as_object().ok_or_else(|| {
            format!("print context layer {id:?} must be a GeoJSON FeatureCollection")
        })?;
        if object.len() != 2
            || object.get("type").and_then(Value::as_str) != Some("FeatureCollection")
        {
            return Err(format!(
                "print context layer {id:?} must contain only type=FeatureCollection and features"
            ));
        }
        let rows = object
            .get("features")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("print context layer {id:?} features must be an array"))?;
        if rows.is_empty() {
            return Err(format!("print context layer {id:?} is empty"));
        }
        for (index, feature) in rows.iter().enumerate() {
            let shaped = feature.get("type").and_then(Value::as_str) == Some("Feature")
                && feature.get("properties").is_some_and(Value::is_object)
                && feature
                    .get("geometry")
                    .is_some_and(|geometry| geometry.is_object() || geometry.is_null());
            if !shaped {
                return Err(format!(
                    "print context layer {id:?} feature[{index}] is not a GeoJSON Feature with object properties"
                ));
            }
        }
        features = features
            .checked_add(rows.len())
            .filter(|total| *total <= MAX_PRINT_CONTEXT_FEATURES)
            .ok_or_else(|| {
                format!("print context carries more than {MAX_PRINT_CONTEXT_FEATURES} features")
            })?;
    }
    let entries = sources
        .as_array()
        .ok_or("print context sources must be an array")?;
    if entries.len() > MAX_PRINT_CONTEXT_LAYERS {
        return Err(format!(
            "print context names {} sources, over the {MAX_PRINT_CONTEXT_LAYERS}-source bound",
            entries.len()
        ));
    }
    for (index, entry) in entries.iter().enumerate() {
        let named = entry
            .get("layer")
            .and_then(Value::as_str)
            .is_some_and(|layer| layers.contains_key(layer));
        if !entry.is_object() || !named {
            return Err(format!(
                "print context source[{index}] must be an object naming a layer of the document"
            ));
        }
    }
    let bytes = serde_json::to_vec(&json!({
        "schema": PRINT_CONTEXT_SCHEMA,
        "coverage": coverage,
        "sources": sources,
        "layers": layers,
    }))
    .map_err(|error| format!("could not encode print context: {error}"))?;
    if bytes.len() as u64 > MAX_PRINT_CONTEXT_BYTES {
        return Err(format!(
            "print context is {} bytes, over the 64 MiB bound",
            bytes.len()
        ));
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// Everything one run is bound to before the engine starts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_print_recipe: Option<proof::Provenance>,
    pub project_id: String,
    pub transformer: String,
    pub server_version: i64,
    pub country: String,
    pub requires_admin_bounds: bool,
    /// The engine formats, in the policy's order; also the output ids.
    pub formats: Vec<String>,
    pub outputs: Vec<Output>,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
    /// ds-brain's `metadata.content_digest` recipe over the same layers, so
    /// a host that fetched the room from the service can prove it holds the
    /// bytes the service holds.
    pub content_digest: String,
    pub sheets_sha256: String,
    pub reference_semantic_sha256: String,
    pub room: RoomEvidence,
}

/// Decide one run from its governed inputs. Refuses an unpublishable
/// identity, a reserved name, an empty room, a receipt that does not prove
/// its sheets, and a selection the project cannot render.
pub fn plan_transformer_export(
    project_id: &str,
    receipt: &InputReceipt,
    transformer: &str,
    server_version: i64,
    layers: &BTreeMap<String, Value>,
    selection: Option<&DesignOutputSelection>,
) -> Result<ExportPlan, String> {
    if !valid_project_id(project_id) {
        return Err("Network Reporter project identity is not publishable".to_string());
    }
    reportable_transformer(transformer)?;
    if !(0..=9_007_199_254_740_991).contains(&server_version) {
        return Err(
            "Network Reporter local version must be a non-negative exact JavaScript integer"
                .to_string(),
        );
    }
    receipt.validate()?;
    let room = validate_layers(layers)?;
    let sheets = receipt.sheets()?;
    let outputs = report_formats::selected_plan(&sheets, false, selection)?;
    if outputs.is_empty() {
        return Err("the project's output policy names no individual report output".to_string());
    }
    let formats = outputs
        .iter()
        .map(|output| output.format.clone())
        .collect::<Vec<_>>();
    let room_content_sha256 = jcs::room_content_sha256(layers).map_err(|error| {
        format!("Network Reporter room is not RFC 8785 canonicalizable: {error}")
    })?;
    let content_digest = jcs::layers_content_digest(layers).map_err(|error| {
        format!("Network Reporter room is not RFC 8785 canonicalizable: {error}")
    })?;
    let input_base_fingerprint = input_base_fingerprint(
        transformer,
        server_version,
        &receipt.sheets_sha256,
        &receipt.country,
        &receipt.reference_semantic_sha256,
    )?;
    Ok(ExportPlan {
        local_print_recipe: receipt.local_print_recipe.clone(),
        project_id: project_id.to_string(),
        transformer: transformer.to_string(),
        server_version,
        country: receipt.country.clone(),
        requires_admin_bounds: receipt.requires_admin_bounds(),
        formats,
        outputs,
        input_base_fingerprint,
        room_content_sha256,
        content_digest,
        sheets_sha256: receipt.sheets_sha256.clone(),
        reference_semantic_sha256: receipt.reference_semantic_sha256.clone(),
        room,
    })
}

// ---------------------------------------------------------------------------
// The engine request
// ---------------------------------------------------------------------------

/// The files a host staged for one run. Paths are the host's; digests are the
/// exact bytes it wrote or verified.
#[derive(Debug, Clone)]
pub struct StagedInputs<'a> {
    pub transformer_document: &'a Path,
    pub network_config: &'a Path,
    pub out_dir: &'a Path,
    /// The verified Rwanda villages asset and the SHA-256 of its exact bytes.
    pub admin_bounds: Option<(&'a Path, &'a str)>,
    pub media_grant: Option<&'a Path>,
    /// A `ds.print-context/v1` document and the SHA-256 of its exact bytes.
    pub print_context: Option<(&'a Path, &'a str)>,
    /// This transformer's one-based position in the drawing set and the set's
    /// size, so a sheet can print "12 of 86"; `None` prints a set of one.
    pub sheet: Option<(u32, u32)>,
}

/// The exact typed request document `ds-report export-transformer-report`
/// reads. The country decides whether the administrative asset may be staged
/// at all: a Rwanda run without it is refused before the engine reports the
/// same, and another country never stages Rwanda bounds.
pub fn engine_request(plan: &ExportPlan, staged: &StagedInputs<'_>) -> Result<Value, String> {
    if plan.requires_admin_bounds != staged.admin_bounds.is_some() {
        return Err(if plan.requires_admin_bounds {
            "a Rwanda report requires the verified administrative reference asset".to_string()
        } else {
            "Network Reporter must not stage Rwanda administrative bounds for another country"
                .to_string()
        });
    }
    if staged
        .admin_bounds
        .is_some_and(|(_, digest)| !exact_lower_sha256(digest))
        || staged
            .print_context
            .is_some_and(|(_, digest)| !exact_lower_sha256(digest))
    {
        return Err("a staged input digest is not a lowercase SHA-256".to_string());
    }
    let mut request = serde_json::Map::new();
    request.insert(
        "transformer_document".into(),
        json!(staged.transformer_document),
    );
    request.insert("network_config".into(), json!(staged.network_config));
    request.insert("input_shape".into(), json!(INPUT_SHAPE));
    request.insert("transformer".into(), json!(plan.transformer));
    request.insert("out_dir".into(), json!(staged.out_dir));
    request.insert("country".into(), json!(plan.country));
    if let Some((path, digest)) = staged.admin_bounds {
        request.insert("admin_bounds_asset".into(), json!(path));
        request.insert("admin_bounds_sha256".into(), json!(digest));
    }
    if let Some(path) = staged.media_grant {
        request.insert("media_grant".into(), json!(path));
    }
    if let Some((index, count)) = staged.sheet {
        if index == 0 || count == 0 || index > count {
            return Err(format!(
                "sheet {index} of {count} is not a position in a drawing set"
            ));
        }
        request.insert("sheet_index".into(), json!(index));
        request.insert("sheet_count".into(), json!(count));
    }
    if let Some((path, digest)) = staged.print_context {
        request.insert("print_context_asset".into(), json!(path));
        request.insert("print_context_sha256".into(), json!(digest));
    }
    request.insert("formats".into(), json!(plan.formats));
    Ok(Value::Object(request))
}

// ---------------------------------------------------------------------------
// The engine identity
// ---------------------------------------------------------------------------

/// The release identity of the engine that produced a run, read from
/// `ds-report build-info`. Artifacts are bound to it; a receipt names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineIdentity {
    pub engine_version: String,
    pub build_manifest_sha256: String,
    pub package_version: String,
    pub source_sha: String,
    pub profile: String,
}

impl EngineIdentity {
    /// A development build's artifacts stay local; a release build's may be
    /// published.
    pub fn publication_state(&self) -> Result<PublicationState, String> {
        publication_state(&self.engine_version, true, false)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildInfo {
    schema: String,
    service: String,
    binary: String,
    package_version: String,
    source_sha: String,
    engine_version: String,
    cargo_lock_sha256: String,
    features: Vec<String>,
    rustc: String,
    target: String,
    profile: String,
    schemas: Vec<String>,
    build_manifest_sha256: String,
}

/// Prove one `build-info` document is the reporter's closed build identity.
pub fn engine_identity(document: &Value) -> Result<EngineIdentity, String> {
    let info: BuildInfo = serde_json::from_value(document.clone())
        .map_err(|error| format!("reporter build identity is malformed: {error}"))?;
    if info.schema != "ds.engine-build/v1"
        || info.service != "ds-network-reporter"
        || info.binary != "ds-report"
        || info.engine_version
            != format!(
                "ds-network-reporter@{}+{}",
                info.package_version, info.source_sha
            )
        || !exact_lower_sha256(&info.build_manifest_sha256)
        || !exact_lower_sha256(&info.cargo_lock_sha256)
        || info.rustc.is_empty()
        || info.target.is_empty()
        || info.profile.is_empty()
        || info.schemas.is_empty()
        || info.features.len() > 64
    {
        return Err("reporter build identity is outside the closed contract".to_string());
    }
    // A release identity or the one development spelling; anything else is
    // not an engine this product knows.
    publication_state(&info.engine_version, true, false)?;
    Ok(EngineIdentity {
        engine_version: info.engine_version,
        build_manifest_sha256: info.build_manifest_sha256,
        package_version: info.package_version,
        source_sha: info.source_sha,
        profile: info.profile,
    })
}

// ---------------------------------------------------------------------------
// The result document
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    pub format: String,
    pub filename: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ArtifactPresentation>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Blocker {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AdminBoundsEvidence {
    pub configured: bool,
    pub villages_loaded: usize,
    pub features_matched: usize,
    pub features_requested: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultDocument {
    #[serde(default)]
    warnings: Vec<PrintWarning>,
    task: String,
    status: String,
    transformer: String,
    country: String,
    layers_decoded: usize,
    features_decoded: usize,
    admin_bounds: AdminBoundsEvidence,
    geometry_changed: bool,
    formats_requested: Vec<String>,
    artifacts: Vec<ArtifactRecord>,
    blockers: Vec<Blocker>,
    elapsed_ms: u64,
}

/// One artifact the engine declared, matched to the output it answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifiedArtifact {
    pub output_id: String,
    pub format: String,
    pub filename: String,
    pub content_type: &'static str,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ArtifactPresentation>,
}

/// A completed result that answers its plan. The host still has to prove the
/// bytes on disk are the declared ones.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifiedResult {
    pub artifacts: Vec<VerifiedArtifact>,
    pub warnings: Vec<PrintWarning>,
    pub elapsed_ms: u64,
    pub geometry_changed: bool,
    pub layers_decoded: usize,
    pub features_decoded: usize,
    pub admin_bounds: AdminBoundsEvidence,
}

/// Why a result document is not an answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultRefusal {
    /// The engine ran and refused, or completed only part of the request;
    /// its own typed blockers say which format and why.
    Blocked {
        status: String,
        blockers: Vec<Blocker>,
        artifacts: usize,
    },
    /// The document is not this request's answer: malformed, mismatched, or
    /// declaring something the contract does not allow.
    Invalid { message: String },
}

impl ResultRefusal {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }
}

/// A bounded, human-readable summary of the first blockers.
pub fn blocker_summary(blockers: &[Blocker]) -> String {
    let bounded = |message: &str| {
        message
            .chars()
            .filter(|character| !character.is_control())
            .take(180)
            .collect::<String>()
    };
    blockers
        .iter()
        .take(3)
        .map(|blocker| {
            let format = blocker
                .format
                .as_deref()
                .map(|format| format!("/{format}"))
                .unwrap_or_default();
            format!(
                "{}{format}: {}",
                bounded(&blocker.code),
                bounded(&blocker.message)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The filename the engine must have given an artifact of this format.
pub fn valid_artifact_filename(filename: &str, format: &str) -> bool {
    if filename.is_empty()
        || filename.len() > 255
        || filename
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b'\0'))
        || filename.chars().any(char::is_control)
        || filename == "."
        || filename == ".."
    {
        return false;
    }
    let path = Path::new(filename);
    if path.file_name().and_then(|name| name.to_str()) != Some(filename)
        || path.components().count() != 1
    {
        return false;
    }
    match format {
        "shp" => filename.ends_with(".shp.zip"),
        "kmz" => filename.ends_with(".kmz"),
        "xlsx" => filename.ends_with(".xlsx"),
        "network_information" => filename.ends_with(".network-information.json"),
        "geojsonseq" => filename.ends_with("_geojsonl.zip"),
        "gpkg" => filename.ends_with(".gpkg"),
        "pdf_a0" => filename.ends_with(".a0.pdf"),
        "pdf_a3" => filename.ends_with(".a3.pdf"),
        format => match report_formats::output(format) {
            Ok(output) if report_formats::named_print_output(format).is_some() => {
                filename.ends_with(&output.suffix)
            }
            _ => false,
        },
    }
}

fn content_type(format: &str) -> Result<&'static str, ResultRefusal> {
    report_formats::output(format)
        .map(|output| output.content_type)
        .map_err(ResultRefusal::invalid)
}

fn parse_result(document: &[u8]) -> Result<ResultDocument, ResultRefusal> {
    if document.len() as u64 > MAX_RESULT_BYTES {
        return Err(ResultRefusal::invalid(
            "Network Reporter result exceeds its byte bound",
        ));
    }
    serde_json::from_slice(document).map_err(|error| {
        ResultRefusal::invalid(format!("Network Reporter result is malformed: {error}"))
    })
}

/// Read the engine's typed refusal out of a result it wrote before exiting
/// non-zero. `None` when the document is absent, malformed, or not this
/// request's — the host then reports the process failure itself.
pub fn typed_failure(document: &[u8], plan: &ExportPlan) -> Option<ResultRefusal> {
    let result = parse_result(document).ok()?;
    if result.task != REPORT_TASK
        || result.transformer != plan.transformer
        || result.country != plan.country
        || (result.status == "completed" && result.blockers.is_empty())
    {
        return None;
    }
    Some(ResultRefusal::Blocked {
        status: result.status,
        blockers: result.blockers,
        artifacts: result.artifacts.len(),
    })
}

/// Prove one result document answers the plan: the same task, transformer
/// and country; completed with no blocker; every requested format declared
/// exactly once as a direct child of the staged output directory with a
/// plausible name, size and digest; administrative evidence consistent with
/// the country; and only the print warnings the kernel's tables can raise.
pub fn verify_result(
    document: &[u8],
    plan: &ExportPlan,
    out_dir: &Path,
) -> Result<VerifiedResult, ResultRefusal> {
    let result = parse_result(document)?;
    if result.task != REPORT_TASK
        || result.transformer != plan.transformer
        || result.country != plan.country
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter result does not match its semantic request",
        ));
    }
    if result.status != "completed" || !result.blockers.is_empty() {
        return Err(ResultRefusal::Blocked {
            status: result.status,
            blockers: result.blockers,
            artifacts: result.artifacts.len(),
        });
    }
    if result
        .formats_requested
        .iter()
        .map(String::as_str)
        .ne(plan.formats.iter().map(String::as_str))
        || result.artifacts.len() != plan.formats.len()
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter did not complete every requested format",
        ));
    }
    if result.layers_decoded == 0 || result.features_decoded == 0 {
        return Err(ResultRefusal::invalid(
            "Network Reporter result has no decoded local input",
        ));
    }
    if result.admin_bounds.features_matched > result.admin_bounds.features_requested {
        return Err(ResultRefusal::invalid(
            "Network Reporter admin-bounds evidence is inconsistent",
        ));
    }
    if plan.requires_admin_bounds {
        if !result.admin_bounds.configured || result.admin_bounds.villages_loaded == 0 {
            return Err(ResultRefusal::invalid(
                "Network Reporter did not use the verified Rwanda reference asset",
            ));
        }
    } else if result.admin_bounds.configured
        || result.admin_bounds.villages_loaded != 0
        || result.admin_bounds.features_requested != 0
        || result.admin_bounds.features_matched != 0
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter performed Rwanda administrative-bound work for another country",
        ));
    }
    let mut filenames = HashSet::new();
    let mut artifacts = Vec::with_capacity(result.artifacts.len());
    for (artifact, output) in result.artifacts.into_iter().zip(plan.outputs.iter()) {
        if artifact.format != output.format
            || content_type(&artifact.format)? != output.content_type
            || !valid_artifact_filename(&artifact.filename, &artifact.format)
            || !filenames.insert(artifact.filename.clone())
            || artifact.bytes == 0
            || artifact.bytes > MAX_ARTIFACT_BYTES
            || !exact_lower_sha256(&artifact.sha256)
        {
            return Err(ResultRefusal::invalid(
                "Network Reporter returned an invalid artifact declaration",
            ));
        }
        if Path::new(&artifact.path) != out_dir.join(&artifact.filename) {
            return Err(ResultRefusal::invalid(
                "Network Reporter artifact is not the declared direct output child",
            ));
        }
        crate::printing::validate_artifact_presentation(
            &artifact.format,
            artifact.paper_size.as_deref(),
            artifact.presentation.as_ref(),
        )
        .map_err(ResultRefusal::invalid)?;
        artifacts.push(VerifiedArtifact {
            output_id: output.output_id.clone(),
            format: artifact.format,
            filename: artifact.filename,
            content_type: output.content_type,
            size_bytes: artifact.bytes,
            sha256: artifact.sha256,
            paper_size: artifact.paper_size,
            presentation: artifact.presentation,
        });
    }
    if result.warnings.len() > MAX_PRINT_WARNINGS
        || result.warnings.iter().any(|warning| {
            let placed = warning.transformer == plan.transformer
                && plan.formats.contains(&warning.output_id)
                && report_formats::named_print_output(&warning.output_id).map(|(_, id)| id)
                    == Some(warning.layout_id.as_str())
                && warning.message.len() <= 2048;
            let valid = match warning.code.as_str() {
                // A table that could not show every row names the panel and
                // the honest counts.
                "table_truncated" => {
                    placed
                        && crate::printing::identifier(&warning.element_id)
                        && warning.omitted_rows > 0
                        && warning.omitted_rows < warning.total_rows
                        && warning.total_rows <= 1_000_000
                }
                // Measured furniture diagnostics (printing::diagnostics) and
                // their bounded summary: mechanical findings in millimetres,
                // no table rows behind them. The summary names no element.
                "print_element_clipped" | "print_furniture_overlap" | "print_layout_warning" => {
                    placed
                        && crate::printing::identifier(&warning.element_id)
                        && warning.omitted_rows == 0
                        && warning.total_rows == 0
                }
                "print_diagnostics_summary" => {
                    placed
                        && warning.element_id.is_empty()
                        && warning.omitted_rows == 0
                        && warning.total_rows == 0
                }
                // A text cell whose `{key}` had no value in the title block or
                // the project printed empty. Same shape as the summary: no
                // element, no rows.
                "print_text_unbound" => {
                    placed
                        && warning.element_id.is_empty()
                        && warning.omitted_rows == 0
                        && warning.total_rows == 0
                }
                // Cable consistency findings (report_consistency::lv_cables):
                // one row per pole whose saved cable label disagrees with its
                // LV line, mirrored on the workbook's Engineering Review
                // sheet. They ride the xlsx output and name no paper; the
                // transformer is the saved data label, which differs from the
                // run's name only by case; the element is the saved line
                // number. No table rows stand behind them.
                "lv_pole_line_cable_conflict" => {
                    warning.transformer.eq_ignore_ascii_case(&plan.transformer)
                        && warning.output_id == "xlsx"
                        && plan.formats.contains(&warning.output_id)
                        && warning.layout_id.is_empty()
                        && !warning.element_id.is_empty()
                        && warning.element_id.len() <= 240
                        && warning.omitted_rows == 0
                        && warning.total_rows == 0
                        && warning.message.len() <= 2048
                }
                _ => false,
            };
            !valid
        })
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter returned invalid print warnings",
        ));
    }
    Ok(VerifiedResult {
        artifacts,
        warnings: result.warnings,
        elapsed_ms: result.elapsed_ms,
        geometry_changed: result.geometry_changed,
        layers_decoded: result.layers_decoded,
        features_decoded: result.features_decoded,
        admin_bounds: result.admin_bounds,
    })
}

// ---------------------------------------------------------------------------
// Receipts
// ---------------------------------------------------------------------------

/// The facts a run receipt is written from. `artifact_dir` is the path the
/// host placed the artifacts under, relative to the batch root; `now_ms` is
/// the host's clock.
#[derive(Debug, Clone)]
pub struct RunReceiptFacts<'a> {
    pub plan: &'a ExportPlan,
    pub client_run_id: &'a str,
    pub engine: &'a EngineIdentity,
    pub result: &'a VerifiedResult,
    pub artifact_dir: &'a str,
    pub now_ms: u64,
    /// ds-brain's `metadata.content_digest` for the fetched room, when the
    /// host had one to compare against.
    pub expected_content_digest: Option<&'a str>,
    /// The digest of the `ds.print-context/v1` document staged beside the
    /// room (`print_context_document`), or `None` when the run printed
    /// without external context.
    pub print_context_sha256: Option<&'a str>,
    /// The layer ids that document carried, in document order.
    pub print_context_layers: &'a [String],
    /// The selected context ids the host could not supply on this run,
    /// each named so a receipt never reads as complete when it is not.
    pub print_context_omitted: &'a [String],
}

/// The publication identity this run would be enqueued under — the same
/// members the desktop shell's batch receipt carries, minus the local
/// artifact locators a pending-upload store mints.
pub fn publication_facts(plan: &ExportPlan, engine: &EngineIdentity) -> Result<Value, String> {
    let state = if plan.local_print_recipe.is_some() {
        PublicationState::LocalOnly
    } else {
        engine.publication_state()?
    };
    Ok(json!({
        "state": state,
        "schema": if state == PublicationState::LocalOnly { LOCAL_ONLY_SCHEMA } else { PUBLICATION_SCHEMA },
        "engine": ENGINE,
        "operation": format!("export-{}", plan.transformer),
        "variant": VARIANT,
        "root": format!("eds_project/{}/eds_design", plan.project_id),
        "engine_version": engine.engine_version,
        "engine_build_manifest_sha256": engine.build_manifest_sha256,
        "transformer": plan.transformer,
        "transformer_revision": plan.server_version,
        "input_base_fingerprint": plan.input_base_fingerprint,
        "room_content_sha256": plan.room_content_sha256,
    }))
}

/// Refuse a room that does not match the governing service's saved content.
pub fn verify_content_digest(plan: &ExportPlan, expected: Option<&str>) -> Result<(), String> {
    if expected.is_some_and(|digest| digest != plan.content_digest) {
        return Err("saved transformer content digest does not match the fetched layers".into());
    }
    Ok(())
}

/// The receipt one run leaves beside its artifacts.
pub fn run_receipt(facts: &RunReceiptFacts<'_>) -> Result<Value, String> {
    if !valid_run_id(facts.client_run_id) {
        return Err("Network Reporter run identity is invalid".to_string());
    }
    verify_content_digest(facts.plan, facts.expected_content_digest)?;
    match facts.print_context_sha256 {
        Some(digest) if !exact_lower_sha256(digest) => {
            return Err("Network Reporter print context digest is invalid".to_string());
        }
        Some(_) if facts.print_context_layers.is_empty() => {
            return Err("Network Reporter print context digest names no layers".to_string());
        }
        None if !facts.print_context_layers.is_empty() => {
            return Err("Network Reporter print context layers have no digest".to_string());
        }
        _ => {}
    }
    let mut context_ids = HashSet::new();
    for id in facts
        .print_context_layers
        .iter()
        .chain(facts.print_context_omitted)
    {
        if !crate::printing::identifier(id) || !context_ids.insert(id.as_str()) {
            return Err(format!(
                "Network Reporter print context names an invalid or repeated id {id:?}"
            ));
        }
    }
    let content_digest_verified = facts
        .expected_content_digest
        .map(|expected| expected == facts.plan.content_digest);
    let outputs = facts
        .result
        .artifacts
        .iter()
        .map(|artifact| {
            let mut output = json!({
                "output_id": artifact.output_id,
                "format": artifact.format,
                "filename": artifact.filename,
                "path": format!("{}/{}", facts.artifact_dir, artifact.filename),
                "content_type": artifact.content_type,
                "sha256": artifact.sha256,
                "size_bytes": artifact.size_bytes,
            });
            if let Some(paper) = &artifact.paper_size {
                output["paper_size"] = json!(paper);
            }
            if let Some(presentation) = &artifact.presentation {
                output["presentation"] = json!(presentation);
            }
            output
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": RUN_RECEIPT_SCHEMA,
        "task": REPORT_TASK,
        "status": "completed",
        "project_id": facts.plan.project_id,
        "transformer": facts.plan.transformer,
        "transformer_revision": facts.plan.server_version,
        "country": facts.plan.country,
        "client_run_id": facts.client_run_id,
        "produced_at_ms": facts.now_ms,
        "engine_version": facts.engine.engine_version,
        "engine_build_manifest_sha256": facts.engine.build_manifest_sha256,
        "input_base_fingerprint": facts.plan.input_base_fingerprint,
        "room_content_sha256": facts.plan.room_content_sha256,
        "content_digest": facts.plan.content_digest,
        "content_digest_verified": content_digest_verified,
        "sheets_sha256": facts.plan.sheets_sha256,
        "local_print_recipe": facts.plan.local_print_recipe,
        "reference_semantic_sha256": facts.plan.reference_semantic_sha256,
        "room": facts.plan.room,
        "print_context_sha256": facts.print_context_sha256,
        "print_context_layers": facts.print_context_layers,
        "print_context_omitted": facts.print_context_omitted,
        "formats_requested": facts.plan.formats,
        "outputs": outputs,
        "warnings": facts.result.warnings,
        "admin_bounds": facts.result.admin_bounds,
        "layers_decoded": facts.result.layers_decoded,
        "features_decoded": facts.result.features_decoded,
        "geometry_changed": facts.result.geometry_changed,
        "elapsed_ms": facts.result.elapsed_ms,
        "publication": publication_facts(facts.plan, facts.engine)?,
    }))
}

/// A run identity a host minted: `report-` plus 32 lowercase hex characters.
pub fn valid_run_id(value: &str) -> bool {
    value.strip_prefix("report-").is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

/// One transformer's outcome inside a batch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatchResult {
    pub transformer: String,
    /// `ok` or `error` — the batch-outcome vocabulary every host folds.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

impl BatchResult {
    pub fn ok(transformer: &str, receipt: String, artifacts: usize) -> Self {
        Self {
            transformer: transformer.to_string(),
            status: "ok",
            receipt: Some(receipt),
            artifacts: Some(artifacts),
            error: None,
        }
    }

    pub fn error(transformer: &str, code: &str, message: &str, detail: Option<Value>) -> Self {
        let mut error = json!({"code": code, "message": message});
        if let Some(detail) = detail {
            error["detail"] = detail;
        }
        Self {
            transformer: transformer.to_string(),
            status: "error",
            receipt: None,
            artifacts: None,
            error: Some(error),
        }
    }
}

/// The batch's own status: every run completed, some did, or none did.
pub fn batch_status(results: &[BatchResult]) -> &'static str {
    let failed = results.iter().filter(|r| r.status == "error").count();
    if results.is_empty() || failed == results.len() {
        "failed"
    } else if failed == 0 {
        "completed"
    } else {
        "partial"
    }
}

/// The receipt a batch leaves at the root of its output directory, results
/// in the kernel's stable order.
pub fn batch_receipt(
    project_id: &str,
    lane: &str,
    engine: Option<&EngineIdentity>,
    concurrency: usize,
    requested: &[String],
    results: &[BatchResult],
    now_ms: u64,
) -> Result<Value, String> {
    let rows = results
        .iter()
        .map(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let request = json!({"command": "batch_outcome", "results": rows});
    let reply = crate::report::evaluate(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)?;
    let reply: Value = serde_json::from_str(&reply).map_err(|error| error.to_string())?;
    let ordered = reply["result"]["results"].clone();
    let failed = reply["result"]["failed"].as_u64().unwrap_or(0);
    Ok(json!({
        "schema": BATCH_RECEIPT_SCHEMA,
        "task": REPORT_TASK,
        "status": batch_status(results),
        "project_id": project_id,
        "lane": lane,
        "produced_at_ms": now_ms,
        "engine_version": engine.map(|engine| engine.engine_version.clone()),
        "engine_build_manifest_sha256": engine.map(|engine| engine.build_manifest_sha256.clone()),
        "publication_state": engine.map(|engine| engine.publication_state()).transpose()?,
        "concurrency": concurrency,
        "requested": requested,
        "requested_count": requested.len(),
        "completed": results.len() as u64 - failed,
        "failed": failed,
        "results": ordered,
    }))
}

/// The names a batch runs, in the kernel's order, and how many engines it may
/// keep resident: `min(scope, processors, resident_limit)` — the same
/// decision the browser batch runner asked `report::plan` for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchPlan {
    pub names: Vec<String>,
    pub concurrency: usize,
}

pub fn batch_plan(
    requested: &[String],
    processors: usize,
    resident_limit: usize,
) -> Result<BatchPlan, String> {
    if requested.is_empty() {
        return Err("a report batch names at least one transformer".to_string());
    }
    if requested.len() > MAX_BATCH_TRANSFORMERS {
        return Err(format!(
            "a report batch names at most {MAX_BATCH_TRANSFORMERS} transformers"
        ));
    }
    for name in requested {
        reportable_transformer(name)?;
    }
    let rooms = requested
        .iter()
        .map(|name| json!({"name": name, "available": true, "dirty": false}))
        .collect::<Vec<_>>();
    let request = json!({
        "command": "plan",
        "requested": requested,
        "inventory": Value::Null,
        "rooms": rooms,
        "processors": processors,
        "resident_limit": resident_limit,
    });
    let reply = crate::report::evaluate(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)?;
    let reply: Value = serde_json::from_str(&reply).map_err(|error| error.to_string())?;
    let names = reply["result"]["names"]
        .as_array()
        .ok_or("report plan returned no names")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let concurrency = reply["result"]["concurrency"]
        .as_u64()
        .ok_or("report plan returned no concurrency")? as usize;
    Ok(BatchPlan { names, concurrency })
}

/// Stable one-based sheet positions across a complete active project inventory.
/// A selected export looks up its members here; it never renumbers its subset.
pub fn drawing_set_positions(active: &[String]) -> Result<BTreeMap<String, (u32, u32)>, String> {
    let mut names = batch_plan(active, 1, 1)?.names;
    names.sort();
    let count = u32::try_from(names.len()).map_err(|_| "drawing set is too large")?;
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, (index as u32 + 1, count)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn selected_sheets_retain_positions_in_the_complete_drawing_set() {
        let active = ["zihare", "agahehe", "kibungo"].map(str::to_string);
        let positions = drawing_set_positions(&active).unwrap();
        assert_eq!(positions["agahehe"], (1, 3));
        assert_eq!(positions["kibungo"], (2, 3));
        assert_eq!(positions["zihare"], (3, 3));
        assert!(!positions.contains_key("retired"));
        assert_eq!(
            drawing_set_positions(&active.iter().rev().cloned().collect::<Vec<_>>()).unwrap(),
            positions
        );
        assert!(drawing_set_positions(&[]).is_err());
        assert!(drawing_set_positions(&["combined_transformer".into()]).is_err());
    }

    const SHEETS: &str = r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx","pdf__detail"]}],"printing_setups":[]}"#;

    fn print_receipt(country: &str) -> InputReceipt {
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: country.to_string(),
            sheets_json: SHEETS.to_string(),
            sheets_sha256: format!("{:x}", Sha256::digest(SHEETS.as_bytes())),
            reference_semantic_sha256:
                "70f0de17599e81d8d85809668cb172ee6ccf68c1edecac6e3709838ab0eb50c1".to_string(),
        }
    }

    fn simple_receipt(country: &str) -> InputReceipt {
        let sheets =
            r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx"]}]}"#;
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: country.to_string(),
            sheets_json: sheets.to_string(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256:
                "70f0de17599e81d8d85809668cb172ee6ccf68c1edecac6e3709838ab0eb50c1".to_string(),
        }
    }

    fn layers() -> BTreeMap<String, Value> {
        let mut layers = BTreeMap::new();
        layers.insert(
            "lv_poles".to_string(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"pole_number":"P1"},"geometry":{"type":"Point","coordinates":[30.06,-1.94]}}]}),
        );
        layers
    }

    fn engine() -> EngineIdentity {
        EngineIdentity {
            engine_version: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            build_manifest_sha256: "b".repeat(64),
            package_version: "0.1.0".into(),
            source_sha: "a".repeat(40),
            profile: "release".into(),
        }
    }

    #[test]
    fn the_fingerprint_matches_the_desktop_shell_and_ds_brain() {
        assert_eq!(
            input_base_fingerprint(
                "_001_main",
                7,
                "da506c8a9c8a9f31aa00eaeef23d49764b9ace97158a1a0a7aa628e6d446b0fb",
                "Rwanda",
                "70f0de17599e81d8d85809668cb172ee6ccf68c1edecac6e3709838ab0eb50c1",
            )
            .unwrap(),
            "43b3f123c37d0576a8fbb4bc2833e9e57e7c5abee1ec2389e792f397e28b91b3"
        );
        assert!(input_base_fingerprint(
            "Not Canonical",
            7,
            &"a".repeat(64),
            "Rwanda",
            &"b".repeat(64)
        )
        .is_err());
        assert!(
            input_base_fingerprint("tx_a", -1, &"a".repeat(64), "Rwanda", &"b".repeat(64)).is_err()
        );
        assert!(input_base_fingerprint("tx_a", 1, "short", "Rwanda", &"b".repeat(64)).is_err());
    }

    #[test]
    fn transformer_identities_are_the_canonical_publishable_ones() {
        for ok in ["tx_a", "_001_main", "agatare", "combined_transformer"] {
            assert!(valid_transformer_identity(ok), "{ok}");
        }
        for bad in [
            "",
            "TX_A",
            "tx a",
            "001_main",
            "_",
            "all_transformers",
            "tx-a",
            &"a".repeat(122),
        ] {
            assert!(!valid_transformer_identity(bad), "{bad:?}");
        }
        // Publishable but reserved: what a report produces or the project holds.
        for reserved in [
            "combined_transformer",
            "collisions",
            "mv_data",
            "compounded_report",
        ] {
            assert!(reportable_transformer(reserved).is_err(), "{reserved}");
        }
        assert!(reportable_transformer("tx_a").is_ok());
    }

    #[test]
    fn the_receipt_is_read_from_the_configuration_and_proves_its_sheets() {
        let receipt = print_receipt("Rwanda");
        let document = json!({"sheets": {}, "network_reporter_input_receipt": receipt});
        assert_eq!(InputReceipt::from_config(&document).unwrap(), receipt);
        assert!(receipt.requires_admin_bounds());
        assert!(!simple_receipt("Benin").requires_admin_bounds());
        assert!(InputReceipt::from_config(&json!({"sheets": {}}))
            .unwrap_err()
            .contains("network_reporter_input_receipt"));
        let mut wrong = receipt.clone();
        wrong.sheets_sha256 = "1".repeat(64);
        assert!(wrong.validate().unwrap_err().contains("does not match"));
        let mut unknown = serde_json::to_value(&receipt).unwrap();
        unknown["extra"] = json!(1);
        assert!(
            InputReceipt::from_config(&json!({"network_reporter_input_receipt": unknown})).is_err()
        );
        let mut schema = receipt.clone();
        schema.schema = 2;
        assert!(schema.validate().is_err());
    }

    #[test]
    fn the_configuration_bytes_splice_the_servers_own_sheets_unchanged() {
        let sheets = r#"{"adversarial":{"big":9007199254740993,"float":100000000000000000000,"negative_zero":-0,"separators":"line paragraph end"}}"#;
        let receipt = InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Rwanda".into(),
            sheets_json: sheets.into(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256: "c".repeat(64),
        };
        assert_eq!(
            String::from_utf8(receipt.network_config_bytes().unwrap()).unwrap(),
            r#"{"groups":{},"sheets":{"adversarial":{"big":9007199254740993,"float":100000000000000000000,"negative_zero":-0,"separators":"line paragraph end"}},"models":{}}"#
        );
    }

    #[test]
    fn a_plan_binds_the_run_to_its_inputs_and_policy() {
        let receipt = simple_receipt("Benin");
        let plan = plan_transformer_export("proj-1", &receipt, "tx_a", 7, &layers(), None).unwrap();
        assert_eq!(plan.formats, vec!["shp", "xlsx"]);
        assert_eq!(plan.outputs[0].output_id, "shp");
        assert!(!plan.requires_admin_bounds);
        assert_eq!(
            plan.room,
            RoomEvidence {
                layers: 1,
                features: 1
            }
        );
        assert_eq!(
            plan.room_content_sha256,
            jcs::room_content_sha256(&layers()).unwrap()
        );
        assert_eq!(
            plan.content_digest,
            jcs::layers_content_digest(&layers()).unwrap()
        );
        assert_eq!(
            plan.input_base_fingerprint,
            input_base_fingerprint(
                "tx_a",
                7,
                &receipt.sheets_sha256,
                "Benin",
                &receipt.reference_semantic_sha256
            )
            .unwrap()
        );
        // The same decisions the desktop shell refuses with.
        assert!(plan_transformer_export("../p", &receipt, "tx_a", 7, &layers(), None).is_err());
        assert!(
            plan_transformer_export("proj-1", &receipt, "collisions", 7, &layers(), None).is_err()
        );
        assert!(plan_transformer_export("proj-1", &receipt, "tx_a", -1, &layers(), None).is_err());
        assert!(
            plan_transformer_export("proj-1", &receipt, "tx_a", 7, &BTreeMap::new(), None).is_err()
        );
        let mut empty = layers();
        empty.get_mut("lv_poles").unwrap()["features"] = json!([]);
        assert!(plan_transformer_export("proj-1", &receipt, "tx_a", 7, &empty, None).is_err());
        let mut extra = layers();
        extra.get_mut("lv_poles").unwrap()["bbox"] = json!([0, 0, 1, 1]);
        assert!(plan_transformer_export("proj-1", &receipt, "tx_a", 7, &extra, None).is_err());
        // A local selection narrows the run without touching the policy.
        let selection: DesignOutputSelection = serde_json::from_value(
            json!({"schema":"ds.design-output-selection/v1","tabular":["xlsx"]}),
        )
        .unwrap();
        let narrowed =
            plan_transformer_export("proj-1", &receipt, "tx_a", 7, &layers(), Some(&selection))
                .unwrap();
        assert_eq!(narrowed.formats, vec!["xlsx"]);
        // A named print output needs its setup in the sheets.
        assert!(plan_transformer_export(
            "proj-1",
            &print_receipt("Rwanda"),
            "tx_a",
            7,
            &layers(),
            None
        )
        .unwrap_err()
        .contains("detail"));
    }

    #[test]
    fn the_engine_request_is_the_exact_typed_document() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Rwanda"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let staging = PathBuf::from("/tmp/run");
        let staged = StagedInputs {
            transformer_document: &staging.join("transformer.json"),
            network_config: &staging.join("config.json"),
            out_dir: &staging.join("out"),
            admin_bounds: Some((Path::new("/assets/asset.dsab"), &"d".repeat(64))),
            media_grant: None,
            print_context: None,
            sheet: None,
        };
        let request = engine_request(&plan, &staged).unwrap();
        assert_eq!(
            request,
            json!({
                "transformer_document": "/tmp/run/transformer.json",
                "network_config": "/tmp/run/config.json",
                "input_shape": "plain_local",
                "transformer": "tx_a",
                "out_dir": "/tmp/run/out",
                "country": "Rwanda",
                "admin_bounds_asset": "/assets/asset.dsab",
                "admin_bounds_sha256": "d".repeat(64),
                "formats": ["shp", "xlsx"],
            })
        );
        let without = StagedInputs {
            admin_bounds: None,
            ..staged.clone()
        };
        assert!(engine_request(&plan, &without)
            .unwrap_err()
            .contains("Rwanda"));
        // A place in the drawing set reaches the engine as two integers, and
        // only a real place does.
        let numbered = StagedInputs {
            sheet: Some((12, 86)),
            ..staged.clone()
        };
        let request = engine_request(&plan, &numbered).unwrap();
        assert_eq!(request["sheet_index"], json!(12));
        assert_eq!(request["sheet_count"], json!(86));
        for bad in [(0, 86), (87, 86), (1, 0)] {
            let wrong = StagedInputs {
                sheet: Some(bad),
                ..staged.clone()
            };
            assert!(engine_request(&plan, &wrong)
                .unwrap_err()
                .contains("drawing set"));
        }
        let benin = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        assert!(engine_request(&benin, &staged)
            .unwrap_err()
            .contains("another country"));
        let request = engine_request(&benin, &without).unwrap();
        assert!(request.get("admin_bounds_asset").is_none());
    }

    #[test]
    fn the_engine_identity_is_the_closed_build_document() {
        let info = json!({
            "schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
            "package_version": "0.1.0", "source_sha": "development",
            "engine_version": "ds-network-reporter@0.1.0+development",
            "cargo_lock_sha256": "d".repeat(64), "features": [], "rustc": "rustc 1.95.0",
            "target": "x86_64-unknown-linux-gnu", "profile": "debug",
            "schemas": ["ds-network-reporter.task-result/v1", "ds.engine-build/v1"],
            "build_manifest_sha256": "f".repeat(64)
        });
        let identity = engine_identity(&info).unwrap();
        assert_eq!(
            identity.publication_state().unwrap(),
            PublicationState::LocalOnly
        );
        let mut release = info.clone();
        release["source_sha"] = json!("a".repeat(40));
        release["engine_version"] = json!(format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)));
        release["profile"] = json!("release");
        assert_eq!(
            engine_identity(&release)
                .unwrap()
                .publication_state()
                .unwrap(),
            PublicationState::Pending
        );
        let mut mismatched = release.clone();
        mismatched["engine_version"] =
            json!("ds-network-reporter@0.2.0+".to_string() + &"a".repeat(40));
        assert!(engine_identity(&mismatched).is_err());
        let mut other = release.clone();
        other["binary"] = json!("ds-grid");
        assert!(engine_identity(&other).is_err());
        let mut unknown = release;
        unknown["extra"] = json!(true);
        assert!(engine_identity(&unknown).is_err());
    }

    fn result_document(plan: &ExportPlan, out_dir: &Path) -> Value {
        json!({
            "task": REPORT_TASK, "status": "completed", "transformer": plan.transformer,
            "country": plan.country, "layers_decoded": 1, "features_decoded": 1,
            "admin_bounds": {"configured": plan.requires_admin_bounds, "villages_loaded": if plan.requires_admin_bounds {14920} else {0}, "features_matched": 0, "features_requested": 0},
            "geometry_changed": false, "formats_requested": plan.formats,
            "artifacts": [
                {"format": "shp", "filename": "tx_a.shp.zip", "path": out_dir.join("tx_a.shp.zip"), "bytes": 10, "sha256": "1".repeat(64)},
                {"format": "xlsx", "filename": "tx_a.xlsx", "path": out_dir.join("tx_a.xlsx"), "bytes": 20, "sha256": "2".repeat(64)},
            ],
            "blockers": [], "elapsed_ms": 42
        })
    }

    #[test]
    fn a_result_is_verified_against_its_plan_and_refused_typed() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let document = result_document(&plan, &out_dir);
        let verified =
            verify_result(&serde_json::to_vec(&document).unwrap(), &plan, &out_dir).unwrap();
        assert_eq!(verified.artifacts.len(), 2);
        assert_eq!(verified.artifacts[0].output_id, "shp");
        assert_eq!(verified.artifacts[0].content_type, "application/zip");
        assert_eq!(verified.artifacts[1].size_bytes, 20);

        let mut partial = document.clone();
        partial["status"] = json!("partial");
        partial["blockers"] =
            json!([{"code": "format_build_failed", "message": "kmz OOM", "format": "kmz"}]);
        match verify_result(&serde_json::to_vec(&partial).unwrap(), &plan, &out_dir).unwrap_err() {
            ResultRefusal::Blocked {
                status,
                blockers,
                artifacts,
            } => {
                assert_eq!(status, "partial");
                assert_eq!(blockers[0].code, "format_build_failed");
                assert_eq!(artifacts, 2);
                assert_eq!(
                    blocker_summary(&blockers),
                    "format_build_failed/kmz: kmz OOM"
                );
            }
            other => panic!("{other:?}"),
        }
        // The typed failure read after a non-zero exit is the same refusal.
        assert!(matches!(
            typed_failure(&serde_json::to_vec(&partial).unwrap(), &plan),
            Some(ResultRefusal::Blocked { .. })
        ));
        assert!(typed_failure(&serde_json::to_vec(&document).unwrap(), &plan).is_none());
        assert!(typed_failure(b"not json", &plan).is_none());

        let invalid = |mutate: &dyn Fn(&mut Value)| {
            let mut doc = document.clone();
            mutate(&mut doc);
            match verify_result(&serde_json::to_vec(&doc).unwrap(), &plan, &out_dir).unwrap_err() {
                ResultRefusal::Invalid { message } => message,
                other => panic!("{other:?}"),
            }
        };
        assert!(invalid(&|d| d["transformer"] = json!("tx_b")).contains("semantic request"));
        assert!(
            invalid(&|d| d["formats_requested"] = json!(["xlsx", "shp"]))
                .contains("every requested format")
        );
        assert!(
            invalid(&|d| d["artifacts"][1]["filename"] = json!("tx_a.shp.zip"))
                .contains("invalid artifact")
        );
        assert!(
            invalid(&|d| d["artifacts"][0]["path"] = json!("/elsewhere/tx_a.shp.zip"))
                .contains("direct output child")
        );
        assert!(invalid(&|d| d["artifacts"][0]["bytes"] = json!(0)).contains("invalid artifact"));
        assert!(invalid(&|d| d["features_decoded"] = json!(0)).contains("no decoded"));
        assert!(
            invalid(&|d| d["admin_bounds"]["configured"] = json!(true)).contains("another country")
        );
        assert!(invalid(&|d| d["warnings"] = json!([{"code":"other","transformer":"tx_a","output_id":"shp","layout_id":"x","element_id":"e","total_rows":2,"omitted_rows":1,"message":""}])).contains("print warnings"));
        assert!(invalid(&|d| d["extra"] = json!(1)).contains("malformed"));

        // A Rwanda plan needs the administrative evidence the engine reports.
        let rwanda = plan_transformer_export(
            "proj-1",
            &simple_receipt("Rwanda"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let mut stamped = result_document(&rwanda, &out_dir);
        assert!(verify_result(&serde_json::to_vec(&stamped).unwrap(), &rwanda, &out_dir).is_ok());
        stamped["admin_bounds"]["villages_loaded"] = json!(0);
        assert!(matches!(
            verify_result(&serde_json::to_vec(&stamped).unwrap(), &rwanda, &out_dir),
            Err(ResultRefusal::Invalid { .. })
        ));
    }

    fn context_layer(id: &str) -> (String, Value) {
        (
            id.to_string(),
            json!({"type":"FeatureCollection","features":[{
                "type":"Feature","properties":{"class":"national"},
                "geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.01,-2.0]]}
            }]}),
        )
    }

    fn coverage() -> Value {
        json!({"type":"Polygon","coordinates":[[[29.99,-2.01],[30.02,-2.01],[30.02,-1.99],[29.99,-1.99],[29.99,-2.01]]]})
    }

    #[test]
    fn the_print_context_document_is_the_engine_document_and_deterministic() {
        // The exact input the engine's own test stages
        // (`ds-network-reporter` `desktop_print_context_is_digest_bound_and_decoded_as_vectors`).
        let layers: BTreeMap<String, Value> = [context_layer("roads")].into_iter().collect();
        let sources = json!([{"layer":"roads","resource_id":"r","version":"v","cache":"local"}]);
        let bytes = print_context_document(&coverage(), &sources, &layers).unwrap();
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["schema"], PRINT_CONTEXT_SCHEMA);
        assert_eq!(
            document.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["schema", "coverage", "sources", "layers"]
        );
        assert_eq!(
            document["layers"]["roads"]["features"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // Byte-identical to the recipe the desktop shell applied on its own
        // until now, so a desktop and a server stage one digest.
        let desktop = serde_json::to_vec(&json!({
            "schema":"ds.print-context/v1", "coverage":coverage(), "sources":sources, "layers":layers
        }))
        .unwrap();
        assert_eq!(bytes, desktop);
        assert_eq!(
            sha256_hex(&bytes),
            format!("{:x}", Sha256::digest(&desktop))
        );
        // Layer order is the map's, whatever order a host gathered them in.
        let reversed: BTreeMap<String, Value> = [context_layer("rivers"), context_layer("roads")]
            .into_iter()
            .collect();
        let forward: BTreeMap<String, Value> = [context_layer("roads"), context_layer("rivers")]
            .into_iter()
            .collect();
        assert_eq!(
            print_context_document(&coverage(), &json!([]), &reversed).unwrap(),
            print_context_document(&coverage(), &json!([]), &forward).unwrap()
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_print_context_document_is_refused_where_the_engine_would_refuse_it() {
        let layers: BTreeMap<String, Value> = [context_layer("roads")].into_iter().collect();
        let refused = |coverage: &Value, sources: &Value, layers: &BTreeMap<String, Value>| {
            print_context_document(coverage, sources, layers).unwrap_err()
        };
        assert!(refused(
            &json!({"type":"Point","coordinates":[30.,-2.]}),
            &json!([]),
            &layers
        )
        .contains("coverage"));
        assert!(refused(&coverage(), &json!([]), &BTreeMap::new()).contains("no layers"));
        let bad_id: BTreeMap<String, Value> =
            [context_layer("roads national")].into_iter().collect();
        assert!(refused(&coverage(), &json!([]), &bad_id).contains("not an identifier"));
        let mut empty = layers.clone();
        empty.insert(
            "rivers".into(),
            json!({"type":"FeatureCollection","features":[]}),
        );
        assert!(refused(&coverage(), &json!([]), &empty).contains("is empty"));
        let mut decorated = layers.clone();
        decorated.insert(
            "rivers".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":null}],"name":"x"}),
        );
        assert!(
            refused(&coverage(), &json!([]), &decorated).contains("only type=FeatureCollection")
        );
        let mut shapeless = layers.clone();
        shapeless.insert(
            "rivers".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":[],"geometry":null}]}),
        );
        assert!(refused(&coverage(), &json!([]), &shapeless).contains("feature[0]"));
        assert!(refused(&coverage(), &json!({}), &layers).contains("sources must be an array"));
        assert!(refused(&coverage(), &json!([{"layer":"rivers"}]), &layers).contains("source[0]"));
        let mut many = BTreeMap::new();
        for index in 0..=MAX_PRINT_CONTEXT_LAYERS {
            many.insert(format!("layer_{index}"), context_layer("x").1);
        }
        assert!(refused(&coverage(), &json!([]), &many).contains("-layer bound"));
        let features: Vec<Value> = (0..=MAX_PRINT_CONTEXT_FEATURES)
            .map(|_| json!({"type":"Feature","properties":{},"geometry":null}))
            .collect();
        let crowded: BTreeMap<String, Value> = [(
            "roads".to_string(),
            json!({"type":"FeatureCollection","features":features}),
        )]
        .into_iter()
        .collect();
        assert!(refused(&coverage(), &json!([]), &crowded).contains("more than"));
    }

    #[test]
    fn receipts_record_the_print_context_they_were_rendered_with() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let result = verify_result(
            &serde_json::to_vec(&result_document(&plan, &out_dir)).unwrap(),
            &plan,
            &out_dir,
        )
        .unwrap();
        let run_id = format!("report-{}", "0".repeat(32));
        let digest = sha256_hex(b"print-context");
        let supplied = ["roads".to_string(), "contours_index".to_string()];
        let omitted = ["project_dsgrid_mv".to_string()];
        let facts = |sha: Option<&str>, layers: &[String], omitted: &[String]| {
            run_receipt(&RunReceiptFacts {
                plan: &plan,
                client_run_id: &run_id,
                engine: &engine(),
                result: &result,
                artifact_dir: "tx_a",
                now_ms: 1_700_000_000_000,
                expected_content_digest: None,
                print_context_sha256: sha,
                print_context_layers: layers,
                print_context_omitted: omitted,
            })
        };
        let receipt = facts(Some(&digest), &supplied, &omitted).unwrap();
        assert_eq!(receipt["print_context_sha256"], digest);
        assert_eq!(
            receipt["print_context_layers"],
            json!(["roads", "contours_index"])
        );
        assert_eq!(
            receipt["print_context_omitted"],
            json!(["project_dsgrid_mv"])
        );
        // A run without context says so, and may still name what it omitted.
        let bare = facts(None, &[], &omitted).unwrap();
        assert_eq!(bare["print_context_sha256"], Value::Null);
        assert_eq!(bare["print_context_omitted"], json!(["project_dsgrid_mv"]));
        // A digest without layers, layers without a digest, a malformed
        // digest, and an id that is not an identifier or is named twice.
        assert!(facts(Some(&digest), &[], &[]).is_err());
        assert!(facts(None, &supplied, &[]).is_err());
        assert!(facts(Some("abc"), &supplied, &[]).is_err());
        assert!(facts(Some(&digest), &["roads x".to_string()], &[]).is_err());
        assert!(facts(Some(&digest), &supplied, &["roads".to_string()]).is_err());
    }

    #[test]
    fn artifact_filenames_follow_their_format() {
        assert!(valid_artifact_filename("tx_a.shp.zip", "shp"));
        assert!(valid_artifact_filename(
            "tx_a.network-information.json",
            "network_information"
        ));
        assert!(!valid_artifact_filename("tx_a.json", "network_information"));
        assert!(valid_artifact_filename("tx_a.detail.pdf", "pdf__detail"));
        assert!(valid_artifact_filename("tx_a.detail.png", "png__detail"));
        assert!(valid_artifact_filename("tx_a.detail.jpg", "jpeg__detail"));
        assert!(!valid_artifact_filename("tx_a.detail.pdf", "png__detail"));
        assert!(!valid_artifact_filename("tx_a.other.png", "png__detail"));
        assert!(!valid_artifact_filename(
            "../tx_a.detail.png",
            "png__detail"
        ));
        assert!(!valid_artifact_filename("tx_a.pdf", "pdf__detail"));
        assert!(!valid_artifact_filename("../tx_a.xlsx", "xlsx"));
        assert!(!valid_artifact_filename("tx_a.xlsx", "shp"));
        assert!(!valid_artifact_filename("", "xlsx"));
    }

    #[test]
    fn named_raster_results_keep_the_full_artifact_admission_contract() {
        let mut plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Chad"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let layout = crate::printing::named_layout(
            Some("detail".into()),
            None,
            crate::printing::Paper::A3,
            crate::printing::Orientation::Landscape,
        )
        .unwrap();
        for format in ["png__detail", "jpeg__detail", "pdf__detail"] {
            let output = report_formats::output(format).unwrap();
            plan.formats = vec![format.into()];
            plan.outputs = vec![output.clone()];
            let filename = format!("tx_a{}", output.suffix);
            let mut document = result_document(&plan, &out_dir);
            document["artifacts"] = json!([{
                "format":format, "filename":filename, "path":out_dir.join(&filename),
                "bytes":123, "sha256":"a".repeat(64), "paper_size":"A3",
                "presentation":crate::printing::ArtifactPresentation::from_layout(&layout).unwrap()
            }]);
            document["warnings"] = json!([{
                "code":"table_truncated", "transformer":"tx_a", "output_id":format,
                "layout_id":"detail", "element_id":"poles", "total_rows":10,
                "omitted_rows":1, "message":"One pole row omitted."
            }]);
            let verify =
                |value: &Value| verify_result(&serde_json::to_vec(value).unwrap(), &plan, &out_dir);
            let result = verify(&document).unwrap();
            assert_eq!(result.artifacts[0].content_type, output.content_type);
            assert_eq!(result.artifacts[0].sha256, "a".repeat(64));
            assert_eq!(result.warnings.len(), 1);
            for (key, invalid) in [
                ("filename", json!("tx_a.other.png")),
                ("path", json!("/tmp/elsewhere.png")),
                ("sha256", json!("not-a-digest")),
                ("bytes", json!(0)),
                ("paper_size", json!("A0")),
            ] {
                let mut changed = document.clone();
                changed["artifacts"][0][key] = invalid;
                assert!(
                    verify(&changed).is_err(),
                    "{format}: accepted invalid {key}"
                );
            }
            document["warnings"][0]["layout_id"] = json!("another-layout");
            assert!(verify(&document).is_err());

            // Measured furniture diagnostics travel as warning rows too: a
            // named finding, and the bounded summary that names no element.
            // Neither has table rows behind it. (2026-09-14: the reporter
            // began emitting them and every sheet with a finding was refused
            // as "invalid print warnings".)
            document["warnings"] = json!([
                {
                    "code":"print_furniture_overlap", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"legend", "total_rows":0,
                    "omitted_rows":0, "message":"legend overlaps house-schedule by 4.2 mm"
                },
                {
                    "code":"print_diagnostics_summary", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"", "total_rows":0,
                    "omitted_rows":0, "message":"3 furniture findings; details truncated=false; 2 labels omitted"
                },
                {
                    "code":"print_text_unbound", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"", "total_rows":0,
                    "omitted_rows":0, "message":"{note} has no value in the title block or the project; the cell printed empty"
                }
            ]);
            assert_eq!(verify(&document).unwrap().warnings.len(), 3);
            for (index, key, invalid) in [
                (0, "code", json!("print_visual_approval")),
                (0, "element_id", json!("")),
                (0, "total_rows", json!(5)),
                (1, "element_id", json!("legend")),
                (1, "omitted_rows", json!(1)),
                (1, "transformer", json!("tx_b")),
                (2, "element_id", json!("title")),
                (2, "layout_id", json!("")),
                (2, "total_rows", json!(1)),
            ] {
                let mut changed = document.clone();
                changed["warnings"][index][key] = invalid;
                assert!(
                    verify(&changed).is_err(),
                    "{format}: accepted a diagnostics row with invalid {key}"
                );
            }
        }
    }

    #[test]
    fn cable_conflict_rows_ride_the_workbook_not_a_paper() {
        // 2026-09-15: the reporter began mirroring report_consistency findings
        // as warning rows on the xlsx output, labelled with the saved
        // transformer name, and 59 of 86 Gisagara reports were refused as
        // "invalid print warnings".
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        assert!(plan.formats.iter().any(|format| format == "xlsx"));
        let out_dir = PathBuf::from("/tmp/run/out");
        let mut document = result_document(&plan, &out_dir);
        let verify = |document: &Value| {
            verify_result(&serde_json::to_vec(document).unwrap(), &plan, &out_dir)
        };
        document["warnings"] = json!([{
            "code":"lv_pole_line_cable_conflict", "transformer":"TX_A", "output_id":"xlsx",
            "layout_id":"", "element_id":"LV_Line_01", "total_rows":0, "omitted_rows":0,
            "message":"LV_Line_01 P01: saved pole cable '3 x 50 + 54.6mm² ABC' differs from saved LV-line cable(s) '3 x 35 + 54.6mm² ABC'; pending engineering review. Values preserved."
        }]);
        let verified = verify(&document).unwrap();
        assert_eq!(verified.warnings.len(), 1);
        assert_eq!(verified.warnings[0].transformer, "TX_A");
        for (key, invalid) in [
            ("transformer", json!("tx_b")),
            ("output_id", json!("shp")),
            ("layout_id", json!("detail")),
            ("element_id", json!("")),
            ("element_id", json!("x".repeat(241))),
            ("total_rows", json!(3)),
            ("omitted_rows", json!(1)),
            ("message", json!("m".repeat(2049))),
        ] {
            let mut changed = document.clone();
            changed["warnings"][0][key] = invalid;
            assert!(
                verify(&changed).is_err(),
                "accepted a cable-conflict row with invalid {key}"
            );
        }
    }

    #[test]
    fn receipts_carry_the_publication_identity_and_the_batch_order() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let result = verify_result(
            &serde_json::to_vec(&result_document(&plan, &out_dir)).unwrap(),
            &plan,
            &out_dir,
        )
        .unwrap();
        let run_id = format!("report-{}", "0".repeat(32));
        let receipt = run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: &run_id,
            engine: &engine(),
            result: &result,
            artifact_dir: "tx_a",
            now_ms: 1_700_000_000_000,
            expected_content_digest: Some(&plan.content_digest),
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .unwrap();
        assert_eq!(receipt["schema"], RUN_RECEIPT_SCHEMA);
        assert_eq!(receipt["print_context_sha256"], Value::Null);
        assert_eq!(receipt["print_context_layers"], json!([]));
        assert_eq!(receipt["print_context_omitted"], json!([]));
        assert_eq!(receipt["outputs"][0]["path"], "tx_a/tx_a.shp.zip");
        assert_eq!(
            receipt["outputs"][1]["content_type"],
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(receipt["content_digest_verified"], true);
        assert_eq!(receipt["publication"]["state"], "pending");
        assert_eq!(receipt["publication"]["schema"], 3);
        assert_eq!(receipt["publication"]["operation"], "export-tx_a");
        assert_eq!(
            receipt["publication"]["root"],
            "eds_project/proj-1/eds_design"
        );
        assert_eq!(receipt["publication"]["transformer_revision"], 7);
        assert_eq!(
            receipt["input_base_fingerprint"],
            plan.input_base_fingerprint
        );
        assert!(run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: "run-1",
            engine: &engine(),
            result: &result,
            artifact_dir: "tx_a",
            now_ms: 0,
            expected_content_digest: None,
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .is_err());

        let mut development = engine();
        development.engine_version = crate::report::DEVELOPMENT_ENGINE.to_string();
        let local = publication_facts(&plan, &development).unwrap();
        assert_eq!(local["state"], "local_only");
        assert_eq!(local["schema"], LOCAL_ONLY_SCHEMA);

        let results = vec![
            BatchResult::error("tx_b", "export_blocked", "kmz failed", None),
            BatchResult::ok("tx_a", "tx_a/report-run.json".into(), 2),
        ];
        assert_eq!(batch_status(&results), "partial");
        let batch = batch_receipt(
            "proj-1",
            "stable",
            Some(&engine()),
            2,
            &["tx_b".into(), "tx_a".into()],
            &results,
            5,
        )
        .unwrap();
        assert_eq!(batch["status"], "partial");
        assert_eq!(batch["failed"], 1);
        assert_eq!(batch["completed"], 1);
        assert_eq!(batch["results"][0]["transformer"], "tx_a");
        assert_eq!(batch["results"][1]["error"]["code"], "export_blocked");
        assert_eq!(batch["publication_state"], "pending");
        assert_eq!(batch_status(&[]), "failed");
        assert_eq!(batch_status(&results[1..]), "completed");
    }

    #[test]
    fn a_batch_plan_is_the_kernels_scope_and_concurrency() {
        let plan = batch_plan(&["tx_b".into(), "tx_a".into(), "tx_b".into()], 16, 4).unwrap();
        assert_eq!(plan.names, vec!["tx_a", "tx_b"]);
        assert_eq!(plan.concurrency, 2);
        assert_eq!(
            batch_plan(
                &(0..10).map(|i| format!("tx_{i}")).collect::<Vec<_>>(),
                16,
                4
            )
            .unwrap()
            .concurrency,
            4
        );
        assert_eq!(
            batch_plan(
                &(0..10).map(|i| format!("tx_{i}")).collect::<Vec<_>>(),
                2,
                4
            )
            .unwrap()
            .concurrency,
            2
        );
        assert!(batch_plan(&[], 16, 4).is_err());
        assert!(batch_plan(&["collisions".into()], 16, 4).is_err());
        assert!(batch_plan(&["tx_a".into()], 16, 0).is_err());
        assert!(batch_plan(&["tx_a".into()], 16, 65).is_err());
    }

    #[test]
    fn run_ids_are_the_hosts_own_shape() {
        assert!(valid_run_id(&format!(
            "report-{}",
            "0123456789abcdef".repeat(2)
        )));
        assert!(!valid_run_id("report-ABC"));
        assert!(!valid_run_id(&"0".repeat(39)));
    }
}
