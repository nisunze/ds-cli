//! The media outbox: a person's captured media on its way to the bucket,
//! held in the Kernel's SQLite until its upload is verified, by the same
//! rusqlite code in Core's protected Store and the browser's storage worker.
//!
//! One row per pending upload: the survey-photo outbox row
//! ([`crate::survey_photo_outbox::Row`], whose transitions this store applies
//! in the same transaction as the bytes they govern) and the bytes it owns,
//! the original followed by its thumbnail companion, in this store's own work
//! tables ([`crate::byte_parts`]). No cache retention reaches them. They leave
//! only when their upload is verified, and then move into the byte cache as
//! the photo they now are ([`crate::byte_cache`]), so they stay readable
//! offline.
//!
//! The host binds the identity: a row's account is the bound account, never
//! a caller's word, and a signed-out identity queues nothing. The retired
//! browser outbox imports once per account, that account's rows only.
use crate::byte_parts::{Blob, Blobs, Received, Share, Stage, PART_BYTES};
use crate::durable_store::{held_receipt, Identity, Input, Outcome, Principal};
use crate::execution_context::valid_project;
use crate::import_batches::{self, Batch};
use crate::survey_media_address;
use crate::survey_photo_outbox::{self as outbox, Action, Companion, Event, Row, Scope, Status, TransportOutcome};
use crate::transfer::Cause;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "media_outbox";
/// A malformed call.
pub const INVALID: &str = "media_outbox_invalid";
/// A well-formed call the outbox declines, in the words a person is shown.
pub const REFUSED: &str = "media_outbox_refused";
/// The retired browser outbox, imported once per account.
pub const LEGACY_SOURCE: &str = "blob_outbox";
const MIB: u64 = 1024 * 1024;
/// The largest original one row queues.
pub const MAX_MEDIA_BYTES: u64 = 1024 * MIB;
/// The largest thumbnail companion.
pub const MAX_COMPANION_BYTES: u64 = 64 * MIB;
/// The most live transfers a claim is told about.
pub const MAX_IN_FLIGHT: usize = 64;
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const MISSING_BYTES: &str = "Photo bytes are missing from local storage.";
pub(crate) const BLOBS: Blobs = Blobs {
    blobs: "media_outbox_blobs",
    parts: "media_outbox_parts",
    code: INVALID,
    content_addressed: false,
};
const TABLES: &str = "CREATE TABLE IF NOT EXISTS media_outbox (owner TEXT NOT NULL, path TEXT NOT NULL, companion_path TEXT, status TEXT NOT NULL, row TEXT NOT NULL, blob INTEGER, original_size INTEGER NOT NULL CHECK(original_size>=0), companion_size INTEGER NOT NULL CHECK(companion_size>=0), bytes_at_ms INTEGER NOT NULL CHECK(bytes_at_ms>=0 AND bytes_at_ms<=9007199254740991), PRIMARY KEY(owner,path)); CREATE INDEX IF NOT EXISTS media_outbox_by_companion ON media_outbox(owner,companion_path); CREATE TABLE IF NOT EXISTS media_outbox_import_log (owner TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY(owner,path))";

/// The thumbnail companion an admission queues with its original.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompanionBytes {
    pub path: String,
    pub content_type: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_generation: Option<String>,
}

