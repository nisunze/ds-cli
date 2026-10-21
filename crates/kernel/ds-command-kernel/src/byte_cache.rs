//! The byte cache: one owner-scoped, content-addressed Kernel store for every
//! reconstructible byte cache (photos, compute artifacts, report previews,
//! remote model versions), run by the same rusqlite code in Core's protected
//! Store and the browser's storage worker.
//!
//! An entry is a key of a closed [`Kind`] naming content: bytes kept once
//! per owner by their SHA-256 ([`crate::byte_parts`] carries them in parts).
//! A put completes only when its payload's length, and its SHA-256 when the
//! caller declares one, match. A read verifies the parts it returns; a corrupt
//! part drops its content with every entry naming it and answers a miss, so
//! the next fetch repairs it. `has` compares stored lengths and reads no
//! bytes. One rule bounds each kind: past its byte budget, the owner's oldest
//! entries of that kind leave first.
//!
//! A cache is never work. A photo read serves the bytes of a pending upload
//! first ([`crate::media_outbox`]) so a photo waiting to sync reads like a
//! synced one, and a photo put never replaces them.
use crate::byte_parts::{part_length, parts_of, Blob, Blobs, Received, Share, Stage, PART_BYTES};
use crate::durable_store::{Input, Outcome};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "byte_cache";
pub const INVALID: &str = "byte_cache_invalid";
const MIB: u64 = 1024 * 1024;
const MAX_SAFE: u64 = 9_007_199_254_740_991;
/// The largest entry of any kind.
pub const MAX_ENTRY_BYTES: u64 = 512 * MIB;
pub const MAX_KEY_BYTES: usize = 2048;
pub const MAX_META_BYTES: usize = 16 * 1024;
pub(crate) const CONTENT: Blobs = Blobs {
    blobs: "byte_cache_blobs",
    parts: "byte_cache_parts",
    code: INVALID,
    content_addressed: true,
};
const ENTRIES: &str = "CREATE TABLE IF NOT EXISTS byte_cache_entries (owner TEXT NOT NULL, kind TEXT NOT NULL, key TEXT NOT NULL, blob INTEGER NOT NULL, media_type TEXT NOT NULL, meta TEXT NOT NULL, stored_at_ms INTEGER NOT NULL CHECK(stored_at_ms>=0 AND stored_at_ms<=9007199254740991), PRIMARY KEY(owner,kind,key)); CREATE INDEX IF NOT EXISTS byte_cache_entries_by_blob ON byte_cache_entries(blob)";

/// The closed vocabulary of cached bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Survey and Solar media and their thumbnails, by object path or media key.
    Photo,
    /// Digest-pinned published compute artifacts.
    ComputeArtifact,
    /// Saved project assets for offline preview.
    ReportPreview,
    /// Immutable project model versions (`.dsgrid` bytes).
    GridRemoteVersion,
}
impl Kind {
    pub const ALL: [Kind; 4] = [
        Kind::Photo,
        Kind::ComputeArtifact,
        Kind::ReportPreview,
        Kind::GridRemoteVersion,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Kind::Photo => "photo",
            Kind::ComputeArtifact => "compute_artifact",
            Kind::ReportPreview => "report_preview",
            Kind::GridRemoteVersion => "grid_remote_version",
        }
    }
    /// The bytes one owner keeps of this kind; past it the oldest leave first.
    pub fn budget(self) -> u64 {
        match self {
            Kind::Photo => 2048 * MIB,
            Kind::ComputeArtifact | Kind::ReportPreview => 256 * MIB,
            Kind::GridRemoteVersion => 4096 * MIB,
        }
    }
    /// The largest single entry of this kind.
    pub fn max_entry(self) -> u64 {
        self.budget().min(MAX_ENTRY_BYTES)
    }
    pub fn generation_key(self) -> String {
        format!("{STORE}.{}", self.name())
    }
}

