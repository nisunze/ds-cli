//! Design transformer rooms held on this device: each transformer's complete
//! layers with their save state, and the light header a Save, the status merge
//! and digest reconciliation read without touching geometry. One Kernel
//! schema, run by the same rusqlite code in Core's protected Store (Desktop and
//! headless) and in the browser storage worker on OPFS.
//!
//! A dirty room is the person's unsaved work, and no write discards it
//! implicitly:
//! * an acquisition (a download) commits only over the exact room revision it
//!   captured, never while a save is pending, and consumes the Latest
//!   invalidations it observed in the same transaction;
//! * a saved analysis attaches only to the unchanged clean room it names;
//! * marking a room clean requires the room still to carry the pending save
//!   being acknowledged. Every write clears that marker, so an edit made while
//!   a save was in flight stays dirty; its base version and digests advance;
//! * only an explicit delete (Discard) removes a room.
//!
//! Rows are fenced by the bound identity's owner and the project; a caller
//! names the project and transformers, never an owner. The browser's
//! IndexedDB rooms move once per identity, in batches, and never replace a
//! held dirty room.
use crate::durable_store::{
    held_receipt, import_once, sha256, Identity, ImportReceipt, Input, Outcome, Principal,
};
use crate::import_batches::{self, Batch};
use crate::report_export::jcs;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "design_rooms";
pub const INVALID: &str = "design_room_invalid";
pub const EXISTS: &str = "design_room_exists";
pub const SUPERSEDED: &str = "design_room_superseded";
pub const NOT_CLEAN: &str = "design_room_not_clean";
/// The MV design of a project is the one room with this name.
pub const MV_ROOM: &str = "mv_data";
pub const MAX_ROOM_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_ROOMS: usize = 64;
pub const MAX_NAMES: usize = 500;
pub const MAX_LIST: usize = 16;
pub const MAX_LAYER_FEATURES: usize = 50_000;
pub const MAX_IMPORT_ENTRIES: usize = 4096;
/// The legacy IndexedDB sources, read once per identity.
pub const ROOMS_SOURCE: &str = "design_transformers";
pub const INVALIDATIONS_SOURCE: &str = "transformer_latest_invalidation";
const INVALIDATION_PREFIX: &str = "transformer-latest-invalidation:";
/// The only legacy room layout IndexedDB still served.
const LEGACY_CACHE_VERSION: u64 = 3;
const MAX_TEXT: usize = 512;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS design_rooms(
 scope TEXT NOT NULL, transformer TEXT NOT NULL,
 source TEXT NOT NULL CHECK(source IN ('lv','mv')),
 updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=0),
 header TEXT NOT NULL, voltage_drop TEXT, voltage_drop_json TEXT, layers TEXT NOT NULL,
 PRIMARY KEY(scope,transformer));
CREATE TABLE IF NOT EXISTS design_room_invalidations(
 scope TEXT NOT NULL, transformer TEXT NOT NULL, change_id TEXT NOT NULL,
 sequence INTEGER NOT NULL, receipt TEXT NOT NULL,
 PRIMARY KEY(scope,transformer,change_id));";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Lv,
    Mv,
}
impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Lv => "lv",
            Source::Mv => "mv",
        }
    }
    /// The MV design lives in its one room; every other name is an LV room.
    fn of(transformer: &str) -> Source {
        if transformer == MV_ROOM {
            Source::Mv
        } else {
            Source::Lv
        }
    }
}

/// How a write treats the room it replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteKind {
    /// A local edit: an omitted flag keeps the held room's, else dirty and unsaved.
    Edited,
    /// A replacement: an omitted flag keeps the held room's, else clean and unsaved.
    Replace,
    /// A new local room, dirty and unsaved; refused when the project holds the name.
    Create,
}

/// One complete room to write. Flags left out follow the write.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomWrite {
    pub transformer: String,
    pub layers: Map<String, Value>,
    #[serde(default)]
    pub server_known: Option<bool>,
    #[serde(default)]
    pub local_dirty: Option<bool>,
    #[serde(default)]
    pub server_version: Option<u64>,
    /// Left out keeps the held room's; `null` clears it.
    #[serde(default, deserialize_with = "present")]
    pub process_metadata: Option<Option<Map<String, Value>>>,
    /// The analysis paired with exactly these layers; a write without one
    /// clears the held pair.
    #[serde(default)]
    pub voltage_drop: Option<Map<String, Value>>,
    #[serde(default)]
    pub voltage_drop_metadata: Option<Map<String, Value>>,
    #[serde(default)]
    pub locally_computed: Option<bool>,
}

/// One header change, applied to a held room only.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderPatch {
    pub transformer: String,
    #[serde(default)]
    pub local_dirty: Option<bool>,
    #[serde(default)]
    pub server_known: Option<bool>,
    #[serde(default)]
    pub server_version: Option<u64>,
    #[serde(default)]
    pub server_content_digest: Option<String>,
    /// Left out keeps it; `null` clears the in-flight save marker.
    #[serde(default, deserialize_with = "present")]
    pub pending_operation_id: Option<Option<String>>,
    #[serde(default)]
    pub voltage_drop_metadata: Option<Map<String, Value>>,
}