/// Which bytes of a row an upload target reads.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Original,
    Companion,
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Queue captured bytes: the original followed by its companion.
    /// `generation` is a fresh host nonce naming this byte revision.
    Admit {
        project: String,
        path: String,
        surface: String,
        owner_ref: String,
        #[serde(default)]
        content_type: String,
        size: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_generation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_protocol: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        companion: Option<CompanionBytes>,
        generation: String,
        payload: Share,
        now: u64,
    },
    /// Recover abandoned leases and claim the uploads that may start now.
    /// `in_flight` names the paths whose transfers the caller is running.
    Claim {
        online: bool,
        #[serde(default)]
        in_flight: Vec<String>,
        now: u64,
    },
    /// Bytes of one claimed target, for its transfer.
    Read {
        path: String,
        generation: String,
        target: Target,
        offset: u64,
        length: u64,
    },
    /// What one claimed upload observed.
    Settle {
        path: String,
        generation: String,
        lease: u64,
        outcome: TransportOutcome,
        now: u64,
    },
    /// Queue every failed or blocked upload again.
    RetryFailed { now: u64 },
    Counts,
    Failed,
    /// What the host reads, read-only, to import the retired browser outbox.
    Legacy,
    /// One retired row as the host read it, and the bytes it pinned: the
    /// original followed by its companion, absent when the host held none.
    ImportEntry {
        row: Value,
        #[serde(default)]
        original_size: u64,
        #[serde(default)]
        companion_size: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Share>,
        now: u64,
    },
    ImportFinish { now: u64 },
}

fn invalid(message: impl std::fmt::Display) -> String {
    format!("{INVALID}: {message}")
}
fn refused(message: impl std::fmt::Display) -> String {
    format!("{REFUSED}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    invalid(error)
}
fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}
fn token(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.trim() == value && !value.chars().any(char::is_control)
}
/// A host nonce naming one byte revision; it travels as a revision id.
fn nonce(value: &str) -> bool {
    (8..=128).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
fn account(identity: &Identity) -> Option<&str> {
    match &identity.principal {
        Principal::Account(uid) => Some(uid),
        Principal::Anonymous => None,
    }
}
fn status_name(status: &Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Uploading => "uploading",
        Status::Failed => "failed",
        Status::Blocked => "blocked",
    }
}
fn wake_at(actions: &[Action]) -> Option<u64> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::WakeAt { at } => Some(*at),
            _ => None,
        })
        .min()
}
fn time(now: u64) -> Result<u64, String> {
    if now > MAX_SAFE {
        return Err(invalid("the time exceeds its bound"));
    }
    Ok(now)
}

pub(crate) fn schema(connection: &Connection) -> Result<(), String> {
    connection.execute_batch(&BLOBS.schema()).map_err(sql)?;
    connection.execute_batch(TABLES).map_err(sql)
}

/// One held row with its bytes.
struct Held {
    row: Row,
    blob: Option<Blob>,
    original_size: u64,
    companion_size: u64,
    bytes_at_ms: u64,
}

type Stored = (String, Option<i64>, i64, i64, i64);
const SELECT: &str = "SELECT row,blob,original_size,companion_size,bytes_at_ms FROM media_outbox";

fn stored_row(row: &rusqlite::Row) -> rusqlite::Result<Stored> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
}

fn decode(connection: &Connection, stored: Stored) -> Result<Option<Held>, String> {
    let (text, blob, original_size, companion_size, bytes_at_ms) = stored;
    let Ok(row) = serde_json::from_str::<Row>(&text) else {
        return Ok(None);
    };
    let blob = match blob {
        Some(id) => BLOBS.get(connection, id)?,
        None => None,
    };
    Ok(Some(Held {
        row,
        blob,
        original_size: original_size as u64,
        companion_size: companion_size as u64,
        bytes_at_ms: bytes_at_ms as u64,
    }))
}

fn load(connection: &Connection, owner: &str, path: &str) -> Result<Option<Held>, String> {
    let stored = connection
        .query_row(
            &format!("{SELECT} WHERE owner=?1 AND path=?2"),
            params![owner, path],
            stored_row,
        )
        .optional()
        .map_err(sql)?;
    match stored {
        Some(stored) => decode(connection, stored),
        None => Ok(None),
    }
}

fn rows_of(connection: &Connection, owner: &str, filter: &str) -> Result<Vec<Held>, String> {
    let mut statement = connection
        .prepare(&format!("{SELECT} WHERE owner=?1 {filter} ORDER BY path"))
        .map_err(sql)?;
    let stored = statement
        .query_map(params![owner], stored_row)
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let mut rows = Vec::with_capacity(stored.len());
    for stored in stored {
        if let Some(held) = decode(connection, stored)? {
            rows.push(held);
        }
    }
    Ok(rows)
}

