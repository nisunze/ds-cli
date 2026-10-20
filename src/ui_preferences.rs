//! Durable UI preferences: one Kernel-owned SQLite table and a closed key
//! vocabulary, run by the same rusqlite code in Core's protected Store
//! (Desktop and headless) and in the browser's storage worker (hosted Web).
//!
//! Each key declares whether it belongs to the account or to one project, its
//! byte bound and its value contract (`values.rs`), which admits a value into
//! its canonical form. A preference is never authority: a stored value that
//! no longer admits reads as absent and stays in place until the next write
//! replaces it. The host binds the identity; a caller names keys, values and,
//! for a project key, the project.
use crate::durable_store::{Input, Outcome};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

mod values;

pub const STORE: &str = "ui_preferences";
pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS ui_preferences (scope TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=0 AND updated_at_ms<=9007199254740991), PRIMARY KEY(scope,key))";
/// The bound of a key that declares none.
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024;
pub const MAX_KEYS: usize = 32;
pub const INVALID: &str = "ui_preference_invalid";
const MAX_SAFE: u64 = 9_007_199_254_740_991;

/// The closed preference vocabulary. Each key's name, home, bound and value
/// contract are one row of `values/catalog.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    /// The project the window last worked in, restored at the next start.
    ActiveProject,
    // The account's window: layer-group folding, the camera mode, the
    // cartography defaults, the editing toolbar, the browser's raster
    // overlays, the application settings, the editing projection, the pinned
    // group's visibility, the Profile restart record, the map-data sources a
    // new transformer preselects, panel and dialog geometry, the messages
    // dismissed from the foreground notifications, and the class styles kept
    // on this device for temporary layers.
    LayerGroupCollapse,
    HillshadeExaggeration,
    MapDefaults,
    ToolLayout,
    TileLayers,
    ApplicationSettings,
    EditProjectionChoice,
    EditProjectionCustom,
    PinnedGroupVisible,
    ProfileOccupancy,
    CreateTransformerMapSources,
    UploadDialogLayout,
    PropertiesDockWidth,
    StyleCenterLayout,
    MessagingNotificationDismissals,
    LocalClassStyles,
    // One project's choices: layer visibility, disabled popups, the survey
    // working area, the process preset and process setup, the active LV
    // transformer, the status register's warning filter and sort, the camera,
    // the pinned transformers and the Fast-mode customer and pole sources.
    LayerVisibility,
    PopupLayersDisabled,
    WorkingArea,
    ProcessPreset,
    ProcessSetup,
    ActiveLvTransformer,
    StatusWarningTypes,
    StatusSort,
    MapViewport,
    PinnedTransformers,
    FastModeCustomerSource,
    FastModePoleSource,
}
impl Key {
    pub const ALL: &'static [Key] = &[
        Key::ActiveProject,
        Key::LayerGroupCollapse,
        Key::HillshadeExaggeration,
        Key::MapDefaults,
        Key::ToolLayout,
        Key::TileLayers,
        Key::ApplicationSettings,
        Key::EditProjectionChoice,
        Key::EditProjectionCustom,
        Key::PinnedGroupVisible,
        Key::ProfileOccupancy,
        Key::CreateTransformerMapSources,
        Key::UploadDialogLayout,
        Key::PropertiesDockWidth,
        Key::StyleCenterLayout,
        Key::MessagingNotificationDismissals,
        Key::LocalClassStyles,
        Key::LayerVisibility,
        Key::PopupLayersDisabled,
        Key::WorkingArea,
        Key::ProcessPreset,
        Key::ProcessSetup,
        Key::ActiveLvTransformer,
        Key::StatusWarningTypes,
        Key::StatusSort,
        Key::MapViewport,
        Key::PinnedTransformers,
        Key::FastModeCustomerSource,
        Key::FastModePoleSource,
    ];
    pub fn name(self) -> &'static str {
        values::name(self)
    }
    fn parse(name: &str) -> Option<Key> {
        Self::ALL.iter().copied().find(|key| key.name() == name)
    }
    /// Whether the key belongs to one project rather than to the account.
    pub fn project_scoped(self) -> bool {
        values::project_scoped(self)
    }
    /// The largest stored value, in bytes of its JSON text.
    pub fn max_bytes(self) -> usize {
        values::max_bytes(self)
    }
    /// The canonical form of an offered or stored value, or `None` when the
    /// key's contract refuses it.
    pub fn admit(self, value: &Value) -> Option<Value> {
        values::admit(self, value)
    }
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Get {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        keys: Vec<Key>,
    },
    Set {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        key: Key,
        value: Value,
        now: u64,
    },
    Clear {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        key: Key,
    },
}

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}