/// A verified Latest promotion: the room's saved source moved to this version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invalidation {
    pub working_source_version: u64,
    pub working_content_digest: String,
    pub change_id: String,
}

/// One legacy IndexedDB room with its header, exactly as the browser held them.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRoom {
    pub room: Value,
    #[serde(default)]
    pub header: Option<Value>,
}

/// One legacy `persist:` entry, exactly as the browser held it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyEntry {
    pub legacy_key: String,
    pub value: Value,
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Write {
        project: String,
        kind: WriteKind,
        rooms: Vec<RoomWrite>,
        now: u64,
    },
    /// One downloaded saved room. It commits only over the room revision the
    /// download captured (`null`: none was held), never while a save is
    /// pending, and consumes the Latest invalidations the download observed.
    Acquire {
        project: String,
        room: RoomWrite,
        expected_revision: Option<u64>,
        #[serde(default)]
        consume: Vec<String>,
        now: u64,
    },
    /// One complete room and its pending Latest invalidations.
    Get {
        project: String,
        transformer: String,
    },
    /// Complete rooms, newest first, at most [`MAX_LIST`].
    List {
        project: String,
        #[serde(default)]
        source: Option<Source>,
        limit: usize,
    },
    /// Every room's header, newest first; never its geometry.
    Headers {
        project: String,
        #[serde(default)]
        source: Option<Source>,
        /// Only these rooms, when named.
        #[serde(default)]
        transformers: Option<Vec<String>>,
    },
    Patch {
        project: String,
        patches: Vec<HeaderPatch>,
        now: u64,
    },
    /// Attach the exact saved analysis to the unchanged clean room it names.
    AttachSavedAnalysis {
        project: String,
        transformer: String,
        expected_server_version: u64,
        expected_updated_at: u64,
        document: Map<String, Value>,
        metadata: Map<String, Value>,
        raw_json: String,
    },
    Delete {
        project: String,
        transformers: Vec<String>,
    },
    /// The features of one design layer across the project's rooms.
    LayerFeatures {
        project: String,
        layer: String,
        limit: usize,
    },
    Invalidate {
        project: String,
        transformer: String,
        receipt: Invalidation,
    },
    /// How many rooms the project holds on this device.
    Count { project: String },
    /// Whether this identity's one-time import still has to run, and the
    /// retired rows the host reads, read-only.
    Legacy,
    /// One batch of retired rooms with their headers.
    ImportRooms {
        account_email: String,
        rooms: Vec<LegacyRoom>,
        now: u64,
    },
    /// Record the room import's receipt once every batch has arrived.
    ImportFinish { now: u64 },
    /// The retired Latest invalidations, in one call.
    ImportInvalidations { entries: Vec<LegacyEntry>, now: u64 },
}

/// What a header records about one room: everything but its geometry and
/// analysis. Stored as JSON and answered with ds-web's header names.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Header {
    transformer_name: String,
    source: Source,
    /// Advances whenever the room itself changes; an acquisition fences on it.
    revision: u64,
    local_dirty: bool,
    server_known: bool,
    #[serde(rename = "base_server_version")]
    server_version: Option<u64>,
    local_content_digest: Option<String>,
    server_content_digest: Option<String>,
    feature_count: u64,
    estimated_payload_bytes: u64,
    /// Some layer carries a feature.
    has_layers: bool,
    layer_feature_counts: Map<String, Value>,
    process_metadata: Option<Map<String, Value>>,
    voltage_drop_metadata: Option<Map<String, Value>>,
    locally_computed: bool,
    pending_operation_id: Option<String>,
    /// When the header last changed.
    updated_at: u64,
    /// When the room's layers were last written.
    room_updated_at: u64,
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::<T>::deserialize(d)?))
}
fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    invalid(&error.to_string())
}
fn to_text(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| invalid(&e.to_string()))
}
fn from_text<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| invalid(&format!("a stored room is unreadable: {e}")))
}
fn exact(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bounded(value: u64, what: &str) -> Result<u64, String> {
    if value > MAX_SAFE {
        return Err(invalid(&format!("{what} exceeds its bound")));
    }
    Ok(value)
}
fn answer(key: &str, value: Value) -> Map<String, Value> {
    Map::from_iter([(key.to_string(), value)])
}
fn receipt_value(receipt: Option<ImportReceipt>) -> Result<Value, String> {
    serde_json::to_value(receipt).map_err(|e| e.to_string())
}

/// The row fence: the bound identity's owner and the exact project.
fn fence(owner: &str, project: &str) -> Result<String, String> {
    if !crate::execution_context::valid_project(project) {
        return Err(invalid("the project is not an exact project id"));
    }
    serde_json::to_string(&(owner, project)).map_err(|e| e.to_string())
}

/// The room key: ds-brain's canonical transformer reference. An aggregate or
/// collisions name never holds a room; `mv_data` is the project's MV room.
pub fn room_name(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() || raw.len() > MAX_TEXT {
        return Err(invalid("a transformer name is required"));
    }
    let name = crate::assets::tree::canonical_transformer_reference_name(raw);
    if name != MV_ROOM && crate::assets::tree::is_reserved_transformer_name(&name) {
        return Err(invalid(&format!("{name} is not a transformer room")));
    }
    Ok(name)
}

/// ds-system's `clean_name` as ds-web spelled it (`dsSystemCleanName`): ASCII
/// only, punctuation runs folded, lower case, a leading digit prefixed `col_`.
fn ds_system_clean_name(value: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<[Regex; 5]> = OnceLock::new();
    let [non_ascii, underscore_run, caret_run, non_word, repeated] = PATTERNS.get_or_init(|| {
        [
            r"[^\x00-\x7F]+",
            r"([A-Za-z0-9_])_[^A-Za-z0-9_\t\n\x0B\x0C\r ]+",
            r"([A-Za-z0-9_])\^[^A-Za-z0-9_\t\n\x0B\x0C\r ]+",
            r"[^A-Za-z0-9_]+",
            r"_{2,}",
        ]
        .map(|pattern| Regex::new(pattern).expect("a fixed pattern"))
    });
    let name = non_ascii.replace_all(value, "_");
    let name = underscore_run.replace_all(&name, "$1");
    let name = caret_run.replace_all(&name, "$1");
    let name = non_word.replace_all(&name, "_").to_ascii_lowercase();
    let name = repeated.replace_all(&name, "_");
    let name = if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("col_{name}")
    } else {
        name.into_owned()
    };
    name.trim_matches('_').to_string()
}

