//! Prepared local layers — one owner for the descriptor a host keeps on its
//! own machine and for the four things that can happen to it.
//!
//! A prepared local layer is a bounded catalogue row: a name, a source origin,
//! a geometry type, a flat style, a column schema and a feature count, plus
//! wherever its payload is parked. The browser keeps them in IndexedDB and
//! paints them with MapLibre; a native host keeps them in a catalogue file
//! beside its payloads. Both ask the same questions — what is in the
//! catalogue, admit this new layer, reopen the store after a restart, apply
//! this admitted change, remove this logical layer — and both get the same
//! answers here.
//!
//! The host decides nothing: not the id, not a default, not a clamp, not
//! which source metadata survives, not what removal may delete. It performs
//! I/O on the answer. `remove` and `restore` name **exactly** what the host
//! may delete (`release`), which is how a removal is provably unable to touch
//! an original source file, a project asset or another layer's storage — the
//! kernel never names one.
//!
//! Pure: `now_ms`, `sequence` and `colorIndex` are inputs, so the same request
//! always has the same answer. A refusal leaves the caller's registry
//! unchanged and `revision` advances only on a change.
//!
//! The rules are transcribed from ds-web's `src/lib/stores/sketches.ts`
//! (`normalizeSourceMetadata`, `normalizedSourceKind`, `defaultSourceName`,
//! `sketchLayerSourceLabel`, `normalizeFeatureStorage`, `normalizeMetaLayer`,
//! `reconcileSourceOrigin`, `clampNumber`, the `SKETCH_COLORS` palette and the
//! `createSketchLayer*` defaults), which is the parity baseline: the descriptor
//! is the same JSON that store already persists, so an existing browser
//! profile reopens unchanged. `tests/fixtures/local-layers-cases.json` is the
//! named proof, replayed by the kernel, the browser over the vendored WASM and
//! the native store.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const SCHEMA: &str = "ds.local-layers/v1";
pub const REGISTRY_SCHEMA: u32 = 1;
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// One catalogue holds at most this many layers; `restore` reads at most this
/// many records.
pub const MAX_LAYERS: usize = 2_000;
pub const MAX_NAME_CHARS: usize = 200;
pub const MAX_TEXT_CHARS: usize = 500;
pub const MAX_COLOR_CHARS: usize = 100;
pub const MAX_SCHEMA_FIELDS: usize = 256;
pub const MAX_FIELD_OPTIONS: usize = 256;

pub const DEFAULT_LINE_WIDTH: f64 = 2.5;
pub const DEFAULT_POINT_RADIUS: f64 = 5.0;
pub const DEFAULT_FILL_OPACITY: f64 = 0.15;
pub const MIN_LINE_WIDTH: f64 = 0.5;
pub const MAX_LINE_WIDTH: f64 = 12.0;
pub const MIN_POINT_RADIUS: f64 = 1.0;
pub const MAX_POINT_RADIUS: f64 = 24.0;
pub const DEFAULT_PREVIEW_LIMIT: i64 = 5_000;
pub const MAX_PREVIEW_LIMIT: i64 = 50_000;

/// The palette, in order, that `sketches.ts` handed out. The host passes the
/// index it has reached; the colour is the kernel's answer.
pub const SKETCH_COLORS: [&str; 10] = [
    "#2563eb", "#dc2626", "#16a34a", "#d97706", "#7c3aed", "#0891b2", "#be185d", "#4d7c0f",
    "#9333ea", "#ea580c",
];

/// The class colour a pinned-transformer row is painted in. Pinned rows are
/// one visual class, so they never take a palette entry: the host names the
/// pair it materialised, never the colour.
pub const PINNED_COLOR: &str = "#64748b";
/// How much of a transformer or class name a pinned id spells out before its
/// fingerprint takes over.
pub const MAX_PINNED_SLUG_CHARS: usize = 64;

pub const MANUAL_SOURCE_NAME: &str = "Manual layers";
pub const IMPORT_SOURCE_NAME: &str = "Imported files";
pub const DERIVED_SOURCE_NAME: &str = "Derived layers";
pub const PINNED_SOURCE_NAME: &str = "Pinned transformers";
pub const PROJECT_WORK_SOURCE_NAME: &str = "Project Work";

pub const MALFORMED_DESCRIPTOR: &str = "malformed_descriptor";
pub const DUPLICATE_LAYER: &str = "duplicate_layer";
pub const UNKNOWN_LAYER: &str = "unknown_layer";
pub const SCOPE_MISMATCH: &str = "scope_mismatch";
pub const UNSUPPORTED_SOURCE_KIND: &str = "unsupported_source_kind";

/// The descriptor keys, in the order they are written. `restore` reports a row
/// as repaired at exactly these keys.
const DESCRIPTOR_KEYS: [&str; 17] = [
    "id",
    "name",
    "sourceName",
    "sourceLayerName",
    "sourceKind",
    "sourceMetadata",
    "geometryType",
    "color",
    "lineWidth",
    "pointRadius",
    "fillOpacity",
    "styleDoc",
    "schema",
    "featureCount",
    "featureStorage",
    "visible",
    "createdAt",
];

// ── The descriptor ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Manual,
    Import,
    Derived,
    Pinned,
    /// Accepted on the wire so it can be refused by name: a Project Work root
    /// is an ephemeral projection of canonical PM entities, never a persisted
    /// local layer.
    ProjectWork,
}

impl SourceKind {
    /// `defaultSourceName`.
    pub const fn default_source_name(self) -> &'static str {
        match self {
            Self::Manual => MANUAL_SOURCE_NAME,
            Self::Import => IMPORT_SOURCE_NAME,
            Self::Derived => DERIVED_SOURCE_NAME,
            Self::Pinned => PINNED_SOURCE_NAME,
            Self::ProjectWork => PROJECT_WORK_SOURCE_NAME,
        }
    }
    /// `normalizedSourceKind`: anything that is not one of the four other
    /// words is a manual layer.
    fn from_raw(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("import") => Self::Import,
            Some("derived") => Self::Derived,
            Some("pinned") => Self::Pinned,
            Some("project_work") => Self::ProjectWork,
            _ => Self::Manual,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeometryType {
    Point,
    LineString,
    Polygon,
}

impl GeometryType {
    fn from_raw(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("LineString") => Self::LineString,
            Some("Polygon") => Self::Polygon,
            _ => Self::Point,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Number,
    Integer,
    Date,
    Boolean,
    Select,
    Multiselect,
    Radio,
}

impl FieldType {
    fn from_raw(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("number") => Self::Number,
            Some("integer") => Self::Integer,
            Some("date") => Self::Date,
            Some("boolean") => Self::Boolean,
            Some("select") => Self::Select,
            Some("multiselect") => Self::Multiselect,
            Some("radio") => Self::Radio,
            _ => Self::Text,
        }
    }
}

/// One column shared by every feature of the layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: FieldType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl Field {
    fn validate(&self) -> Result<(), Fault> {
        if !bounded(&self.name, MAX_NAME_CHARS) {
            return Err(malformed("a schema field needs a bounded trimmed name"));
        }
        if self
            .default_value
            .as_ref()
            .is_some_and(|v| v.chars().count() > MAX_TEXT_CHARS || v.chars().any(char::is_control))
        {
            return Err(malformed("a schema field default value is out of bounds"));
        }
        if self.options.as_ref().is_some_and(|options| {
            options.len() > MAX_FIELD_OPTIONS
                || options
                    .iter()
                    .any(|o| o.chars().count() > MAX_TEXT_CHARS || o.chars().any(char::is_control))
        }) {
            return Err(malformed("a schema field's options are out of bounds"));
        }
        Ok(())
    }
    /// `restore` repairs a legacy column instead of refusing the whole store:
    /// an unreadable type reads as `text`, a nameless field is dropped.
    fn from_raw(raw: &Value) -> Option<Self> {
        let row = raw.as_object()?;
        let name = sanitize(&js_string(row.get("name")), MAX_NAME_CHARS);
        if name.is_empty() {
            return None;
        }
        let options = row.get("options").and_then(Value::as_array).map(|values| {
            values
                .iter()
                .take(MAX_FIELD_OPTIONS)
                .map(|value| sanitize(&js_string(Some(value)), MAX_TEXT_CHARS))
                .collect()
        });
        Some(Self {
            name,
            kind: FieldType::from_raw(row.get("type")),
            default_value: row
                .get("defaultValue")
                .filter(|value| !value.is_null())
                .map(|value| sanitize(&js_string(Some(value)), MAX_TEXT_CHARS)),
            options,
            required: row.get("required").and_then(Value::as_bool),
        })
    }
}

