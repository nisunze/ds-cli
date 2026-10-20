//! The local DS Grid model workspace one identity keeps on this machine, in
//! the Kernel's SQLite: retained check results, temporary model groups and
//! the remembered working session of each project (or of no project).
//!
//! A result is the engine's opaque artifact for one model revision; a
//! temporary group is the import provenance and status of one materialized
//! `.dsgrid`; a session remembers which models were open and which one held
//! the Profile. Removing a model removes everything here that belongs to it in
//! one transaction. The `.dsgrid` working copies themselves join this store
//! with the byte channel.
use crate::durable_store::{Input, Outcome};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const STORE: &str = "grid_model_store";
pub const INVALID: &str = "grid_model_store_invalid";
pub const UNKNOWN_GROUP: &str = "grid_model_group_unknown";
const MAX_ID: usize = 256;
const MAX_TEXT: usize = 4096;
pub const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_SESSION_BYTES: usize = 1024 * 1024;
const MAX_GROUP_BYTES: usize = 4 * 1024 * 1024;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

pub const RESULTS: &str = "CREATE TABLE IF NOT EXISTS grid_model_results (owner TEXT NOT NULL, result_id TEXT NOT NULL, model_id TEXT NOT NULL, created_ms INTEGER NOT NULL, record TEXT NOT NULL, PRIMARY KEY(owner, result_id))";
pub const RESULTS_BY_MODEL: &str = "CREATE INDEX IF NOT EXISTS grid_model_results_by_model ON grid_model_results(owner, model_id, created_ms)";
pub const GROUPS: &str = "CREATE TABLE IF NOT EXISTS grid_model_groups (owner TEXT NOT NULL, group_id TEXT NOT NULL, session_model_id TEXT, created_ms INTEGER NOT NULL, record TEXT NOT NULL, PRIMARY KEY(owner, group_id))";
/// `scope` is the project, or empty outside a project.
pub const SESSIONS: &str = "CREATE TABLE IF NOT EXISTS grid_model_sessions (owner TEXT NOT NULL, scope TEXT NOT NULL, session TEXT NOT NULL, PRIMARY KEY(owner, scope))";

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}
fn exact(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.trim() == value && !value.chars().any(char::is_control)
}
fn id(value: &str, what: &str) -> Result<(), String> {
    if exact(value, MAX_ID) {
        Ok(())
    } else {
        Err(invalid(&format!("{what} is an exact identity of at most 256 bytes")))
    }
}
fn time(value: u64) -> Result<i64, String> {
    if value <= MAX_SAFE {
        Ok(value as i64)
    } else {
        Err(invalid("a time exceeds its bound"))
    }
}
fn text(value: &impl Serialize, max: usize, what: &str) -> Result<String, String> {
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if text.len() > max {
        return Err(invalid(&format!("{what} exceeds its bound")));
    }
    Ok(text)
}

// --- records -----------------------------------------------------------------