/// The layers exactly as a save transmits them: each LV feature names its
/// transformer document (`transfo`, and `name` on the `tr` layer). An analysed
/// room and the MV room travel unchanged. `None` when a feature is not an
/// object the transmission could stamp.
fn transmitted(
    layers: &Map<String, Value>,
    transformer: &str,
    analysed: bool,
) -> Option<Map<String, Value>> {
    let document = ds_system_clean_name(transformer)
        .to_uppercase()
        .replace("EX", "FILL_IN");
    if analysed || transformer == MV_ROOM || document.is_empty() {
        return Some(layers.clone());
    }
    let stamp = |feature: Value, tr: bool| -> Option<Value> {
        let Value::Object(mut feature) = feature else {
            return None;
        };
        let mut properties = match feature.remove("properties") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(properties)) => properties,
            Some(_) => return None,
        };
        properties.insert("transfo".into(), json!(document));
        if tr {
            properties.insert("name".into(), json!(document));
        }
        feature.insert("properties".into(), Value::Object(properties));
        Some(Value::Object(feature))
    };
    layers
        .iter()
        .map(|(name, collection)| {
            let mut collection = collection.as_object()?.clone();
            let features = match collection.remove("features") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(features)) => features,
                Some(_) => return None,
            };
            // ds-web `canonicalName(layer) === 'tr'`.
            let tr = name.trim().eq_ignore_ascii_case("tr");
            let stamped = features
                .into_iter()
                .map(|feature| stamp(feature, tr))
                .collect::<Option<Vec<_>>>()?;
            collection.insert("features".into(), Value::Array(stamped));
            Some((name.clone(), Value::Object(collection)))
        })
        .collect()
}

fn features(collection: &Value) -> usize {
    collection
        .get("features")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

/// A new header for these layers: their transmitted digest, size and counts.
fn summarize(
    name: &str,
    layers: &Map<String, Value>,
    analysis: Option<&Map<String, Value>>,
) -> Header {
    let canonical = transmitted(layers, name, analysis.is_some())
        .and_then(|payload| jcs::canonicalize(&Value::Object(payload)).ok());
    let analysis_bytes = analysis
        .and_then(|analysis| jcs::canonicalize(&Value::Object(analysis.clone())).ok())
        .map_or(0, |bytes| bytes.len());
    Header {
        transformer_name: name.to_string(),
        source: Source::of(name),
        revision: 1,
        local_dirty: false,
        server_known: false,
        server_version: None,
        local_content_digest: canonical.as_deref().map(sha256),
        server_content_digest: None,
        feature_count: layers.values().map(features).sum::<usize>() as u64,
        estimated_payload_bytes: (canonical.as_ref().map_or(0, Vec::len) + analysis_bytes) as u64,
        has_layers: layers.values().any(|layer| features(layer) > 0),
        layer_feature_counts: layers
            .iter()
            .map(|(name, layer)| (name.clone(), json!(features(layer))))
            .collect(),
        process_metadata: None,
        voltage_drop_metadata: None,
        locally_computed: analysis.is_some(),
        pending_operation_id: None,
        updated_at: 0,
        room_updated_at: 0,
    }
}

fn held(connection: &Connection, scope: &str, transformer: &str) -> Result<Option<Header>, String> {
    connection
        .query_row(
            "SELECT header FROM design_rooms WHERE scope=?1 AND transformer=?2",
            params![scope, transformer],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql)?
        .map(|text| from_text(&text))
        .transpose()
}

fn save_header(connection: &Connection, scope: &str, header: &Header) -> Result<(), String> {
    connection
        .execute(
            "UPDATE design_rooms SET header=?3 WHERE scope=?1 AND transformer=?2",
            params![scope, header.transformer_name, to_text(header)?],
        )
        .map_err(sql)?;
    Ok(())
}

/// Store one complete room: its header, layers and analysis pair.
fn store(
    connection: &Connection,
    scope: &str,
    header: &Header,
    layers: &Map<String, Value>,
    analysis: (Option<&Map<String, Value>>, Option<&str>),
) -> Result<(), String> {
    if layers.iter().any(|(layer, collection)| {
        layer.is_empty() || layer.len() > MAX_TEXT || !collection.is_object()
    }) {
        return Err(invalid(
            "each room layer is a named feature collection object",
        ));
    }
    let layers = to_text(layers)?;
    if layers.len() > MAX_ROOM_BYTES {
        return Err(invalid("a room exceeds 48 MiB"));
    }
    connection
        .execute(
            "INSERT OR REPLACE INTO design_rooms(scope,transformer,source,updated_at_ms,header,voltage_drop,voltage_drop_json,layers) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                scope,
                header.transformer_name,
                header.source.name(),
                header.room_updated_at as i64,
                to_text(header)?,
                analysis.0.map(to_text).transpose()?,
                analysis.1,
                layers
            ],
        )
        .map_err(sql)?;
    Ok(())
}

