//! One local room per (principal, project, canonical dataset, query parameters).
//!
//! This is the store behind the project reference-data cache: bounded,
//! spatially indexed extracts of canonical datasets, so a project queries what
//! its own coverage needs instead of downloading a nation. Buildings are the
//! first dataset through it; the tables carry dataset identity explicitly so
//! the next provider needs no second storage design.
//!
//! Policy lives in `ds_command_kernel::project_dataset_cache` and geometry in
//! `ds_geo`. This module owns persistence, locking and fencing only.
//!
//! Three invariants it exists to hold:
//!
//! * **Requested is not completed.** Both are stored. A query that never
//!   returned leaves the requested area recorded and the completed coverage
//!   untouched, so nothing can later mistake an attempt for a holding.
//! * **A failure never costs held data.** `fail` writes a message and a query
//!   status; it does not touch features, coverage or the index.
//! * **Late answers are refused, not applied.** Every mutation carries the
//!   generation it read, and the room re-checks its own principal/project
//!   before writing, so a completion that arrives after a project or account
//!   switch cannot land in the wrong room. A pending acquisition is abandoned
//!   at `policy::ACQUISITION_TICKET_TTL_MS`, reported `expired` rather than
//!   pending, and can no longer settle anything.
//! * **Freshness is reported, never enforced by deletion.** The room records
//!   the newest source version it has observed and which coverage completed
//!   under an older one. Reading that says nothing is destroyed: an explicit
//!   refresh re-acquires stale coverage, and until it does the project keeps
//!   every row it paid for.

use ds_command_kernel::project_dataset_cache::{self as policy, feature_key};
use ds_command_kernel::reference_cache::{feature_bounds, intersects_area, validate_area};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const ROOM_FILE: &str = "room.sqlite";
const ROOT: &str = "project-dataset-cache";
const VERSION: &str = "v1";
/// A room is a project extract, not a national dataset. This is a generous
/// ceiling on that, and the point at which a caller should be narrowing its
/// coverage rather than growing one room without bound.
const MAX_ROOM_FEATURES: i64 = 5_000_000;
const MAX_QUERY_FEATURES: usize = 500_000;
const DEFAULT_QUERY_LIMIT: usize = 5_000;
const MAX_QUERY_LIMIT: usize = 50_000;
/// A recorded request older than this is abandoned (a killed tab, a lost
/// worker): it stops fencing new attempts, is reported `expired` instead of
/// pending, and may no longer settle. The age is policy, so it is the kernel's.
use ds_command_kernel::project_dataset_cache::ACQUISITION_TICKET_TTL_MS;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn io_error(error: impl std::fmt::Display) -> String {
    format!("project dataset room could not be read or persisted: {error}")
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn digest_key(value: &str) -> Result<&str, String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(value);
    }
    Err("project dataset room key must be one lowercase 64-character digest".into())
}

fn directory(root: &Path, key: &str) -> Result<PathBuf, String> {
    digest_key(key)?;
    let mut current = root.to_path_buf();
    fs::create_dir_all(&current).map_err(io_error)?;
    for segment in [ROOT, VERSION, key] {
        current.push(segment);
        if !current.exists() {
            match fs::create_dir(&current) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(io_error(error)),
            }
        }
        let metadata = fs::symlink_metadata(&current).map_err(io_error)?;
        if linked(&metadata) || !metadata.is_dir() {
            return Err("linked or invalid project dataset room directory".into());
        }
    }
    Ok(current)
}

fn room_lock(root: &Path, key: &str) -> Result<fs::File, String> {
    digest_key(key)?;
    fs::create_dir_all(root).map_err(io_error)?;
    let path = root.join(format!("{ROOT}-{key}.lock"));
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if linked(&metadata) || !metadata.is_file() {
            return Err("invalid project dataset room lock".into());
        }
    }
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(io_error)
}

fn open_room(dir: &Path) -> Result<Connection, String> {
    // Windows path lengths, not a preference: a room lives under a storage root
    // the operator may relocate, and SQLite does not reach past 260 characters
    // the way `std::fs` silently does. See `crate::sqlite_path`.
    let connection =
        Connection::open(crate::sqlite_path(&dir.join(ROOM_FILE))?).map_err(io_error)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS metadata (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               schema TEXT NOT NULL,
               dataset_key TEXT NOT NULL,
               principal TEXT NOT NULL,
               project TEXT NOT NULL,
               dataset_id TEXT NOT NULL,
               provider TEXT NOT NULL,
               quality TEXT NOT NULL,
               parameters TEXT NOT NULL,
               crs TEXT NOT NULL,
               source_version TEXT NOT NULL,
               available_version TEXT NOT NULL DEFAULT '',
               buffer_policy TEXT NOT NULL,
               requested TEXT NOT NULL,
               completed TEXT NOT NULL,
               feature_count INTEGER NOT NULL,
               generation INTEGER NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL,
               last_error TEXT
             );
             CREATE TABLE IF NOT EXISTS queries (
               query_digest TEXT PRIMARY KEY,
               area TEXT NOT NULL,
               cells TEXT NOT NULL,
               source_version TEXT NOT NULL,
               status TEXT NOT NULL,
               requested_at INTEGER NOT NULL,
               settled_at INTEGER,
               feature_count INTEGER NOT NULL,
               message TEXT
             );
             CREATE TABLE IF NOT EXISTS features (
               id INTEGER PRIMARY KEY,
               feature_key TEXT NOT NULL UNIQUE,
               feature_json TEXT NOT NULL
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS feature_rtree USING rtree(
               id, min_x, max_x, min_y, max_y
             );",
        )
        .map_err(io_error)?;
    for (column, declaration) in [
        ("ticket", "TEXT NOT NULL DEFAULT ''"),
        ("retained_cells", "TEXT NOT NULL DEFAULT '{\"cells\":[]}'"),
        ("result_source_version", "TEXT NOT NULL DEFAULT ''"),
        ("source_receipt", "TEXT NOT NULL DEFAULT '{}'"),
        // Whether this acquisition was an explicit refresh of coverage the
        // room already held, rather than a first acquisition of it.
        ("refresh", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(queries)")
            .map_err(io_error)?
            .query_map([], |row| row.get(1))
            .map_err(io_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(io_error)?;
        if !columns.iter().any(|name| name == column) {
            connection
                .execute(
                    &format!("ALTER TABLE queries ADD COLUMN {column} {declaration}"),
                    [],
                )
                .map_err(io_error)?;
            if column == "retained_cells" {
                connection
                    .execute(
                        "UPDATE queries SET retained_cells = cells WHERE status = 'completed'",
                        [],
                    )
                    .map_err(io_error)?;
            }
            if column == "result_source_version" {
                connection.execute("UPDATE queries SET result_source_version = source_version WHERE status = 'completed'", []).map_err(io_error)?;
            }
        }
    }
    // The newest source version the provider has been observed publishing.
    // Deliberately separate from `source_version`, which describes the rows
    // actually held: the difference between the two IS the staleness report,
    // so folding them together would erase the only evidence of it.
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(metadata)")
        .map_err(io_error)?
        .query_map([], |row| row.get(1))
        .map_err(io_error)?
        .collect::<rusqlite::Result<_>>()
        .map_err(io_error)?;
    if !columns.iter().any(|name| name == "available_version") {
        connection
            .execute(
                "ALTER TABLE metadata ADD COLUMN available_version TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(io_error)?;
        connection
            .execute("UPDATE metadata SET available_version = source_version", [])
            .map_err(io_error)?;
    }
    Ok(connection)
}

struct Metadata {
    principal: String,
    project: String,
    dataset_id: String,
    source_version: String,
    /// Newest version the provider has been observed publishing for this room.
    available_version: String,
    requested: policy::Coverage,
    completed: policy::Coverage,
    feature_count: i64,
    generation: i64,
    created_at: i64,
    updated_at: i64,
    last_error: Option<String>,
    quality: String,
    provider: String,
    parameters: Value,
    crs: String,
    buffer_policy: Value,
}

fn coverage_of(raw: &str) -> Result<policy::Coverage, String> {
    serde_json::from_str(raw).map_err(|_| "project dataset room coverage is unreadable".into())
}

fn read_metadata(connection: &Connection) -> Result<Option<Metadata>, String> {
    connection
        .query_row(
            "SELECT principal, project, dataset_id, source_version, requested, completed,
                    feature_count, generation, created_at, updated_at, last_error,
                    quality, provider, parameters, crs, buffer_policy, available_version
             FROM metadata WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, String>(16)?,
                ))
            },
        )
        .optional()
        .map_err(io_error)?
        .map(|row| {
            Ok(Metadata {
                principal: row.0,
                project: row.1,
                dataset_id: row.2,
                source_version: row.3,
                requested: coverage_of(&row.4)?,
                completed: coverage_of(&row.5)?,
                feature_count: row.6,
                generation: row.7,
                created_at: row.8,
                updated_at: row.9,
                last_error: row.10,
                quality: row.11,
                provider: row.12,
                parameters: serde_json::from_str(&row.13)
                    .map_err(|_| "project dataset room parameters are unreadable")?,
                crs: row.14,
                buffer_policy: serde_json::from_str(&row.15)
                    .map_err(|_| "project dataset room buffer policy is unreadable")?,
                available_version: row.16,
            })
        })
        .transpose()
}

/// Readiness of the spatial index, reported rather than assumed.
///
/// A room with rows but no envelopes is not "ready with zero results" — it is
/// a room whose index does not answer, and saying so is the difference between
/// a truthful empty map and a silent one.
fn index_state(connection: &Connection, features: i64) -> Result<&'static str, String> {
    let indexed: i64 = connection
        .query_row("SELECT count(*) FROM feature_rtree", [], |row| row.get(0))
        .map_err(io_error)?;
    Ok(match (features, indexed) {
        (0, _) => "empty",
        (rows, entries) if entries >= rows => "ready",
        _ => "incomplete",
    })
}