fn insert(connection: &Connection, owner: &str, row: &Row, blob: Option<&Blob>, sizes: (u64, u64), bytes_at_ms: u64) -> Result<bool, String> {
    let inserted = connection
        .execute(
            "INSERT INTO media_outbox(owner,path,companion_path,status,row,blob,original_size,companion_size,bytes_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(owner,path) DO NOTHING",
            params![
                owner,
                row.path,
                row.companion.as_ref().map(|companion| companion.path.as_str()),
                status_name(&row.status),
                serde_json::to_string(row).map_err(|e| e.to_string())?,
                blob.map(|blob| blob.id),
                sizes.0 as i64,
                sizes.1 as i64,
                bytes_at_ms as i64
            ],
        )
        .map_err(sql)?;
    Ok(inserted == 1)
}

fn update(connection: &Connection, owner: &str, row: &Row) -> Result<(), String> {
    connection
        .execute(
            "UPDATE media_outbox SET status=?3,row=?4 WHERE owner=?1 AND path=?2",
            params![owner, row.path, status_name(&row.status), serde_json::to_string(row).map_err(|e| e.to_string())?],
        )
        .map_err(sql)?;
    Ok(())
}

/// Whether a path or companion path is already pending for this owner.
fn collides(connection: &Connection, owner: &str, path: &str, companion: Option<&str>) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM media_outbox WHERE owner=?1 AND (path IN (?2,?3) OR companion_path IN (?2,?3)))",
            params![owner, path, companion],
            |row| row.get(0),
        )
        .map_err(sql)
}

/// Whether a row's bytes are held whole at the sizes the row declares.
fn present(connection: &Connection, held: &Held) -> Result<bool, String> {
    let Some(blob) = &held.blob else {
        return Ok(false);
    };
    Ok(held.original_size == held.row.size
        && held.companion_size == held.row.companion.as_ref().map_or(0, |companion| companion.size)
        && blob.byte_length == held.original_size + held.companion_size
        && BLOBS.intact(connection, blob)?)
}

/// A pending upload's bytes at one path: the original or its companion.
pub(crate) struct Pending {
    blob: Option<Blob>,
    offset: u64,
    pub length: u64,
    pub media_type: String,
    pub generation: String,
    pub bytes_at_ms: u64,
}
impl Pending {
    /// Bytes of this path, verified; `None` when they are missing or corrupt.
    pub(crate) fn read(&self, connection: &Connection, at: u64, length: u64) -> Result<Option<Vec<u8>>, String> {
        let Some(blob) = &self.blob else {
            return Ok(None);
        };
        if at.saturating_add(length) > self.length || self.offset + self.length > blob.byte_length {
            return Ok(None);
        }
        BLOBS.range(connection, blob, self.offset + at, length)
    }
    /// Whether the bytes are held at their recorded lengths; reads no bytes.
    pub(crate) fn present(&self, connection: &Connection) -> Result<bool, String> {
        match &self.blob {
            Some(blob) => Ok(self.offset + self.length <= blob.byte_length && BLOBS.intact(connection, blob)?),
            None => Ok(false),
        }
    }
}

/// The pending upload that owns `path` for this owner, if any: a photo read
/// serves it first and a photo put never replaces it.
pub(crate) fn pending(connection: &Connection, owner: &str, path: &str) -> Result<Option<Pending>, String> {
    schema(connection)?;
    let stored = connection
        .query_row(
            &format!("{SELECT} WHERE owner=?1 AND (path=?2 OR companion_path=?2) ORDER BY path=?2 DESC LIMIT 1"),
            params![owner, path],
            stored_row,
        )
        .optional()
        .map_err(sql)?;
    let Some(held) = (match stored {
        Some(stored) => decode(connection, stored)?,
        None => None,
    }) else {
        return Ok(None);
    };
    let (offset, length, media_type) = if held.row.path == path {
        (0, held.original_size, held.row.content_type.clone())
    } else {
        let Some(companion) = held.row.companion.as_ref().filter(|companion| companion.path == path) else {
            return Ok(None);
        };
        (held.original_size, held.companion_size, companion.content_type.clone())
    };
    Ok(Some(Pending {
        blob: held.blob,
        offset,
        length,
        media_type,
        generation: held.row.generation,
        bytes_at_ms: held.bytes_at_ms,
    }))
}