/// Admit one written room against what this device holds and store it.
/// `kind` is `None` for an acquisition, whose caller fenced the revision.
fn write_room(
    connection: &Connection,
    scope: &str,
    kind: Option<WriteKind>,
    room: RoomWrite,
    now: u64,
) -> Result<Header, String> {
    let name = room_name(&room.transformer)?;
    room.server_version
        .map(|version| bounded(version, "a server version"))
        .transpose()?;
    let existing = held(connection, scope, &name)?;
    if kind == Some(WriteKind::Create) && existing.is_some() {
        return Err(format!(
            "{EXISTS}: {name} is already held locally; open it for editing"
        ));
    }
    let acquired = kind.is_none();
    let mut header = summarize(&name, &room.layers, room.voltage_drop.as_ref());
    header.revision = existing.as_ref().map_or(1, |held| held.revision + 1);
    header.server_known = acquired
        || room
            .server_known
            .or(existing.as_ref().map(|held| held.server_known))
            .unwrap_or(false);
    header.local_dirty = !acquired
        && room
            .local_dirty
            .or(existing.as_ref().map(|held| held.local_dirty))
            .unwrap_or(matches!(kind, Some(WriteKind::Edited | WriteKind::Create)));
    header.server_version = match (acquired, &existing) {
        (false, Some(held)) => room.server_version.or(held.server_version),
        _ => room.server_version,
    };
    header.process_metadata = match (room.process_metadata, &existing) {
        (Some(metadata), _) => metadata,
        (None, Some(held)) if !acquired => held.process_metadata.clone(),
        (None, _) => None,
    }
    .filter(|metadata| !metadata.is_empty());
    if header.server_known && !header.local_dirty {
        header.server_content_digest = header.local_content_digest.clone();
    }
    header.voltage_drop_metadata = room.voltage_drop_metadata;
    header.locally_computed |= room.locally_computed == Some(true);
    (header.updated_at, header.room_updated_at) = (now, now);
    store(
        connection,
        scope,
        &header,
        &room.layers,
        (room.voltage_drop.as_ref(), None),
    )?;
    Ok(header)
}

/// One complete room as ds-web reads it.
fn room_answer(
    header: Header,
    layers: &str,
    analysis: Option<String>,
    raw: Option<String>,
) -> Result<Value, String> {
    Ok(json!({
        "transformer_name": header.transformer_name,
        "source": header.source,
        "revision": header.revision,
        "updated_at": header.room_updated_at,
        "server_known": header.server_known,
        "local_dirty": header.local_dirty,
        "server_version": header.server_version,
        "process_metadata": header.process_metadata,
        "voltage_drop_metadata": header.voltage_drop_metadata,
        "locally_computed": header.locally_computed,
        "voltage_drop": analysis.as_deref().map(from_text::<Value>).transpose()?,
        "voltage_drop_json": raw,
        "layers": from_text::<Value>(layers)?,
    }))
}

/// One room's header as ds-web reads it: never its geometry.
fn header_answer(header: Header) -> Result<Value, String> {
    let lv = header.source == Source::Lv;
    let mut value = serde_json::to_value(header).map_err(|e| e.to_string())?;
    value["requires_analysis_receipt"] = json!(lv);
    Ok(value)
}

type RoomRow = (String, String, Option<String>, Option<String>);
const ROOM: &str = "SELECT header,layers,voltage_drop,voltage_drop_json FROM design_rooms";

fn room_row(row: &rusqlite::Row) -> rusqlite::Result<RoomRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn names(raw: &[String]) -> Result<Vec<String>, String> {
    if raw.len() > MAX_NAMES {
        return Err(invalid("name at most 500 rooms at once"));
    }
    let mut names = raw
        .iter()
        .map(|name| room_name(name))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    names.dedup();
    Ok(names)
}