/// Where the layer came from: `import_file` is a one-way import of a file the
/// host read once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceMetadata {
    #[serde(rename_all = "camelCase")]
    ImportFile {
        file_name: String,
        source_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        archive_inner_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_bytes: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_modified: Option<i64>,
        last_refresh_at: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dirty_since_refresh: Option<i64>,
    },
}

impl SourceMetadata {
    fn validate(&self) -> Result<(), Fault> {
        let texts: Vec<&String> = match self {
            Self::ImportFile {
                file_name,
                source_name,
                ..
            } => vec![file_name, source_name],
        };
        if texts.iter().any(|value| !bounded(value, MAX_TEXT_CHARS)) {
            return Err(malformed("source metadata carries an unbounded field"));
        }
        let last_refresh_at = match self {
            Self::ImportFile {
                last_refresh_at, ..
            } => *last_refresh_at,
        };
        if last_refresh_at <= 0 {
            return Err(malformed("source metadata needs a refresh timestamp"));
        }
        Ok(())
    }
    /// The bounded origin a catalogue row carries: what the layer came from.
    fn row(&self) -> Value {
        let mut row = match self {
            Self::ImportFile {
                file_name,
                archive_inner_name,
                ..
            } => json!({
                "kind": "import_file",
                "label": match archive_inner_name {
                    Some(inner) => format!("{file_name} → {inner}"),
                    None => file_name.clone(),
                },
            }),
        };
        let Self::ImportFile {
            dirty_since_refresh,
            ..
        } = self;
        if let (Some(map), Some(dirty)) = (row.as_object_mut(), dirty_since_refresh) {
            map.insert("dirtySinceRefresh".into(), json!(dirty));
        }
        row
    }

