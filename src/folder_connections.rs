//! Folder connections: what this machine remembers about a folder a person
//! connected, and which opaque token holds its directory handle.
//!
//! A browser persists a `FileSystemDirectoryHandle` only by structured clone
//! into IndexedDB, so the web platform adapter keeps one IndexedDB object
//! store `{random 128-bit token → handle}` and nothing else: the single
//! IndexedDB use DS keeps, an owner-approved platform exception. Every fact
//! about a connection (identity, project, logical key, folder name, token,
//! times) and every lifecycle decision (which token is current, which tokens
//! the adapter drops) is this store's.
//!
//! Order: the adapter puts a freshly minted token's handle before `connect`
//! names it, so a committed row never names a token without a handle. A
//! replaced, refused or no longer referenced token is answered in `drop`, and
//! the adapter deletes exactly those.
//!
//! The maintenance sweep ([`retain_referenced_gis`], run by the local layer
//! store) drops only GIS folder connections no prepared local layer
//! references. PLS workspace links (`plsFolder:`) and the remembered export
//! workspace (`grid-export:`) are never swept: no local layer references them,
//! and the retired browser sweep deleted them with the person's links.
use crate::durable_store::{held_receipt, import_once, sha256, Identity, Input, Outcome, Principal};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const STORE: &str = "folder_connections";
pub const INVALID: &str = "folder_connection_invalid";
/// The retired browser store every import receipt names.
pub const LEGACY_SOURCE: &str = "ds_web_store/folder_connections";
const LEGACY_DATABASE: &str = "ds_web_store";
const LEGACY_STORE: &str = "folder_connections";
pub const GIS_FOLDER_PREFIX: &str = "gisFolder:";
pub const PLS_FOLDER_PREFIX: &str = "plsFolder:";
pub const EXPORT_WORKSPACE_PREFIX: &str = "grid-export:";
const MAX_KEY: usize = 1024;
const MAX_FOLDER: usize = 512;
/// The most connections one identity keeps, and the most targets, rows or
/// tokens one call names.
pub const MAX_ROWS: usize = 4096;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS folder_connections (owner TEXT NOT NULL, logical_key TEXT NOT NULL, project TEXT NOT NULL, folder_name TEXT NOT NULL, token TEXT NOT NULL UNIQUE CHECK(length(token)=32), created_at_ms INTEGER NOT NULL CHECK(created_at_ms>=0 AND created_at_ms<=9007199254740991), updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=0 AND updated_at_ms<=9007199254740991), PRIMARY KEY(owner, logical_key))";
const ROW: &str = "SELECT logical_key,project,folder_name,token,created_at_ms,updated_at_ms FROM folder_connections WHERE owner=?1 AND logical_key=?2";
const PUT: &str = "INSERT INTO folder_connections(owner,logical_key,project,folder_name,token,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(owner,logical_key) DO UPDATE SET project=excluded.project,folder_name=excluded.folder_name,token=excluded.token,created_at_ms=excluded.created_at_ms,updated_at_ms=excluded.updated_at_ms";

/// What a logical key connects. Only GIS folders are swept by reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    GisFolder,
    PlsFolder,
    ExportWorkspace,
    Other,
}
impl Kind {
    pub fn of(logical_key: &str) -> Kind {
        if logical_key.starts_with(GIS_FOLDER_PREFIX) {
            Kind::GisFolder
        } else if logical_key.starts_with(PLS_FOLDER_PREFIX) {
            Kind::PlsFolder
        } else if logical_key.starts_with(EXPORT_WORKSPACE_PREFIX) {
            Kind::ExportWorkspace
        } else {
            Kind::Other
        }
    }
}

/// One remembered connection, as every answer names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub logical_key: String,
    pub project: String,
    pub folder_name: String,
    pub kind: Kind,
    /// The adapter's key for the handle; get it, never derive it.
    pub token: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// One row of the retired store, as the importer read it. The handle itself
