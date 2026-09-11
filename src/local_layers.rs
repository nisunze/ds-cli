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
//! an original source file, a folder handle, a project asset or another
//! layer's storage — the kernel never names one.
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

pub const MANUAL_SOURCE_NAME: &str = "Manual layers";
pub const IMPORT_SOURCE_NAME: &str = "Imported files";
pub const DERIVED_SOURCE_NAME: &str = "Derived layers";
pub const PINNED_SOURCE_NAME: &str = "Pinned transformers";
pub const PROJECT_WORK_SOURCE_NAME: &str = "Project Work";

pub const MALFORMED_DESCRIPTOR: &str = "malformed_descriptor";
pub const DUPLICATE_LAYER: &str = "duplicate_layer";
pub const UNKNOWN_LAYER: &str = "unknown_layer";
pub const SCOPE_MISMATCH: &str = "scope_mismatch";
pub const PROJECT_CONTEXT_CHANGED: &str = "project_context_changed";
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

/// Where the layer came from. `gis_folder` is a file inside a locally
/// connected folder that can be refreshed and saved back; `import_file` is a
/// one-way import of a file the host read once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceMetadata {
    #[serde(rename_all = "camelCase")]
    GisFolder {
        ds_project: String,
        handle_key: String,
        folder_name: String,
        file_path: String,
        file_name: String,
        format: String,
        last_refresh_at: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dirty_since_refresh: Option<i64>,
    },
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
    /// The project a `gis_folder` layer belongs to. An `import_file` layer
    /// belongs to no project, so no project switch can fence it.
    pub fn ds_project(&self) -> Option<&str> {
        match self {
            Self::GisFolder { ds_project, .. } => Some(ds_project),
            Self::ImportFile { .. } => None,
        }
    }
    fn validate(&self) -> Result<(), Fault> {
        let texts: Vec<&String> = match self {
            Self::GisFolder {
                ds_project,
                handle_key,
                folder_name,
                file_path,
                file_name,
                ..
            } => vec![ds_project, handle_key, folder_name, file_path, file_name],
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
            Self::GisFolder {
                last_refresh_at, ..
            }
            | Self::ImportFile {
                last_refresh_at, ..
            } => *last_refresh_at,
        };
        if last_refresh_at <= 0 {
            return Err(malformed("source metadata needs a refresh timestamp"));
        }
        Ok(())
    }
    /// The bounded origin a catalogue row carries: what the layer came from
    /// and, for a folder connection, which project it belongs to. Never the
    /// handle key — a list is not a capability.
    fn row(&self) -> Value {
        match self {
            Self::GisFolder {
                ds_project,
                file_path,
                dirty_since_refresh,
                ..
            } => json!({
                "kind": "gis_folder",
                "label": file_path,
                "dsProject": ds_project,
                "dirtySinceRefresh": dirty_since_refresh,
            }),
            Self::ImportFile {
                file_name,
                archive_inner_name,
                dirty_since_refresh,
                ..
            } => json!({
                "kind": "import_file",
                "label": match archive_inner_name {
                    Some(inner) => format!("{file_name} → {inner}"),
                    None => file_name.clone(),
                },
                "dirtySinceRefresh": dirty_since_refresh,
            }),
        }
    }

    /// `normalizeSourceMetadata`. A folder connection is admitted for any
    /// source kind; an imported file only for an `import` layer — a manual
    /// layer that is handed import metadata simply has none. Anything missing
    /// an identifying field is dropped whole rather than half-kept.
    fn from_raw(raw: Option<&Value>, kind: SourceKind, clock: Option<i64>) -> Option<Self> {
        let row = raw?.as_object()?;
        match row.get("kind").and_then(Value::as_str) {
            Some("gis_folder") => {
                let ds_project = js_string(row.get("dsProject"));
                let handle_key = js_string(row.get("handleKey"));
                let folder_name = js_string(row.get("folderName"));
                let file_path = js_string(row.get("filePath"));
                let file_name = match row.get("fileName") {
                    Some(value) if !value.is_null() => js_string(Some(value)),
                    _ => last_segment(&file_path, '/').to_owned(),
                };
                if ds_project.is_empty()
                    || handle_key.is_empty()
                    || folder_name.is_empty()
                    || file_path.is_empty()
                    || file_name.is_empty()
                {
                    return None;
                }
                let format = match row.get("format") {
                    Some(value) if !value.is_null() => js_string(Some(value)),
                    _ => last_segment(&file_name, '.').to_owned(),
                }
                .to_lowercase();
                Some(Self::GisFolder {
                    ds_project: sanitize(&ds_project, MAX_TEXT_CHARS),
                    handle_key: sanitize(&handle_key, MAX_TEXT_CHARS),
                    folder_name: sanitize(&folder_name, MAX_TEXT_CHARS),
                    file_path: sanitize(&file_path, MAX_TEXT_CHARS),
                    file_name: sanitize(&file_name, MAX_TEXT_CHARS),
                    format: sanitize(&format, MAX_TEXT_CHARS),
                    last_refresh_at: refresh_time(row.get("lastRefreshAt"), clock)?,
                    dirty_since_refresh: finite_positive(row.get("dirtySinceRefresh")),
                })
            }
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
    /// payload record, and its own parked chunks. Nothing else exists to name.
    fn release(&self) -> Vec<Value> {
        let mut release = vec![json!({"kind": "features_payload", "id": self.id})];
        if let Some(storage) = &self.feature_storage {
            release.push(storage.release());
        }
        release
    }
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

/// `'a/b/c.geojson'.split('/').pop()`.
fn last_segment(value: &str, separator: char) -> &str {
    value.rsplit(separator).next().unwrap_or("")
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
        project: Option<String>,
        #[serde(default)]
        now_ms: Option<i64>,
        change: Change,
    },
    /// Remove one logical layer and name what the host may delete with it.
    Remove {
        registry: Registry,
        scope: Scope,
        id: String,
        #[serde(default)]
        project: Option<String>,
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
            if registry.layers.iter().any(|row| row.id == layer.id) {
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
            project,
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
            project_fence(&registry.layers[index], project.as_deref())?;
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
            project,
        } => {
            registry.validate()?;
            scope.validate()?;
            registry.fence(&scope)?;
            let index = registry.find(&id)?;
            let layer = &registry.layers[index];
            project_fence(layer, project.as_deref())?;
            let release = layer.release();
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

/// A layer prepared from a connected folder belongs to the project that folder
/// was connected for. An op raised under another project is a stale hand: it
/// changes nothing. An op that names no project is not fenced — the browser's
/// catalogue is not project-bound.
fn project_fence(layer: &LocalLayer, project: Option<&str>) -> Result<(), Fault> {
    let (Some(project), Some(owner)) = (
        project,
        layer
            .source_metadata
            .as_ref()
            .and_then(SourceMetadata::ds_project),
    ) else {
        return Ok(());
    };
    if owner != project {
        return Err(refusal(
            PROJECT_CONTEXT_CHANGED,
            &format!("local layer {} belongs to project {owner}", layer.id),
        ));
    }
    Ok(())
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
    let name = draft.name.trim().to_owned();
    let id = format!("sketch-{now_ms}-{sequence}");
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
        color: SKETCH_COLORS[(color_index % SKETCH_COLORS.len() as u64) as usize].to_owned(),
        line_width: DEFAULT_LINE_WIDTH,
        point_radius: DEFAULT_POINT_RADIUS,
        fill_opacity: DEFAULT_FILL_OPACITY,
        style_doc: None,
        schema: draft.schema.clone().unwrap_or_default(),
        feature_count: draft.feature_count.unwrap_or(0).max(0) as u64,
        feature_storage: FeatureStorage::from_raw(draft.feature_storage.as_ref(), &id),
        visible: true,
        created_at: now_ms,
        name,
        id,
    };
    layer.validate()?;
    Ok(layer)
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
    let mut retired: Vec<String> = vec![];
    let mut release: Vec<Value> = vec![];
    let mut repaired: Vec<Value> = vec![];
    let mut palette = 0usize;
    let mut ids = HashSet::new();
    for (index, record) in records.iter().enumerate() {
        let Some(row) = record.as_object() else {
            return Err(malformed(&format!(
                "record {index} of the local layer store is not an object"
            )));
        };
        let kind = SourceKind::from_raw(row.get("sourceKind"));
        let id = restored_id(row, index, now_ms);
        if kind == SourceKind::ProjectWork {
            // Ephemeral by rule: it is released, never reopened.
            if !retired.contains(&id) {
                release.push(json!({"kind": "features_payload", "id": id}));
                if let Some(storage) = FeatureStorage::from_raw(row.get("featureStorage"), &id) {
                    release.push(storage.release());
                }
                retired.push(id);
            }
            continue;
        }
        if !ids.insert(id.clone()) {
            return Err(refusal(
                DUPLICATE_LAYER,
                &format!("local layer {id} appears twice in the store"),
            ));
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
    let changed = !retired.is_empty() || !repaired.is_empty();
    if changed {
        registry.revision = 1;
    }
    registry.validate()?;
    Ok(json!({
        "registry": registry,
        "receipt": {
            "restored": registry.layers.len(),
            "retired": retired,
            "repaired": repaired,
            "release": release,
        },
        "changed": changed,
    }))
}

/// A row that lost its id gets one from the host's clock and its place in the
/// store, so the same store reopens to the same ids.
fn restored_id(row: &Map<String, Value>, index: usize, now_ms: i64) -> String {
    let id = sanitize(&js_string(row.get("id")), MAX_NAME_CHARS);
    if id.is_empty() {
        format!("sketch-{now_ms}-{index}")
    } else {
        id
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
        source_name: source_label(&name, row.get("sourceName").and_then(Value::as_str), kind),
        source_layer_name: if source_layer_name.is_empty() {
            name.clone()
        } else {
            source_layer_name
        },
        source_kind: kind,
        source_metadata: SourceMetadata::from_raw(row.get("sourceMetadata"), kind, Some(now_ms)),
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
        feature_storage: FeatureStorage::from_raw(row.get("featureStorage"), &id),
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
            let next_storage =
                FeatureStorage::from_raw(feature_storage.as_ref(), &registry.layers[index].id);
            if let Some(storage) = &next_storage {
                storage.validate()?;
            }
            let layer = &mut registry.layers[index];
            if let Some(previous) = layer.feature_storage.take() {
                // A replaced payload always leaves the parked chunks behind —
                // unless the replacement is those same chunks.
                if !next_storage
                    .as_ref()
                    .is_some_and(|next| next.same_payload(&previous))
                {
                    release.push(previous.release());
                }
            }
            layer.schema = schema;
            layer.feature_count = feature_count.max(0) as u64;
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
            for layer in &mut registry.layers {
                if layer.source_kind == kind && layer.source_name == source_name {
                    layer.source_name = name.clone();
                }
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
            if let Some(line_width) = line_width {
                layer.line_width = clamp_f64(
                    Some(line_width),
                    DEFAULT_LINE_WIDTH,
                    MIN_LINE_WIDTH,
                    MAX_LINE_WIDTH,
                );
            }
            if let Some(point_radius) = point_radius {
                layer.point_radius = clamp_f64(
                    Some(point_radius),
                    DEFAULT_POINT_RADIUS,
                    MIN_POINT_RADIUS,
                    MAX_POINT_RADIUS,
                );
            }
            if let Some(fill_opacity) = fill_opacity {
                layer.fill_opacity = clamp_f64(Some(fill_opacity), DEFAULT_FILL_OPACITY, 0.0, 1.0);
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
mod tests {
    use super::*;

    fn ask(op: Value) -> Value {
        let request = serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).expect("encodes");
        let answer = evaluate(&request).unwrap_or_else(|error| panic!("{error}"));
        serde_json::from_str(&answer).expect("the answer is JSON")
    }
    fn refused(op: Value) -> String {
        let answer = ask(op);
        assert_eq!(answer["outcome"], "refused", "{answer}");
        answer["refusal"]["code"]
            .as_str()
            .expect("a refusal names its code")
            .to_owned()
    }
    fn rejected(op: Value) -> String {
        let request = serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).expect("encodes");
        evaluate(&request).expect_err("the request itself is malformed")
    }
    fn browser() -> Value {
        json!({"host": "browser"})
    }
    fn store(layers: Vec<Value>) -> Value {
        json!({"schema": 1, "revision": 7, "scope": browser(), "layers": layers})
    }
    fn empty() -> Value {
        json!({"schema": 1, "revision": 0, "scope": browser(), "layers": []})
    }
    fn register(registry: Value, draft: Value) -> Value {
        json!({"kind": "register", "registry": registry, "scope": browser(),
               "now_ms": 1_700_000_000_000i64, "sequence": 4, "colorIndex": 3, "draft": draft})
    }
    fn manual_draft() -> Value {
        json!({"name": "Access road", "geometryType": "LineString"})
    }
    /// One admitted layer, as the registry holds it.
    fn admitted(draft: Value) -> Value {
        ask(register(empty(), draft))["registry"]["layers"][0].clone()
    }
    fn update(registry: Value, id: &str, change: Value) -> Value {
        json!({"kind": "update", "registry": registry, "scope": browser(), "id": id, "change": change})
    }

    #[test]
    fn register_fills_every_default_the_host_no_longer_decides() {
        let answer = ask(register(empty(), manual_draft()));
        let layer = &answer["registry"]["layers"][0];
        assert_eq!(layer["id"], "sketch-1700000000000-4");
        assert_eq!(layer["name"], "Access road");
        assert_eq!(layer["sourceName"], MANUAL_SOURCE_NAME);
        assert_eq!(layer["sourceLayerName"], "Access road");
        assert_eq!(layer["sourceKind"], "manual");
        assert_eq!(layer["geometryType"], "LineString");
        assert_eq!(layer["color"], SKETCH_COLORS[3]);
        assert_eq!(layer["lineWidth"], 2.5);
        assert_eq!(layer["pointRadius"], 5.0);
        assert_eq!(layer["fillOpacity"], 0.15);
        assert_eq!(layer["schema"], json!([]));
        assert_eq!(layer["featureCount"], 0);
        assert_eq!(layer["visible"], true);
        assert_eq!(layer["createdAt"], 1_700_000_000_000i64);
        assert!(layer.get("styleDoc").is_none() && layer.get("sourceMetadata").is_none());
        assert_eq!(
            &answer["receipt"], layer,
            "the receipt is the admitted layer"
        );
        assert_eq!(answer["changed"], true);
        assert_eq!(answer["registry"]["revision"], 1);
    }

    #[test]
    fn the_palette_wraps_and_belongs_to_the_kernel() {
        for index in [0u64, 9, 10, 23] {
            let mut op = register(empty(), manual_draft());
            op["colorIndex"] = json!(index);
            assert_eq!(
                ask(op)["registry"]["layers"][0]["color"],
                json!(SKETCH_COLORS[(index % 10) as usize])
            );
        }
    }

    #[test]
    fn a_source_name_defaults_by_kind_and_never_repeats_a_manual_layers_own_name() {
        for (kind, expected) in [
            ("manual", MANUAL_SOURCE_NAME),
            ("import", IMPORT_SOURCE_NAME),
            ("derived", DERIVED_SOURCE_NAME),
            ("pinned", PINNED_SOURCE_NAME),
        ] {
            let mut draft = manual_draft();
            draft["sourceKind"] = json!(kind);
            assert_eq!(admitted(draft)["sourceName"], expected);
        }
        let mut draft = manual_draft();
        draft["sourceName"] = json!("  Access road  ");
        assert_eq!(admitted(draft)["sourceName"], MANUAL_SOURCE_NAME);
        let mut draft = manual_draft();
        draft["sourceName"] = json!("Field survey");
        draft["sourceLayerName"] = json!("  roads  ");
        let layer = admitted(draft);
        assert_eq!(layer["sourceName"], "Field survey");
        assert_eq!(layer["sourceLayerName"], "roads");
    }

    #[test]
    fn a_folder_connection_is_normalised_and_kept_for_any_kind() {
        let mut draft = manual_draft();
        draft["sourceMetadata"] = json!({
            "kind": "gis_folder", "dsProject": " p1 ", "handleKey": "h1",
            "folderName": "gis", "filePath": "project/gis/roads.GeoJSON",
        });
        let layer = admitted(draft);
        assert_eq!(
            layer["sourceMetadata"],
            json!({
                "kind": "gis_folder", "dsProject": "p1", "handleKey": "h1", "folderName": "gis",
                "filePath": "project/gis/roads.GeoJSON", "fileName": "roads.GeoJSON",
                "format": "geojson", "lastRefreshAt": 1_700_000_000_000i64,
            }),
            "the file name comes from the path and the format from the file name"
        );
    }

    #[test]
    fn a_folder_connection_missing_an_identifying_field_is_dropped_whole() {
        let mut draft = manual_draft();
        draft["sourceMetadata"] = json!({
            "kind": "gis_folder", "dsProject": "p1", "folderName": "gis",
            "filePath": "gis/roads.geojson", "lastRefreshAt": 5,
        });
        assert!(admitted(draft).get("sourceMetadata").is_none());
    }

    #[test]
    fn imported_file_metadata_belongs_only_to_an_imported_layer() {
        let metadata = json!({
            "kind": "import_file", "fileName": "roads.zip", "archiveInnerName": "roads",
            "sourcePath": "", "sizeBytes": 12.5, "lastModified": 0, "lastRefreshAt": 42,
        });
        let mut draft = manual_draft();
        draft["sourceMetadata"] = metadata.clone();
        assert!(
            admitted(draft).get("sourceMetadata").is_none(),
            "a manual layer has no imported file behind it"
        );
        let mut draft = manual_draft();
        draft["sourceKind"] = json!("import");
        draft["sourceMetadata"] = metadata;
        assert_eq!(
            admitted(draft)["sourceMetadata"],
            json!({
                "kind": "import_file", "fileName": "roads.zip", "sourceName": "roads.zip",
                "archiveInnerName": "roads", "sizeBytes": 12, "lastRefreshAt": 42,
            }),
            "an empty path and a non-positive timestamp are absent, not zeroed"
        );
    }

    #[test]
    fn project_work_is_refused_by_name_wherever_it_is_offered() {
        let mut draft = manual_draft();
        draft["sourceKind"] = json!("project_work");
        assert_eq!(refused(register(empty(), draft)), UNSUPPORTED_SOURCE_KIND);
        let layer = admitted(manual_draft());
        let id = layer["id"].as_str().expect("id").to_owned();
        assert_eq!(
            refused(update(
                store(vec![layer]),
                &id,
                json!({"kind": "rename_source", "sourceKind": "project_work",
                       "sourceName": MANUAL_SOURCE_NAME, "name": "Anything"}),
            )),
            UNSUPPORTED_SOURCE_KIND
        );
    }

    #[test]
    fn an_id_is_minted_once_and_a_second_layer_with_it_is_refused() {
        let first = admitted(manual_draft());
        assert_eq!(
            refused(register(store(vec![first]), manual_draft())),
            DUPLICATE_LAYER,
            "the same clock and sequence mint the same id"
        );
    }

    #[test]
    fn a_descriptor_that_breaks_its_own_bounds_is_refused_and_changes_nothing() {
        let mut draft = manual_draft();
        draft["name"] = json!("n".repeat(MAX_NAME_CHARS + 1));
        assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
        let mut draft = manual_draft();
        draft["name"] = json!("   ");
        assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
        let mut draft = manual_draft();
        draft["schema"] = json!(vec![
            json!({"name": "f", "type": "text"});
            MAX_SCHEMA_FIELDS + 1
        ]);
        assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
    }

    #[test]
    fn a_full_registry_admits_nothing_more() {
        let layer = admitted(manual_draft());
        let layers: Vec<Value> = (0..MAX_LAYERS)
            .map(|index| {
                let mut row = layer.clone();
                row["id"] = json!(format!("sketch-{index}"));
                row
            })
            .collect();
        assert_eq!(
            refused(register(store(layers), manual_draft())),
            MALFORMED_DESCRIPTOR
        );
    }

    #[test]
    fn parked_chunk_storage_is_normalised_at_admission() {
        let mut draft = manual_draft();
        draft["featureCount"] = json!(9);
        draft["featureStorage"] = json!({
            "kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
            "parsedLayer": "rows", "renderMode": "preview", "previewLimit": 9_000_000,
        });
        let layer = admitted(draft);
        assert_eq!(layer["featureCount"], 9);
        assert_eq!(
            layer["featureStorage"],
            json!({
                "kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
                "parsedLayer": "rows", "idPrefix": "feat-sketch-1700000000000-4",
                "renderMode": "preview", "previewLimit": MAX_PREVIEW_LIMIT,
                "renderPreviewLoaded": false,
            })
        );
        let mut draft = manual_draft();
        draft["featureStorage"] = json!({"kind": "upload_chunks", "workspaceKey": "w1"});
        assert!(
            admitted(draft).get("featureStorage").is_none(),
            "storage that cannot locate its chunks is not storage"
        );
    }

    #[test]
    fn list_answers_bounded_rows_with_the_origin_and_never_the_handle_key() {
        let mut draft = manual_draft();
        draft["sourceMetadata"] = json!({
            "kind": "gis_folder", "dsProject": "p1", "handleKey": "h1", "folderName": "gis",
            "filePath": "gis/roads.geojson", "lastRefreshAt": 11, "dirtySinceRefresh": 12,
        });
        let layer = admitted(draft);
        let answer = ask(
            json!({"kind": "list", "registry": store(vec![layer.clone()]),
                                "scope": browser(), "limit": 1}),
        );
        assert_eq!(answer["changed"], false);
        assert_eq!(answer["registry"]["revision"], 7, "a list is not a change");
        let row = &answer["receipt"]["layers"][0];
        assert_eq!(answer["receipt"]["count"], 1);
        assert_eq!(
            row["origin"],
            json!({"kind": "gis_folder", "label": "gis/roads.geojson",
                   "dsProject": "p1", "dirtySinceRefresh": 12})
        );
        assert!(!row.to_string().contains("h1"));
        for key in [
            "id",
            "name",
            "sourceName",
            "sourceLayerName",
            "sourceKind",
            "geometryType",
            "featureCount",
            "visible",
            "createdAt",
        ] {
            assert!(row.get(key).is_some(), "a row carries {key}");
        }
        for key in ["color", "schema", "styleDoc", "sourceMetadata"] {
            assert!(
                row.get(key).is_none(),
                "a row is bounded, {key} is not in it"
            );
        }
        let many = store(vec![layer.clone(), {
            let mut other = layer;
            other["id"] = json!("sketch-2");
            other
        }]);
        let answer = ask(json!({"kind": "list", "registry": many, "scope": browser(), "limit": 1}));
        assert_eq!(
            answer["receipt"]["layers"].as_array().expect("rows").len(),
            1
        );
        assert_eq!(answer["receipt"]["count"], 2, "the count is the catalogue");
    }

    fn restore(records: Vec<Value>) -> Value {
        ask(
            json!({"kind": "restore", "scope": browser(), "records": records,
                   "now_ms": 1_700_000_000_000i64}),
        )
    }

    #[test]
    fn restore_repairs_a_legacy_row_instead_of_losing_it() {
        let answer = restore(vec![json!({
            "name": "Old layer", "geometryType": "Polygon", "lineWidth": 99, "pointRadius": 0,
            "fillOpacity": -1, "features": [{"id": "f1"}, {"id": "f2"}],
        })]);
        let layer = &answer["registry"]["layers"][0];
        assert_eq!(
            layer["id"], "sketch-1700000000000-0",
            "minted from clock and place"
        );
        assert_eq!(layer["color"], SKETCH_COLORS[0], "the palette answers");
        assert_eq!(layer["lineWidth"], MAX_LINE_WIDTH);
        assert_eq!(layer["pointRadius"], MIN_POINT_RADIUS);
        assert_eq!(layer["fillOpacity"], 0.0);
        assert_eq!(layer["visible"], true, "an absent visible reads as visible");
        assert_eq!(layer["featureCount"], 2, "counted from the legacy payload");
        assert_eq!(layer["createdAt"], 1_700_000_000_000i64);
        assert_eq!(layer["sourceName"], MANUAL_SOURCE_NAME);
        assert_eq!(layer["sourceLayerName"], "Old layer");
        assert_eq!(answer["receipt"]["restored"], 1);
        assert_eq!(answer["changed"], true, "the host must rewrite its store");
        let repaired = &answer["receipt"]["repaired"][0];
        assert_eq!(repaired["id"], "sketch-1700000000000-0");
        assert_eq!(
            repaired["fields"],
            json!([
                "id",
                "sourceName",
                "sourceLayerName",
                "sourceKind",
                "color",
                "lineWidth",
                "pointRadius",
                "fillOpacity",
                "schema",
                "featureCount",
                "visible",
                "createdAt"
            ]),
            "every descriptor key the row did not already carry as admitted"
        );
        assert!(
            !repaired["fields"].to_string().contains("name\""),
            "the name the row carried is the name it keeps"
        );
    }

    #[test]
    fn a_row_that_needs_no_repair_is_not_reported_as_repaired() {
        let layer = admitted(manual_draft());
        let answer = restore(vec![layer.clone()]);
        assert_eq!(answer["registry"]["layers"][0], layer);
        assert_eq!(answer["receipt"]["repaired"], json!([]));
        assert_eq!(answer["changed"], false);
        assert_eq!(answer["registry"]["revision"], 0);
    }

    #[test]
    fn colourless_rows_take_the_palette_in_turn() {
        let answer = restore(vec![
            json!({"id": "a", "color": "#123456"}),
            json!({"id": "b"}),
            json!({"id": "c"}),
        ]);
        let layers = &answer["registry"]["layers"];
        assert_eq!(layers[0]["color"], "#123456");
        assert_eq!(layers[1]["color"], SKETCH_COLORS[0]);
        assert_eq!(layers[2]["color"], SKETCH_COLORS[1]);
    }

    #[test]
    fn project_work_rows_are_retired_with_their_keys_named_for_release() {
        let answer = restore(vec![
            json!({"id": "keep", "name": "Kept"}),
            json!({"id": "pw", "sourceKind": "project_work", "featureStorage": {
                "kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l",
            }}),
            json!({"sourceKind": "project_work"}),
        ]);
        assert_eq!(answer["receipt"]["restored"], 1);
        assert_eq!(
            answer["receipt"]["retired"],
            json!(["pw", "sketch-1700000000000-2"])
        );
        assert_eq!(
            answer["receipt"]["release"],
            json!([
                {"kind": "features_payload", "id": "pw"},
                {"kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l"},
                {"kind": "features_payload", "id": "sketch-1700000000000-2"},
            ])
        );
        assert_eq!(
            answer["registry"]["layers"]
                .as_array()
                .expect("layers")
                .len(),
            1
        );
    }

    #[test]
    fn a_duplicate_id_refuses_the_whole_store_rather_than_dropping_a_layer() {
        let answer = ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1,
                                "records": [json!({"id": "a"}), json!({"id": "a"})]}));
        assert_eq!(answer["refusal"]["code"], DUPLICATE_LAYER);
        assert!(answer.get("registry").is_none(), "nothing is reopened");
    }

    #[test]
    fn a_restored_folder_connection_missing_its_handle_is_dropped_and_reported() {
        let answer = restore(vec![json!({
            "id": "a", "name": "Roads", "color": "#123456", "visible": true, "createdAt": 5,
            "sourceKind": "import", "sourceName": IMPORT_SOURCE_NAME, "sourceLayerName": "Roads",
            "geometryType": "Point", "lineWidth": 2.5, "pointRadius": 5, "fillOpacity": 0.15,
            "schema": [], "featureCount": 0,
            "sourceMetadata": {"kind": "gis_folder", "dsProject": "p1", "folderName": "gis",
                               "filePath": "gis/roads.geojson", "lastRefreshAt": 7},
        })]);
        assert!(answer["registry"]["layers"][0]
            .get("sourceMetadata")
            .is_none());
        assert_eq!(
            answer["receipt"]["repaired"][0]["fields"],
            json!(["sourceMetadata"])
        );
    }

    #[test]
    fn restore_reads_only_objects_and_only_a_bounded_store() {
        assert_eq!(
            ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1, "records": [7]}))
                ["refusal"]["code"],
            MALFORMED_DESCRIPTOR
        );
        let records: Vec<Value> = (0..=MAX_LAYERS)
            .map(|i| json!({"id": i.to_string()}))
            .collect();
        assert_eq!(
            ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1, "records": records}))
                ["refusal"]["code"],
            MALFORMED_DESCRIPTOR
        );
    }

    #[test]
    fn a_replaced_payload_names_the_chunks_it_left_behind() {
        let mut draft = manual_draft();
        draft["featureStorage"] = json!({"kind": "upload_chunks", "workspaceKey": "w1",
                                         "fileName": "big.csv", "parsedLayer": "rows"});
        let layer = admitted(draft);
        let id = layer["id"].as_str().expect("id").to_owned();
        let answer = ask(update(
            store(vec![layer.clone()]),
            &id,
            json!({"kind": "payload", "schema": [{"name": "road", "type": "text"}],
                   "featureCount": 3, "featureStorage": null}),
        ));
        let next = &answer["registry"]["layers"][0];
        assert_eq!(next["featureCount"], 3);
        assert_eq!(next["schema"], json!([{"name": "road", "type": "text"}]));
        assert!(next.get("featureStorage").is_none());
        assert_eq!(
            answer["receipt"],
            json!({"id": id, "applied": "payload", "release": [
                {"kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
                 "parsedLayer": "rows"}]})
        );
        assert_eq!(answer["registry"]["revision"], 8);
        // Re-parking the same chunks releases nothing: the payload is the one just written.
        let same = ask(update(
            store(vec![layer]),
            &id,
            json!({"kind": "payload", "schema": [], "featureCount": 3,
                   "featureStorage": {"kind": "upload_chunks", "workspaceKey": "w1",
                                      "fileName": "big.csv", "parsedLayer": "rows"}}),
        ));
        assert_eq!(same["receipt"]["release"], json!([]));
    }

    #[test]
    fn a_rename_follows_the_source_layer_name_only_for_a_manual_layer() {
        let manual = admitted(manual_draft());
        let id = manual["id"].as_str().expect("id").to_owned();
        let answer = ask(update(
            store(vec![manual]),
            &id,
            json!({"kind": "rename", "name": " New "}),
        ));
        let next = &answer["registry"]["layers"][0];
        assert_eq!(next["name"], "New");
        assert_eq!(next["sourceLayerName"], "New");
        assert_eq!(answer["receipt"]["applied"], "rename");

        let mut draft = manual_draft();
        draft["sourceKind"] = json!("import");
        draft["sourceName"] = json!("roads.zip");
        draft["sourceLayerName"] = json!("roads");
        let imported = admitted(draft);
        let answer = ask(update(
            store(vec![imported]),
            &id,
            json!({"kind": "rename", "name": "Renamed"}),
        ));
        let next = &answer["registry"]["layers"][0];
        assert_eq!(next["name"], "Renamed");
        assert_eq!(
            next["sourceLayerName"], "roads",
            "the name inside the source is unchanged"
        );
        assert_eq!(next["sourceName"], "roads.zip");
        assert_eq!(
            refused(update(
                store(vec![]),
                &id,
                json!({"kind": "rename", "name": " "})
            )),
            UNKNOWN_LAYER
        );
    }

    #[test]
    fn renaming_a_source_renames_every_layer_filed_under_it() {
        let first = admitted(manual_draft());
        let id = first["id"].as_str().expect("id").to_owned();
        let mut second = first.clone();
        second["id"] = json!("sketch-2");
        second["name"] = json!("Other");
        let mut elsewhere = first.clone();
        elsewhere["id"] = json!("sketch-3");
        elsewhere["sourceName"] = json!("Field survey");
        let answer = ask(update(
            store(vec![first, second, elsewhere]),
            &id,
            json!({"kind": "rename_source", "sourceKind": "manual",
                   "sourceName": MANUAL_SOURCE_NAME, "name": "  Drawn today  "}),
        ));
        let layers = &answer["registry"]["layers"];
        assert_eq!(layers[0]["sourceName"], "Drawn today");
        assert_eq!(layers[1]["sourceName"], "Drawn today");
        assert_eq!(
            layers[2]["sourceName"], "Field survey",
            "another source is untouched"
        );
        assert_eq!(answer["receipt"]["applied"], "rename_source");
    }

    #[test]
    fn style_clamps_what_it_is_given_and_a_null_document_clears_it() {
        let layer = admitted(manual_draft());
        let id = layer["id"].as_str().expect("id").to_owned();
        let answer = ask(update(
            store(vec![layer]),
            &id,
            json!({"kind": "style", "color": " #000000 ", "lineWidth": 900,
                   "pointRadius": 0, "fillOpacity": 2, "styleDoc": {"paint": {}}}),
        ));
        let next = &answer["registry"]["layers"][0];
        assert_eq!(next["color"], "#000000");
        assert_eq!(next["lineWidth"], MAX_LINE_WIDTH);
        assert_eq!(next["pointRadius"], MIN_POINT_RADIUS);
        assert_eq!(next["fillOpacity"], 1.0);
        assert_eq!(next["styleDoc"], json!({"paint": {}}));
        let cleared = ask(update(
            store(vec![next.clone()]),
            &id,
            json!({"kind": "style", "styleDoc": null}),
        ));
        assert!(cleared["registry"]["layers"][0].get("styleDoc").is_none());
        let untouched = ask(update(
            store(vec![next.clone()]),
            &id,
            json!({"kind": "style"}),
        ));
        assert_eq!(
            untouched["registry"]["layers"][0]["styleDoc"],
            json!({"paint": {}})
        );
        assert_eq!(
            untouched["changed"], false,
            "a style change that changes nothing is not a revision"
        );
        assert_eq!(untouched["registry"]["revision"], 7);
        assert_eq!(
            refused(update(
                store(vec![next.clone()]),
                &id,
                json!({"kind": "style", "color": "  "})
            )),
            MALFORMED_DESCRIPTOR
        );
        assert_eq!(
            refused(update(
                store(vec![next.clone()]),
                &id,
                json!({"kind": "style", "styleDoc": 4})
            )),
            MALFORMED_DESCRIPTOR
        );
    }

    #[test]
    fn visibility_and_source_metadata_are_persisted_decisions() {
        let layer = admitted(manual_draft());
        let id = layer["id"].as_str().expect("id").to_owned();
        let hidden = ask(update(
            store(vec![layer]),
            &id,
            json!({"kind": "visible", "visible": false}),
        ));
        assert_eq!(hidden["registry"]["layers"][0]["visible"], false);
        assert_eq!(
            hidden["receipt"],
            json!({"id": id, "applied": "visible", "release": []})
        );
        let attached = ask(update(
            store(vec![hidden["registry"]["layers"][0].clone()]),
            &id,
            json!({"kind": "source_metadata", "sourceMetadata": {
                "kind": "gis_folder", "dsProject": "p1", "handleKey": "h1", "folderName": "gis",
                "filePath": "gis/roads.geojson", "lastRefreshAt": 99}}),
        ));
        assert_eq!(
            attached["registry"]["layers"][0]["sourceMetadata"]["dsProject"],
            "p1"
        );
        let detached = ask(update(
            store(vec![attached["registry"]["layers"][0].clone()]),
            &id,
            json!({"kind": "source_metadata", "sourceMetadata": null}),
        ));
        assert!(detached["registry"]["layers"][0]
            .get("sourceMetadata")
            .is_none());
    }

    #[test]
    fn metadata_with_no_refresh_time_and_no_clock_is_not_metadata() {
        let layer = admitted(manual_draft());
        let id = layer["id"].as_str().expect("id").to_owned();
        let metadata = json!({"kind": "gis_folder", "dsProject": "p1", "handleKey": "h1",
                              "folderName": "gis", "filePath": "gis/roads.geojson"});
        let blind = ask(update(
            store(vec![layer.clone()]),
            &id,
            json!({"kind": "source_metadata", "sourceMetadata": metadata.clone()}),
        ));
        assert!(blind["registry"]["layers"][0]
            .get("sourceMetadata")
            .is_none());
        let mut op = update(
            store(vec![layer]),
            &id,
            json!({"kind": "source_metadata", "sourceMetadata": metadata}),
        );
        op["now_ms"] = json!(1_700_000_000_000i64);
        assert_eq!(
            ask(op)["registry"]["layers"][0]["sourceMetadata"]["lastRefreshAt"],
            1_700_000_000_000i64
        );
    }

    #[test]
    fn a_layer_from_a_connected_folder_is_fenced_to_its_own_project() {
        let mut draft = manual_draft();
        draft["sourceMetadata"] = json!({"kind": "gis_folder", "dsProject": "p1", "handleKey": "h1",
                                         "folderName": "gis", "filePath": "gis/roads.geojson",
                                         "lastRefreshAt": 9});
        let layer = admitted(draft);
        let id = layer["id"].as_str().expect("id").to_owned();
        let mut op = update(
            store(vec![layer.clone()]),
            &id,
            json!({"kind": "visible", "visible": false}),
        );
        op["project"] = json!("p2");
        assert_eq!(refused(op.clone()), PROJECT_CONTEXT_CHANGED);
        op["project"] = json!("p1");
        assert_eq!(ask(op)["changed"], true);
        let mut remove = json!({"kind": "remove", "registry": store(vec![layer.clone()]),
                                "scope": browser(), "id": id, "project": "p2"});
        assert_eq!(refused(remove.clone()), PROJECT_CONTEXT_CHANGED);
        remove["project"] = Value::Null;
        assert_eq!(
            ask(remove)["changed"],
            true,
            "an op that names no project is not fenced"
        );
        // An imported file belongs to no project, so nothing fences it.
        let mut draft = manual_draft();
        draft["sourceKind"] = json!("import");
        draft["sourceMetadata"] = json!({"kind": "import_file", "fileName": "roads.geojson",
                                         "lastRefreshAt": 9});
        let imported = admitted(draft);
        let mut op = update(
            store(vec![imported]),
            &id,
            json!({"kind": "visible", "visible": false}),
        );
        op["project"] = json!("p2");
        assert_eq!(ask(op)["changed"], true);
    }

    #[test]
    fn remove_names_the_layers_own_records_and_nothing_else() {
        let mut draft = manual_draft();
        draft["sourceKind"] = json!("import");
        draft["sourceMetadata"] = json!({"kind": "import_file", "fileName": "roads.geojson",
                                         "sourcePath": "/home/me/gis/roads.geojson",
                                         "lastRefreshAt": 9});
        draft["featureStorage"] = json!({"kind": "upload_chunks", "workspaceKey": "w1",
                                         "fileName": "big.csv", "parsedLayer": "rows"});
        let layer = admitted(draft);
        let id = layer["id"].as_str().expect("id").to_owned();
        let mut other = layer.clone();
        other["id"] = json!("sketch-other");
        let answer = ask(
            json!({"kind": "remove", "registry": store(vec![layer, other]),
                                "scope": browser(), "id": id}),
        );
        assert_eq!(
            answer["receipt"],
            json!({"id": id, "removed": true, "release": [
                {"kind": "features_payload", "id": id},
                {"kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
                 "parsedLayer": "rows"}]})
        );
        let release = answer["receipt"]["release"].to_string();
        assert!(
            !release.contains("/home/me"),
            "a source file is never named for deletion"
        );
        assert!(
            !release.contains("sketch-other"),
            "another layer is never named"
        );
        assert_eq!(
            answer["registry"]["layers"]
                .as_array()
                .expect("layers")
                .len(),
            1
        );
        assert_eq!(answer["registry"]["revision"], 8);
    }

    #[test]
    fn an_unknown_layer_and_another_scope_change_nothing() {
        let layer = admitted(manual_draft());
        let id = layer["id"].as_str().expect("id").to_owned();
        let registry = store(vec![layer]);
        assert_eq!(
            refused(update(
                registry.clone(),
                "sketch-nope",
                json!({"kind": "visible", "visible": false})
            )),
            UNKNOWN_LAYER
        );
        assert_eq!(
            refused(
                json!({"kind": "remove", "registry": registry.clone(), "scope": browser(),
                           "id": "sketch-nope"})
            ),
            UNKNOWN_LAYER
        );
        let native = json!({"host": "native", "lane": "run", "uid": "u1"});
        for op in [
            json!({"kind": "list", "registry": registry.clone(), "scope": native}),
            json!({"kind": "update", "registry": registry.clone(), "scope": native, "id": id,
                   "change": {"kind": "visible", "visible": true}}),
            json!({"kind": "remove", "registry": registry, "scope": native, "id": id}),
        ] {
            assert_eq!(refused(op), SCOPE_MISMATCH);
        }
    }

    #[test]
    fn a_registry_that_is_not_a_catalogue_is_refused_before_anything_is_read() {
        let layer = admitted(manual_draft());
        let mut twice = store(vec![layer.clone(), layer.clone()]);
        assert_eq!(
            refused(json!({"kind": "list", "registry": twice.clone(), "scope": browser()})),
            DUPLICATE_LAYER
        );
        twice["layers"][1]["id"] = json!("sketch-2");
        twice["layers"][1]["lineWidth"] = json!(99);
        assert_eq!(
            refused(json!({"kind": "list", "registry": twice, "scope": browser()})),
            MALFORMED_DESCRIPTOR
        );
        let mut retired = store(vec![layer]);
        retired["layers"][0]["sourceKind"] = json!("project_work");
        assert_eq!(
            refused(json!({"kind": "list", "registry": retired, "scope": browser()})),
            UNSUPPORTED_SOURCE_KIND
        );
    }

    #[test]
    fn a_malformed_request_is_the_hosts_mistake_not_a_refusal() {
        assert!(evaluate(b"{").is_err());
        assert!(evaluate(
            &serde_json::to_vec(
                &json!({"schema": "ds.other/v1", "op": {"kind": "list", "registry": empty(),
                    "scope": browser()}})
            )
            .expect("encodes")
        )
        .is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
        assert!(rejected(json!({"kind": "sideways"})).contains("invalid local layer request"));
        let mut op = json!({"kind": "list", "registry": empty(), "scope": browser()});
        op["extra"] = json!(true);
        assert!(rejected(op).contains("unknown field"));
        let mut op = register(empty(), manual_draft());
        op["now_ms"] = json!(0);
        assert!(rejected(op).contains("clock"));
        assert!(rejected(json!({"kind": "list", "registry": empty(),
                                "scope": {"host": "browser", "lane": "run"}}))
        .contains("browser scope"));
        assert!(rejected(
            json!({"kind": "list", "registry": empty(), "scope": {"host": "native"}})
        )
        .contains("native scope"));
    }

    #[test]
    fn a_native_store_is_the_same_catalogue_under_another_scope() {
        let scope = json!({"host": "native", "lane": "run", "uid": "u1"});
        let answer = ask(json!({"kind": "restore", "scope": scope, "records": [],
                                "now_ms": 1_700_000_000_000i64}));
        assert_eq!(
            answer["registry"],
            json!({"schema": 1, "revision": 0, "scope": scope,
                                              "layers": []})
        );
        let mut op = register(answer["registry"].clone(), manual_draft());
        op["scope"] = scope;
        let answer = ask(op);
        assert_eq!(
            answer["registry"]["layers"][0]["id"],
            "sketch-1700000000000-4"
        );
    }
}