/// One entry's name: its kind and a printable key the caller chooses.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub kind: Kind,
    pub key: String,
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// The part size and the bounds, so a binding slices without a constant.
    Limits,
    /// Whether the entry is held whole (at this length); reads no bytes.
    Has {
        key: Key,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        byte_length: Option<u64>,
    },
    /// One part of an entry, verified. Later parts name the `version` the
    /// first answered, so a replaced entry is never read half old.
    Get {
        key: Key,
        #[serde(default)]
        part: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_version: Option<String>,
    },
    /// Keep bytes under a key, part by part for a payload over one part.
    /// A declared `sha256` must match the bytes.
    Put {
        key: Key,
        payload: Share,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        #[serde(default)]
        media_type: String,
        /// A small JSON object kept beside the bytes, answered by `get`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
        now: u64,
    },
    Delete { key: Key },
}

fn invalid(message: impl std::fmt::Display) -> String {
    format!("{INVALID}: {message}")
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
fn kinded(mut outcome: Outcome, kind: Kind) -> Outcome {
    outcome.generation_key = Some(kind.generation_key());
    outcome
}
fn printable(value: &str, max: usize) -> bool {
    value.len() <= max && value.trim() == value && !value.chars().any(char::is_control)
}
fn admit_key(key: &Key) -> Result<(), String> {
    if key.key.is_empty() || !printable(&key.key, MAX_KEY_BYTES) {
        return Err(invalid("a key is printable and holds 1 to 2048 bytes"));
    }
    Ok(())
}
fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn schema(connection: &Connection) -> Result<(), String> {
    connection.execute_batch(&CONTENT.schema()).map_err(sql)?;
    connection.execute_batch(ENTRIES).map_err(sql)
}

/// One held entry and its content.
struct Entry {
    blob_id: i64,
    blob: Option<Blob>,
    media_type: String,
    meta: String,
    stored_at_ms: u64,
}

fn entry(connection: &Connection, owner: &str, key: &Key) -> Result<Option<Entry>, String> {
    let held: Option<(i64, String, String, i64)> = connection
        .query_row(
            "SELECT blob,media_type,meta,stored_at_ms FROM byte_cache_entries WHERE owner=?1 AND kind=?2 AND key=?3",
            params![owner, key.kind.name(), key.key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(sql)?;
    let Some((blob_id, media_type, meta, stored_at_ms)) = held else {
        return Ok(None);
    };
    Ok(Some(Entry {
        blob_id,
        blob: CONTENT.get(connection, blob_id)?,
        media_type,
        meta,
        stored_at_ms: stored_at_ms as u64,
    }))
}

/// Remove content no entry names any more.
fn release(connection: &Connection, blob: i64) -> Result<(), String> {
    let named: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM byte_cache_entries WHERE blob=?1)",
            params![blob],
            |row| row.get(0),
        )
        .map_err(sql)?;
    if !named {
        CONTENT.delete(connection, blob)?;
    }
    Ok(())
}

/// Corrupt content leaves with every entry of its owner naming it.
fn drop_content(connection: &Connection, owner: &str, blob: i64) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM byte_cache_entries WHERE owner=?1 AND blob=?2",
            params![owner, blob],
        )
        .map_err(sql)?;
    CONTENT.delete(connection, blob)
}