/// Provenance of everything this room still holds: one entry per completed
/// acquisition, carrying the coverage it still accounts for and the source
/// version it landed under. Later acquisitions subtract from earlier ones, so
/// these coverages partition completed coverage rather than overlapping it.
fn held_versions(connection: &Connection) -> Result<Vec<policy::HeldVersion>, String> {
    let mut statement = connection
        .prepare(
            "SELECT result_source_version, retained_cells FROM queries
             WHERE status = 'completed' ORDER BY query_digest",
        )
        .map_err(io_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(io_error)?;
    let mut held = Vec::new();
    for row in rows {
        let (source_version, raw) = row.map_err(io_error)?;
        let coverage = coverage_of(&raw)?;
        if coverage.is_empty() {
            continue;
        }
        held.push(policy::HeldVersion {
            source_version,
            coverage,
        });
    }
    Ok(held)
}

/// Which held coverage was completed under a superseded source version.
///
/// The whole answer is the kernel's; this only gathers the rows. Nothing here
/// acts on the result: a stale room keeps every feature and answers every query
/// it could answer before, and only an explicit refresh re-acquires anything.
fn freshness_of(connection: &Connection, metadata: &Metadata) -> Result<policy::Freshness, String> {
    policy::freshness(&metadata.available_version, &held_versions(connection)?)
}

fn status_value(
    key: &str,
    metadata: Option<&Metadata>,
    connection: Option<&Connection>,
) -> Result<Value, String> {
    let Some(metadata) = metadata else {
        return Ok(json!({
            "schema": policy::SCHEMA,
            "dataset_key": key,
            "exists": false,
            "ready": false,
            "ready_reason": policy::READY_REASON_NOT_SEEDED,
            "feature_count": 0,
            "requested": policy::Coverage::default(),
            "completed": policy::Coverage::default(),
            "index_state": "absent",
            // A room that holds nothing holds nothing obsolete. Saying so
            // keeps the freshness projection total, so no reader has to guess
            // what an absent room's freshness means.
            "available_version": "",
            "source_versions": [],
            "fresh": true,
            "stale": policy::Coverage::default(),
            "stale_geometry": Value::Null,
            "stale_count": 0,
            "unattributed": policy::Coverage::default(),
            "pending_queries": 0,
            "expired_queries": 0,
            "queries": [],
        }));
    };
    let (index, queries) = match connection {
        Some(connection) => (
            index_state(connection, metadata.feature_count)?,
            query_rows(connection)?,
        ),
        None => ("absent", Vec::new()),
    };
    // An abandoned request stops counting as pending at a bounded age, or a
    // killed tab would leave this room reporting work in progress for ever.
    let horizon = now_ms() - ACQUISITION_TICKET_TTL_MS;
    let (pending, expired): (i64, i64) = match connection {
        Some(db) => (
            db.query_row(
                "SELECT count(*) FROM queries WHERE status = 'requested' AND requested_at > ?1",
                params![horizon],
                |row| row.get(0),
            )
            .map_err(io_error)?,
            db.query_row(
                "SELECT count(*) FROM queries WHERE status = 'requested' AND requested_at <= ?1",
                params![horizon],
                |row| row.get(0),
            )
            .map_err(io_error)?,
        ),
        None => (0, 0),
    };
    let fresh = match connection {
        Some(db) => Some(freshness_of(db, metadata)?),
        None => None,
    };
    // Ready means: this dataset holds a usable index and nothing is
    // mid-flight. It says nothing about any other dataset. The kernel decides
    // it once and names the first failing condition.
    let readiness = policy::readiness(
        index,
        pending,
        &metadata.completed,
        &metadata.requested,
        metadata.last_error.as_deref(),
    )?;
    Ok(json!({
        "schema": policy::SCHEMA,
        "dataset_key": key,
        "exists": true,
        "ready": readiness.ready,
        "ready_reason": readiness.ready_reason,
        "principal": metadata.principal,
        "project": metadata.project,
        "dataset_id": metadata.dataset_id,
        "provider": metadata.provider,
        "quality": metadata.quality,
        "parameters": metadata.parameters,
        "crs": metadata.crs,
        "source_version": metadata.source_version,
        // Freshness is a reported property of held rows, never a gate on them:
        // `ready` stays a statement about coverage and the index, and a room
        // that has gone stale still answers every read it answered before.
        "available_version": metadata.available_version,
        "source_versions": fresh.as_ref().map(|report| report.versions.clone()).unwrap_or_default(),
        "fresh": fresh.as_ref().is_none_or(|report| report.fresh),
        "stale": fresh.as_ref().map(|report| serde_json::to_value(&report.stale)).transpose().map_err(io_error)?
            .unwrap_or_else(|| json!(policy::Coverage::default())),
        "stale_geometry": fresh.as_ref().and_then(|report| report.stale_geometry.clone()),
        "stale_count": fresh.as_ref().map(|report| report.stale_count).unwrap_or(0),
        "unattributed": fresh.as_ref().map(|report| serde_json::to_value(&report.unattributed)).transpose().map_err(io_error)?
            .unwrap_or_else(|| json!(policy::Coverage::default())),
        "buffer_policy": metadata.buffer_policy,
        "requested": metadata.requested,
        "requested_geometry": metadata.requested.geometry(),
        "completed": metadata.completed,
        "completed_geometry": metadata.completed.geometry(),
        "feature_count": metadata.feature_count,
        "generation": metadata.generation,
        "created_at": metadata.created_at,
        "updated_at": metadata.updated_at,
        "last_error": metadata.last_error,
        "index_state": index,
        "pending_queries": pending,
        "expired_queries": expired,
        "queries": queries,
    }))
}

/// Every recorded acquisition, with the state it is actually in.
///
/// `status` is what is stored; `state` is what the kernel says that means now,
/// which is how an abandoned request stops presenting itself as pending work.
fn query_rows(connection: &Connection) -> Result<Vec<Value>, String> {
    let now = now_ms();
    let mut statement = connection
        .prepare(
            "SELECT query_digest, area, cells, source_version, status, requested_at,
                    settled_at, feature_count, message, result_source_version, source_receipt,
                    retained_cells, refresh
             FROM queries ORDER BY requested_at DESC, query_digest LIMIT 200",
        )
        .map_err(io_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                json!({
                    "query_digest": row.get::<_, String>(0)?,
                    "area": serde_json::from_str::<Value>(&row.get::<_, String>(1)?).unwrap_or(Value::Null),
                    "cells": serde_json::from_str::<Value>(&row.get::<_, String>(2)?).unwrap_or(Value::Null),
                    "source_version": row.get::<_, String>(3)?,
                    "status": row.get::<_, String>(4)?,
                    "requested_at": row.get::<_, i64>(5)?,
                    "settled_at": row.get::<_, Option<i64>>(6)?,
                    "feature_count": row.get::<_, i64>(7)?,
                    "message": row.get::<_, Option<String>>(8)?,
                    "result_source_version": row.get::<_, String>(9)?,
                    "source_receipt": serde_json::from_str::<Value>(&row.get::<_, String>(10)?).unwrap_or(Value::Null),
                    "retained_cells": serde_json::from_str::<Value>(&row.get::<_, String>(11)?).unwrap_or(Value::Null),
                    "refresh": row.get::<_, i64>(12)? != 0,
                }),
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(io_error)?;
    let mut out = Vec::new();
    for row in rows {
        let (mut value, status, requested_at) = row.map_err(io_error)?;
        value["state"] = json!(policy::acquisition_state(&status, requested_at, now)?);
        out.push(value);
    }
    Ok(out)
}

fn text(request: &Value, field: &str) -> Result<String, String> {
    request[field]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{field} is required"))
}

fn strict_coverage(request: &Value, field: &str) -> Result<policy::Coverage, String> {
    let coverage: policy::Coverage = serde_json::from_value(
        request
            .get(field)
            .cloned()
            .ok_or_else(|| format!("{field} is required"))?,
    )
    .map_err(|_| format!("{field} is unreadable"))?;
    coverage.rects()?;
    Ok(coverage)
}

fn identity_projection(
    metadata: Option<&Metadata>,
    request: &Value,
    coverage: &policy::Coverage,
    source_version: &str,
) -> Result<Value, String> {
    let (principal, project, dataset_id, provider, quality, parameters, crs, buffer_policy) =
        match metadata {
            Some(metadata) => (
                metadata.principal.clone(),
                metadata.project.clone(),
                metadata.dataset_id.clone(),
                metadata.provider.clone(),
                metadata.quality.clone(),
                metadata.parameters.clone(),
                metadata.crs.clone(),
                metadata.buffer_policy.clone(),
            ),
            None => (
                text(request, "principal")?,
                text(request, "project")?,
                text(request, "dataset_id")?,
                text(request, "provider")?,
                text(request, "quality")?,
                request
                    .get("parameters")
                    .cloned()
                    .ok_or("parameters is required")?,
                text(request, "crs")?,
                request
                    .get("buffer_policy")
                    .cloned()
                    .ok_or("buffer_policy is required")?,
            ),
        };
    let command = json!({
        "schema": policy::SCHEMA,
        "action": "identity",
        "scope": { "principal": principal, "project": project },
        "dataset": {
            "id": dataset_id,
            "provider": provider,
            "quality": quality,
            "parameters": parameters,
            "source_version": source_version,
            "crs": crs,
        },
        "policy": buffer_policy,
        "coverage": coverage,
    });
    let encoded = serde_json::to_vec(&command).map_err(io_error)?;
    let result = policy::evaluate(&encoded)?;
    serde_json::from_str(&result).map_err(io_error)
}

fn bind_open_identity(key: &str, request: &Value) -> Result<(), String> {
    let projection = identity_projection(
        None,
        request,
        &policy::Coverage::default(),
        request["source_version"].as_str().unwrap_or(""),
    )?;
    if projection["dataset_key"] != key {
        return Err("project dataset room key does not match its canonical identity".into());
    }
    Ok(())
}

fn bind_existing_identity(metadata: &Metadata, request: &Value) -> Result<(), String> {
    if metadata.dataset_id != text(request, "dataset_id")?
        || metadata.provider != text(request, "provider")?
        || metadata.quality != text(request, "quality")?
        || metadata.parameters
            != request
                .get("parameters")
                .cloned()
                .ok_or("parameters is required")?
        || metadata.crs != text(request, "crs")?
    {
        return Err("this room already belongs to a different canonical dataset".into());
    }
    Ok(())
}

fn bind_stored_room(connection: &Connection, key: &str) -> Result<(), String> {
    let stored: Option<(String, String)> = connection
        .query_row(
            "SELECT schema, dataset_key FROM metadata WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(io_error)?;
    if let Some((schema, dataset_key)) = stored {
        if schema != policy::SCHEMA || dataset_key != key {
            return Err("project dataset room identity is unreadable".into());
        }
    }
    Ok(())
}

fn area_matches_cell(area: &Value, cell: [f64; 4]) -> bool {
    let Some(ring) = area
        .get("coordinates")
        .and_then(Value::as_array)
        .and_then(|rings| rings.first())
        .and_then(Value::as_array)
    else {
        return false;
    };
    if area["type"] != "Polygon" || ring.len() != 5 {
        return false;
    }
    let [w, s, e, n] = cell;
    let expected = [[w, s], [e, s], [e, n], [w, n], [w, s]];
    ring.iter().zip(expected).all(|(position, expected)| {
        position.as_array().is_some_and(|coordinates| {
            coordinates.len() >= 2
                && coordinates[0].as_f64() == Some(expected[0])
                && coordinates[1].as_f64() == Some(expected[1])
        })
    })
}

/// Re-assert who this room belongs to before writing to it.
///
/// The path already folds principal and project into its digest, so this
/// catches the case that digest cannot: a caller handing this room a result it
/// computed for a different one.
fn fenced<'a>(metadata: &'a Metadata, request: &Value) -> Result<&'a Metadata, String> {
    let principal = text(request, "principal")?;
    let project = text(request, "project")?;
    if metadata.principal != principal || metadata.project != project {
        return Err("this result belongs to a different account or project".into());
    }
    Ok(metadata)
}

pub fn default_root() -> Result<PathBuf, String> {
    crate::reference_cache::configured_root()
}

pub fn execute(request: Value) -> Result<Value, String> {
    execute_at(&default_root()?, request)
}

pub fn execute_at(root: &Path, request: Value) -> Result<Value, String> {
    let key = text(&request, "dataset_key")?;
    digest_key(&key)?;
    let action = request["action"].as_str().unwrap_or("").to_owned();
    let lock = room_lock(root, &key)?;
    let wait = action == "begin" && request["wait_for_pending"].as_bool() == Some(true);
    let mut pause_ms = 25;
    loop {
        if matches!(action.as_str(), "status" | "query" | "freshness") {
            FileExt::lock_shared(&lock).map_err(io_error)?;
        } else {
            lock.lock_exclusive().map_err(io_error)?;
        }
        let result = execute_locked(root, &key, &action, &request);
        let _ = FileExt::unlock(&lock);
        if !wait || !matches!(&result, Ok(value) if value["reason"] == "in_flight") {
            return result;
        }
        // A3/A0 viewports overlap without having identical query digests.
        // Wait outside the lock so the owning process can commit or fail.
        // Re-entering begin rechecks coverage, identity and the kernel's lease
        // expiry; an abandoned worker cannot strand this waiter. All reads
        // stay in this local room, never the provider or Firestore.
        std::thread::sleep(std::time::Duration::from_millis(pause_ms));
        pause_ms = (pause_ms * 2).min(1000);
    }
}

fn execute_locked(root: &Path, key: &str, action: &str, request: &Value) -> Result<Value, String> {
    // Validate the caller's semantic address before a bad open can create even
    // an empty digest-named directory.
    if action == "open" {
        bind_open_identity(key, request)?;
    }
    // A read of a room that was never opened must not create one on disk.
    if matches!(action, "status" | "query" | "freshness") {
        let dir = root.join(ROOT).join(VERSION).join(key);
        if !dir.join(ROOM_FILE).is_file() {
            return match action {
                "status" => status_value(key, None, None),
                _ => Err("this project dataset has not been seeded yet".into()),
            };
        }
    }
    let dir = directory(root, key)?;
    let mut connection = open_room(&dir)?;
    bind_stored_room(&connection, key)?;
    match action {
        "open" => {
            let metadata = read_metadata(&connection)?;
            if let Some(held) = &metadata {
                fenced(held, request)?;
                bind_existing_identity(held, request)?;
                connection
                    .execute(
                        "UPDATE metadata SET buffer_policy = ?1 WHERE id = 1",
                        params![request["buffer_policy"].to_string()],
                    )
                    .map_err(io_error)?;
                // A caller that knows the provider has re-published says so
                // here. It records what is AVAILABLE and never touches
                // `source_version`, which describes the rows actually held —
                // relabelling those would erase the staleness it just declared.
                if let Some(available) = request["source_version"]
                    .as_str()
                    .filter(|version| !version.is_empty())
                {
                    if available.len() > 512 {
                        return Err("dataset source version is too long".into());
                    }
                    connection
                        .execute(
                            "UPDATE metadata SET available_version = ?1 WHERE id = 1",
                            params![available],
                        )
                        .map_err(io_error)?;
                }
            } else {
                let stamp = now_ms();
                connection
                    .execute(
                        "INSERT INTO metadata (id, schema, dataset_key, principal, project, dataset_id,
                            provider, quality, parameters, crs, source_version, buffer_policy,
                            requested, completed, feature_count, generation, created_at, updated_at, last_error,
                            available_version)
                         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, 0, ?14, ?14, NULL, ?10)",
                        params![
                            policy::SCHEMA,
                            key,
                            text(request, "principal")?,
                            text(request, "project")?,
                            text(request, "dataset_id")?,
                            text(request, "provider")?,
                            text(request, "quality")?,
                            request["parameters"].to_string(),
                            request["crs"].as_str().unwrap_or("EPSG:4326"),
                            request["source_version"].as_str().unwrap_or(""),
                            request["buffer_policy"].to_string(),
                            serde_json::to_string(&policy::Coverage::default()).map_err(io_error)?,
                            serde_json::to_string(&policy::Coverage::default()).map_err(io_error)?,
                            stamp
                        ],
                    )
                    .map_err(io_error)?;
            }
            let metadata = read_metadata(&connection)?;
            status_value(key, metadata.as_ref(), Some(&connection))
        }
        "status" => {
            let metadata = read_metadata(&connection)?;
            if let Some(metadata) = &metadata {
                fenced(metadata, request)?;
            }
            status_value(key, metadata.as_ref(), Some(&connection))
        }
        "begin" => {
            let metadata =
                read_metadata(&connection)?.ok_or("this project dataset has not been opened")?;
            fenced(&metadata, request)?;
            let digest = text(request, "query_digest")?;
            digest_key(&digest)?;
            let area = request
                .get("area")
                .filter(|value| !value.is_null())
                .ok_or("an acquisition needs the exact area it will request")?;
            validate_area(area)?;
            let cells = strict_coverage(request, "cells")?;
            if cells.cells.len() != 1 || !area_matches_cell(area, cells.cells[0]) {
                return Err(
                    "an acquisition area must exactly match its one requested coverage cell".into(),
                );
            }
            let source_version = request["source_version"].as_str().unwrap_or("");
            let identity = identity_projection(Some(&metadata), request, &cells, source_version)?;
            if identity["query_digest"] != digest {
                return Err(
                    "query digest does not match this acquisition's canonical identity".into(),
                );
            }
            let refresh = request["refresh"].as_bool().unwrap_or(false);
            if !refresh && policy::assess(&metadata.completed, &cells)?.covered {
                // Held coverage answers this — but coverage alone must not
                // reuse rows the provider has since replaced. The refusal
                // carries that verdict so a caller reuses knowingly, and can
                // re-ask with `refresh` instead of assuming what it holds is
                // current.
                let report = freshness_of(&connection, &metadata)?;
                let stale_here = !report.stale.is_empty()
                    && policy::assess(&report.stale, &cells)?.gaps != cells;
                return Ok(
                    json!({"schema":policy::SCHEMA,"accepted":false,"reason":"already_completed",
                    "query_digest":digest,"generation":metadata.generation,
                    "stale":stale_here,"available_version":metadata.available_version,
                    "stale_coverage":report.stale}),
                );
            }
            let stamp = now_ms();
            let pending: Vec<(String, String)> = connection.prepare(
                "SELECT query_digest, cells FROM queries WHERE status = 'requested' AND requested_at > ?1"
            ).map_err(io_error)?.query_map(params![stamp-ACQUISITION_TICKET_TTL_MS], |row| Ok((row.get(0)?,row.get(1)?)))
                .map_err(io_error)?.collect::<rusqlite::Result<_>>().map_err(io_error)?;
            for (_, pending_cells) in pending {
                let pending: policy::Coverage =
                    serde_json::from_str(&pending_cells).map_err(io_error)?;
                if cells.cells.iter().any(|a| {
                    pending
                        .cells
                        .iter()
                        .any(|b| a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1])
                }) {
                    return Ok(
                        json!({"schema":policy::SCHEMA,"accepted":false,"reason":"in_flight",
                        "query_digest":digest,"generation":metadata.generation}),
                    );
                }
            }
            let ticket: String = connection
                .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
                .map_err(io_error)?;
            let transaction = connection.transaction().map_err(io_error)?;
            transaction
                .execute(
                    "INSERT INTO queries (query_digest, area, cells, source_version, status,
                        requested_at, settled_at, feature_count, message, ticket, refresh)
                     VALUES (?1, ?2, ?3, ?4, 'requested', ?5, NULL, 0, NULL, ?6, ?7)
                     ON CONFLICT(query_digest) DO UPDATE SET status = 'requested',
                        requested_at = ?5, settled_at = NULL, message = NULL, ticket = ?6,
                        refresh = ?7",
                    params![
                        digest,
                        area.to_string(),
                        serde_json::to_string(&cells).map_err(io_error)?,
                        source_version,
                        stamp,
                        ticket,
                        i64::from(refresh)
                    ],
                )
                .map_err(io_error)?;
            // Requested coverage grows now; completed coverage does not. An
            // attempt is recorded as an attempt.
            let requested = policy::extend(&metadata.requested, &cells)?;
            transaction
                .execute(
                    "UPDATE metadata SET requested = ?1, updated_at = ?2 WHERE id = 1",
                    params![serde_json::to_string(&requested).map_err(io_error)?, stamp],
                )
                .map_err(io_error)?;
            transaction.commit().map_err(io_error)?;
            Ok(
                json!({"schema": policy::SCHEMA, "accepted": true, "query_digest": digest,
                "generation": metadata.generation, "ticket":ticket, "refresh":refresh,
                "expires_at": stamp + ACQUISITION_TICKET_TTL_MS}),
            )
        }
        "commit" => {
            let metadata =
                read_metadata(&connection)?.ok_or("this project dataset has not been opened")?;
            fenced(&metadata, request)?;
            let digest = text(request, "query_digest")?;
            digest_key(&digest)?;
            let features = request["features"]
                .as_array()
                .ok_or("a completed acquisition carries a feature array")?;
            if features.len() > MAX_QUERY_FEATURES {
                return Err("one acquisition returned more features than a room accepts".into());
            }
            let completed = strict_coverage(request, "cells")?;
            if completed.is_empty() {
                return Err("a completed acquisition must name the coverage it completed".into());
            }
            // Provenance is bounded before anything is written, and refused
            // rather than trimmed: a receipt cut down to fit would sit beside
            // the rows claiming to account for them.
            let receipt = request.get("source_receipt").cloned().unwrap_or(json!({}));
            policy::validate_source_receipt(&receipt)?;
            let begun: Option<(String, String, String, i64)> = connection
                .query_row(
                    "SELECT status, cells, ticket, requested_at FROM queries WHERE query_digest = ?1",
                    params![digest],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(io_error)?;
            let Some((state, begun_cells, ticket, requested_at)) = begun else {
                return Err("a completion has no matching begun acquisition".into());
            };
            if state != "requested" {
                return Err("only a requested acquisition may be completed".into());
            }
            if ticket.is_empty() || request["ticket"].as_str() != Some(ticket.as_str()) {
                return Err("completion does not own the current acquisition ticket".into());
            }
            // The room already reported this attempt abandoned and released its
            // area for re-acquisition, so a newer ticket may be holding it.
            // Landing an answer the room has written off would make `expired` a
            // lie and could overwrite rows a fresher acquisition just placed.
            if policy::acquisition_expired(requested_at, now_ms()) {
                return Err(
                    "this acquisition ticket expired and its area was released; begin the acquisition again"
                        .into(),
                );
            }
            let begun_coverage: policy::Coverage = serde_json::from_str(&begun_cells)
                .map_err(|_| "the begun acquisition coverage is unreadable")?;
            if begun_coverage != completed {
                return Err("completed coverage differs from the begun acquisition".into());
            }
            let stamp = now_ms();
            let version = request["source_version"].as_str().unwrap_or("").to_owned();
            if version.len() > 512 {
                return Err("dataset source version is too long".into());
            }
            let committed_version = version.clone();
            let coverage = policy::extend(&metadata.completed, &completed)?;
            let transaction = connection.transaction().map_err(io_error)?;
            let area = completed.geometry().ok_or("completed area is empty")?;
            // ONE predicate, named by the kernel from the provider's own
            // selection rule, for both halves of a replacement: the rows this
            // acquisition removes and the rows it is allowed to put back. If
            // deletion swept wider than the provider fetches, a footprint
            // crossing the edge of this area would be deleted and never
            // returned — the building would simply disappear from the room.
            let dataset_id = metadata.dataset_id.clone();
            let provider = metadata.provider.clone();
            policy::acquisition_selection(&dataset_id, &provider)?;
            let mut deletions = Vec::new();
            {
                let bounds = ds_command_kernel::reference_cache::area_bounds(&area)?;
                let mut candidates = transaction.prepare("SELECT f.id, f.feature_json FROM feature_rtree r JOIN features f ON f.id = r.id WHERE r.min_x <= ?1 AND r.max_x >= ?2 AND r.min_y <= ?3 AND r.max_y >= ?4").map_err(io_error)?;
                let mut rows = candidates
                    .query(params![bounds[2], bounds[0], bounds[3], bounds[1]])
                    .map_err(io_error)?;
                while let Some(row) = rows.next().map_err(io_error)? {
                    let feature: Value =
                        serde_json::from_str(&row.get::<_, String>(1).map_err(io_error)?)
                            .map_err(io_error)?;
                    if policy::acquisition_selects(&dataset_id, &provider, &feature, &area)? {
                        deletions.push(row.get::<_, i64>(0).map_err(io_error)?);
                    }
                }
            }
            for id in &deletions {
                transaction
                    .execute("DELETE FROM feature_rtree WHERE id = ?1", params![id])
                    .map_err(io_error)?;
                transaction
                    .execute("DELETE FROM features WHERE id = ?1", params![id])
                    .map_err(io_error)?;
            }
            let mut next_id: i64 = transaction
                .query_row(
                    "SELECT coalesce(max(id), -1) + 1 FROM features",
                    [],
                    |row| row.get(0),
                )
                .map_err(io_error)?;
            for feature in features {
                if feature["type"] != "Feature"
                    || !feature["properties"].is_object()
                    || !feature["geometry"].is_object()
                {
                    return Err(
                        "an acquisition returned something that is not a GeoJSON feature".into(),
                    );
                }
                if !policy::acquisition_selects(&dataset_id, &provider, feature, &area)?
                    || feature_bounds(feature).is_none()
                {
                    return Err(
                        "acquired feature is unindexable or outside the completed area".into(),
                    );
                }
                let key = feature_key(feature);
                // Overlapping acquisitions are normal; counting the same
                // building twice is not.
                let id: i64 = transaction.query_row(
                    "INSERT INTO features (id, feature_key, feature_json) VALUES (?1, ?2, ?3)
                     ON CONFLICT(feature_key) DO UPDATE SET feature_json = excluded.feature_json RETURNING id",
                    params![next_id,key,feature.to_string()], |row| row.get(0),
                ).map_err(io_error)?;
                let bounds = feature_bounds(feature).ok_or("acquired feature cannot be indexed")?;
                transaction.execute(
                    "INSERT OR REPLACE INTO feature_rtree (id, min_x, max_x, min_y, max_y) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id,bounds[0],bounds[2],bounds[1],bounds[3]],
                ).map_err(io_error)?;
                if id == next_id {
                    next_id += 1;
                }
            }
            let total: i64 = transaction
                .query_row("SELECT count(*) FROM features", [], |row| row.get(0))
                .map_err(io_error)?;
            if total > MAX_ROOM_FEATURES {
                return Err("this project dataset room exceeds its feature limit".into());
            }
            let prior: Vec<(String, String)> = transaction
                .prepare(
                    "SELECT query_digest, retained_cells FROM queries WHERE query_digest != ?1",
                )
                .map_err(io_error)?
                .query_map(params![digest], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(io_error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(io_error)?;
            for (prior_digest, raw) in prior {
                let old: policy::Coverage = serde_json::from_str(&raw).map_err(io_error)?;
                let difference = policy::assess(&completed, &old)?;
                if difference.simplified {
                    return Err("retained provenance cannot be approximated".into());
                }
                transaction
                    .execute(
                        "UPDATE queries SET retained_cells = ?2 WHERE query_digest = ?1",
                        params![
                            prior_digest,
                            serde_json::to_string(&difference.gaps).map_err(io_error)?
                        ],
                    )
                    .map_err(io_error)?;
            }
            transaction
                .execute(
                    "UPDATE queries SET status = 'completed', settled_at = ?2, feature_count = ?3,
                        message = NULL, result_source_version = ?4, retained_cells = ?5, source_receipt = ?6 WHERE query_digest = ?1",
                    params![digest, stamp, features.len() as i64, version, serde_json::to_string(&completed).map_err(io_error)?, receipt.to_string()],
                )
                .map_err(io_error)?;
            let versions: Vec<(String, String)> = transaction
                .prepare("SELECT result_source_version, retained_cells FROM queries")
                .map_err(io_error)?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(io_error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(io_error)?;
            let mut held_versions = std::collections::BTreeSet::new();
            for (value, raw) in versions {
                let cells: policy::Coverage = serde_json::from_str(&raw).map_err(io_error)?;
                if !cells.is_empty() {
                    held_versions.insert(value);
                }
            }
            let version = if held_versions.len() == 1 {
                held_versions.into_iter().next().unwrap()
            } else {
                String::new()
            };
            // What the provider just answered with is the newest publication
            // this room has seen. Recording it is what makes every OTHER
            // holding's older version visible as staleness; it is last-writer
            // by design, because opaque version strings carry no order.
            let available = if committed_version.is_empty() {
                metadata.available_version.clone()
            } else {
                committed_version
            };
            transaction
                .execute(
                    "UPDATE metadata SET completed = ?1, feature_count = ?2, generation = ?3,
                        updated_at = ?4, last_error = NULL, source_version = ?5,
                        available_version = ?6 WHERE id = 1",
                    params![
                        serde_json::to_string(&coverage).map_err(io_error)?,
                        total,
                        metadata.generation + 1,
                        stamp,
                        version,
                        available
                    ],
                )
                .map_err(io_error)?;
            transaction.commit().map_err(io_error)?;
            let metadata = read_metadata(&connection)?;
            status_value(key, metadata.as_ref(), Some(&connection))
        }
        "fail" => {
            let metadata =
                read_metadata(&connection)?.ok_or("this project dataset has not been opened")?;
            fenced(&metadata, request)?;
            let digest = text(request, "query_digest")?;
            let message = request["message"].as_str().unwrap_or("acquisition failed");
            let stamp = now_ms();
            let transaction = connection.transaction().map_err(io_error)?;
            let changed = transaction
                .execute(
                    "UPDATE queries SET status = 'failed', settled_at = ?2, message = ?3
                     WHERE query_digest = ?1 AND status = 'requested' AND ticket = ?4
                       AND requested_at > ?5",
                    params![
                        digest,
                        stamp,
                        message,
                        text(request, "ticket")?,
                        stamp - ACQUISITION_TICKET_TTL_MS
                    ],
                )
                .map_err(io_error)?;
            if changed != 1 {
                // An expired attempt was already written off as abandoned;
                // recording a failure now would assert something went wrong
                // that the room has no evidence of.
                return Err(
                    "failure does not own a live requested acquisition ticket; it may have expired"
                        .into(),
                );
            }
            // Completed coverage, features and the index are untouched: a
            // failure must never make already-paid-for data disappear.
            transaction
                .execute(
                    "UPDATE metadata SET last_error = ?1, updated_at = ?2 WHERE id = 1",
                    params![message, stamp],
                )
                .map_err(io_error)?;
            transaction.commit().map_err(io_error)?;
            let metadata = read_metadata(&connection)?;
            status_value(key, metadata.as_ref(), Some(&connection))
        }
        "query" => {
            let metadata =
                read_metadata(&connection)?.ok_or("this project dataset has not been opened")?;
            fenced(&metadata, request)?;
            let area = request
                .get("area")
                .filter(|value| !value.is_null())
                .ok_or("a dataset query needs an area")?;
            validate_area(area)?;
            let held = metadata
                .completed
                .geometry()
                .ok_or("dataset query area has not been acquired")?;
            if !ds_command_kernel::reference_cache::covers_area(&held, area)? {
                return Err("dataset query extends beyond completed coverage; ensure or seed this area first".into());
            }

            let after_id = match request.get("after_id") {
                None | Some(Value::Null) => -1,
                Some(value) => value
                    .as_i64()
                    .filter(|id| *id >= 0)
                    .ok_or("invalid dataset page cursor")?,
            };
            if after_id >= 0 && request["generation"].as_i64() != Some(metadata.generation) {
                return Err(
                    "dataset changed between pages; restart the read from its current generation"
                        .into(),
                );
            }
            let bounds = ds_command_kernel::reference_cache::area_bounds(area)?;
            let limit = request["limit"]
                .as_u64()
                .unwrap_or(DEFAULT_QUERY_LIMIT as u64)
                .clamp(1, MAX_QUERY_LIMIT as u64) as usize;
            let mut statement = connection
                .prepare(
                    "SELECT f.id, f.feature_json FROM feature_rtree r JOIN features f ON f.id = r.id
                     WHERE r.min_x <= ?1 AND r.max_x >= ?2 AND r.min_y <= ?3 AND r.max_y >= ?4 AND f.id > ?5
                     ORDER BY f.id",
                )
                .map_err(io_error)?;
            let mut rows = statement
                .query(params![
                    bounds[2], bounds[0], bounds[3], bounds[1], after_id
                ])
                .map_err(io_error)?;
            let mut features = Vec::new();
            let mut truncated = false;
            let mut last_id = after_id;
            while let Some(row) = rows.next().map_err(io_error)? {
                let id: i64 = row.get(0).map_err(io_error)?;
                let raw: String = row.get(1).map_err(io_error)?;
                let feature: Value = serde_json::from_str(&raw).map_err(io_error)?;
                if !intersects_area(&feature, area)? {
                    continue;
                }
                if features.len() >= limit {
                    truncated = true;
                    break;
                }
                last_id = id;
                features.push(feature);
            }
            Ok(json!({
                "schema": policy::SCHEMA,
                "dataset_key": key,
                "dataset_id": metadata.dataset_id,
                "source_version": metadata.source_version,
                "generation": metadata.generation,
                "features": features,
                "truncated": truncated,
                "next_after_id": if truncated { Some(last_id) } else { None },
                "cache": "project_room",
            }))
        }
        "freshness" => {
            // The explicit staleness read: which held coverage completed under
            // a source version the provider has since replaced. Free, and it
            // changes nothing — an obsolete room keeps every row it holds until
            // an explicit refresh replaces them.
            let metadata =
                read_metadata(&connection)?.ok_or("this project dataset has not been opened")?;
            fenced(&metadata, request)?;
            let report = freshness_of(&connection, &metadata)?;
            let mut value = serde_json::to_value(&report).map_err(io_error)?;
            value["dataset_key"] = json!(key);
            value["dataset_id"] = json!(metadata.dataset_id);
            value["source_version"] = json!(metadata.source_version);
            Ok(value)
        }
        "forget" => {
            let metadata = read_metadata(&connection)?.ok_or("dataset is absent")?;
            fenced(&metadata, request)?;
            drop(connection);
            fs::remove_dir_all(&dir).map_err(io_error)?;
            Ok(json!({"schema": policy::SCHEMA, "dataset_key": key, "removed": true}))
        }
        _ => Err("unsupported project dataset room operation".into()),
    }
}

/// Every room this computer holds **for one principal**.
///
/// The principal is required, not a filter applied afterwards. One computer can
/// hold rooms for several accounts, and a digest-named directory does not say
/// whose it is until it is opened — so a foreign room is dropped here, before
/// its coverage, feature counts or project name have been projected into an
/// answer at all. An account never learns that another account's room exists.
pub fn inventory(root: &Path, principal: &str) -> Result<Value, String> {
    if principal.trim().is_empty() {
        return Err("a project dataset inventory needs the principal it is for".into());
    }
    let base = root.join(ROOT).join(VERSION);
    let mut rooms: Vec<Value> = Vec::new();
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"schema": policy::SCHEMA, "rooms": []}))
        }
        Err(error) => return Err(io_error(error)),
    };
    let mut keys: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if digest_key(&name).is_ok() && entry.path().join(ROOM_FILE).is_file() {
            keys.push(name);
        }
    }
    keys.sort();
    for key in keys {
        let lock = room_lock(root, &key)?;
        FileExt::lock_shared(&lock).map_err(io_error)?;
        let connection = open_room(&directory(root, &key)?)?;
        bind_stored_room(&connection, &key)?;
        let metadata = read_metadata(&connection)?;
        let Some(held) = metadata else { continue };
        if held.principal != principal {
            continue;
        }
        rooms.push(status_value(&key, Some(&held), Some(&connection))?);
    }
    Ok(json!({"schema": policy::SCHEMA, "principal": principal, "rooms": rooms}))
}