fn patch(
    connection: &Connection,
    scope: &str,
    patches: Vec<HeaderPatch>,
    now: u64,
) -> Result<Map<String, Value>, String> {
    if patches.is_empty() || patches.len() > MAX_NAMES {
        return Err(invalid("patch between 1 and 500 rooms at once"));
    }
    let now = bounded(now, "the time")?;
    let (mut patched, mut kept_dirty) = (Vec::new(), Vec::new());
    for change in patches {
        let name = room_name(&change.transformer)?;
        change
            .server_version
            .map(|version| bounded(version, "a server version"))
            .transpose()?;
        if change
            .server_content_digest
            .as_deref()
            .is_some_and(|digest| !sha256_hex(digest))
            || change
                .pending_operation_id
                .clone()
                .flatten()
                .is_some_and(|operation| !exact(&operation))
        {
            return Err(invalid(
                "a digest is lowercase hex SHA-256 and an operation id is exact",
            ));
        }
        let Some(mut header) = held(connection, scope, &name)? else {
            continue;
        };
        let before = (
            header.local_dirty,
            header.server_known,
            header.server_version,
        );
        // A save acknowledges the content it sent; a room rewritten since it
        // was marked pending holds newer work and stays dirty.
        let mut local_dirty = change.local_dirty;
        if local_dirty == Some(false) && header.local_dirty && header.pending_operation_id.is_none()
        {
            local_dirty = None;
            kept_dirty.push(name.clone());
        }
        header.local_dirty = local_dirty.unwrap_or(header.local_dirty);
        header.server_known = change.server_known.unwrap_or(header.server_known);
        header.server_version = change.server_version.or(header.server_version);
        if let Some(digest) = change.server_content_digest {
            header.server_content_digest = Some(digest);
        } else if local_dirty == Some(false) && change.server_known == Some(true) {
            header.server_content_digest = header
                .local_content_digest
                .clone()
                .or(header.server_content_digest);
        }
        if let Some(operation) = change.pending_operation_id {
            header.pending_operation_id = operation;
        }
        let analysed = change.voltage_drop_metadata.is_some();
        header.voltage_drop_metadata = change
            .voltage_drop_metadata
            .or(header.voltage_drop_metadata);
        if analysed
            || before
                != (
                    header.local_dirty,
                    header.server_known,
                    header.server_version,
                )
        {
            header.revision += 1;
        }
        header.updated_at = now;
        save_header(connection, scope, &header)?;
        patched.push(name);
    }
    Ok(Map::from_iter([
        ("patched".into(), json!(patched)),
        ("kept_dirty".into(), json!(kept_dirty)),
    ]))
}

/// The Style Center's design layer identity: the last path segment, without
/// its `_vt` tile suffix, lower case.
fn style_layer(name: &str) -> String {
    let last = name.trim().rsplit('/').next().unwrap_or_default();
    last.strip_suffix("_vt").unwrap_or(last).to_lowercase()
}

fn layer_features(
    connection: &Connection,
    scope: &str,
    layer: &str,
    limit: usize,
) -> Result<Map<String, Value>, String> {
    if layer.trim().is_empty() || layer.len() > MAX_TEXT || limit == 0 || limit > MAX_LAYER_FEATURES
    {
        return Err(invalid("name one layer and at most 50000 features"));
    }
    let wanted = style_layer(layer);
    let mut statement = connection
        .prepare("SELECT layers FROM design_rooms WHERE scope=?1 ORDER BY updated_at_ms DESC, transformer")
        .map_err(sql)?;
    let mut rows = statement.query(params![scope]).map_err(sql)?;
    let (mut found, mut truncated) = (Vec::new(), false);
    while let Some(row) = rows.next().map_err(sql)? {
        let layers: Map<String, Value> = from_text(&row.get::<_, String>(0).map_err(sql)?)?;
        for (name, mut collection) in layers {
            if style_layer(&name) != wanted {
                continue;
            }
            if let Value::Array(items) = collection["features"].take() {
                truncated |= found.len() + items.len() > limit;
                let room = limit - found.len();
                found.extend(items.into_iter().take(room));
            }
        }
    }
    Ok(Map::from_iter([
        ("features".into(), Value::Array(found)),
        ("truncated".into(), json!(truncated)),
    ]))
}

fn invalidate(
    connection: &Connection,
    scope: &str,
    name: &str,
    receipt: &Invalidation,
) -> Result<bool, String> {
    if receipt.working_source_version == 0
        || receipt.working_source_version > MAX_SAFE
        || !exact(&receipt.change_id)
        || !exact(&receipt.working_content_digest)
    {
        return Err(invalid(
            "a Latest invalidation names its change, source version and digest",
        ));
    }
    let added = connection
        .execute(
            "INSERT INTO design_room_invalidations(scope,transformer,change_id,sequence,receipt)
             SELECT ?1,?2,?3,coalesce(max(sequence),0)+1,?4 FROM design_room_invalidations WHERE scope=?1 AND transformer=?2
             ON CONFLICT DO NOTHING",
            params![scope, name, receipt.change_id, to_text(receipt)?],
        )
        .map_err(sql)?;
    Ok(added == 1)
}

fn invalidations(
    connection: &Connection,
    scope: &str,
    transformer: &str,
) -> Result<Vec<Value>, String> {
    let mut statement = connection
        .prepare("SELECT receipt FROM design_room_invalidations WHERE scope=?1 AND transformer=?2 ORDER BY sequence")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![scope, transformer], |row| row.get::<_, String>(0))
        .map_err(sql)?;
    rows.map(|text| from_text(&text.map_err(sql)?)).collect()
}