/// stays in the adapter: the importer already put it under `token`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRow {
    /// The retired physical key (`handle_key`).
    pub key: String,
    #[serde(default)]
    pub owner_uid: Option<String>,
    #[serde(default)]
    pub logical_handle_key: Option<String>,
    #[serde(default)]
    pub ds_project: Option<String>,
    #[serde(default)]
    pub folder_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<f64>,
    #[serde(default)]
    pub updated_at: Option<f64>,
    /// The stored value was a directory handle.
    pub directory: bool,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub project: String,
    pub logical_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Remember a connection under a token whose handle the adapter already
    /// put. A replaced token is answered in `drop`.
    Connect {
        project: String,
        logical_key: String,
        folder_name: String,
        token: String,
        now: u64,
    },
    /// The connections these targets name, in order; a row in another project
    /// is not this target's.
    Lookup { targets: Vec<Target> },
    /// Whether this identity's one-time import still has to run, and where the
    /// retired store lives for the host's read-only importer.
    Legacy,
    /// The one-time import of the retired store for this identity. Every
    /// token the import does not admit is answered in `drop`.
    Import { rows: Vec<LegacyRow>, now: u64 },
}

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.trim() == value && !value.chars().any(char::is_control)
}
/// A token is 32 lowercase hex characters: 128 bits of host randomness.
pub fn token(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
fn project(value: &str) -> Result<&str, String> {
    if crate::execution_context::valid_project(value) {
        Ok(value)
    } else {
        Err(invalid("a connection's project is one project id"))
    }
}
fn logical_key(value: &str) -> Result<&str, String> {
    if bounded(value, MAX_KEY) {
        Ok(value)
    } else {
        Err(invalid("a logical key is bounded, trimmed text"))
    }
}
fn folder_name(value: &str) -> Result<&str, String> {
    if bounded(value, MAX_FOLDER) {
        Ok(value)
    } else {
        Err(invalid("a folder name is bounded, trimmed text"))
    }
}
fn time(value: u64) -> Result<u64, String> {
    if value <= MAX_SAFE {
        Ok(value)
    } else {
        Err(invalid("a time exceeds its bound"))
    }
}
fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}

fn read(connection: &Connection, owner: &str, key: &str) -> Result<Option<Row>, String> {
    connection
        .query_row(ROW, params![owner, key], |row| {
            let logical_key: String = row.get(0)?;
            Ok(Row {
                kind: Kind::of(&logical_key),
                logical_key,
                project: row.get(1)?,
                folder_name: row.get(2)?,
                token: row.get(3)?,
                created_at_ms: row.get::<_, i64>(4)? as u64,
                updated_at_ms: row.get::<_, i64>(5)? as u64,
            })
        })
        .optional()
        .map_err(sql)
}