/// Rooms belonging to one principal and project. Dataset rows stay separate:
/// this never folds one dataset's coverage into another's readiness.
pub fn project_inventory(root: &Path, principal: &str, project: &str) -> Result<Value, String> {
    if project.trim().is_empty() {
        return Err("a project dataset inventory needs the project it is for".into());
    }
    let all = inventory(root, principal)?;
    let rooms: Vec<Value> = all["rooms"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|room| room["project"] == project)
        .collect();
    let mut datasets = Map::new();
    for room in &rooms {
        if let Some(id) = room["dataset_id"].as_str() {
            datasets.insert(id.to_owned(), room.clone());
        }
    }
    Ok(json!({
        "schema": policy::SCHEMA,
        "principal": principal,
        "project": project,
        "datasets": datasets,
        "rooms": rooms,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::project_dataset_cache::{
        dataset_key, BufferPolicy, Coverage, Dataset, IsolatedRule, Quality, Scope,
    };
    use std::collections::BTreeMap;

    fn scope() -> Scope {
        Scope {
            principal: "uid-1".into(),
            project: "project-a".into(),
        }
    }

    fn buildings() -> Dataset {
        Dataset {
            id: "google_open_buildings".into(),
            provider: "ds-brain:query_print_context".into(),
            quality: Quality::Source,
            parameters: BTreeMap::from([("min_area_m2".into(), json!(25))]),
            source_version: String::new(),
            crs: "EPSG:4326".into(),
        }
    }

    fn policy_value() -> Value {
        serde_json::to_value(BufferPolicy {
            design_buffer_m: 500.0,
            isolated_buffer_m: 1_000.0,
            isolated_rule: IsolatedRule::PointOnly,
            provisional: true,
            sheets: Vec::new(),
        })
        .unwrap()
    }

    fn open(root: &Path, scope: &Scope, dataset: &Dataset) -> String {
        let key = dataset_key(scope, dataset);
        execute_at(
            root,
            json!({
                "action": "open", "dataset_key": key,
                "principal": scope.principal, "project": scope.project,
                "dataset_id": dataset.id, "provider": dataset.provider,
                "quality": "source", "parameters": dataset.parameters,
                "crs": dataset.crs, "source_version": dataset.source_version,
                "buffer_policy": policy_value(),
            }),
        )
        .unwrap();
        key
    }

    fn feature(lon: f64, lat: f64, name: &str) -> Value {
        json!({"type":"Feature","properties":{"name":name},
            "geometry":{"type":"Polygon","coordinates":[[
                [lon, lat], [lon + 0.0001, lat], [lon + 0.0001, lat + 0.0001],
                [lon, lat + 0.0001], [lon, lat]]]}})
    }

    fn area(w: f64, s: f64, e: f64, n: f64) -> Value {
        json!({"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]})
    }

    fn acquisition_digest_at(cells: [f64; 4], version: &str) -> String {
        let mut dataset = buildings();
        dataset.source_version = version.into();
        let policy: BufferPolicy = serde_json::from_value(policy_value()).unwrap();
        ds_command_kernel::project_dataset_cache::query_digest(
            &scope(),
            &dataset,
            &policy,
            &Coverage { cells: vec![cells] },
        )
    }

    fn acquisition_digest(cells: [f64; 4]) -> String {
        acquisition_digest_at(cells, "v1")
    }

    /// One acquisition at an explicit source version, optionally as a refresh
    /// of coverage the room already holds.
    fn acquire_at(
        root: &Path,
        key: &str,
        cells: [f64; 4],
        features: Vec<Value>,
        version: &str,
        refresh: bool,
    ) -> Value {
        let digest = acquisition_digest_at(cells, version);
        let [w, s, e, n] = cells;
        let begun = execute_at(
            root,
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(w,s,e,n),"cells":{"cells":[cells]},
                "source_version":version,"refresh":refresh}),
        )
        .unwrap();
        assert_eq!(begun["accepted"], true, "{begun}");
        execute_at(
            root,
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cells]},
                "features":features,"source_version":version}),
        )
        .unwrap()
    }

    fn status_of(root: &Path, key: &str) -> Value {
        execute_at(
            root,
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
        )
        .unwrap()
    }

    #[test]
    fn paginated_queries_are_complete_ordered_and_generation_pinned() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        acquire(
            root.path(),
            &key,
            [0., 0., 1., 1.],
            vec![
                feature(0.1, 0.1, "one"),
                feature(0.2, 0.2, "two"),
                feature(0.3, 0.3, "three"),
            ],
            0,
        );
        let mut request = json!({"action":"query","dataset_key":key,"principal":"uid-1","project":"project-a","area":area(0.,0.,1.,1.),"limit":1});
        let zero = execute_at(root.path(), request.clone()).unwrap();
        assert_eq!(zero["features"][0]["properties"]["name"], "one");
        assert_eq!(zero["next_after_id"], 0);
        request["limit"] = json!(2);
        let first = execute_at(root.path(), request.clone()).unwrap();
        assert_eq!(first["features"].as_array().unwrap().len(), 2);
        assert_eq!(first["truncated"], true);
        request["after_id"] = first["next_after_id"].clone();
        request["generation"] = first["generation"].clone();
        let second = execute_at(root.path(), request.clone()).unwrap();
        assert_eq!(second["features"].as_array().unwrap().len(), 1);
        assert_eq!(second["truncated"], false);
        assert!(second["next_after_id"].is_null());
        let names = first["features"]
            .as_array()
            .unwrap()
            .iter()
            .chain(second["features"].as_array().unwrap())
            .map(|f| f["properties"]["name"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), 3);
        request["generation"] = json!(0);
        assert!(execute_at(root.path(), request)
            .unwrap_err()
            .contains("between pages"));
    }

    /// Move every recorded request back in time, so expiry can be proved
    /// without a test that sleeps for half an hour.
    fn age_requests(root: &Path, key: &str, by_ms: i64) {
        let connection = Connection::open(directory(root, key).unwrap().join(ROOM_FILE)).unwrap();
        connection
            .execute(
                "UPDATE queries SET requested_at = requested_at - ?1",
                params![by_ms],
            )
            .unwrap();
    }

    fn acquire(
        root: &Path,
        key: &str,
        cells: [f64; 4],
        features: Vec<Value>,
        generation: i64,
    ) -> Value {
        let digest = acquisition_digest(cells);
        let [w, s, e, n] = cells;
        let begun = execute_at(
            root,
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(w,s,e,n),"cells":{"cells":[cells]},
                "source_version":"v1","expected_generation":generation}),
        )
        .unwrap();
        assert_eq!(begun["accepted"], true, "{begun}");
        execute_at(
            root,
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cells]},"features":features,
                "source_version":"v1","expected_generation":generation}),
        )
        .unwrap()
    }

    #[test]
    fn an_unseeded_dataset_reads_as_absent_and_creates_nothing() {
        let root = tempfile::tempdir().unwrap();
        let key = dataset_key(&scope(), &buildings());
        let status = execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
        )
        .unwrap();
        assert_eq!(status["exists"], false);
        assert_eq!(status["ready"], false);
        assert_eq!(status["index_state"], "absent");
        assert!(!root.path().join(ROOT).join(VERSION).join(&key).exists());
        // Querying an unseeded dataset is an explicit refusal, never an
        // empty answer that would read as "there are no buildings here".
        assert!(execute_at(
            root.path(),
            json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(0.,0.,1.,1.)})
        )
        .is_err());
    }

    #[test]
    fn opening_a_room_requires_its_canonical_dataset_identity() {
        let root = tempfile::tempdir().unwrap();
        let dataset = buildings();
        let key = dataset_key(&scope(), &dataset);
        let wrong = execute_at(
            root.path(),
            json!({
                "action": "open", "dataset_key": key,
                "principal": "uid-1", "project": "project-a",
                "dataset_id": "elevation_contours", "provider": dataset.provider,
                "quality": "source", "parameters": dataset.parameters,
                "crs": dataset.crs, "source_version": "v1",
                "buffer_policy": policy_value(),
            }),
        );
        assert!(
            wrong.unwrap_err().contains("canonical identity"),
            "a caller must not relabel a digest-addressed room"
        );
        assert!(!root
            .path()
            .join(ROOT)
            .join(VERSION)
            .join(&key)
            .join(ROOM_FILE)
            .exists());
    }

    #[test]
    fn begin_binds_the_digest_and_area_to_the_requested_coverage() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let cell = [29.9, -2.1, 30.1, -1.9];

        let wrong_digest = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":"a".repeat(64),"area":area(cell[0],cell[1],cell[2],cell[3]),
                "cells":{"cells":[cell]},"source_version":"v1","expected_generation":0}),
        );
        assert!(wrong_digest.unwrap_err().contains("canonical identity"));

        let wrong_area = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":acquisition_digest(cell),"area":area(0.,0.,1.,1.),
                "cells":{"cells":[cell]},"source_version":"v1","expected_generation":0}),
        );
        assert!(wrong_area.unwrap_err().contains("exactly match"));

        let status = execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
        )
        .unwrap();
        assert_eq!(status["queries"].as_array().unwrap().len(), 0);
        assert!(status["requested"]["cells"].as_array().unwrap().is_empty());
    }

    #[test]
    fn commit_requires_the_matching_requested_state_and_exact_cells() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let cell = [29.9, -2.1, 30.1, -1.9];
        let digest = acquisition_digest(cell);

        let unbegun = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"cells":{"cells":[cell]},"features":[],
                "source_version":"v1","expected_generation":0}),
        );
        assert!(unbegun
            .unwrap_err()
            .contains("no matching begun acquisition"));

        let begun = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(cell[0],cell[1],cell[2],cell[3]),
                "cells":{"cells":[cell]},"source_version":"v1","expected_generation":0}),
        )
        .unwrap();
        let altered = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[[29.8,-2.2,30.2,-1.8]]},"features":[],
                "source_version":"v1","expected_generation":0}),
        );
        assert!(altered.unwrap_err().contains("differs from the begun"));

        let completed = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},"features":[],
                "source_version":"v1","expected_generation":0}),
        )
        .unwrap();
        assert_eq!(completed["generation"], 1);

        let repeated = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},"features":[],
                "source_version":"v1","expected_generation":1}),
        );
        assert!(repeated.unwrap_err().contains("only a requested"));
    }

    #[test]
    fn a_completed_acquisition_indexes_and_answers_by_area() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let status = acquire(
            root.path(),
            &key,
            [29.9, -2.1, 30.1, -1.9],
            vec![feature(30.0, -2.0, "inside"), feature(30.05, -2.05, "also")],
            0,
        );
        assert_eq!(status["feature_count"], 2);
        assert_eq!(status["index_state"], "ready");
        assert_eq!(status["ready"], true);
        assert_eq!(status["generation"], 1);
        assert_eq!(status["last_error"], Value::Null);

        let hit = execute_at(
            root.path(),
            json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(29.99,-2.01,30.01,-1.99)}),
        )
        .unwrap();
        assert_eq!(hit["features"].as_array().unwrap().len(), 1);
        assert_eq!(hit["features"][0]["properties"]["name"], "inside");
        assert_eq!(hit["cache"], "project_room");

        let miss = execute_at(
            root.path(),
            json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(10.,10.,11.,11.)}),
        );
        assert!(miss.unwrap_err().contains("completed coverage"));
    }

    #[test]
    fn overlapping_acquisitions_never_double_count_a_building() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let shared = feature(30.0, -2.0, "shared");
        acquire(
            root.path(),
            &key,
            [29.9, -2.1, 30.1, -1.9],
            vec![shared.clone()],
            0,
        );
        let status = acquire(
            root.path(),
            &key,
            [30.0, -2.05, 30.2, -1.85],
            vec![shared, feature(30.15, -1.9, "new")],
            1,
        );
        assert_eq!(status["feature_count"], 2);
        let all = execute_at(
            root.path(),
            json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":status["completed_geometry"]}),
        )
        .unwrap();
        assert_eq!(all["features"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_repeat_of_identical_work_is_refused_and_concurrency_is_fenced() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let digest = acquisition_digest([29.9, -2.1, 30.1, -1.9]);
        acquire(
            root.path(),
            &key,
            [29.9, -2.1, 30.1, -1.9],
            vec![feature(30.0, -2.0, "one")],
            0,
        );
        // The same acquisition asked for again is answered from what is held.
        let again = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(29.9,-2.1,30.1,-1.9),
                "cells":{"cells":[[29.9,-2.1,30.1,-1.9]]},"source_version":"v1"}),
        )
        .unwrap();
        assert_eq!(again["accepted"], false);
        assert_eq!(again["reason"], "already_completed");

        // A second worker starting the same NEW acquisition is told it is
        // already in flight rather than buying the same rows twice.
        let fresh = acquisition_digest([31., -2.1, 31.2, -1.9]);
        let first = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":fresh,"area":area(31.,-2.1,31.2,-1.9),
                "cells":{"cells":[[31.,-2.1,31.2,-1.9]]},"source_version":"v1"}),
        )
        .unwrap();
        assert_eq!(first["accepted"], true);
        let second = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":fresh,"area":area(31.,-2.1,31.2,-1.9),
                "cells":{"cells":[[31.,-2.1,31.2,-1.9]]},"source_version":"v1"}),
        )
        .unwrap();
        assert_eq!(second["accepted"], false);
        assert_eq!(second["reason"], "in_flight");
    }

    #[test]
    fn waiting_claims_recheck_overlapping_completion_and_failed_owners() {
        for (other, fail, accepted) in [
            ([30.0, -2.05, 30.05, -1.95], false, false),
            ([30.0, -2.05, 30.2, -1.95], false, true),
            ([30.0, -2.05, 30.05, -1.95], true, true),
        ] {
            let root = tempfile::tempdir().unwrap();
            let key = open(root.path(), &scope(), &buildings());
            let cell = [29.9, -2.1, 30.1, -1.9];
            let digest = acquisition_digest(cell);
            let begun = execute_at(
                root.path(),
                json!({
                    "action":"begin", "dataset_key":key, "principal":"uid-1", "project":"project-a",
                    "query_digest":digest, "area":area(cell[0],cell[1],cell[2],cell[3]),
                    "cells":{"cells":[cell]}, "source_version":"v1"
                }),
            )
            .unwrap();
            let path = root.path().to_owned();
            let other_key = key.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let waiter = std::thread::spawn(move || {
                let result = execute_at(
                    &path,
                    json!({
                        "action":"begin", "dataset_key":other_key, "principal":"uid-1", "project":"project-a",
                        "query_digest":acquisition_digest(other), "area":area(other[0],other[1],other[2],other[3]),
                        "cells":{"cells":[other]}, "source_version":"v1", "wait_for_pending":true
                    }),
                );
                send.send(result).unwrap();
            });
            assert!(matches!(
                receive.recv_timeout(std::time::Duration::from_millis(80)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            execute_at(
                root.path(),
                json!({
                    "action":if fail {"fail"} else {"commit"}, "dataset_key":key,
                    "principal":"uid-1", "project":"project-a", "query_digest":digest,
                    "ticket":begun["ticket"], "cells":{"cells":[cell]}, "features":[],
                    "source_version":"v1", "message":"provider failed"
                }),
            )
            .unwrap();
            let result = receive
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
                .unwrap();
            waiter.join().unwrap();
            assert_eq!(result["accepted"], accepted, "{result}");
            if !accepted {
                assert_eq!(result["reason"], "already_completed");
            }
        }
    }

    #[test]
    fn a_stale_completion_is_refused_and_held_data_survives_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        acquire(
            root.path(),
            &key,
            [29.9, -2.1, 30.1, -1.9],
            vec![feature(30.0, -2.0, "kept")],
            0,
        );
        // A completion with no matching acquisition is refused.
        let stale = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":"e".repeat(64),"cells":{"cells":[[0.,0.,1.,1.]]},
                "features":[],"source_version":"v1","expected_generation":0}),
        );
        assert!(stale.is_err());
        // So is one carrying the wrong account or project.
        let wrong = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-2","project":"project-a",
                "query_digest":"e".repeat(64),"cells":{"cells":[[0.,0.,1.,1.]]},
                "features":[],"source_version":"v1"}),
        );
        assert!(wrong.is_err());

        let pending = execute_at(root.path(), json!({"action":"begin","dataset_key":key,
            "principal":"uid-1","project":"project-a","query_digest":acquisition_digest([0.,0.,1.,1.]),
            "area":area(0.,0.,1.,1.),"cells":{"cells":[[0.,0.,1.,1.]]},"source_version":"v1"})).unwrap();
        let failed = execute_at(
            root.path(),
            json!({"action":"fail","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":pending["query_digest"],"ticket":pending["ticket"],"message":"the provider was unreachable"}),
        )
        .unwrap();
        assert_eq!(failed["last_error"], "the provider was unreachable");
        assert_eq!(failed["feature_count"], 1, "a failure kept held data");
        assert_eq!(failed["completed"]["cells"].as_array().unwrap().len(), 1);
        let still = execute_at(
            root.path(),
            json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(29.9,-2.1,30.1,-1.9)}),
        )
        .unwrap();
        assert_eq!(still["features"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_complete_empty_result_counts_as_covered() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let status = acquire(root.path(), &key, [40.0, 10.0, 40.1, 10.1], vec![], 0);
        // A complete provider query returned no rows. Preserve that receipt
        // without claiming the provider covers every real-world building.
        assert_eq!(status["feature_count"], 0);
        assert_eq!(status["index_state"], "empty");
        assert_eq!(status["ready"], true);
        let completed: Coverage = serde_json::from_value(status["completed"].clone()).unwrap();
        let wanted = Coverage {
            cells: vec![[40.0, 10.0, 40.1, 10.1]],
        };
        assert!(
            ds_command_kernel::project_dataset_cache::assess(&completed, &wanted)
                .unwrap()
                .covered
        );
    }

    #[test]
    fn a_requested_area_that_never_completed_is_not_held() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":acquisition_digest([0.,0.,1.,1.]),"area":area(0.,0.,1.,1.),
                "cells":{"cells":[[0.,0.,1.,1.]]},"source_version":"v1"}),
        )
        .unwrap();
        let status = execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
        )
        .unwrap();
        assert_eq!(status["requested"]["cells"].as_array().unwrap().len(), 1);
        assert_eq!(status["completed"]["cells"].as_array().unwrap().len(), 0);
        // Something is mid-flight, so the dataset is not ready even though
        // nothing has failed.
        assert_eq!(status["pending_queries"], 1);
        assert_eq!(status["ready"], false);
    }

    #[test]
    fn datasets_and_principals_never_share_a_room_or_a_readiness() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        acquire(
            root.path(),
            &key,
            [29.9, -2.1, 30.1, -1.9],
            vec![feature(30.0, -2.0, "building")],
            0,
        );
        let contours = Dataset {
            id: "elevation_contours".into(),
            ..buildings()
        };
        let other_key = dataset_key(&scope(), &contours);
        let other = execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":other_key}),
        )
        .unwrap();
        assert_eq!(other["exists"], false, "buildings spoke for contours");

        let elsewhere = Scope {
            principal: "uid-2".into(),
            ..scope()
        };
        let foreign = dataset_key(&elsewhere, &buildings());
        assert_eq!(
            execute_at(
                root.path(),
                json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":foreign})
            )
            .unwrap()["exists"],
            false
        );

        let inventory = project_inventory(root.path(), "uid-1", "project-a").unwrap();
        assert_eq!(inventory["rooms"].as_array().unwrap().len(), 1);
        assert_eq!(
            inventory["datasets"]["google_open_buildings"]["feature_count"],
            1
        );
        assert_eq!(
            project_inventory(root.path(), "uid-2", "project-a").unwrap()["rooms"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn a_new_source_version_is_freshness_and_keeps_the_same_room() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        acquire(
            root.path(),
            &key,
            [29.9, -2.1, 30.1, -1.9],
            vec![feature(30.0, -2.0, "held")],
            0,
        );
        let republished = Dataset {
            source_version: "v2".into(),
            ..buildings()
        };
        assert_eq!(dataset_key(&scope(), &republished), key);
        let reopened = open(root.path(), &scope(), &republished);
        let status = execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":reopened}),
        )
        .unwrap();
        assert_eq!(
            status["source_version"], "v1",
            "opening must not relabel held data"
        );
        assert_eq!(status["feature_count"], 1, "freshness reset held coverage");
        assert_eq!(status["completed"]["cells"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_refresh_deletes_under_the_same_rule_the_provider_selects_with() {
        // A footprint that straddles the eastern edge of the refreshed cell is
        // the case that separates the two possible rules. The buildings door
        // fetches by intersection, so this footprint belongs to this cell and a
        // refresh both removes it and is allowed to put it back. Were deletion
        // wider than acquisition, the building would leave the room and never
        // return.
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let cell = [29.9, -2.1, 30.1, -1.9];
        let crossing = json!({"type":"Feature","properties":{"name":"crossing"},
            "geometry":{"type":"Polygon","coordinates":[[
                [30.05,-2.0],[30.15,-2.0],[30.15,-1.99],[30.05,-1.99],[30.05,-2.0]]]}});
        assert_eq!(
            policy::acquisition_selection(policy::BUILDINGS_DATASET_ID, "").unwrap(),
            ds_command_kernel::reference_cache::AreaSelection::Intersects
        );
        acquire(root.path(), &key, cell, vec![crossing.clone()], 0);
        assert_eq!(status_of(root.path(), &key)["feature_count"], 1);

        // Re-acquired unchanged: still exactly one row, never a duplicate.
        let again = acquire_at(root.path(), &key, cell, vec![crossing.clone()], "v2", true);
        assert_eq!(again["feature_count"], 1);

        // Re-acquired as gone: a provider that selects by intersection would
        // have returned it if it still existed, so the room stops holding it.
        let emptied = acquire_at(root.path(), &key, cell, vec![], "v3", true);
        assert_eq!(emptied["feature_count"], 0);
    }

    #[test]
    fn a_dataset_with_no_recorded_selection_rule_cannot_land_rows_at_all() {
        // Deleting under a guessed predicate is the operation that loses data,
        // so an unregistered dataset is refused rather than defaulted.
        let root = tempfile::tempdir().unwrap();
        let unknown = Dataset {
            id: "some_new_dataset".into(),
            ..buildings()
        };
        let key = open(root.path(), &scope(), &unknown);
        let cell = [29.9, -2.1, 30.1, -1.9];
        let digest = ds_command_kernel::project_dataset_cache::query_digest(
            &scope(),
            &Dataset {
                source_version: "v1".into(),
                ..unknown.clone()
            },
            &serde_json::from_value(policy_value()).unwrap(),
            &Coverage { cells: vec![cell] },
        );
        let begun = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(cell[0],cell[1],cell[2],cell[3]),
                "cells":{"cells":[cell]},"source_version":"v1"}),
        )
        .unwrap();
        let refused = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},
                "features":[feature(30.0,-2.0,"nowhere")],"source_version":"v1"}),
        )
        .unwrap_err();
        assert!(refused.contains("acquisition_selection"), "{refused}");
        assert_eq!(status_of(root.path(), &key)["feature_count"], 0);
    }

    #[test]
    fn coverage_completed_under_a_replaced_version_is_reported_and_never_silently_reused() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let older = [29.9, -2.1, 30.1, -1.9];
        let newer = [30.9, -2.1, 31.1, -1.9];
        acquire(
            root.path(),
            &key,
            older,
            vec![feature(30.0, -2.0, "held")],
            0,
        );
        let first = status_of(root.path(), &key);
        assert_eq!(first["fresh"], true, "one version is not stale");
        assert_eq!(first["available_version"], "v1");

        // A second acquisition answers under a newer publication. That is how
        // the room learns the first one is now behind.
        acquire_at(
            root.path(),
            &key,
            newer,
            vec![feature(31.0, -2.0, "fresh")],
            "v2",
            false,
        );
        let stale = status_of(root.path(), &key);
        assert_eq!(stale["available_version"], "v2");
        assert_eq!(stale["fresh"], false);
        assert_eq!(stale["stale"]["cells"], json!([older]));
        assert_eq!(stale["source_versions"], json!(["v1", "v2"]));
        // Staleness on its own destroyed nothing: both holdings are intact and
        // the older area still answers reads.
        assert_eq!(stale["feature_count"], 2);
        assert_eq!(
            execute_at(
                root.path(),
                json!({"action":"query","dataset_key":key,
                "principal":"uid-1","project":"project-a",
                "area":area(older[0],older[1],older[2],older[3])})
            )
            .unwrap()["features"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // The dedicated read says the same thing and costs nothing.
        let report = execute_at(
            root.path(),
            json!({"action":"freshness","dataset_key":key,"principal":"uid-1","project":"project-a"}),
        )
        .unwrap();
        assert_eq!(report["current"], "v2");
        assert_eq!(report["stale"]["cells"], json!([older]));
        assert_eq!(report["stale_geometry"]["type"], "MultiPolygon");

        // Held coverage still answers a repeat — but the refusal now carries
        // the verdict, so no caller reuses obsolete rows without being told.
        let reused = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":acquisition_digest_at(older,"v2"),
                "area":area(older[0],older[1],older[2],older[3]),
                "cells":{"cells":[older]},"source_version":"v2"}),
        )
        .unwrap();
        assert_eq!(reused["accepted"], false);
        assert_eq!(reused["reason"], "already_completed");
        assert_eq!(reused["stale"], true);
        assert_eq!(reused["available_version"], "v2");

        // The explicit refresh is the only thing that replaces those rows.
        let refreshed = acquire_at(
            root.path(),
            &key,
            older,
            vec![feature(30.0, -2.0, "re-acquired")],
            "v2",
            true,
        );
        assert_eq!(refreshed["fresh"], true);
        assert_eq!(refreshed["stale_count"], 0);
        assert_eq!(refreshed["source_versions"], json!(["v2"]));
        assert_eq!(refreshed["source_version"], "v2");
        assert_eq!(refreshed["feature_count"], 2);
        let rows = refreshed["queries"].as_array().unwrap();
        assert!(
            rows.iter().any(|row| row["refresh"] == true),
            "a refresh is recorded as one: {rows:?}"
        );
    }

    #[test]
    fn an_abandoned_acquisition_expires_and_can_no_longer_settle_anything() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let cell = [0., 0., 1., 1.];
        let digest = acquisition_digest(cell);
        let pending = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(0.,0.,1.,1.),
                "cells":{"cells":[cell]},"source_version":"v1"}),
        )
        .unwrap();
        assert_eq!(status_of(root.path(), &key)["pending_queries"], 1);

        age_requests(
            root.path(),
            &key,
            ds_command_kernel::project_dataset_cache::ACQUISITION_TICKET_TTL_MS + 1,
        );
        let after = status_of(root.path(), &key);
        // The count stops spinning: nothing is pending, one attempt is gone.
        assert_eq!(after["pending_queries"], 0);
        assert_eq!(after["expired_queries"], 1);
        assert_eq!(after["queries"][0]["state"], "expired");
        assert_eq!(
            after["queries"][0]["status"], "requested",
            "expiry is a verdict on a stored request, not a rewrite of it"
        );

        // An expired ticket settles nothing, either way.
        let late = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":pending["ticket"],"cells":{"cells":[cell]},
                "features":[],"source_version":"v1"}),
        )
        .unwrap_err();
        assert!(late.contains("expired"), "{late}");
        let blamed = execute_at(
            root.path(),
            json!({"action":"fail","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":pending["ticket"],"message":"too late"}),
        )
        .unwrap_err();
        assert!(blamed.contains("expired"), "{blamed}");
        assert_eq!(status_of(root.path(), &key)["last_error"], Value::Null);

        // And the area it was holding is free to be acquired again.
        assert_eq!(
            acquire_at(root.path(), &key, cell, vec![], "v1", false)["completed"]["cells"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn an_oversized_source_receipt_is_refused_and_costs_no_held_data() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let first = [29.9, -2.1, 30.1, -1.9];
        acquire(
            root.path(),
            &key,
            first,
            vec![feature(30.0, -2.0, "held")],
            0,
        );
        let cell = [30.9, -2.1, 31.1, -1.9];
        let digest = acquisition_digest(cell);
        let begun = execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(cell[0],cell[1],cell[2],cell[3]),
                "cells":{"cells":[cell]},"source_version":"v1"}),
        )
        .unwrap();
        let refused = execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},
                "features":[],"source_version":"v1",
                "source_receipt":{"body":"x".repeat(
                    ds_command_kernel::project_dataset_cache::MAX_SOURCE_RECEIPT_BYTES)}}),
        )
        .unwrap_err();
        assert!(refused.contains("source receipt"), "{refused}");
        // A receipt that will not fit is a refusal, not a trimmed receipt, and
        // it costs the room nothing it already held.
        let after = status_of(root.path(), &key);
        assert_eq!(after["feature_count"], 1);
        assert_eq!(after["completed"]["cells"], json!([first]));
        // A receipt that is not an object is refused for the same reason.
        assert!(execute_at(
            root.path(),
            json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},
                "features":[],"source_version":"v1","source_receipt":"a receipt"}),
        )
        .is_err());
    }

    #[test]
    fn an_inventory_never_reveals_another_accounts_rooms() {
        let root = tempfile::tempdir().unwrap();
        let mine = open(root.path(), &scope(), &buildings());
        acquire(
            root.path(),
            &mine,
            [29.9, -2.1, 30.1, -1.9],
            vec![feature(30.0, -2.0, "mine")],
            0,
        );
        let theirs = Scope {
            principal: "uid-2".into(),
            project: "project-z".into(),
        };
        open(root.path(), &theirs, &buildings());

        let ours = inventory(root.path(), "uid-1").unwrap();
        assert_eq!(ours["rooms"].as_array().unwrap().len(), 1);
        assert_eq!(ours["rooms"][0]["dataset_key"], mine);
        assert_eq!(ours["principal"], "uid-1");
        // The other account's room exists on this computer and is invisible —
        // not merely filtered out of the answer, but never projected into one.
        let names: Vec<String> = ours["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|room| room["project"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert!(!names.contains(&"project-z".to_string()));
        assert_eq!(
            inventory(root.path(), "uid-2").unwrap()["rooms"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // An inventory with no principal is refused rather than answered for
        // everyone.
        assert!(inventory(root.path(), "  ").is_err());
        assert!(project_inventory(root.path(), "uid-1", " ").is_err());
    }

    #[test]
    fn traversal_and_linked_rooms_are_refused() {
        let root = tempfile::tempdir().unwrap();
        assert!(execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":"../escape"})
        )
        .is_err());
        assert!(execute_at(root.path(), json!({"action":"status"})).is_err());
        assert!(execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":"A".repeat(64)})
        )
        .is_err());
    }
    #[test]
    fn disjoint_acquisitions_commit_in_any_order_and_refresh_replaces_rows() {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let begin = |cell: [f64; 4], refresh: bool| {
            execute_at(
                root.path(),
                json!({
            "action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":acquisition_digest(cell),"cells":{"cells":[cell]},
            "area":area(cell[0],cell[1],cell[2],cell[3]),"source_version":"v1","refresh":refresh}),
            )
            .unwrap()
        };
        let commit = |cell: [f64; 4], ticket: &Value, features: Vec<Value>| {
            execute_at(
                root.path(),
                json!({
            "action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":acquisition_digest(cell),"cells":{"cells":[cell]},
            "ticket":ticket["ticket"],"features":features,"source_version":"v1"}),
            )
        };
        let a = [29.9, -2.1, 30.1, -1.9];
        let b = [30.9, -2.1, 31.1, -1.9];
        let first = begin(a, false);
        let second = begin(b, false);
        assert_eq!(first["generation"], second["generation"]);
        commit(b, &second, vec![feature(31., -2., "retained")]).unwrap();
        assert_eq!(
            commit(a, &first, vec![feature(30., -2., "old")]).unwrap()["feature_count"],
            2
        );
        let refresh = begin(a, true);
        assert!(commit(a, &first, vec![]).unwrap_err().contains("ticket"));
        let replaced = commit(a, &refresh, vec![]).unwrap();
        assert_eq!(replaced["feature_count"], 1);
        let forbidden = execute_at(
            root.path(),
            json!({"action":"query","dataset_key":key,
            "principal":"uid-2","project":"project-a","area":area(30.9,-2.1,31.1,-1.9)}),
        );
        assert!(forbidden.unwrap_err().contains("different account"));
    }
}