/// A legacy field, read leniently: the browser kept whatever it wrote.
fn legacy_flag(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}
fn legacy_count(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= MAX_SAFE as f64)
        .map(|n| n as u64)
}
fn legacy_object(value: &Value, key: &str) -> Option<Map<String, Value>> {
    value
        .get(key)
        .and_then(Value::as_object)
        .filter(|fields| !fields.is_empty())
        .cloned()
}
fn legacy_digest(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|digest| sha256_hex(digest))
        .map(str::to_string)
}

/// Move one legacy room for this account: `true` when it now holds the room.
/// A held dirty room is the newer work and stays (so a batch repeated after
/// an interruption writes nothing new); unsaved legacy work replaces a clean
/// held copy, which the server can always serve again.
fn import_room(
    connection: &Connection,
    owner: &str,
    account: &str,
    legacy: &LegacyRoom,
) -> Result<bool, String> {
    let room = &legacy.room;
    let header = legacy.header.clone().unwrap_or(Value::Null);
    let (Some(project), Some(raw_name), Some(layers)) = (
        room["ds_project"].as_str(),
        room["transformer_name"].as_str(),
        room["layers"].as_object(),
    ) else {
        return Ok(false);
    };
    let owner_email = room["owner_email"]
        .as_str()
        .map(|email| email.trim().to_lowercase());
    if legacy_count(room, "cache_version") != Some(LEGACY_CACHE_VERSION)
        || owner_email.is_some_and(|email| !email.is_empty() && email != account)
    {
        return Ok(false);
    }
    let (Ok(scope), Ok(name)) = (fence(owner, project), room_name(raw_name)) else {
        return Ok(false);
    };
    if room["source"].as_str() != Some(Source::of(&name).name()) {
        return Ok(false);
    }
    let server_known = legacy_flag(room, "server_known").unwrap_or(false);
    // ds-web read an unset flag as "dirty unless the server knows it"; a
    // header that says dirty keeps the room dirty.
    let local_dirty = legacy_flag(room, "local_dirty").unwrap_or(!server_known)
        || legacy_flag(&header, "local_dirty") == Some(true);
    let existing = held(connection, &scope, &name)?;
    if existing
        .as_ref()
        .is_some_and(|held| held.local_dirty || !local_dirty)
    {
        return Ok(false);
    }
    let analysis = legacy_object(room, "voltage_drop");
    let mut moved = summarize(&name, layers, analysis.as_ref());
    moved.revision = existing.as_ref().map_or(1, |held| held.revision + 1);
    moved.local_dirty = local_dirty;
    moved.server_known = server_known;
    moved.server_version =
        legacy_count(room, "server_version").or(legacy_count(&header, "base_server_version"));
    // The digest the server may already hold for an interrupted save.
    moved.local_content_digest =
        legacy_digest(&header, "local_content_digest").or(moved.local_content_digest);
    moved.server_content_digest = legacy_digest(&header, "server_content_digest");
    moved.process_metadata = legacy_object(room, "local_process_metadata");
    moved.voltage_drop_metadata = legacy_object(room, "voltage_drop_metadata")
        .or(legacy_object(&header, "voltage_drop_metadata"));
    moved.locally_computed = analysis.is_some()
        || legacy_flag(room, "locally_computed").unwrap_or(moved.process_metadata.is_some());
    moved.pending_operation_id = header["pending_operation_id"]
        .as_str()
        .filter(|id| exact(id))
        .map(str::to_string);
    moved.room_updated_at = legacy_count(room, "updated_at").unwrap_or(0);
    moved.updated_at = legacy_count(&header, "updated_at").unwrap_or(moved.room_updated_at);
    let raw = analysis.as_ref().and(room["voltage_drop_json"].as_str());
    Ok(store(connection, &scope, &moved, layers, (analysis.as_ref(), raw)).is_ok())
}

/// The invalidations one `transformer-latest-invalidation:` entry yields for
/// this account.
fn import_invalidation(
    connection: &Connection,
    identity: &Identity,
    owner: &str,
    entry: &LegacyEntry,
) -> u64 {
    let Some(scope) = entry
        .legacy_key
        .strip_prefix(INVALIDATION_PREFIX)
        .and_then(|scope| serde_json::from_str::<Value>(scope).ok())
    else {
        return 0;
    };
    let Principal::Account(uid) = &identity.principal else {
        return 0;
    };
    let (Some(project), Some(raw_name)) =
        (scope["project"].as_str(), scope["transformer"].as_str())
    else {
        return 0;
    };
    let (Ok(fenced), Ok(name)) = (fence(owner, project), room_name(raw_name)) else {
        return 0;
    };
    if scope["owner"].as_str() != Some(uid.as_str()) {
        return 0;
    }
    let receipts = entry.value.as_array().into_iter().flatten();
    receipts
        .filter_map(|receipt| serde_json::from_value::<Invalidation>(receipt.clone()).ok())
        .filter(|receipt| invalidate(connection, &fenced, &name, receipt).unwrap_or(false))
        .count() as u64
}