/// The row fence for these keys: an account key is kept without a project,
/// a project key only inside its exact project.
fn scope(owner: &str, project: Option<&str>, keys: &[Key]) -> Result<String, String> {
    let project_keys = keys.iter().filter(|key| key.project_scoped()).count();
    match project {
        Some(project)
            if project_keys == keys.len() && crate::execution_context::valid_project(project) => {}
        None if project_keys == 0 => {}
        _ => {
            return Err(invalid(
                "account preferences are kept without a project and project preferences inside their exact project",
            ))
        }
    }
    serde_json::to_string(&(owner, project)).map_err(|e| e.to_string())
}

/// The stored JSON text of an admitted value, within the key's bound.
fn canonical(key: Key, value: &Value) -> Result<(Value, String), String> {
    let value = key
        .admit(value)
        .ok_or_else(|| invalid(&format!("{} does not admit that value", key.name())))?;
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    if text.len() > key.max_bytes() {
        return Err(invalid(&format!("{} exceeds its byte bound", key.name())));
    }
    Ok((value, text))
}

/// A stored value: unreadable, over its bound or no longer admitted reads as
/// absent.
fn stored(key: Key, text: &str) -> Value {
    if text.len() > key.max_bytes() {
        return Value::Null;
    }
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| key.admit(&value))
        .unwrap_or(Value::Null)
}

fn get(
    connection: &Connection,
    owner: &str,
    project: Option<&str>,
    keys: Vec<Key>,
) -> Result<Map<String, Value>, String> {
    if keys.is_empty() || keys.len() > MAX_KEYS {
        return Err(invalid("read between 1 and 32 keys at once"));
    }
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != keys.len() {
        return Err(invalid("a read names each key once"));
    }
    let fence = scope(owner, project, &keys)?;
    let names: Vec<&str> = keys.iter().map(|key| key.name()).collect();
    let names = serde_json::to_string(&names).map_err(|e| e.to_string())?;
    let mut values: Map<String, Value> = keys
        .iter()
        .map(|key| (key.name().to_string(), Value::Null))
        .collect();
    let mut statement = connection
        .prepare("SELECT key,value FROM ui_preferences WHERE scope=?1 AND key IN (SELECT value FROM json_each(?2))")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![fence, names], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql)?;
    for row in rows {
        let (name, text) = row.map_err(sql)?;
        let key = Key::parse(&name)
            .filter(|key| keys.contains(key))
            .ok_or_else(|| invalid("the store returned an unrequested key"))?;
        values.insert(key.name().into(), stored(key, &text));
    }
    Ok(values)
}

/// Run one operation inside the caller's transaction. Reads and writes answer
/// `{"values": {key: value|null}}`.
pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("preferences carry no payload bytes"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let owner = input.identity.owner();
    let mut answer = Map::new();
    Ok(match operation {
        Operation::Get { project, keys } => {
            answer.insert("values".into(), Value::Object(get(connection, &owner, project.as_deref(), keys)?));
            Outcome::read(answer)
        }
        Operation::Set {
            project,
            key,
            value,
            now,
        } => {
            if now > MAX_SAFE {
                return Err(invalid("the time exceeds its bound"));
            }
            let (value, text) = canonical(key, &value)?;
            let fence = scope(&owner, project.as_deref(), &[key])?;
            connection
                .execute(
                    "INSERT INTO ui_preferences(scope,key,value,updated_at_ms) VALUES(?1,?2,?3,?4) ON CONFLICT(scope,key) DO UPDATE SET value=excluded.value,updated_at_ms=excluded.updated_at_ms",
                    params![fence, key.name(), text, now as i64],
                )
                .map_err(sql)?;
            answer.insert("values".into(), json!({ key.name(): value }));
            Outcome::written(answer)
        }
        Operation::Clear { project, key } => {
            let fence = scope(&owner, project.as_deref(), &[key])?;
            connection
                .execute(
                    "DELETE FROM ui_preferences WHERE scope=?1 AND key=?2",
                    params![fence, key.name()],
                )
                .map_err(sql)?;
            answer.insert("values".into(), json!({ key.name(): Value::Null }));
            Outcome::written(answer)
        }
    })
}

#[cfg(test)]
#[path = "ui_preferences_tests.rs"]
mod tests;