/// One retained check result: the engine's artifact, stored verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRecord {
    pub result_id: String,
    pub model_id: String,
    pub pinned_content_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_type: Option<String>,
    pub created: u64,
    pub artifact: Map<String, Value>,
}
impl ResultRecord {
    fn admit(&self) -> Result<(), String> {
        id(&self.result_id, "a result id")?;
        id(&self.model_id, "a model id")?;
        if !exact(&self.pinned_content_root, MAX_TEXT)
            || self.check_type.as_deref().is_some_and(|kind| !exact(kind, MAX_ID))
            || self.created > MAX_SAFE
            || self.artifact.get("result_id").and_then(Value::as_str) != Some(self.result_id.as_str())
        {
            return Err(invalid("a result names its revision root, its check, a time and its own artifact"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub filename: String,
    /// `sha256:<hex>` of the exact source bytes.
    pub sha256: String,
    pub byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Status {
    Importing,
    Ready,
    /// The engine's refusal, verbatim.
    Blocked {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        candidates: Option<Vec<Value>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreatedFrom {
    Drop,
    Example,
    Version,
    Interchange,
    Combine,
}

/// One temporary model group: the identity, provenance and status of one
/// materialized `.dsgrid`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub group_id: String,
    pub name: String,
    pub source: Source,
    pub session_model_id: Option<String>,
    pub active_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_summary: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_system: Option<String>,
    pub status: Status,
    pub created_from: CreatedFrom,
    pub created: u64,
}

/// The stable group id of a source: content-derived, so the same bytes
/// resolve the same group.
pub fn group_id(sha256: &str) -> Result<String, String> {
    let hex = sha256.strip_prefix("sha256:").unwrap_or(sha256);
    if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(invalid("a group source names the sha256 of its exact bytes"));
    }
    Ok(format!("tmg-{}", &hex[..24]))
}

impl Group {
    fn admit(&self) -> Result<(), String> {
        id(&self.group_id, "a group id")?;
        if group_id(&self.source.sha256)? != self.group_id {
            return Err(invalid("a group id is derived from its source digest"));
        }
        if !exact(&self.name, MAX_TEXT)
            || !exact(&self.source.filename, MAX_TEXT)
            || self.source.byte_size > MAX_SAFE
            || self.created > MAX_SAFE
            || self.session_model_id.as_deref().is_some_and(|model| !exact(model, MAX_ID))
            || self.active_revision.as_deref().is_some_and(|revision| !exact(revision, MAX_TEXT))
        {
            return Err(invalid("a group names its source, its model and a time"));
        }
        if let Status::Blocked { code, message, .. } = &self.status {
            if !exact(code, MAX_ID) || message.len() > MAX_TEXT {
                return Err(invalid("a blocked group carries the engine's code and message"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceOwner {
    Temporary,
    MvDesign,
}

/// How one remembered model was opened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub imported_from: Option<String>,
    pub template_origin: Value,
    pub example_origin: Value,
    pub version_origin: Value,
    pub read_only: bool,
    pub temp_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_owner: Option<WorkspaceOwner>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    Layer,
    Source,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewState {
    pub layer_visibility: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_layer_visibility: Option<Map<String, Value>>,
    pub grouping_mode: Grouping,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remembered {
    pub model_id: String,
    pub provenance: Provenance,
}

/// The working session one project (or no project) remembers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// The model that held Profile/editing focus; `None` when none did.
    pub selected_model_id: Option<String>,
    pub selected_revision_id: Option<String>,
    pub selected_provenance: Provenance,
    pub view_state: ViewState,
    pub last_selected: u64,
    /// Every model source the session reopens.
    #[serde(default)]
    pub open_models: Vec<Remembered>,
}
impl Session {
    fn admit(&self) -> Result<(), String> {
        let visibility_ok = |map: &Map<String, Value>| map.values().all(Value::is_boolean);
        let models_ok = self
            .view_state
            .model_layer_visibility
            .as_ref()
            .is_none_or(|map| map.values().all(|inner| inner.as_object().is_some_and(visibility_ok)));
        let ids_ok = self.selected_model_id.as_deref().is_none_or(|model| exact(model, MAX_ID))
            && self.open_models.iter().all(|entry| exact(&entry.model_id, MAX_ID));
        if !visibility_ok(&self.view_state.layer_visibility) || !models_ok || !ids_ok || self.last_selected > MAX_SAFE {
            return Err(invalid("a session names models and boolean layer visibility"));
        }
        Ok(())
    }
}

fn scope(project: Option<&str>) -> Result<&str, String> {
    match project {
        None => Ok(""),
        Some(project) if crate::execution_context::valid_project(project) => Ok(project),
        Some(_) => Err(invalid("a session belongs to one project id or to none")),
    }
}

// --- operations ---------------------------------------------------------------

/// Which model a group's materialization settled on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupModel {
    pub session_model_id: String,
    pub active_revision: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Retain one check result (re-running a check mints a new id).
    PutResult { result: ResultRecord },
    /// One model's retained results, newest first.
    ListResults { model_id: String },
    /// Start materializing one source as a temporary group (the same source
    /// resolves the same group, which starts over).
    BeginGroup {
        name: String,
        source: Source,
        created_from: CreatedFrom,
        now: u64,
    },
    /// Settle a group's status; `model` names the model it settled on, when
    /// one was opened.
    SettleGroup {
        group_id: String,
        status: Status,
        #[serde(default)]
        model: Option<GroupModel>,
    },
    GetGroup { group_id: String },
    /// Every group, newest first.
    ListGroups,
    PutSession {
        #[serde(default)]
        project: Option<String>,
        session: Box<Session>,
    },
    GetSession {
        #[serde(default)]
        project: Option<String>,
    },
    ClearSession {
        #[serde(default)]
        project: Option<String>,
    },
    /// Remove everything a model (or an explicit group) owns here.
    RemoveModel {
        #[serde(default)]
        model_id: Option<String>,
        #[serde(default)]
        group_id: Option<String>,
    },
}

fn read_group(connection: &Connection, owner: &str, group: &str) -> Result<Option<Group>, String> {
    connection
        .query_row(
            "SELECT record FROM grid_model_groups WHERE owner=?1 AND group_id=?2",
            params![owner, group],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql)?
        .map(|text| serde_json::from_str(&text).map_err(|_| invalid("a stored group is corrupt")))
        .transpose()
}
fn write_group(connection: &Connection, owner: &str, group: &Group) -> Result<(), String> {
    group.admit()?;
    connection
        .execute(
            "INSERT INTO grid_model_groups(owner,group_id,session_model_id,created_ms,record) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,group_id) DO UPDATE SET session_model_id=excluded.session_model_id,created_ms=excluded.created_ms,record=excluded.record",
            params![owner, group.group_id, group.session_model_id, time(group.created)?, text(group, MAX_GROUP_BYTES, "a group")?],
        )
        .map_err(sql)?;
    Ok(())
}
fn write_result(connection: &Connection, owner: &str, result: &ResultRecord) -> Result<(), String> {
    result.admit()?;
    connection
        .execute(
            "INSERT INTO grid_model_results(owner,result_id,model_id,created_ms,record) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,result_id) DO UPDATE SET model_id=excluded.model_id,created_ms=excluded.created_ms,record=excluded.record",
            params![owner, result.result_id, result.model_id, time(result.created)?, text(result, MAX_ARTIFACT_BYTES, "a result")?],
        )
        .map_err(sql)?;
    Ok(())
}
fn rows<T: for<'a> Deserialize<'a>>(connection: &Connection, query: &str, bind: &[&dyn rusqlite::ToSql]) -> Result<Vec<T>, String> {
    let mut statement = connection.prepare(query).map_err(sql)?;
    let texts = statement
        .query_map(bind, |row| row.get::<_, String>(0))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    texts
        .iter()
        .map(|text| serde_json::from_str(text).map_err(|_| invalid("a stored record is corrupt")))
        .collect()
}
fn read_session(connection: &Connection, owner: &str, scope: &str) -> Result<Option<Session>, String> {
    connection
        .query_row(
            "SELECT session FROM grid_model_sessions WHERE owner=?1 AND scope=?2",
            params![owner, scope],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql)?
        .map(|text| serde_json::from_str(&text).map_err(|_| invalid("a stored session is corrupt")))
        .transpose()
}
fn write_session(connection: &Connection, owner: &str, scope: &str, session: &Session) -> Result<(), String> {
    session.admit()?;
    connection
        .execute(
            "INSERT INTO grid_model_sessions(owner,scope,session) VALUES(?1,?2,?3) ON CONFLICT(owner,scope) DO UPDATE SET session=excluded.session",
            params![owner, scope, text(session, MAX_SESSION_BYTES, "a session")?],
        )
        .map_err(sql)?;
    Ok(())
}

fn remove_model(connection: &Connection, owner: &str, model: Option<String>, group: Option<String>) -> Result<Value, String> {
    let mut model = model.filter(|model| !model.trim().is_empty());
    let group = group.filter(|group| !group.trim().is_empty());
    if model.is_none() && group.is_none() {
        return Err(invalid("a removal names a model or a temporary group"));
    }
    let mut removed: BTreeSet<String> = BTreeSet::new();
    if let Some(group) = group {
        if model.is_none() {
            model = read_group(connection, owner, &group)?.and_then(|held| held.session_model_id);
        }
        removed.insert(group);
    }
    let mut results = 0;
    if let Some(model) = &model {
        let backed: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT group_id FROM grid_model_groups WHERE owner=?1 AND session_model_id=?2")
                .map_err(sql)?;
            let groups = statement
                .query_map(params![owner, model], |row| row.get::<_, String>(0))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            groups
        };
        removed.extend(backed);
        results = connection
            .execute("DELETE FROM grid_model_results WHERE owner=?1 AND model_id=?2", params![owner, model])
            .map_err(sql)?;
        // A session focused on the model forgets itself; one that only
        // remembered it in the background forgets just that model.
        let sessions: Vec<(String, String)> = {
            let mut statement = connection
                .prepare("SELECT scope,session FROM grid_model_sessions WHERE owner=?1")
                .map_err(sql)?;
            let sessions = statement
                .query_map(params![owner], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            sessions
        };
        for (scope, text) in sessions {
            let Ok(mut session) = serde_json::from_str::<Session>(&text) else {
                continue;
            };
            if session.selected_model_id.as_deref() == Some(model) {
                connection
                    .execute("DELETE FROM grid_model_sessions WHERE owner=?1 AND scope=?2", params![owner, scope])
                    .map_err(sql)?;
            } else if session.open_models.iter().any(|entry| &entry.model_id == model) {
                session.open_models.retain(|entry| &entry.model_id != model);
                write_session(connection, owner, &scope, &session)?;
            }
        }
    }
    for group in &removed {
        connection
            .execute("DELETE FROM grid_model_groups WHERE owner=?1 AND group_id=?2", params![owner, group])
            .map_err(sql)?;
    }
    Ok(json!({
        "model_id": model,
        "removed_group_ids": removed,
        "removed_result_count": results,
    }))
}

fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}
/// The answer of one kind: results, groups and sessions advance their own
/// generations.
fn kind(outcome: Outcome, kind: &str) -> Outcome {
    Outcome {
        generation_key: Some(format!("{STORE}.{kind}")),
        ..outcome
    }
}

pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("results, groups and sessions carry no payload bytes"));
    }
    connection
        .execute_batch(&format!("{RESULTS};{RESULTS_BY_MODEL};{GROUPS};{SESSIONS}"))
        .map_err(sql)?;
    let owner = input.identity.owner();
    Ok(match operation {
        Operation::PutResult { result } => {
            write_result(connection, &owner, &result)?;
            kind(Outcome::written(fields(json!({"result_id": result.result_id}))), "results")
        }
        Operation::ListResults { model_id } => {
            id(&model_id, "a model id")?;
            let results: Vec<ResultRecord> = rows(
                connection,
                "SELECT record FROM grid_model_results WHERE owner=?1 AND model_id=?2 ORDER BY created_ms DESC, result_id",
                &[&owner, &model_id],
            )?;
            kind(Outcome::read(fields(json!({"results": results}))), "results")
        }
        Operation::BeginGroup { name, source, created_from, now } => {
            let group = Group {
                group_id: group_id(&source.sha256)?,
                name,
                source,
                session_model_id: None,
                active_revision: None,
                import_summary: None,
                coordinate_system: None,
                status: Status::Importing,
                created_from,
                created: now,
            };
            write_group(connection, &owner, &group)?;
            kind(Outcome::written(fields(json!({"group": group}))), "groups")
        }
        Operation::SettleGroup { group_id, status, model } => {
            let mut group = read_group(connection, &owner, &group_id)?
                .ok_or_else(|| format!("{UNKNOWN_GROUP}: no temporary group {group_id}"))?;
            group.status = status;
            if let Some(model) = model {
                group.session_model_id = Some(model.session_model_id);
                group.active_revision = model.active_revision;
            }
            write_group(connection, &owner, &group)?;
            kind(Outcome::written(fields(json!({"group": group}))), "groups")
        }
        Operation::GetGroup { group_id } => {
            kind(Outcome::read(fields(json!({"group": read_group(connection, &owner, &group_id)?}))), "groups")
        }
        Operation::ListGroups => {
            let groups: Vec<Group> = rows(
                connection,
                "SELECT record FROM grid_model_groups WHERE owner=?1 ORDER BY created_ms DESC, group_id",
                &[&owner],
            )?;
            kind(Outcome::read(fields(json!({"groups": groups}))), "groups")
        }
        Operation::PutSession { project, session } => {
            write_session(connection, &owner, scope(project.as_deref())?, &session)?;
            kind(Outcome::written(fields(json!({"session": session}))), "sessions")
        }
        Operation::GetSession { project } => kind(
            Outcome::read(fields(json!({"session": read_session(connection, &owner, scope(project.as_deref())?)?}))),
            "sessions",
        ),
        Operation::ClearSession { project } => {
            connection
                .execute(
                    "DELETE FROM grid_model_sessions WHERE owner=?1 AND scope=?2",
                    params![owner, scope(project.as_deref())?],
                )
                .map_err(sql)?;
            kind(Outcome::written(fields(json!({"session": null}))), "sessions")
        }
        Operation::RemoveModel { model_id, group_id } => Outcome::written(fields(remove_model(connection, &owner, model_id, group_id)?)),
    })
}

#[cfg(test)]
#[path = "grid_model_store_tests.rs"]
mod tests;