    /// `normalizeSourceMetadata`. An imported file is admitted only for an
    /// `import` layer — a manual layer that is handed import metadata simply
    /// has none. Anything missing an identifying field is dropped whole rather
    /// than half-kept.
    fn from_raw(raw: Option<&Value>, kind: SourceKind, clock: Option<i64>) -> Option<Self> {
        let row = raw?.as_object()?;
        match row.get("kind").and_then(Value::as_str) {
            Some("import_file") if kind == SourceKind::Import => {
                let file_name = js_string(row.get("fileName"));
                if file_name.is_empty() {
                    return None;
                }
                let source_name = match row.get("sourceName") {
                    Some(value) if !value.is_null() => js_string(Some(value)),
                    _ => file_name.clone(),
                };
                let source_path = js_string(row.get("sourcePath"));
                let archive_inner_name = js_string(row.get("archiveInnerName"));
                Some(Self::ImportFile {
                    file_name: sanitize(&file_name, MAX_TEXT_CHARS),
                    source_name: sanitize(
                        if source_name.is_empty() {
                            &file_name
                        } else {
                            &source_name
                        },
                        MAX_TEXT_CHARS,
                    ),
                    source_path: (!source_path.is_empty())
                        .then(|| sanitize(&source_path, MAX_TEXT_CHARS)),
                    archive_inner_name: (!archive_inner_name.is_empty())
                        .then(|| sanitize(&archive_inner_name, MAX_TEXT_CHARS)),
                    size_bytes: finite_non_negative(row.get("sizeBytes")),
                    last_modified: finite_positive(row.get("lastModified")),
                    last_refresh_at: refresh_time(row.get("lastRefreshAt"), clock)?,
                    dirty_since_refresh: finite_positive(row.get("dirtySinceRefresh")),
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    Full,
    Preview,
}

/// Where a large payload is parked instead of the layer's own record. The
/// kernel never reads it — it only admits it, carries it and names it for
/// release when the layer that owns it loses it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeatureStorage {
    #[serde(rename_all = "camelCase")]
    UploadChunks {
        workspace_key: String,
        file_name: String,
        parsed_layer: String,
        id_prefix: String,
        render_mode: RenderMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview_limit: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        render_preview_loaded: Option<bool>,
    },
}

impl FeatureStorage {
    fn validate(&self) -> Result<(), Fault> {
        let Self::UploadChunks {
            workspace_key,
            file_name,
            parsed_layer,
            id_prefix,
            preview_limit,
            ..
        } = self;
        if [workspace_key, file_name, parsed_layer, id_prefix]
            .iter()
            .any(|value| !bounded(value, MAX_TEXT_CHARS))
        {
            return Err(malformed("feature storage carries an unbounded field"));
        }
        if preview_limit.is_some_and(|limit| !(1..=MAX_PREVIEW_LIMIT).contains(&limit)) {
            return Err(malformed("a preview limit is out of bounds"));
        }
        Ok(())
    }
    /// What the host may delete when this storage is released.
    fn release(&self) -> Value {
        let Self::UploadChunks {
            workspace_key,
            file_name,
            parsed_layer,
            ..
        } = self;
        json!({
            "kind": "upload_chunks",
            "workspaceKey": workspace_key,
            "fileName": file_name,
            "parsedLayer": parsed_layer,
        })
    }
    /// Two storages are the same parked payload when they name the same
    /// chunks; replacing a payload with the one already parked releases
    /// nothing.
    fn same_payload(&self, other: &Self) -> bool {
        self.release() == other.release()
    }
    /// `normalizeFeatureStorage`. A storage missing any of the three keys that
    /// locate the chunks is not storage at all. A missing `idPrefix` is minted
    /// from the owning layer's id — the prefix only has to be unique inside
    /// one layer — never from randomness.
    fn from_raw(raw: Option<&Value>, owner: &str) -> Option<Self> {
        let row = raw?.as_object()?;
        if row.get("kind").and_then(Value::as_str) != Some("upload_chunks") {
            return None;
        }
        let workspace_key = js_string(row.get("workspaceKey"));
        let file_name = js_string(row.get("fileName"));
        let parsed_layer = js_string(row.get("parsedLayer"));
        if workspace_key.is_empty() || file_name.is_empty() || parsed_layer.is_empty() {
            return None;
        }
        let id_prefix = js_string(row.get("idPrefix"));
        let preview = row.get("renderMode").and_then(Value::as_str) == Some("preview");
        Some(Self::UploadChunks {
            workspace_key: sanitize(&workspace_key, MAX_TEXT_CHARS),
            file_name: sanitize(&file_name, MAX_TEXT_CHARS),
            parsed_layer: sanitize(&parsed_layer, MAX_TEXT_CHARS),
            id_prefix: if id_prefix.is_empty() {
                format!("feat-{owner}")
            } else {
                sanitize(&id_prefix, MAX_TEXT_CHARS)
            },
            render_mode: if preview {
                RenderMode::Preview
            } else {
                RenderMode::Full
            },
            preview_limit: preview.then(|| {
                number_or(row.get("previewLimit"), DEFAULT_PREVIEW_LIMIT)
                    .clamp(1, MAX_PREVIEW_LIMIT)
            }),
            render_preview_loaded: preview
                .then(|| row.get("renderPreviewLoaded") == Some(&Value::Bool(true))),
        })
    }
}

/// One prepared local layer, exactly as a host persists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalLayer {
    pub id: String,
    pub name: String,
    pub source_name: String,
    pub source_layer_name: String,
    pub source_kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_metadata: Option<SourceMetadata>,
    pub geometry_type: GeometryType,
    pub color: String,
    pub line_width: f64,
    pub point_radius: f64,
    pub fill_opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_doc: Option<Value>,
    pub schema: Vec<Field>,
    pub feature_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_storage: Option<FeatureStorage>,
    pub visible: bool,
    pub created_at: i64,
}

impl LocalLayer {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.source_kind == SourceKind::ProjectWork {
            return Err(refusal(
                UNSUPPORTED_SOURCE_KIND,
                "Project Work is an ephemeral governed map root, not a prepared local layer",
            ));
        }
        if !bounded(&self.id, MAX_NAME_CHARS) {
            return Err(malformed("a local layer needs a bounded trimmed id"));
        }
        if !one_segment(&self.id) {
            return Err(malformed(
                "a local layer id is one path segment: no separator, and never . or ..",
            ));
        }
        if !bounded(&self.name, MAX_NAME_CHARS) {
            return Err(malformed(
                "a local layer needs a bounded trimmed name of 1 to 200 characters",
            ));
        }
        if !bounded(&self.source_name, MAX_TEXT_CHARS)
            || !bounded(&self.source_layer_name, MAX_TEXT_CHARS)
        {
            return Err(malformed(
                "a local layer needs bounded trimmed source names",
            ));
        }
        if !bounded(&self.color, MAX_COLOR_CHARS) {
            return Err(malformed("a local layer needs a bounded colour"));
        }
        if !(MIN_LINE_WIDTH..=MAX_LINE_WIDTH).contains(&self.line_width)
            || !(MIN_POINT_RADIUS..=MAX_POINT_RADIUS).contains(&self.point_radius)
            || !(0.0..=1.0).contains(&self.fill_opacity)
        {
            return Err(malformed("a local layer's flat style is out of bounds"));
        }
        if self.schema.len() > MAX_SCHEMA_FIELDS {
            return Err(malformed(&format!(
                "a local layer carries at most {MAX_SCHEMA_FIELDS} schema fields"
            )));
        }
        for field in &self.schema {
            field.validate()?;
        }
        if self.style_doc.as_ref().is_some_and(|doc| !doc.is_object()) {
            return Err(malformed("a style document must be an object"));
        }
        if self.created_at < 0 {
            return Err(malformed(
                "a local layer needs a non-negative creation time",
            ));
        }
        if let Some(metadata) = &self.source_metadata {
            // The kind rule is the descriptor's, not one op's: an imported
            // file belongs to an imported layer. `register` and `restore`
            // drop metadata that does not belong; a registry a host hands in
            // is held to the same rule on every op, so a row no path here
            // could have minted is never carried on.
            if matches!(metadata, SourceMetadata::ImportFile { .. })
                && self.source_kind != SourceKind::Import
            {
                return Err(malformed(
                    "imported-file metadata belongs only to a layer imported from a file",
                ));
            }
            metadata.validate()?;
        }
        if let Some(storage) = &self.feature_storage {
            storage.validate()?;
        }
        Ok(())
    }

    /// The bounded row `list` answers with.
    fn row(&self) -> Value {
        let mut row = json!({
            "id": self.id,
            "name": self.name,
            "sourceName": self.source_name,
            "sourceLayerName": self.source_layer_name,
            "sourceKind": self.source_kind,
            "geometryType": self.geometry_type,
            "featureCount": self.feature_count,
            "visible": self.visible,
            "createdAt": self.created_at,
        });
        if let (Some(map), Some(metadata)) = (row.as_object_mut(), self.source_metadata.as_ref()) {
            map.insert("origin".into(), metadata.row());
        }
        row
    }

    /// Everything the host may delete once this layer is gone: its own
    /// payload record, and its own parked chunks. Nothing else exists to name
    /// — and chunks another layer of the catalogue still parks its payload in
    /// are that layer's storage, so they are not named either.
    fn release<'a>(&self, others: impl IntoIterator<Item = &'a Self>) -> Vec<Value> {
        let mut release = vec![json!({"kind": "features_payload", "id": self.id})];
        if let Some(storage) = &self.feature_storage {
            if !chunks_still_parked(storage, others) {
                release.push(storage.release());
            }
        }
        release
    }
}

/// Whether any of `layers` still parks its payload in `storage`'s chunks.
fn chunks_still_parked<'a>(
    storage: &FeatureStorage,
    layers: impl IntoIterator<Item = &'a LocalLayer>,
) -> bool {
    layers.into_iter().any(|layer| {
        layer
            .feature_storage
            .as_ref()
            .is_some_and(|other| other.same_payload(storage))
    })
}

// ── Scope and registry ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    Browser,
    Native,
}

/// Where the catalogue lives. One browser profile is one scope; a native store
/// is keyed by the lane and the user it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub host: Host,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
}