/// Run one operation inside the caller's transaction.
pub fn execute(
    connection: &Connection,
    input: Input,
    operation: Operation,
) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("design rooms carry no payload bytes"));
    }
    connection.execute_batch(SCHEMA).map_err(sql)?;
    let owner = input.identity.owner();
    let identity = input.identity;
    Ok(match operation {
        Operation::Write {
            project,
            kind,
            rooms,
            now,
        } => {
            let scope = fence(&owner, &project)?;
            if rooms.is_empty() || rooms.len() > MAX_ROOMS {
                return Err(invalid("write between 1 and 64 rooms at once"));
            }
            let now = bounded(now, "the time")?;
            let mut written: Vec<Value> = Vec::new();
            for room in rooms {
                let header = write_room(connection, &scope, Some(kind), room, now)?;
                if written
                    .iter()
                    .any(|row| row["transformer_name"] == header.transformer_name.as_str())
                {
                    return Err(invalid("a write names each room once"));
                }
                written.push(json!({"transformer_name": header.transformer_name, "revision": header.revision}));
            }
            Outcome::written(answer("written", Value::Array(written)))
        }
        Operation::Acquire {
            project,
            room,
            expected_revision,
            consume,
            now,
        } => {
            let scope = fence(&owner, &project)?;
            let name = room_name(&room.transformer)?;
            let current = held(connection, &scope, &name)?;
            if current.as_ref().map(|held| held.revision) != expected_revision {
                return Err(format!("{SUPERSEDED}: transformer_acquisition_superseded: the local room of {name} changed during the download"));
            }
            if current.is_some_and(|held| held.pending_operation_id.is_some()) {
                return Err(format!("{SUPERSEDED}: transformer_acquisition_superseded: a save of {name} overtook this download"));
            }
            let header = write_room(connection, &scope, None, room, bounded(now, "the time")?)?;
            for change in &consume {
                connection
                    .execute(
                        "DELETE FROM design_room_invalidations WHERE scope=?1 AND transformer=?2 AND change_id=?3",
                        params![scope, name, change],
                    )
                    .map_err(sql)?;
            }
            Outcome::written(answer(
                "written",
                json!([{"transformer_name": name, "revision": header.revision}]),
            ))
        }
        Operation::Get {
            project,
            transformer,
        } => {
            let (scope, name) = (fence(&owner, &project)?, room_name(&transformer)?);
            let row = connection
                .query_row(
                    &format!("{ROOM} WHERE scope=?1 AND transformer=?2"),
                    params![scope, name],
                    room_row,
                )
                .optional()
                .map_err(sql)?;
            let room = match row {
                Some((header, layers, analysis, raw)) => {
                    room_answer(from_text(&header)?, &layers, analysis, raw)?
                }
                None => Value::Null,
            };
            let mut value = answer("room", room);
            value.insert(
                "invalidations".into(),
                Value::Array(invalidations(connection, &scope, &name)?),
            );
            Outcome::read(value)
        }
        Operation::List {
            project,
            source,
            limit,
        } => {
            if limit == 0 || limit > MAX_LIST {
                return Err(invalid("list between 1 and 16 complete rooms"));
            }
            let scope = fence(&owner, &project)?;
            let mut statement = connection
                .prepare(&format!("{ROOM} WHERE scope=?1 AND (?2 IS NULL OR source=?2) ORDER BY updated_at_ms DESC, transformer LIMIT ?3"))
                .map_err(sql)?;
            let rows = statement
                .query_map(
                    params![scope, source.map(Source::name), limit as i64],
                    room_row,
                )
                .map_err(sql)?;
            let mut rooms = Vec::new();
            for row in rows {
                let (header, layers, analysis, raw) = row.map_err(sql)?;
                rooms.push(room_answer(from_text(&header)?, &layers, analysis, raw)?);
            }
            Outcome::read(answer("rooms", Value::Array(rooms)))
        }
        Operation::Headers {
            project,
            source,
            transformers,
        } => {
            let scope = fence(&owner, &project)?;
            let wanted = transformers.as_deref().map(names).transpose()?;
            let mut statement = connection
                .prepare("SELECT transformer,header FROM design_rooms WHERE scope=?1 AND (?2 IS NULL OR source=?2) ORDER BY updated_at_ms DESC, transformer")
                .map_err(sql)?;
            let rows = statement
                .query_map(params![scope, source.map(Source::name)], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(sql)?;
            let mut headers = Vec::new();
            for row in rows {
                let (name, header) = row.map_err(sql)?;
                if wanted
                    .as_ref()
                    .is_none_or(|wanted| wanted.binary_search(&name).is_ok())
                {
                    headers.push(header_answer(from_text(&header)?)?);
                }
            }
            Outcome::read(answer("headers", Value::Array(headers)))
        }
        Operation::Patch {
            project,
            patches,
            now,
        } => Outcome::written(patch(connection, &fence(&owner, &project)?, patches, now)?),
        Operation::AttachSavedAnalysis {
            project,
            transformer,
            expected_server_version,
            expected_updated_at,
            document,
            metadata,
            raw_json,
        } => {
            let (scope, name) = (fence(&owner, &project)?, room_name(&transformer)?);
            let raw = serde_json::from_str::<Value>(&raw_json)
                .map_err(|_| invalid("the raw analysis is not JSON"))?;
            if jcs::canonicalize(&raw).ok()
                != jcs::canonicalize(&Value::Object(document.clone())).ok()
            {
                return Err(invalid(
                    "the raw analysis JSON must be the exact analysis document",
                ));
            }
            let Some(mut header) = held(connection, &scope, &name)?.filter(|room| {
                room.source == Source::Lv
                    && !room.local_dirty
                    && room.server_known
                    && room.server_version == Some(expected_server_version)
                    && room.room_updated_at == expected_updated_at
            }) else {
                return Err(format!("{NOT_CLEAN}: the saved analysis cache requires the same unchanged clean design room"));
            };
            header.voltage_drop_metadata = Some(metadata);
            header.revision += 1;
            connection
                .execute(
                    "UPDATE design_rooms SET header=?3,voltage_drop=?4,voltage_drop_json=?5 WHERE scope=?1 AND transformer=?2",
                    params![scope, name, to_text(&header)?, to_text(&document)?, raw_json],
                )
                .map_err(sql)?;
            Outcome::written(answer("attached", json!(name)))
        }
        Operation::Delete {
            project,
            transformers,
        } => {
            let scope = fence(&owner, &project)?;
            let mut deleted = Vec::new();
            for name in names(&transformers)? {
                let removed = connection
                    .execute(
                        "DELETE FROM design_rooms WHERE scope=?1 AND transformer=?2",
                        params![scope, name],
                    )
                    .map_err(sql)?;
                if removed == 1 {
                    deleted.push(json!(name));
                }
            }
            Outcome::written(answer("deleted", Value::Array(deleted)))
        }
        Operation::LayerFeatures {
            project,
            layer,
            limit,
        } => Outcome::read(layer_features(
            connection,
            &fence(&owner, &project)?,
            &layer,
            limit,
        )?),
        Operation::Invalidate {
            project,
            transformer,
            receipt,
        } => {
            let (scope, name) = (fence(&owner, &project)?, room_name(&transformer)?);
            let added = invalidate(connection, &scope, &name, &receipt)?;
            Outcome::written(answer("added", json!(added)))
        }
        Operation::Count { project } => {
            let scope = fence(&owner, &project)?;
            let count: i64 = connection
                .query_row(
                    "SELECT count(*) FROM design_rooms WHERE scope=?1",
                    params![scope],
                    |row| row.get(0),
                )
                .map_err(sql)?;
            Outcome::read(answer("rooms", json!(count)))
        }
        Operation::Legacy => {
            let pending = |source| {
                Ok::<_, String>(held_receipt(connection, identity, STORE, source)?.is_none())
            };
            Outcome::read(Map::from_iter([
                (
                    "import_needed".into(),
                    json!(pending(ROOMS_SOURCE)? || pending(INVALIDATIONS_SOURCE)?),
                ),
                (
                    "legacy".into(),
                    json!({
                        "database": "ds_web_store",
                        "rooms": {"stores": ["design_transformers", "design_transformer_headers"]},
                        "persistent_prefixes": [INVALIDATION_PREFIX],
                    }),
                ),
            ]))
        }
        Operation::ImportRooms {
            account_email,
            rooms,
            now,
        } => {
            let account = account_email.trim().to_lowercase();
            if !exact(&account) || !account.contains('@') || rooms.len() > MAX_ROOMS {
                return Err(invalid(
                    "an import names the signed-in account email and at most 64 rooms",
                ));
            }
            bounded(now, "the time")?;
            import_batches::open(connection, identity, STORE, ROOMS_SOURCE)?;
            let mut moved = Vec::new();
            for legacy in &rooms {
                moved.push(import_room(connection, &owner, &account, legacy)?);
            }
            let count = moved.iter().filter(|moved| **moved).count() as u64;
            let batch = Batch {
                entries: &to_text(&rooms)?,
                count,
                skipped: rooms.len() as u64 - count,
            };
            import_batches::tally(connection, identity, STORE, ROOMS_SOURCE, batch)?;
            Outcome::written(answer("moved", json!(moved)))
        }
        Operation::ImportFinish { now } => {
            let (receipt, ran) = import_batches::finish(
                connection,
                identity,
                STORE,
                ROOMS_SOURCE,
                bounded(now, "the time")?,
                0,
            )?;
            let value = answer("receipt", receipt_value(Some(receipt))?);
            if ran {
                Outcome::written(value)
            } else {
                Outcome::read(value)
            }
        }
        Operation::ImportInvalidations { entries, now } => {
            if entries.len() > MAX_IMPORT_ENTRIES || now > MAX_SAFE {
                return Err(invalid(
                    "an import carries at most 4096 entries and its time",
                ));
            }
            let digest = sha256(to_text(&entries)?.as_bytes());
            let (receipt, ran) = import_once(
                connection,
                identity,
                STORE,
                INVALIDATIONS_SOURCE,
                &digest,
                now,
                || {
                    let (mut count, mut skipped) = (0, 0);
                    for entry in &entries {
                        match import_invalidation(connection, identity, &owner, entry) {
                            0 => skipped += 1,
                            moved => count += moved,
                        }
                    }
                    Ok((count, skipped))
                },
            )?;
            let value = answer("receipt", receipt_value(Some(receipt))?);
            if ran {
                Outcome::written(value)
            } else {
                Outcome::read(value)
            }
        }
    })
}

#[cfg(test)]
#[path = "design_rooms_tests.rs"]
mod tests;