fn remove(connection: &Connection, owner: &str, kind: Kind, key: &str) -> Result<bool, String> {
    let blob: Option<i64> = connection
        .query_row(
            "SELECT blob FROM byte_cache_entries WHERE owner=?1 AND kind=?2 AND key=?3",
            params![owner, kind.name(), key],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    let Some(blob) = blob else {
        return Ok(false);
    };
    connection
        .execute(
            "DELETE FROM byte_cache_entries WHERE owner=?1 AND kind=?2 AND key=?3",
            params![owner, kind.name(), key],
        )
        .map_err(sql)?;
    release(connection, blob)?;
    Ok(true)
}

/// The owner's content with this digest when it is held whole, else what
/// `write` keeps.
fn content(connection: &Connection, owner: &str, digest: &str, write: impl FnOnce() -> Result<Option<Blob>, String>) -> Result<Option<Blob>, String> {
    if let Some(held) = CONTENT.find(connection, owner, digest)? {
        if CONTENT.intact(connection, &held)? {
            return Ok(Some(held));
        }
        drop_content(connection, owner, held.id)?;
    }
    write()
}

/// Name `blob` under `key`, then keep the kind within its budget, oldest
/// first, never evicting the entry just written. Answers how many left.
fn name_entry(connection: &Connection, owner: &str, key: &Key, blob: &Blob, media_type: &str, meta: &str, now: u64) -> Result<u64, String> {
    let previous: Option<i64> = connection
        .query_row(
            "SELECT blob FROM byte_cache_entries WHERE owner=?1 AND kind=?2 AND key=?3",
            params![owner, key.kind.name(), key.key],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    connection
        .execute(
            "INSERT INTO byte_cache_entries(owner,kind,key,blob,media_type,meta,stored_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(owner,kind,key) DO UPDATE SET blob=excluded.blob,media_type=excluded.media_type,meta=excluded.meta,stored_at_ms=excluded.stored_at_ms",
            params![owner, key.kind.name(), key.key, blob.id, media_type, meta, now as i64],
        )
        .map_err(sql)?;
    if let Some(old) = previous.filter(|old| *old != blob.id) {
        release(connection, old)?;
    }
    keep_within(connection, owner, key, key.kind.budget())
}

/// The one retention rule: while the owner's entries of a kind exceed the
/// budget, the oldest leave first, never `keep`. Answers how many left.
fn keep_within(connection: &Connection, owner: &str, keep: &Key, budget: u64) -> Result<u64, String> {
    let mut statement = connection
        .prepare("SELECT e.key,b.byte_length FROM byte_cache_entries e JOIN byte_cache_blobs b ON b.id=e.blob WHERE e.owner=?1 AND e.kind=?2 ORDER BY e.stored_at_ms,e.key")
        .map_err(sql)?;
    let held = statement
        .query_map(params![owner, keep.kind.name()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let mut total: u64 = held.iter().map(|(_, length)| length).sum();
    let mut evicted = 0;
    for (oldest, length) in held {
        if total <= budget {
            break;
        }
        if oldest == keep.key {
            continue;
        }
        remove(connection, owner, keep.kind, &oldest)?;
        total -= length;
        evicted += 1;
    }
    Ok(evicted)
}

#[allow(clippy::too_many_arguments)]
fn put(
    connection: &Connection,
    owner: &str,
    key: Key,
    share: Share,
    declared: Option<String>,
    media_type: String,
    meta: Option<Value>,
    now: u64,
    bytes: Option<Vec<u8>>,
) -> Result<Outcome, String> {
    admit_key(&key)?;
    if !printable(&media_type, 255) || now > MAX_SAFE || declared.as_deref().is_some_and(|digest| !hex64(digest)) {
        return Err(invalid("a put names a printable media type, its time and any SHA-256 in lowercase hex"));
    }
    let meta = match meta {
        None => "{}".to_string(),
        Some(meta @ Value::Object(_)) => serde_json::to_string(&meta).map_err(|e| e.to_string())?,
        Some(_) => return Err(invalid("meta is a JSON object")),
    };
    if meta.len() > MAX_META_BYTES {
        return Err(invalid("meta holds at most 16 KiB"));
    }
    if key.kind == Kind::Photo && crate::media_outbox::pending(connection, owner, &key.key)?.is_some() {
        return Ok(kinded(
            Outcome::read(fields(json!({"stored": false, "pending_upload": true}))),
            key.kind,
        ));
    }
    let operation = serde_json::to_string(&(&key, &declared, &media_type, &meta, share.byte_length)).map_err(|e| e.to_string())?;
    let stage = Stage {
        code: INVALID,
        owner,
        store: STORE,
    };
    let payload = match stage.receive(connection, &operation, share, bytes, key.kind.max_entry(), now)? {
        Received::Staged {
            staging,
            received,
            parts,
        } => {
            return Ok(kinded(
                Outcome::read(fields(Received::staged_answer(staging, received, parts))),
                key.kind,
            ))
        }
        Received::Complete(payload) => payload,
    };
    let digest = payload.sha256(connection, INVALID)?;
    if declared.as_ref().is_some_and(|declared| *declared != digest) {
        return Err(invalid("the bytes do not match their declared SHA-256"));
    }
    let blob = content(connection, owner, &digest, || CONTENT.write(connection, owner, &payload, &digest).map(Some))?
        .ok_or_else(|| invalid("the payload could not be kept"))?;
    payload.release(connection, INVALID)?;
    let evicted = name_entry(connection, owner, &key, &blob, &media_type, &meta, now)?;
    Ok(kinded(
        Outcome::written(fields(json!({
            "stored": true,
            "sha256": digest,
            "byte_length": blob.byte_length,
            "evicted": evicted,
        }))),
        key.kind,
    ))
}

fn miss(kind: Kind, wrote: bool, extra: Value) -> Outcome {
    let mut value = fields(extra);
    value.insert("hit".into(), Value::Bool(false));
    kinded(
        if wrote {
            Outcome::written(value)
        } else {
            Outcome::read(value)
        },
        kind,
    )
}

fn hit(kind: Kind, value: Value, bytes: Vec<u8>) -> Outcome {
    let mut value = fields(value);
    value.insert("hit".into(), Value::Bool(true));
    kinded(
        Outcome {
            value,
            bytes: Some(bytes),
            ..Outcome::default()
        },
        kind,
    )
}

fn get(connection: &Connection, owner: &str, key: Key, part: u64, expect_version: Option<String>) -> Result<Outcome, String> {
    admit_key(&key)?;
    let kind = key.kind;
    if kind == Kind::Photo {
        if let Some(pending) = crate::media_outbox::pending(connection, owner, &key.key)? {
            let version = format!("pending:{}", pending.generation);
            if expect_version.as_ref().is_some_and(|expected| *expected != version) {
                return Ok(miss(kind, false, json!({"changed": true})));
            }
            let parts = parts_of(pending.length);
            if parts == 0 {
                return Ok(miss(kind, false, json!({"pending_upload": true})));
            }
            if part >= parts {
                return Err(invalid("the part is past the entry's end"));
            }
            // A pending upload's bytes are work: a read never drops them.
            let Some(bytes) = pending.read(connection, part * PART_BYTES, part_length(pending.length, part))? else {
                return Ok(miss(kind, false, json!({"pending_upload": true})));
            };
            return Ok(hit(
                kind,
                json!({
                    "version": version,
                    "byte_length": pending.length,
                    "parts": parts,
                    "part": part,
                    "media_type": pending.media_type,
                    "meta": {},
                    "stored_at_ms": pending.bytes_at_ms,
                    "pending_upload": true,
                }),
                bytes,
            ));
        }
    }
    let Some(entry) = entry(connection, owner, &key)? else {
        return Ok(miss(kind, false, json!({})));
    };
    let Some(blob) = entry.blob else {
        remove(connection, owner, kind, &key.key)?;
        return Ok(miss(kind, true, json!({"dropped": true})));
    };
    if expect_version.as_ref().is_some_and(|expected| *expected != blob.sha256) {
        return Ok(miss(kind, false, json!({"changed": true})));
    }
    if part >= blob.parts {
        return Err(invalid("the part is past the entry's end"));
    }
    let bytes = if CONTENT.intact(connection, &blob)? {
        CONTENT.part(connection, &blob, part)?
    } else {
        None
    };
    let Some(bytes) = bytes else {
        drop_content(connection, owner, entry.blob_id)?;
        return Ok(miss(kind, true, json!({"dropped": true})));
    };
    Ok(hit(
        kind,
        json!({
            "version": blob.sha256,
            "sha256": blob.sha256,
            "byte_length": blob.byte_length,
            "parts": blob.parts,
            "part": part,
            "media_type": entry.media_type,
            "meta": serde_json::from_str::<Value>(&entry.meta).unwrap_or_else(|_| json!({})),
            "stored_at_ms": entry.stored_at_ms,
            "pending_upload": false,
        }),
        bytes,
    ))
}

fn has(connection: &Connection, owner: &str, key: Key, byte_length: Option<u64>) -> Result<Outcome, String> {
    admit_key(&key)?;
    let pending = match key.kind {
        Kind::Photo => crate::media_outbox::pending(connection, owner, &key.key)?,
        _ => None,
    };
    let held = match pending {
        Some(pending) => byte_length.is_none_or(|length| length == pending.length) && pending.present(connection)?,
        None => match entry(connection, owner, &key)? {
            Some(Entry { blob: Some(blob), .. }) => {
                byte_length.is_none_or(|length| length == blob.byte_length) && CONTENT.intact(connection, &blob)?
            }
            _ => false,
        },
    };
    Ok(kinded(Outcome::read(fields(json!({"has": held}))), key.kind))
}

/// Keep a verified upload's bytes as the cached photo at `path`, copied from
/// the work table that held them, so they stay readable offline once the
/// upload settles. Answers whether the photo was kept.
#[allow(clippy::too_many_arguments)]
pub(crate) fn adopt_photo(
    connection: &Connection,
    owner: &str,
    path: &str,
    media_type: &str,
    from: &Blobs,
    source: &Blob,
    offset: u64,
    length: u64,
    now: u64,
) -> Result<bool, String> {
    let key = Key {
        kind: Kind::Photo,
        key: path.into(),
    };
    if length == 0 || length > Kind::Photo.max_entry() || admit_key(&key).is_err() {
        return Ok(false);
    }
    schema(connection)?;
    let Some(digest) = from.range_sha256(connection, source, offset, length)? else {
        return Ok(false);
    };
    let Some(blob) = content(connection, owner, &digest, || {
        CONTENT.copy_range(connection, owner, &digest, from, source, offset, length)
    })?
    else {
        return Ok(false);
    };
    let media_type = if printable(media_type, 255) { media_type } else { "" };
    name_entry(connection, owner, &key, &blob, media_type, "{}", now)?;
    crate::durable_store::advance(connection, &Kind::Photo.generation_key())?;
    Ok(true)
}

/// Run one operation inside the caller's transaction.
pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() && !matches!(operation, Operation::Put { .. }) {
        return Err(invalid("only a put carries payload bytes"));
    }
    schema(connection)?;
    let owner = input.identity.owner();
    match operation {
        Operation::Limits => {
            let budgets: Map<String, Value> = Kind::ALL
                .iter()
                .map(|kind| (kind.name().to_string(), json!({"budget": kind.budget(), "max_entry": kind.max_entry()})))
                .collect();
            Ok(Outcome::read(fields(json!({"part_bytes": PART_BYTES, "kinds": budgets}))))
        }
        Operation::Has { key, byte_length } => has(connection, &owner, key, byte_length),
        Operation::Get {
            key,
            part,
            expect_version,
        } => get(connection, &owner, key, part, expect_version),
        Operation::Put {
            key,
            payload,
            sha256,
            media_type,
            meta,
            now,
        } => put(connection, &owner, key, payload, sha256, media_type, meta, now, input.bytes),
        Operation::Delete { key } => {
            admit_key(&key)?;
            let deleted = remove(connection, &owner, key.kind, &key.key)?;
            let value = fields(json!({"deleted": deleted}));
            Ok(kinded(
                if deleted {
                    Outcome::written(value)
                } else {
                    Outcome::read(value)
                },
                key.kind,
            ))
        }
    }
}

#[cfg(test)]
#[path = "byte_cache_tests.rs"]
mod tests;