impl Scope {
    fn validate(&self) -> Result<(), Fault> {
        match self.host {
            Host::Browser => {
                if self.lane.is_some() || self.uid.is_some() {
                    return Err(Fault::Hard(
                        "a browser scope is one profile and carries no lane or uid".into(),
                    ));
                }
            }
            Host::Native => {
                let named = self
                    .lane
                    .as_ref()
                    .is_some_and(|lane| bounded(lane, MAX_NAME_CHARS))
                    && self
                        .uid
                        .as_ref()
                        .is_some_and(|uid| bounded(uid, MAX_NAME_CHARS));
                if !named {
                    return Err(Fault::Hard(
                        "a native scope needs a bounded lane and uid".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The catalogue a host keeps, and the only thing an op reads or rewrites.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: u32,
    pub revision: u64,
    pub scope: Scope,
    pub layers: Vec<LocalLayer>,
}

impl Registry {
    fn opened(scope: Scope) -> Self {
        Self {
            schema: REGISTRY_SCHEMA,
            revision: 0,
            scope,
            layers: vec![],
        }
    }
    fn validate(&self) -> Result<(), Fault> {
        if self.schema != REGISTRY_SCHEMA {
            return Err(Fault::Hard(format!(
                "unsupported local layer registry schema; expected {REGISTRY_SCHEMA}"
            )));
        }
        self.scope.validate()?;
        if self.layers.len() > MAX_LAYERS {
            return Err(malformed(&format!(
                "a local layer registry holds at most {MAX_LAYERS} layers"
            )));
        }
        let mut ids = HashSet::new();
        for layer in &self.layers {
            layer.validate()?;
            if !ids.insert(layer.id.as_str()) {
                return Err(refusal(
                    DUPLICATE_LAYER,
                    &format!("layer {} appears twice in the registry", layer.id),
                ));
            }
        }
        Ok(())
    }
    /// Every op names the scope it believes it is in. A catalogue from another
    /// profile, lane or user is never silently rewritten.
    fn fence(&self, scope: &Scope) -> Result<(), Fault> {
        if &self.scope != scope {
            return Err(refusal(
                SCOPE_MISMATCH,
                "the registry belongs to another local layer scope",
            ));
        }
        Ok(())
    }
    fn find(&self, id: &str) -> Result<usize, Fault> {
        self.layers
            .iter()
            .position(|layer| layer.id == id)
            .ok_or_else(|| refusal(UNKNOWN_LAYER, &format!("no local layer {id}")))
    }
}

// ── Refusals ────────────────────────────────────────────────────────────

/// Why an op changed nothing. A malformed *request* is not a refusal — it is
/// an `Err` from `evaluate`, because the host built it wrong.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    Hard(String),
    Refused(Refusal),
}

fn refusal(code: &'static str, message: &str) -> Fault {
    Fault::Refused(Refusal {
        code,
        message: message.to_owned(),
    })
}
fn malformed(message: &str) -> Fault {
    refusal(MALFORMED_DESCRIPTOR, message)
}

// ── JavaScript-shaped coercion, transcribed ─────────────────────────────

fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}

/// An id names a record on a host that keeps records in a file system: the
/// browser keys IndexedDB by it, the native store writes `<id>.geojson` under
/// its payload directory. So an id is ONE path segment — it carries no
/// separator and is never the current or the parent directory. A traversal
/// shape is refused here, where every host asks, rather than neutralised
/// afterwards by each host on its own.
fn one_segment(value: &str) -> bool {
    !value.contains(['/', '\\', ':']) && value != "." && value != ".."
}

/// Drop control characters, trim, and bound by character count. `restore`
/// repairs a legacy row this way; every other op refuses instead.
fn sanitize(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// `String(value ?? '').trim()` over the JSON value space. A structure has no
/// string form worth keeping, so it reads as absent.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(_) => String::new(),
    }
}

/// `Number(value)`; `None` is `NaN`.
fn js_number(value: Option<&Value>) -> Option<f64> {
    match value {
        None => None,
        Some(Value::Null) => Some(0.0),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                Some(0.0)
            } else {
                text.parse::<f64>().ok()
            }
        }
        Some(_) => None,
    }
}

/// `Number(value) || fallback` — a zero or an unreadable number takes the
/// fallback, which is how a legacy row without a refresh time gets one.
fn number_or(value: Option<&Value>, fallback: i64) -> i64 {
    match js_number(value) {
        Some(number) if number.is_finite() && number != 0.0 => number.trunc() as i64,
        _ => fallback,
    }
}

/// A refresh time is the row's own when it has a readable one, otherwise the
/// caller's clock. An op that carries no clock and metadata that carries no
/// time have no refresh time between them, and metadata without one is not
/// admissible metadata.
fn refresh_time(value: Option<&Value>, clock: Option<i64>) -> Option<i64> {
    match js_number(value) {
        Some(number) if number.is_finite() && number > 0.0 => Some(number.trunc() as i64),
        _ => clock.filter(|clock| *clock > 0),
    }
}

fn finite_positive(value: Option<&Value>) -> Option<i64> {
    js_number(value)
        .filter(|n| n.is_finite() && *n > 0.0)
        .map(|n| n.trunc() as i64)
}

fn finite_non_negative(value: Option<&Value>) -> Option<i64> {
    js_number(value)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n.trunc() as i64)
}

/// `clampNumber`: an unreadable number is the default, a readable one is
/// clamped into range.
fn clamp_number(value: Option<&Value>, fallback: f64, min: f64, max: f64) -> f64 {
    clamp_f64(js_number(value), fallback, min, max)
}

fn clamp_f64(value: Option<f64>, fallback: f64, min: f64, max: f64) -> f64 {
    match value {
        Some(number) if number.is_finite() => number.clamp(min, max),
        _ => fallback,
    }
}

/// `sketchLayerSourceLabel`: the container a layer is filed under. A manual
/// layer never files itself under its own name.
fn source_label(name: &str, raw: Option<&str>, kind: SourceKind) -> String {
    let raw = raw.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return kind.default_source_name().to_owned();
    }
    if kind == SourceKind::Manual && raw == name {
        return MANUAL_SOURCE_NAME.to_owned();
    }
    raw.to_owned()
}

// ── The ops ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

/// What a host may ask about its own catalogue.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// What is in the catalogue, as bounded rows.
    List {
        registry: Registry,
        scope: Scope,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Admit one new layer: mint its id, fill every default, normalise its
    /// origin, append it.
    Register {
        registry: Registry,
        scope: Scope,
        now_ms: i64,
        sequence: u64,
        #[serde(rename = "colorIndex", default)]
        color_index: u64,
        draft: Draft,
    },
    /// Reopen a store after a restart: normalise every persisted row, retire
    /// what may not persist, and say what was repaired.
    Restore {
        scope: Scope,
        records: Vec<Value>,
        now_ms: i64,
    },
    /// Apply one admitted change to one layer.
    Update {
        registry: Registry,
        scope: Scope,
        id: String,
        #[serde(default)]
        now_ms: Option<i64>,
        change: Change,
    },
    /// Remove one logical layer and name what the host may delete with it.
    Remove {
        registry: Registry,
        scope: Scope,
        id: String,
    },
}

/// What a host knows about a layer it is asking for. Everything else is the
/// kernel's answer.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Draft {
    name: String,
    geometry_type: GeometryType,
    #[serde(default)]
    source_kind: Option<SourceKind>,
    #[serde(default)]
    source_name: Option<String>,
    #[serde(default)]
    source_layer_name: Option<String>,
    #[serde(default)]
    source_metadata: Option<Value>,
    #[serde(default)]
    schema: Option<Vec<Field>>,
    #[serde(default)]
    feature_count: Option<i64>,
    #[serde(default)]
    feature_storage: Option<Value>,
    /// A pinned-transformer row is named by the pair it was materialised
    /// from, never by a clock: see `PinnedIdentity`.
    #[serde(default)]
    pinned: Option<PinnedIdentity>,
}