fn put(connection: &Connection, owner: &str, row: &Row) -> Result<(), String> {
    let held: Option<(String, String)> = connection
        .query_row(
            "SELECT owner,logical_key FROM folder_connections WHERE token=?1",
            params![row.token],
            |found| Ok((found.get(0)?, found.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    if held.is_some_and(|(held_owner, held_key)| held_owner != owner || held_key != row.logical_key) {
        return Err(invalid("a token names exactly one connection; mint a fresh one"));
    }
    let others: i64 = connection
        .query_row(
            "SELECT count(*) FROM folder_connections WHERE owner=?1 AND logical_key<>?2",
            params![owner, row.logical_key],
            |found| found.get(0),
        )
        .map_err(sql)?;
    if others as usize >= MAX_ROWS {
        return Err(invalid("an identity keeps at most 4096 folder connections"));
    }
    connection
        .execute(
            PUT,
            params![
                owner,
                row.logical_key,
                row.project,
                row.folder_name,
                row.token,
                row.created_at_ms as i64,
                row.updated_at_ms as i64
            ],
        )
        .map_err(sql)?;
    Ok(())
}

/// Drop every GIS folder connection of `owner` whose logical key no prepared
/// local layer references, and answer the tokens the adapter deletes.
pub fn retain_referenced_gis(connection: &Connection, owner: &str, referenced: &BTreeSet<String>) -> Result<Vec<String>, String> {
    connection.execute_batch(TABLE).map_err(sql)?;
    let mut statement = connection
        .prepare("SELECT logical_key,token FROM folder_connections WHERE owner=?1 ORDER BY logical_key")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![owner], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let mut dropped = vec![];
    for (key, token) in rows {
        if Kind::of(&key) == Kind::GisFolder && !referenced.contains(&key) {
            connection
                .execute(
                    "DELETE FROM folder_connections WHERE owner=?1 AND logical_key=?2",
                    params![owner, key],
                )
                .map_err(sql)?;
            dropped.push(token);
        }
    }
    Ok(dropped)
}

/// The tokens among `present` (what the adapter's table holds) that no
/// connection of any identity names: leftovers of an interrupted write.
pub fn unreferenced_tokens(connection: &Connection, present: &[String]) -> Result<Vec<String>, String> {
    connection.execute_batch(TABLE).map_err(sql)?;
    if present.len() > MAX_ROWS || present.iter().any(|value| !token(value)) {
        return Err(invalid("present tokens are at most 4096 tokens of 32 hex characters"));
    }
    let named = serde_json::to_string(present).map_err(|e| e.to_string())?;
    let mut statement = connection
        .prepare("SELECT DISTINCT value FROM json_each(?1) WHERE value NOT IN (SELECT token FROM folder_connections) ORDER BY value")
        .map_err(sql)?;
    let unreferenced = statement
        .query_map(params![named], |row| row.get::<_, String>(0))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    Ok(unreferenced)
}

/// Admit the retired rows exactly as the retired reader did: this account's
/// own rows, then rows that predate account scoping (claimable by an account,
/// by exact key). Answers the admitted connections by logical key.
fn admit_legacy(account: Option<&str>, rows: &[LegacyRow], now: u64) -> BTreeMap<String, Row> {
    let stamp = |value: Option<f64>| match value {
        Some(value) if value.is_finite() && value > 0.0 && value <= MAX_SAFE as f64 => value.trunc() as u64,
        _ => now,
    };
    let mut admitted: BTreeMap<String, Row> = BTreeMap::new();
    // This account's scoped rows first: they win over a pre-scoping row.
    for scoped in [true, false] {
        for row in rows {
            let key = match (scoped, account, &row.owner_uid, &row.logical_handle_key) {
                (true, Some(account), Some(owner), Some(logical)) if owner == account => {
                    let logical = logical.trim();
                    if serde_json::to_string(&(owner, logical)).ok().as_deref() != Some(row.key.as_str()) {
                        continue;
                    }
                    logical.to_owned()
                }
                (false, Some(_), None, None) => row.key.trim().to_owned(),
                _ => continue,
            };
            let project = row.ds_project.as_deref().unwrap_or_default().trim();
            let folder = row.folder_name.as_deref().unwrap_or_default().trim();
            if !row.directory
                || !bounded(&key, MAX_KEY)
                || admitted.contains_key(&key)
                || !crate::execution_context::valid_project(project)
                || !bounded(folder, MAX_FOLDER)
                || !token(&row.token)
            {
                continue;
            }
            admitted.insert(
                key.clone(),
                Row {
                    kind: Kind::of(&key),
                    logical_key: key,
                    project: project.into(),
                    folder_name: folder.into(),
                    token: row.token.clone(),
                    created_at_ms: stamp(row.created_at),
                    updated_at_ms: stamp(row.updated_at),
                },
            );
        }
    }
    admitted
}

pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("folder connections carry no payload bytes"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let identity: &Identity = input.identity;
    let owner = identity.owner();
    match operation {
        Operation::Connect {
            project: p,
            logical_key: k,
            folder_name: n,
            token: t,
            now,
        } => {
            let (project, key, name, now) = (project(&p)?, logical_key(&k)?, folder_name(&n)?, time(now)?);
            if !token(&t) {
                return Err(invalid("a token is 32 lowercase hex characters of fresh randomness"));
            }
            let previous = read(connection, &owner, key)?;
            let row = Row {
                logical_key: key.into(),
                project: project.into(),
                folder_name: name.into(),
                kind: Kind::of(key),
                token: t.clone(),
                // The same connection keeps its first time; one moved to
                // another project starts over.
                created_at_ms: previous
                    .as_ref()
                    .filter(|previous| previous.project == project)
                    .map_or(now, |previous| previous.created_at_ms),
                updated_at_ms: now,
            };
            put(connection, &owner, &row)?;
            let drop: Vec<String> = previous.map(|previous| previous.token).filter(|old| *old != t).into_iter().collect();
            Ok(Outcome::written(fields(json!({"connection": row, "drop": drop}))))
        }
        Operation::Lookup { targets } => {
            if targets.len() > MAX_ROWS {
                return Err(invalid("a lookup names at most 4096 targets"));
            }
            let mut found = Vec::with_capacity(targets.len());
            for target in targets {
                let row = read(connection, &owner, logical_key(&target.logical_key)?)?;
                found.push(row.filter(|row| row.project == target.project));
            }
            Ok(Outcome::read(fields(json!({"connections": found}))))
        }
        Operation::Legacy => Ok(Outcome::read(fields(json!({
            "import_needed": held_receipt(connection, identity, STORE, LEGACY_SOURCE)?.is_none(),
            "legacy": {"database": LEGACY_DATABASE, "store": LEGACY_STORE},
        })))),
        Operation::Import { rows, now } => {
            if rows.len() > MAX_ROWS {
                return Err(invalid("an import names at most 4096 rows"));
            }
            let entries = serde_json::to_string(&rows).map_err(|e| e.to_string())?;
            let mut kept = BTreeSet::new();
            let (receipt, ran) = import_once(connection, identity, STORE, LEGACY_SOURCE, &sha256(entries.as_bytes()), time(now)?, || {
                let account = match &identity.principal {
                    Principal::Account(uid) => Some(uid.as_str()),
                    Principal::Anonymous => None,
                };
                let admitted = admit_legacy(account, &rows, now);
                for (key, row) in &admitted {
                    // A connection made here since the upgrade is newer and stays.
                    if read(connection, &owner, key)?.is_none() {
                        put(connection, &owner, row)?;
                        kept.insert(row.token.clone());
                    }
                }
                Ok((kept.len() as u64, (rows.len() - kept.len()) as u64))
            })?;
            let drop: Vec<&String> = rows.iter().map(|row| &row.token).filter(|token| !kept.contains(*token)).collect();
            let answer = fields(json!({"imported": ran, "receipt": receipt, "drop": drop}));
            Ok(if ran { Outcome::written(answer) } else { Outcome::read(answer) })
        }
    }
}

#[cfg(test)]
#[path = "folder_connections_tests.rs"]
mod tests;