fn photo_changed(connection: &Connection) -> Result<(), String> {
    crate::durable_store::advance(connection, &crate::byte_cache::Kind::Photo.generation_key()).map(|_| ())
}

fn staged(staging: u64, received: u64, parts: u64) -> Outcome {
    Outcome::read(fields(Received::staged_answer(staging, received, parts)))
}

fn admit(connection: &Connection, identity: &Identity, owner: &str, operation: Operation, bytes: Option<Vec<u8>>) -> Result<Outcome, String> {
    let Operation::Admit {
        project,
        path,
        surface,
        owner_ref,
        content_type,
        size,
        expected_generation,
        revision_protocol,
        companion,
        generation,
        payload,
        now,
    } = operation
    else {
        return Err(invalid("not an admission"));
    };
    let uid = account(identity)
        .ok_or_else(|| refused("A signed-in account and project are required to queue survey media."))?;
    if !valid_project(&project) {
        return Err(invalid("an admission names its project"));
    }
    if survey_media_address::parse(&path).map_err(invalid)?.project != project {
        return Err(invalid("the media path names another project"));
    }
    if !token(&surface, 128)
        || !token(&owner_ref, 1024)
        || content_type.len() > 255
        || content_type.chars().any(char::is_control)
        || !nonce(&generation)
        || size == 0
        || size > MAX_MEDIA_BYTES
        || now > MAX_SAFE
        || companion.as_ref().is_some_and(|companion| {
            companion.size == 0 || companion.size > MAX_COMPANION_BYTES || !token(&companion.path, 2048)
        })
    {
        return Err(invalid("an admission names its surface, owner, type, sizes, generation and time"));
    }
    let companion_size = companion.as_ref().map_or(0, |companion| companion.size);
    if payload.byte_length != size + companion_size {
        return Err(invalid("the payload is the original followed by its companion"));
    }
    let content_type = match content_type.trim() {
        "" => "application/octet-stream".to_string(),
        declared => declared.to_string(),
    };
    let staging_text = serde_json::to_string(&(&path, &generation, size, companion_size, &content_type, &project))
        .map_err(|e| e.to_string())?;
    let scope = Scope {
        account_id: uid.into(),
        project_id: project,
    };
    let row = Row {
        path,
        surface,
        owner_ref,
        content_type,
        size,
        status: Status::Pending,
        attempts: 0,
        next_attempt_at: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        scope: Some(scope.clone()),
        lease: 0,
        revision: (revision_protocol == Some(1)).then(|| generation.clone()),
        generation,
        expected_generation,
        companion: companion.map(|companion| Companion {
            path: companion.path,
            content_type: companion.content_type,
            size: companion.size,
            expected_generation: companion.expected_generation,
        }),
        supersedes: Vec::new(),
    };
    let reply = outbox::apply(now, Some(row), Event::Admit { scope }).map_err(refused)?;
    let row = reply
        .row
        .ok_or_else(|| refused("Survey-photo outbox admission was refused."))?;
    let companion_path = row.companion.as_ref().map(|companion| companion.path.clone());
    if collides(connection, owner, &row.path, companion_path.as_deref())? {
        return Err(refused("Survey media is already pending at this path."));
    }
    let stage = Stage {
        code: INVALID,
        owner,
        store: STORE,
    };
    let payload = match stage.receive(connection, &staging_text, payload, bytes, MAX_MEDIA_BYTES + MAX_COMPANION_BYTES, now)? {
        Received::Staged {
            staging,
            received,
            parts,
        } => return Ok(staged(staging, received, parts)),
        Received::Complete(payload) => payload,
    };
    let digest = payload.sha256(connection, INVALID)?;
    let blob = BLOBS.write(connection, owner, &payload, &digest)?;
    payload.release(connection, INVALID)?;
    insert(connection, owner, &row, Some(&blob), (size, companion_size), now)?;
    photo_changed(connection)?;
    Ok(Outcome::written(fields(json!({"row": row, "wake_at": wake_at(&reply.actions)}))))
}