/// What a pinned-transformer row IS — the transformer it was materialised from
/// and the design class inside it. The host names the pair; the id, the class
/// colour and the schema are the kernel's answer, so the same pair always
/// reopens as the same layer and materialising it twice is not two layers.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PinnedIdentity {
    transformer: String,
    class_name: String,
    /// The property keys the pinned features carry. The columns are the
    /// kernel's answer: sanitised like every schema field, ordered,
    /// de-duplicated and bounded, all of them text.
    #[serde(default)]
    fields: Option<Vec<String>>,
}

/// The changes a layer admits. Each one is a whole answer: what a change
/// implies for the rest of the descriptor is decided here, not by the caller.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Change {
    /// The layer's features were replaced. A replaced payload always leaves
    /// the previously parked chunks behind, so the receipt names them.
    #[serde(rename_all = "camelCase")]
    Payload {
        schema: Vec<Field>,
        feature_count: i64,
        feature_storage: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    SourceMetadata {
        source_metadata: Option<Value>,
    },
    Rename {
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    RenameSource {
        #[serde(default)]
        source_kind: Option<SourceKind>,
        source_name: String,
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    Style {
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        line_width: Option<f64>,
        #[serde(default)]
        point_radius: Option<f64>,
        #[serde(default)]
        fill_opacity: Option<f64>,
        #[serde(default, deserialize_with = "present")]
        style_doc: Option<Option<Value>>,
    },
    Visible {
        visible: bool,
    },
}

impl Change {
    const fn wire(&self) -> &'static str {
        match self {
            Self::Payload { .. } => "payload",
            Self::SourceMetadata { .. } => "source_metadata",
            Self::Rename { .. } => "rename",
            Self::RenameSource { .. } => "rename_source",
            Self::Style { .. } => "style",
            Self::Visible { .. } => "visible",
        }
    }
}

/// Absent and `null` are different answers: absent leaves a field alone,
/// `null` clears it.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("local layer request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid local layer request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!("unsupported local layer schema; expected {SCHEMA}"));
    }
    let value = match apply(request.op) {
        Ok(value) => value,
        Err(Fault::Hard(message)) => return Err(message),
        Err(Fault::Refused(refused)) => json!({"outcome": "refused", "refusal": refused}),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

/// The answer every op that rewrote a catalogue gives: the next registry, what
/// it did, and whether anything actually moved.
fn answer(before: &Registry, mut next: Registry, receipt: Value) -> Result<Value, Fault> {
    next.validate()?;
    let changed = next.layers != before.layers;
    if changed {
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| Fault::Hard("local layer revision exhausted".into()))?;
    }
    Ok(json!({"registry": next, "receipt": receipt, "changed": changed}))
}

fn apply(op: Op) -> Result<Value, Fault> {
    match op {
        Op::List {
            registry,
            scope,
            limit,
        } => {
            registry.validate()?;
            scope.validate()?;
            registry.fence(&scope)?;
            let limit = limit.unwrap_or(MAX_LAYERS).clamp(1, MAX_LAYERS);
            let rows: Vec<Value> = registry
                .layers
                .iter()
                .take(limit)
                .map(LocalLayer::row)
                .collect();
            Ok(json!({
                "registry": registry,
                "receipt": {"layers": rows, "count": registry.layers.len()},
                "changed": false,
            }))
        }
        Op::Register {
            registry,
            scope,
            now_ms,
            sequence,
            color_index,
            draft,
        } => {
            registry.validate()?;
            scope.validate()?;
            registry.fence(&scope)?;
            let clock = admitted_clock(now_ms)?;
            let layer = register(&draft, clock, sequence, color_index)?;
            if let Some(held) = registry.layers.iter().position(|row| row.id == layer.id) {
                // A pinned pair has ONE identity, so materialising it again is
                // not a second layer: the catalogue answers the descriptor it
                // already holds — with whatever visibility and style the user
                // gave it — and nothing changes. Every other kind is minted
                // from the clock, so the same id twice is a host mistake.
                if draft.pinned.is_some() {
                    let receipt = serde_json::to_value(&registry.layers[held])
                        .map_err(|error| Fault::Hard(error.to_string()))?;
                    return Ok(json!({
                        "registry": registry,
                        "receipt": receipt,
                        "changed": false,
                    }));
                }
                return Err(refusal(
                    DUPLICATE_LAYER,
                    &format!("local layer {} already exists", layer.id),
                ));
            }
            if registry.layers.len() >= MAX_LAYERS {
                return Err(malformed(&format!(
                    "a local layer registry holds at most {MAX_LAYERS} layers"
                )));
            }
            let receipt =
                serde_json::to_value(&layer).map_err(|error| Fault::Hard(error.to_string()))?;
            let mut next = registry.clone();
            next.layers.push(layer);
            answer(&registry, next, receipt)
        }
        Op::Restore {
            scope,
            records,
            now_ms,
        } => {
            scope.validate()?;
            let clock = admitted_clock(now_ms)?;
            restore(scope, &records, clock)
        }
        Op::Update {
            registry,
            scope,
            id,
            now_ms,
            change,
        } => {
            registry.validate()?;
            scope.validate()?;
            registry.fence(&scope)?;
            let clock = match now_ms {
                Some(now_ms) => Some(admitted_clock(now_ms)?),
                None => None,
            };
            let index = registry.find(&id)?;
            let applied = change.wire();
            let mut next = registry.clone();
            let release = update(&mut next, index, change, clock)?;
            answer(
                &registry,
                next,
                json!({"id": id, "applied": applied, "release": release}),
            )
        }
        Op::Remove {
            registry,
            scope,
            id,
        } => {
            registry.validate()?;
            scope.validate()?;
            registry.fence(&scope)?;
            let index = registry.find(&id)?;
            let layer = &registry.layers[index];
            let release = layer.release(
                registry
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, other)| other),
            );
            let mut next = registry.clone();
            next.layers.remove(index);
            answer(
                &registry,
                next,
                json!({"id": id, "removed": true, "release": release}),
            )
        }
    }
}

fn admitted_clock(now_ms: i64) -> Result<i64, Fault> {
    if now_ms <= 0 {
        return Err(Fault::Hard(
            "a local layer op needs the host's clock in milliseconds".into(),
        ));
    }
    Ok(now_ms)
}

// ── register ────────────────────────────────────────────────────────────

