//! Upload workspaces: the Arrow chunks an import writes while it reads a
//! file, and the chunk-backed features a prepared sketch layer keeps, held in
//! the Kernel's SQLite ([`crate::byte_parts`]) by the same rusqlite code in
//! Core's protected Store and the browser's storage worker.
//!
//! A chunk is work: a person's imported features, read back for previews,
//! cleaning, merges and chunk-backed sketch layers, and removed only by its
//! workspace's own clear. Keys are the caller's (workspace, file, layer,
//! chunk index), so a sketch layer's stored reference keeps naming its
//! chunks; a chunk written again under its key replaces it.
use crate::byte_parts::{Blob, Blobs, Received, Share, Stage};
use crate::durable_store::{Input, Outcome};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "upload_chunks";
pub const INVALID: &str = "upload_chunks_invalid";
/// The one chunk payload format: an Arrow IPC batch of features.
pub const FORMAT: &str = "arrow-ipc-v1";
const MIB: u64 = 1024 * 1024;
pub const MAX_CHUNK_BYTES: u64 = 512 * MIB;
pub const MAX_NAME_BYTES: usize = 1024;
pub const MAX_CHUNK_INDEX: u64 = 1_000_000_000;
/// The most chunks one listing answers.
pub const MAX_LISTED: usize = 100_000;
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const BLOBS: Blobs = Blobs {
    blobs: "upload_chunks_blobs",
    parts: "upload_chunks_parts",
    code: INVALID,
    content_addressed: false,
};
const TABLE: &str = "CREATE TABLE IF NOT EXISTS upload_chunks (owner TEXT NOT NULL, workspace TEXT NOT NULL, file TEXT NOT NULL, layer TEXT NOT NULL, chunk INTEGER NOT NULL CHECK(chunk>=0 AND chunk<=1000000000), format TEXT NOT NULL, feature_count INTEGER NOT NULL CHECK(feature_count>=0 AND feature_count<=9007199254740991), blob INTEGER NOT NULL, stored_at_ms INTEGER NOT NULL CHECK(stored_at_ms>=0 AND stored_at_ms<=9007199254740991), PRIMARY KEY(owner,workspace,file,layer,chunk))";

/// One chunk's key, exactly as the upload workspace names it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChunkKey {
    pub workspace: String,
    pub file: String,
    /// The parsed layer within the file; a single-layer file may name none.
    pub layer: String,
    pub chunk: u64,
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Write one chunk, replacing what its key held.
    Put {
        key: ChunkKey,
        format: String,
        feature_count: u64,
        payload: Share,
        now: u64,
    },
    /// One part of a chunk's payload.
    Get {
        key: ChunkKey,
        #[serde(default)]
        part: u64,
    },
    /// A workspace's chunks without their bytes, in file, layer and chunk
    /// order; narrowed to one file, or one layer of it.
    List {
        workspace: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<String>,
    },
    ClearLayer {
        workspace: String,
        file: String,
        layer: String,
    },
    ClearWorkspace { workspace: String },
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
fn name(value: &str, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty()) && value.len() <= MAX_NAME_BYTES && !value.chars().any(char::is_control)
}
fn admit(key: &ChunkKey) -> Result<(), String> {
    if !name(&key.workspace, false) || !name(&key.file, false) || !name(&key.layer, true) || key.chunk > MAX_CHUNK_INDEX {
        return Err(invalid("a chunk names its workspace, file, layer and index"));
    }
    Ok(())
}

pub(crate) fn schema(connection: &Connection) -> Result<(), String> {
    connection.execute_batch(&BLOBS.schema()).map_err(sql)?;
    connection.execute_batch(TABLE).map_err(sql)
}

/// A held chunk's row.
struct Held {
    format: String,
    feature_count: u64,
    blob_id: i64,
    blob: Option<Blob>,
}