/// The byte range of one target of a row.
fn target_range(held: &Held, target: Target) -> Option<(u64, u64)> {
    match target {
        Target::Original => Some((0, held.original_size)),
        Target::Companion => held
            .row
            .companion
            .as_ref()
            .map(|_| (held.original_size, held.companion_size)),
    }
}

/// The ordered upload plan of a dispatch, each target with its bytes'
/// SHA-256; `None` when its bytes no longer verify.
fn plan(connection: &Connection, held: &Held, targets: &[outbox::UploadTarget]) -> Result<Option<Vec<Value>>, String> {
    let mut planned = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        let which = if index == 0 { Target::Original } else { Target::Companion };
        let digest = match (target_range(held, which), held.blob.as_ref()) {
            (Some((offset, length)), Some(blob)) if length == target.size => BLOBS.range_sha256(connection, blob, offset, length)?,
            _ => None,
        };
        let Some(digest) = digest else {
            return Ok(None);
        };
        let mut described = serde_json::to_value(target).map_err(|e| e.to_string())?;
        described["target"] = serde_json::to_value(which).map_err(|e| e.to_string())?;
        described["sha256"] = Value::String(digest);
        planned.push(described);
    }
    Ok(Some(planned))
}

fn claim(connection: &Connection, identity: &Identity, owner: &str, online: bool, in_flight: Vec<String>, now: u64) -> Result<Outcome, String> {
    if in_flight.len() > MAX_IN_FLIGHT {
        return Err(invalid("a claim names at most 64 live transfers"));
    }
    let now = time(now)?;
    let Some(uid) = account(identity) else {
        return Ok(Outcome::read(fields(json!({"dispatch": [], "wake_at": null}))));
    };
    let mut active = in_flight.len() as u32;
    let mut dispatch = Vec::new();
    let mut earliest: Option<u64> = None;
    let mut wrote = false;
    for held in rows_of(connection, owner, "")? {
        let mut row = held.row.clone();
        let Some(scope) = row.scope.clone().filter(|scope| scope.account_id == uid) else {
            continue;
        };
        if row.generation.trim().is_empty() || in_flight.contains(&row.path) {
            continue;
        }
        if row.status == Status::Uploading {
            // Not among the caller's live transfers: an abandoned lease.
            let Some(next) = outbox::apply(now, Some(row.clone()), Event::Recover).ok().and_then(|reply| reply.row) else {
                continue;
            };
            row = next;
        }
        let drive = Event::Drive {
            online,
            active_uploads: active,
            bytes_present: present(connection, &held)?,
            scope: scope.clone(),
        };
        let Ok(reply) = outbox::apply(now, Some(row.clone()), drive) else {
            continue;
        };
        let Some(mut next) = reply.row else {
            continue;
        };
        if let Some(at) = wake_at(&reply.actions) {
            earliest = Some(earliest.map_or(at, |held| held.min(at)));
        }
        for action in &reply.actions {
            let Action::Dispatch {
                path,
                generation,
                lease,
                targets,
                ..
            } = action
            else {
                continue;
            };
            match plan(connection, &held, targets)? {
                Some(planned) => {
                    active += 1;
                    dispatch.push(json!({"path": path, "generation": generation, "lease": lease, "targets": planned}));
                }
                // Bytes damaged since the presence check are never sent.
                None => {
                    let damaged = Event::Result {
                        scope: scope.clone(),
                        generation: generation.clone(),
                        lease: *lease,
                        outcome: TransportOutcome::TransportFailure {
                            cause: Some(Cause::DigestMismatch),
                            status: None,
                            error_code: None,
                            timed_out: false,
                            retry_after: None,
                            has_session: true,
                            offline_mode: false,
                            message: Some(MISSING_BYTES.into()),
                        },
                    };
                    next = outbox::apply(now, Some(next.clone()), damaged)
                        .map_err(refused)?
                        .row
                        .unwrap_or(next);
                }
            }
        }
        if next != held.row {
            update(connection, owner, &next)?;
            wrote = true;
        }
    }
    let value = fields(json!({"dispatch": dispatch, "wake_at": earliest}));
    Ok(if wrote {
        Outcome::written(value)
    } else {
        Outcome::read(value)
    })
}