/// `createSketchLayer` / `createSketchLayerFromFeatureStorage`, with every
/// default the kernel's.
fn register(
    draft: &Draft,
    now_ms: i64,
    sequence: u64,
    color_index: u64,
) -> Result<LocalLayer, Fault> {
    let kind = draft.source_kind.unwrap_or(SourceKind::Manual);
    if kind == SourceKind::ProjectWork {
        return Err(refusal(
            UNSUPPORTED_SOURCE_KIND,
            "Project Work is an ephemeral governed map root, not a prepared local layer",
        ));
    }
    let pinned = admitted_pair(draft, kind)?;
    let name = draft.name.trim().to_owned();
    let id = match &pinned {
        Some((transformer, class_name)) => pinned_id(transformer, class_name),
        None => format!("sketch-{now_ms}-{sequence}"),
    };
    let feature_count = admitted_count(draft.feature_count.unwrap_or(0))?;
    let feature_storage = admitted_storage(draft.feature_storage.as_ref(), &id)?;
    let layer = LocalLayer {
        source_name: source_label(&name, draft.source_name.as_deref(), kind),
        source_layer_name: match draft.source_layer_name.as_deref().map(str::trim) {
            Some(named) if !named.is_empty() => named.to_owned(),
            _ => name.clone(),
        },
        source_kind: kind,
        source_metadata: SourceMetadata::from_raw(
            draft.source_metadata.as_ref(),
            kind,
            Some(now_ms),
        ),
        geometry_type: draft.geometry_type,
        // Pinned rows are one visual class, painted in the class colour; every
        // other kind takes the palette entry the host has reached.
        color: match kind {
            SourceKind::Pinned => PINNED_COLOR.to_owned(),
            _ => SKETCH_COLORS[(color_index % SKETCH_COLORS.len() as u64) as usize].to_owned(),
        },
        line_width: DEFAULT_LINE_WIDTH,
        point_radius: DEFAULT_POINT_RADIUS,
        fill_opacity: DEFAULT_FILL_OPACITY,
        style_doc: None,
        schema: match draft
            .pinned
            .as_ref()
            .and_then(|pinned| pinned.fields.as_deref())
        {
            Some(fields) => pinned_schema(fields),
            None => draft.schema.clone().unwrap_or_default(),
        },
        feature_count,
        feature_storage,
        visible: true,
        created_at: now_ms,
        name,
        id,
    };
    layer.validate()?;
    Ok(layer)
}

/// The pair a pinned row is named by, or `None` for every other layer. A
/// pinned identity belongs to a pinned layer and names both halves of the
/// pair; a host that offers one for another kind, or half of one, built the
/// request out of a layer it had not identified. It is REQUIRED of a pinned
/// layer: without the pair the id falls back to the clock and the counter, and
/// a room materialised again would be a second row instead of the same one —
/// the collision the pair exists to close.
fn admitted_pair(draft: &Draft, kind: SourceKind) -> Result<Option<(String, String)>, Fault> {
    let Some(pinned) = &draft.pinned else {
        if kind == SourceKind::Pinned {
            return Err(malformed(
                "a pinned layer is named by the transformer and design class it was materialised from",
            ));
        }
        return Ok(None);
    };
    if kind != SourceKind::Pinned {
        return Err(malformed(
            "a pinned identity belongs to a layer of source kind pinned",
        ));
    }
    let transformer = sanitize(&pinned.transformer, MAX_TEXT_CHARS);
    let class_name = sanitize(&pinned.class_name, MAX_TEXT_CHARS);
    if transformer.is_empty() || class_name.is_empty() {
        return Err(malformed(
            "a pinned identity names its transformer and its design class",
        ));
    }
    if pinned.fields.is_some() && draft.schema.is_some() {
        return Err(malformed(
            "a pinned layer's columns are derived from its field names, so it carries no schema of its own",
        ));
    }
    Ok(Some((transformer, class_name)))
}

/// The id of a pinned row, derived from the pair it was materialised from:
/// `pinned-<transformer>__<class>-<fingerprint>`. The slugs are readable, not
/// identifying — lower-casing and folding punctuation loses information, so
/// `Poles A` and `Poles-A` slug the same. The fingerprint is taken over the
/// pair itself, which is what keeps two distinct pairs two distinct layers,
/// and the whole id is one path segment.
fn pinned_id(transformer: &str, class_name: &str) -> String {
    let digest = Sha256::digest(format!("{transformer}\u{1f}{class_name}").as_bytes());
    let fingerprint: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "pinned-{}__{}-{fingerprint}",
        pinned_slug(transformer),
        pinned_slug(class_name)
    )
}

/// One half of a pinned id: lowercase ASCII letters and digits, anything else
/// a single separator, bounded.
fn pinned_slug(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        if slug.chars().count() >= MAX_PINNED_SLUG_CHARS {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "unnamed".to_owned()
    } else {
        slug.to_owned()
    }
}

/// The columns of a pinned row: the property keys its features carry,
/// sanitised like every schema field, ordered and de-duplicated so the same
/// pair always answers the same schema, and bounded.
fn pinned_schema(fields: &[String]) -> Vec<Field> {
    let mut names: Vec<String> = fields
        .iter()
        .map(|name| sanitize(name, MAX_NAME_CHARS))
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names.truncate(MAX_SCHEMA_FIELDS);
    names
        .into_iter()
        .map(|name| Field {
            name,
            kind: FieldType::Text,
            default_value: None,
            options: None,
            required: None,
        })
        .collect()
}

/// A feature count a host asserts is never negative; only `restore` repairs
/// one.
fn admitted_count(count: i64) -> Result<u64, Fault> {
    u64::try_from(count).map_err(|_| malformed("a feature count is never negative"))
}

/// Storage a host offers must locate its chunks: `null` or absent is no
/// storage, but an object the kernel cannot read is a malformed descriptor,
/// not a payload to lose quietly. Only `restore` repairs one.
fn admitted_storage(raw: Option<&Value>, owner: &str) -> Result<Option<FeatureStorage>, Fault> {
    let storage = FeatureStorage::from_raw(raw, owner);
    if raw.is_some_and(|raw| !raw.is_null()) && storage.is_none() {
        return Err(malformed(
            "feature storage must name the upload chunks that hold the payload",
        ));
    }
    if let Some(storage) = &storage {
        storage.validate()?;
    }
    Ok(storage)
}

// ── restore ─────────────────────────────────────────────────────────────