fn load(connection: &Connection, owner: &str, key: &ChunkKey) -> Result<Option<Held>, String> {
    let held: Option<(String, i64, i64)> = connection
        .query_row(
            "SELECT format,feature_count,blob FROM upload_chunks WHERE owner=?1 AND workspace=?2 AND file=?3 AND layer=?4 AND chunk=?5",
            params![owner, key.workspace, key.file, key.layer, key.chunk as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sql)?;
    let Some((format, feature_count, blob_id)) = held else {
        return Ok(None);
    };
    Ok(Some(Held {
        format,
        feature_count: feature_count as u64,
        blob_id,
        blob: BLOBS.get(connection, blob_id)?,
    }))
}

/// Write a chunk's row over a newly kept payload; answers whether it was
/// written (`replace` false keeps a held chunk and drops the new payload).
#[allow(clippy::too_many_arguments)]
fn store(connection: &Connection, owner: &str, key: &ChunkKey, format: &str, feature_count: u64, blob: &Blob, now: u64) -> Result<(), String> {
    let previous = load(connection, owner, key)?;
    connection
        .execute(
            "INSERT INTO upload_chunks(owner,workspace,file,layer,chunk,format,feature_count,blob,stored_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(owner,workspace,file,layer,chunk) DO UPDATE SET format=excluded.format,feature_count=excluded.feature_count,blob=excluded.blob,stored_at_ms=excluded.stored_at_ms",
            params![owner, key.workspace, key.file, key.layer, key.chunk as i64, format, feature_count as i64, blob.id, now as i64],
        )
        .map_err(sql)?;
    if let Some(previous) = previous.filter(|previous| previous.blob_id != blob.id) {
        BLOBS.delete(connection, previous.blob_id)?;
    }
    Ok(())
}

fn described(key: &ChunkKey, format: &str, feature_count: u64, byte_size: u64) -> Value {
    json!({
        "file": key.file,
        "layer": key.layer,
        "chunk": key.chunk,
        "format": format,
        "feature_count": feature_count,
        "byte_size": byte_size,
    })
}

#[allow(clippy::too_many_arguments)]
fn put(connection: &Connection, owner: &str, key: ChunkKey, format: String, feature_count: u64, share: Share, now: u64, bytes: Option<Vec<u8>>) -> Result<Outcome, String> {
    admit(&key)?;
    if format != FORMAT || feature_count > MAX_SAFE || now > MAX_SAFE {
        return Err(invalid("a chunk is an arrow-ipc-v1 batch with its feature count and time"));
    }
    let operation = serde_json::to_string(&(&key, &format, feature_count, share.byte_length)).map_err(|e| e.to_string())?;
    let stage = Stage {
        code: INVALID,
        owner,
        store: STORE,
    };
    let payload = match stage.receive(connection, &operation, share, bytes, MAX_CHUNK_BYTES, now)? {
        Received::Staged {
            staging,
            received,
            parts,
        } => return Ok(Outcome::read(fields(Received::staged_answer(staging, received, parts)))),
        Received::Complete(payload) => payload,
    };
    let digest = payload.sha256(connection, INVALID)?;
    let blob = BLOBS.write(connection, owner, &payload, &digest)?;
    payload.release(connection, INVALID)?;
    store(connection, owner, &key, &format, feature_count, &blob, now)?;
    Ok(Outcome::written(fields(json!({
        "chunk": described(&key, &format, feature_count, blob.byte_length),
    }))))
}

fn get(connection: &Connection, owner: &str, key: ChunkKey, part: u64) -> Result<Outcome, String> {
    admit(&key)?;
    let Some(held) = load(connection, owner, &key)? else {
        return Ok(Outcome::read(fields(json!({"held": false}))));
    };
    let byte_size = held.blob.as_ref().map_or(0, |blob| blob.byte_length);
    let mut value = described(&key, &held.format, held.feature_count, byte_size);
    value["held"] = Value::Bool(true);
    let bytes = match &held.blob {
        Some(blob) if part >= blob.parts => return Err(invalid("the part is past the chunk's end")),
        Some(blob) => BLOBS.part(connection, blob, part)?,
        None => None,
    };
    value["parts"] = json!(held.blob.as_ref().map_or(0, |blob| blob.parts));
    value["part"] = json!(part);
    // A chunk is work: damaged bytes are reported, never dropped.
    value["damaged"] = Value::Bool(bytes.is_none());
    Ok(Outcome {
        value: fields(value),
        bytes,
        ..Outcome::default()
    })
}

fn list(connection: &Connection, owner: &str, workspace: String, file: Option<String>, layer: Option<String>) -> Result<Outcome, String> {
    if !name(&workspace, false)
        || file.as_deref().is_some_and(|file| !name(file, false))
        || layer.as_deref().is_some_and(|layer| !name(layer, true))
        || (layer.is_some() && file.is_none())
    {
        return Err(invalid("a listing names its workspace, then a file, then a layer"));
    }
    let mut statement = connection
        .prepare("SELECT c.file,c.layer,c.chunk,c.format,c.feature_count,COALESCE(b.byte_length,0) FROM upload_chunks c LEFT JOIN upload_chunks_blobs b ON b.id=c.blob WHERE c.owner=?1 AND c.workspace=?2 AND (?3 IS NULL OR c.file=?3) AND (?4 IS NULL OR c.layer=?4) ORDER BY c.file,c.layer,c.chunk LIMIT ?5")
        .map_err(sql)?;
    let rows = statement
        .query_map(params![owner, workspace, file, layer, (MAX_LISTED + 1) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(sql)?;
    let mut chunks = Vec::new();
    for row in rows {
        let (file, layer, chunk, format, feature_count, byte_size) = row.map_err(sql)?;
        if chunks.len() == MAX_LISTED {
            return Err(invalid("the workspace holds more chunks than one listing answers; narrow it to a file or layer"));
        }
        let key = ChunkKey {
            workspace: workspace.clone(),
            file,
            layer,
            chunk: chunk as u64,
        };
        chunks.push(described(&key, &format, feature_count as u64, byte_size as u64));
    }
    Ok(Outcome::read(fields(json!({"chunks": chunks}))))
}

/// Remove every chunk the filter matches, with its bytes.
fn clear(connection: &Connection, owner: &str, workspace: &str, file_layer: Option<(&str, &str)>) -> Result<Outcome, String> {
    let (file, layer) = file_layer.unzip();
    let mut statement = connection
        .prepare("SELECT blob FROM upload_chunks WHERE owner=?1 AND workspace=?2 AND (?3 IS NULL OR file=?3) AND (?4 IS NULL OR layer=?4)")
        .map_err(sql)?;
    let blobs = statement
        .query_map(params![owner, workspace, file, layer], |row| row.get::<_, i64>(0))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    for blob in &blobs {
        BLOBS.delete(connection, *blob)?;
    }
    connection
        .execute(
            "DELETE FROM upload_chunks WHERE owner=?1 AND workspace=?2 AND (?3 IS NULL OR file=?3) AND (?4 IS NULL OR layer=?4)",
            params![owner, workspace, file, layer],
        )
        .map_err(sql)?;
    let value = fields(json!({"cleared": blobs.len()}));
    Ok(if blobs.is_empty() {
        Outcome::read(value)
    } else {
        Outcome::written(value)
    })
}

/// Run one operation inside the caller's transaction.
pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() && !matches!(operation, Operation::Put { .. }) {
        return Err(invalid("this operation carries no payload bytes"));
    }
    schema(connection)?;
    let owner = input.identity.owner();
    match operation {
        Operation::Put {
            key,
            format,
            feature_count,
            payload,
            now,
        } => put(connection, &owner, key, format, feature_count, payload, now, input.bytes),
        Operation::Get { key, part } => get(connection, &owner, key, part),
        Operation::List { workspace, file, layer } => list(connection, &owner, workspace, file, layer),
        Operation::ClearLayer { workspace, file, layer } => {
            if !name(&workspace, false) || !name(&file, false) || !name(&layer, true) {
                return Err(invalid("a clear names its workspace, file and layer"));
            }
            clear(connection, &owner, &workspace, Some((&file, &layer)))
        }
        Operation::ClearWorkspace { workspace } => {
            if !name(&workspace, false) {
                return Err(invalid("a clear names its workspace"));
            }
            clear(connection, &owner, &workspace, None)
        }
    }
}

#[cfg(test)]
#[path = "upload_chunks_tests.rs"]
mod tests;