#[allow(clippy::too_many_arguments)]
fn read(connection: &Connection, identity: &Identity, owner: &str, path: String, generation: String, target: Target, offset: u64, length: u64) -> Result<Outcome, String> {
    if account(identity).is_none() {
        return Err(refused("Sign in to upload queued media."));
    }
    let Some(held) = load(connection, owner, &path)? else {
        return Err(refused("This upload is no longer pending."));
    };
    if held.row.generation != generation {
        return Err(refused("This upload was replaced by a newer revision."));
    }
    let (base, size) = target_range(&held, target).ok_or_else(|| invalid("the row has no companion"))?;
    if length == 0 || length > PART_BYTES || offset.saturating_add(length) > size {
        return Err(invalid("a read takes 1 byte to one part inside its target"));
    }
    let blob = held.blob.as_ref().ok_or_else(|| refused(MISSING_BYTES))?;
    let bytes = BLOBS
        .range(connection, blob, base + offset, length)?
        .ok_or_else(|| refused(MISSING_BYTES))?;
    Ok(Outcome {
        value: fields(json!({"length": bytes.len()})),
        bytes: Some(bytes),
        ..Outcome::default()
    })
}

/// Move a verified upload's bytes into the byte cache, then remove its row.
fn graduate(connection: &Connection, owner: &str, held: &Held, now: u64) -> Result<(), String> {
    if let (Some(blob), true) = (&held.blob, present(connection, held)?) {
        crate::byte_cache::adopt_photo(connection, owner, &held.row.path, &held.row.content_type, &BLOBS, blob, 0, held.original_size, now)?;
        if let Some(companion) = &held.row.companion {
            crate::byte_cache::adopt_photo(
                connection,
                owner,
                &companion.path,
                &companion.content_type,
                &BLOBS,
                blob,
                held.original_size,
                held.companion_size,
                now,
            )?;
        }
    }
    connection
        .execute(
            "DELETE FROM media_outbox WHERE owner=?1 AND path=?2",
            params![owner, held.row.path],
        )
        .map_err(sql)?;
    if let Some(blob) = &held.blob {
        BLOBS.delete(connection, blob.id)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn settle(
    connection: &Connection,
    identity: &Identity,
    owner: &str,
    path: String,
    generation: String,
    lease: u64,
    outcome: TransportOutcome,
    now: u64,
) -> Result<Outcome, String> {
    let uid = account(identity).ok_or_else(|| refused("Sign in to upload queued media."))?;
    let now = time(now)?;
    let unsettled = Ok(Outcome::read(fields(json!({"settled": false}))));
    let Some(held) = load(connection, owner, &path)? else {
        return unsettled;
    };
    let Some(scope) = held.row.scope.clone().filter(|scope| scope.account_id == uid) else {
        return unsettled;
    };
    let result = Event::Result {
        scope,
        generation,
        lease,
        outcome,
    };
    let reply = outbox::apply(now, Some(held.row.clone()), result).map_err(refused)?;
    match reply.row {
        None => {
            graduate(connection, owner, &held, now)?;
            photo_changed(connection)?;
            Ok(Outcome::written(fields(json!({"settled": true, "synced": true}))))
        }
        Some(row) if row == held.row => unsettled,
        Some(row) => {
            update(connection, owner, &row)?;
            Ok(Outcome::written(fields(json!({
                "settled": true,
                "row": row,
                "wake_at": wake_at(&reply.actions),
            }))))
        }
    }
}

fn retry_failed(connection: &Connection, identity: &Identity, owner: &str, now: u64) -> Result<Outcome, String> {
    let now = time(now)?;
    let mut retried = 0u64;
    if let Some(uid) = account(identity) {
        for held in rows_of(connection, owner, "AND status IN ('failed','blocked')")? {
            let Some(scope) = held.row.scope.clone().filter(|scope| scope.account_id == uid) else {
                continue;
            };
            let next = outbox::apply(now, Some(held.row.clone()), Event::ManualRetry { scope })
                .ok()
                .and_then(|reply| reply.row);
            if let Some(row) = next.filter(|row| *row != held.row) {
                update(connection, owner, &row)?;
                retried += 1;
            }
        }
    }
    let value = fields(json!({"retried": retried}));
    Ok(if retried > 0 {
        Outcome::written(value)
    } else {
        Outcome::read(value)
    })
}

fn counts(connection: &Connection, owner: &str) -> Result<Outcome, String> {
    let mut statement = connection
        .prepare("SELECT status,COUNT(*) FROM media_outbox WHERE owner=?1 GROUP BY status")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![owner], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(sql)?;
    let (mut pending, mut uploading, mut failed) = (0i64, 0i64, 0i64);
    for row in rows {
        let (status, count) = row.map_err(sql)?;
        match status.as_str() {
            "pending" => pending += count,
            "uploading" => uploading += count,
            _ => failed += count,
        }
    }
    Ok(Outcome::read(fields(json!({
        "pending": pending,
        "uploading": uploading,
        "failed": failed,
    }))))
}

#[allow(clippy::too_many_arguments)]
fn import_entry(
    connection: &Connection,
    identity: &Identity,
    owner: &str,
    row: Value,
    original_size: u64,
    companion_size: u64,
    share: Option<Share>,
    now: u64,
    bytes: Option<Vec<u8>>,
) -> Result<Outcome, String> {
    import_batches::open(connection, identity, STORE, LEGACY_SOURCE)?;
    let now = time(now)?;
    if original_size > MAX_MEDIA_BYTES || companion_size > MAX_COMPANION_BYTES {
        return Err(invalid("an imported row's bytes exceed their bounds"));
    }
    let payload = match share {
        Some(share) => {
            if share.byte_length != original_size + companion_size {
                return Err(invalid("the payload is the original followed by its companion"));
            }
            let staging_text = serde_json::to_string(&(&row, original_size, companion_size)).map_err(|e| e.to_string())?;
            let stage = Stage {
                code: INVALID,
                owner,
                store: STORE,
            };
            match stage.receive(connection, &staging_text, share, bytes, MAX_MEDIA_BYTES + MAX_COMPANION_BYTES, now)? {
                Received::Staged {
                    staging,
                    received,
                    parts,
                } => return Ok(staged(staging, received, parts)),
                Received::Complete(payload) => Some(payload),
            }
        }
        None if bytes.is_some() => return Err(invalid("bytes travel with their payload share")),
        None => None,
    };
    let digest = match &payload {
        Some(payload) => Some(payload.sha256(connection, INVALID)?),
        None => None,
    };
    // Only this account's rows import; a row that was uploading when the
    // browser closed resumes as pending.
    let mine = serde_json::from_value::<Row>(row.clone())
        .ok()
        .filter(|row| row.scope.as_ref().map(|scope| scope.account_id.as_str()) == account(identity) && !row.generation.trim().is_empty())
        .and_then(|row| outbox::apply(now, Some(row), Event::Recover).ok())
        .and_then(|reply| reply.row);
    let mut written = false;
    if let Some(row) = &mine {
        let logged: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM media_outbox_import_log WHERE owner=?1 AND path=?2)",
                params![owner, row.path],
                |stored| stored.get(0),
            )
            .map_err(sql)?;
        let companion = row.companion.as_ref().map(|companion| companion.path.as_str());
        if !logged && !collides(connection, owner, &row.path, companion)? {
            let blob = match (&payload, &digest) {
                (Some(payload), Some(digest)) => Some(BLOBS.write(connection, owner, payload, digest)?),
                _ => None,
            };
            let sizes = if blob.is_some() { (original_size, companion_size) } else { (0, 0) };
            written = insert(connection, owner, row, blob.as_ref(), sizes, now)?;
        }
        connection
            .execute(
                "INSERT INTO media_outbox_import_log(owner,path) VALUES(?1,?2) ON CONFLICT DO NOTHING",
                params![owner, row.path],
            )
            .map_err(sql)?;
    }
    if let Some(payload) = payload {
        payload.release(connection, INVALID)?;
    }
    let entry = serde_json::to_string(&json!({
        "row": row,
        "original_size": original_size,
        "companion_size": companion_size,
        "sha256": digest,
    }))
    .map_err(|e| e.to_string())?;
    let (count, skipped) = if written { (1, 0) } else { (0, 1) };
    import_batches::tally(
        connection,
        identity,
        STORE,
        LEGACY_SOURCE,
        Batch {
            entries: &entry,
            count,
            skipped,
        },
    )?;
    if written {
        photo_changed(connection)?;
    }
    Ok(Outcome::written(fields(json!({"written": count, "skipped": skipped}))))
}