/// The reopen-after-restart rule set: `normalizeMetaLayer` over every raw
/// persisted row, `project_work` rows retired, and a duplicate id refusing the
/// whole store rather than quietly dropping a layer.
fn restore(scope: Scope, records: &[Value], now_ms: i64) -> Result<Value, Fault> {
    if records.len() > MAX_LAYERS {
        return Err(malformed(&format!(
            "a local layer store holds at most {MAX_LAYERS} layers"
        )));
    }
    let mut registry = Registry::opened(scope);
    let mut retired: Vec<(String, Option<FeatureStorage>)> = vec![];
    let mut repaired: Vec<Value> = vec![];
    // Rows this reopen refuses to re-key, and the chunks they still park in.
    let mut carried: Vec<Value> = vec![];
    let mut carried_storage: Vec<FeatureStorage> = vec![];
    let mut palette = 0usize;
    // One id space for what is reopened and what is retired: a retired row
    // that shares its id with a kept row would otherwise name the kept
    // layer's payload for release.
    let mut ids = HashSet::new();
    for (index, record) in records.iter().enumerate() {
        let Some(row) = record.as_object() else {
            return Err(malformed(&format!(
                "record {index} of the local layer store is not an object"
            )));
        };
        let kind = SourceKind::from_raw(row.get("sourceKind"));
        let id = restored_id(row, index, now_ms);
        // An id is never repaired. Repairing one re-keys the row and orphans
        // the payload the host parked under the id the row actually carries,
        // so a row whose id cannot be admitted as it stands is CARRIED: not
        // reopened, not retired, nothing released for it. The host persists it
        // verbatim beside the admitted registry, a later kernel can still
        // repair it, and the next reopen carries it again — stable, never
        // lossy. The question is asked first, so it holds whatever else the
        // row says of itself.
        if let Some(reason) = id_fault(&id) {
            carried.push(json!({"raw": record, "reason": reason}));
            if let Some(storage) = FeatureStorage::from_raw(row.get("featureStorage"), &id) {
                carried_storage.push(storage);
            }
            continue;
        }
        if !ids.insert(id.clone()) {
            return Err(refusal(
                DUPLICATE_LAYER,
                &format!("local layer {id} appears twice in the store"),
            ));
        }
        if kind == SourceKind::ProjectWork {
            // Ephemeral by rule: it is released, never reopened.
            let storage = FeatureStorage::from_raw(row.get("featureStorage"), &id);
            retired.push((id, storage));
            continue;
        }
        let layer = restore_row(row, id, kind, now_ms, &mut palette);
        layer.validate()?;
        let admitted =
            serde_json::to_value(&layer).map_err(|error| Fault::Hard(error.to_string()))?;
        let fields = repairs(row, &admitted);
        if !fields.is_empty() {
            repaired.push(json!({"id": layer.id, "fields": fields}));
        }
        registry.layers.push(layer);
    }
    // What was admitted is what `changed` speaks for: a carried row leaves the
    // host's bytes exactly as they were, so it is no reason to rewrite them.
    let changed = !retired.is_empty() || !repaired.is_empty();
    if changed {
        registry.revision = 1;
    }
    registry.validate()?;
    // What the host may delete for each retired row: its own payload record
    // and its parked chunks — unless a reopened layer still parks there.
    let mut release: Vec<Value> = vec![];
    for (id, storage) in &retired {
        release.push(json!({"kind": "features_payload", "id": id}));
        if let Some(storage) = storage {
            // A carried row is still a row, and it still parks its payload
            // where it says it does.
            if !chunks_still_parked(storage, &registry.layers)
                && !carried_storage
                    .iter()
                    .any(|parked| parked.same_payload(storage))
            {
                release.push(storage.release());
            }
        }
    }
    let retired: Vec<&String> = retired.iter().map(|(id, _)| id).collect();
    let mut receipt = json!({
        "restored": registry.layers.len(),
        "retired": retired,
        "repaired": repaired,
        "release": release,
    });
    // `carried` is the exception, not the shape: a store with nothing to carry
    // answers the receipt it always has.
    if !carried.is_empty() {
        if let Some(receipt) = receipt.as_object_mut() {
            receipt.insert("carried".to_owned(), Value::Array(carried));
        }
    }
    Ok(json!({
        "registry": registry,
        "receipt": receipt,
        "changed": changed,
    }))
}

/// The id a row carries, read exactly as it was persisted — never trimmed,
/// never truncated, never stripped. A row that carries no id at all has no
/// record keyed under one either, so it is minted one from the host's clock
/// and its place in the store; the same store then reopens to the same ids.
fn restored_id(row: &Map<String, Value>, index: usize, now_ms: i64) -> String {
    let id = match row.get("id") {
        Some(Value::String(id)) => id.clone(),
        other => js_string(other),
    };
    if id.is_empty() {
        format!("sketch-{now_ms}-{index}")
    } else {
        id
    }
}

/// Why a stored id cannot be admitted as it stands — the id rules of
/// `LocalLayer::validate`, asked before the row is normalised so that `restore`
/// can carry the row rather than re-key it. The answer is the reason the
/// receipt reports, so it says which rule the id broke.
fn id_fault(id: &str) -> Option<&'static str> {
    if id.is_empty() {
        Some("a local layer id is never repaired, and this row's id is blank")
    } else if id.chars().any(char::is_control) {
        Some("a local layer id is never repaired, and this row's id carries a control character")
    } else if id.trim() != id {
        Some("a local layer id is never repaired, and this row's id is not trimmed")
    } else if id.chars().count() > MAX_NAME_CHARS {
        Some("a local layer id is never repaired, and this row's id is longer than 200 characters")
    } else if !one_segment(id) {
        Some("a local layer id is never repaired, and this row's id is not one path segment")
    } else {
        None
    }
}

fn restore_row(
    row: &Map<String, Value>,
    id: String,
    kind: SourceKind,
    now_ms: i64,
    palette: &mut usize,
) -> LocalLayer {
    let name = match row.get("name") {
        Some(value) if !value.is_null() => sanitize(&js_string(Some(value)), MAX_NAME_CHARS),
        _ => String::new(),
    };
    let name = if name.is_empty() {
        "Layer".to_owned()
    } else {
        name
    };
    // A legacy source label is repaired like every other text field. It used
    // to be read raw, so one row filed under an unbounded or control-laden
    // label refused the whole reopen and stranded every layer in the store.
    let source_name = sanitize(&js_string(row.get("sourceName")), MAX_TEXT_CHARS);
    let source_layer_name = sanitize(&js_string(row.get("sourceLayerName")), MAX_TEXT_CHARS);
    let color = sanitize(&js_string(row.get("color")), MAX_COLOR_CHARS);
    let color = if color.is_empty() {
        let picked = SKETCH_COLORS[*palette % SKETCH_COLORS.len()];
        *palette += 1;
        picked.to_owned()
    } else {
        color
    };
    let feature_count = match row.get("featureCount") {
        Some(value) if !value.is_null() => js_number(Some(value)),
        _ => row
            .get("features")
            .and_then(Value::as_array)
            .map(|features| features.len() as f64),
    };
    LocalLayer {
        source_name: source_label(&name, Some(&source_name), kind),
        source_layer_name: if source_layer_name.is_empty() {
            name.clone()
        } else {
            source_layer_name
        },
        source_kind: kind,
        // Normalised metadata and storage that still cannot be admitted are
        // dropped, never carried into a refusal: restore repairs.
        source_metadata: SourceMetadata::from_raw(row.get("sourceMetadata"), kind, Some(now_ms))
            .filter(|metadata| metadata.validate().is_ok()),
        geometry_type: GeometryType::from_raw(row.get("geometryType")),
        color,
        line_width: clamp_number(
            row.get("lineWidth"),
            DEFAULT_LINE_WIDTH,
            MIN_LINE_WIDTH,
            MAX_LINE_WIDTH,
        ),
        point_radius: clamp_number(
            row.get("pointRadius"),
            DEFAULT_POINT_RADIUS,
            MIN_POINT_RADIUS,
            MAX_POINT_RADIUS,
        ),
        fill_opacity: clamp_number(row.get("fillOpacity"), DEFAULT_FILL_OPACITY, 0.0, 1.0),
        style_doc: row.get("styleDoc").filter(|doc| doc.is_object()).cloned(),
        schema: row
            .get("schema")
            .and_then(Value::as_array)
            .map(|fields| {
                fields
                    .iter()
                    .take(MAX_SCHEMA_FIELDS)
                    .filter_map(Field::from_raw)
                    .collect()
            })
            .unwrap_or_default(),
        feature_count: feature_count
            .filter(|count| count.is_finite())
            .map_or(0, |count| count.trunc().max(0.0) as u64),
        feature_storage: FeatureStorage::from_raw(row.get("featureStorage"), &id)
            .filter(|storage| storage.validate().is_ok()),
        visible: row.get("visible") != Some(&Value::Bool(false)),
        created_at: match js_number(row.get("createdAt")) {
            Some(created) if created.is_finite() && created >= 0.0 => created.trunc() as i64,
            _ => now_ms,
        },
        name,
        id,
    }
}

/// The descriptor keys whose admitted value is not what the row carried — what
/// the host must rewrite for its store to be the admitted store.
fn repairs(row: &Map<String, Value>, admitted: &Value) -> Vec<String> {
    let admitted = admitted.as_object();
    DESCRIPTOR_KEYS
        .iter()
        .filter(|key| {
            let was = row.get(**key).filter(|value| !value.is_null());
            let now = admitted.and_then(|map| map.get(**key));
            match (was, now) {
                (None, None) => false,
                (Some(was), Some(now)) => !same_json(was, now),
                _ => true,
            }
        })
        .map(|key| (*key).to_owned())
        .collect()
}

/// Two JSON values say the same thing. Numbers compare numerically, so a
/// persisted `5` and an admitted `5.0` are not a repair.
fn same_json(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (left.as_f64(), right.as_f64()) {
            (Some(left), Some(right)) => left == right,
            _ => left == right,
        },
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| same_json(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| same_json(value, other)))
        }
        _ => left == right,
    }
}

// ── update ──────────────────────────────────────────────────────────────

/// Apply one change and answer with what the host may now release.
fn update(
    registry: &mut Registry,
    index: usize,
    change: Change,
    clock: Option<i64>,
) -> Result<Vec<Value>, Fault> {
    let mut release = vec![];
    match change {
        Change::Payload {
            schema,
            feature_count,
            feature_storage,
        } => {
            if schema.len() > MAX_SCHEMA_FIELDS {
                return Err(malformed(&format!(
                    "a local layer carries at most {MAX_SCHEMA_FIELDS} schema fields"
                )));
            }
            for field in &schema {
                field.validate()?;
            }
            let feature_count = admitted_count(feature_count)?;
            let next_storage =
                admitted_storage(feature_storage.as_ref(), &registry.layers[index].id)?;
            if let Some(previous) = registry.layers[index].feature_storage.clone() {
                // A replaced payload always leaves the parked chunks behind —
                // unless the replacement is those same chunks, or another
                // layer still parks its payload there.
                let replaced_by_the_same = next_storage
                    .as_ref()
                    .is_some_and(|next| next.same_payload(&previous));
                let parked_elsewhere = chunks_still_parked(
                    &previous,
                    registry
                        .layers
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != index)
                        .map(|(_, other)| other),
                );
                if !replaced_by_the_same && !parked_elsewhere {
                    release.push(previous.release());
                }
            }
            let layer = &mut registry.layers[index];
            layer.schema = schema;
            layer.feature_count = feature_count;
            layer.feature_storage = next_storage;
        }
        Change::SourceMetadata { source_metadata } => {
            let layer = &mut registry.layers[index];
            let metadata =
                SourceMetadata::from_raw(source_metadata.as_ref(), layer.source_kind, clock);
            if let Some(metadata) = &metadata {
                metadata.validate()?;
            }
            layer.source_metadata = metadata;
        }
        Change::Rename { name } => {
            let name = name.trim().to_owned();
            if !bounded(&name, MAX_NAME_CHARS) {
                return Err(malformed(
                    "a local layer needs a bounded trimmed name of 1 to 200 characters",
                ));
            }
            let layer = &mut registry.layers[index];
            // A manual layer's source layer name is its name; an imported
            // layer keeps the name it had inside its source.
            if layer.source_kind == SourceKind::Manual {
                layer.source_layer_name = name.clone();
            }
            layer.source_name = source_label(&name, Some(&layer.source_name), layer.source_kind);
            layer.name = name;
        }
        Change::RenameSource {
            source_kind,
            source_name,
            name,
        } => {
            let kind = source_kind.unwrap_or(SourceKind::Manual);
            if kind == SourceKind::ProjectWork {
                return Err(refusal(
                    UNSUPPORTED_SOURCE_KIND,
                    "Project Work is an ephemeral governed map root, not a prepared local layer",
                ));
            }
            let name = name.trim().to_owned();
            if !bounded(&name, MAX_TEXT_CHARS) {
                return Err(malformed("a source needs a bounded trimmed name"));
            }
            // A source rename moves every layer filed under that label.
            for layer in registry
                .layers
                .iter_mut()
                .filter(|layer| layer.source_kind == kind && layer.source_name == source_name)
            {
                // The label rule holds here too: a manual layer is never
                // filed under its own name.
                layer.source_name = source_label(&layer.name, Some(&name), kind);
            }
        }
        Change::Style {
            color,
            line_width,
            point_radius,
            fill_opacity,
            style_doc,
        } => {
            let layer = &mut registry.layers[index];
            if let Some(color) = color {
                let color = color.trim().to_owned();
                if !bounded(&color, MAX_COLOR_CHARS) {
                    return Err(malformed("a local layer needs a bounded colour"));
                }
                layer.color = color;
            }
            // Only `restore` clamps; a host that asserts a value outside the
            // descriptor's range is refused, and nothing moves.
            if let Some(line_width) = line_width {
                if !(MIN_LINE_WIDTH..=MAX_LINE_WIDTH).contains(&line_width) {
                    return Err(malformed("a line width is 0.5 to 12"));
                }
                layer.line_width = line_width;
            }
            if let Some(point_radius) = point_radius {
                if !(MIN_POINT_RADIUS..=MAX_POINT_RADIUS).contains(&point_radius) {
                    return Err(malformed("a point radius is 1 to 24"));
                }
                layer.point_radius = point_radius;
            }
            if let Some(fill_opacity) = fill_opacity {
                if !(0.0..=1.0).contains(&fill_opacity) {
                    return Err(malformed("a fill opacity is 0 to 1"));
                }
                layer.fill_opacity = fill_opacity;
            }
            if let Some(style_doc) = style_doc {
                if style_doc.as_ref().is_some_and(|doc| !doc.is_object()) {
                    return Err(malformed("a style document must be an object"));
                }
                layer.style_doc = style_doc;
            }
        }
        Change::Visible { visible } => registry.layers[index].visible = visible,
    }
    Ok(release)
}

#[cfg(test)]
#[path = "local_layers_tests.rs"]
mod tests;