/// Run one operation inside the caller's transaction.
pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() && !matches!(operation, Operation::Admit { .. } | Operation::ImportEntry { .. }) {
        return Err(invalid("this operation carries no payload bytes"));
    }
    schema(connection)?;
    let identity = input.identity;
    let owner = identity.owner();
    match operation {
        operation @ Operation::Admit { .. } => admit(connection, identity, &owner, operation, input.bytes),
        Operation::Claim { online, in_flight, now } => claim(connection, identity, &owner, online, in_flight, now),
        Operation::Read {
            path,
            generation,
            target,
            offset,
            length,
        } => read(connection, identity, &owner, path, generation, target, offset, length),
        Operation::Settle {
            path,
            generation,
            lease,
            outcome,
            now,
        } => settle(connection, identity, &owner, path, generation, lease, outcome, now),
        Operation::RetryFailed { now } => retry_failed(connection, identity, &owner, now),
        Operation::Counts => counts(connection, &owner),
        Operation::Failed => {
            let rows: Vec<Row> = rows_of(connection, &owner, "AND status IN ('failed','blocked')")?
                .into_iter()
                .map(|held| held.row)
                .collect();
            Ok(Outcome::read(fields(json!({"rows": rows}))))
        }
        Operation::Legacy => Ok(Outcome::read(fields(json!({
            "import_needed": held_receipt(connection, identity, STORE, LEGACY_SOURCE)?.is_none(),
            "legacy": {
                "database": "ds_web_store",
                "store": "blob_outbox",
                "bytes_store": "photos",
                "bytes_field": "blob",
            },
        })))),
        Operation::ImportEntry {
            row,
            original_size,
            companion_size,
            payload,
            now,
        } => import_entry(connection, identity, &owner, row, original_size, companion_size, payload, now, input.bytes),
        Operation::ImportFinish { now } => {
            let (receipt, ran) = import_batches::finish(connection, identity, STORE, LEGACY_SOURCE, time(now)?, 0)?;
            connection
                .execute("DELETE FROM media_outbox_import_log WHERE owner=?1", params![owner])
                .map_err(sql)?;
            let value = fields(json!({"imported": ran, "receipt": receipt}));
            Ok(if ran {
                Outcome::written(value)
            } else {
                Outcome::read(value)
            })
        }
    }
}

#[cfg(test)]
#[path = "media_outbox_tests.rs"]
mod tests;
