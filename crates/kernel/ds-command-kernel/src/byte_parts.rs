//! Bytes in the Kernel's durable stores, by parts: the one mechanism every
//! byte store shares (`byte_cache`, `media_outbox`, `upload_chunks`).
//!
//! A payload is cut into parts of at most [`PART_BYTES`]. A part is one
//! stored row and the most one call carries, so neither a transaction nor a
//! host holds more than one part of a large object at once (a call is
//! bounded at 64 MiB, [`crate::durable_store::MAX_BYTES`]). A payload over
//! one part arrives as consecutive calls of the same operation with `part`
//! 0..n-1: the parts wait in staging, and only the last call takes effect,
//! once its store has verified the whole payload. A staging is bound to its
//! owner, store and operation; an untouched one is swept after
//! [`STAGING_TTL_MS`].
//!
//! Each owning table is a pair: `<name>_blobs`, one row per object (its
//! owner, SHA-256, length and part count), and `<name>_parts`, its parts
//! aligned on [`PART_BYTES`], each with its own SHA-256. A read verifies
//! exactly the parts it returns, so a corrupt part is found without
//! rehashing the object, and a presence check compares stored lengths
//! without reading a byte.
use crate::durable_store::sha256;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The most bytes one stored part, and one call's payload, holds.
pub const PART_BYTES: u64 = 16 * 1024 * 1024;
/// A staging untouched for this long is swept when its store starts another.
pub const STAGING_TTL_MS: u64 = 60 * 60 * 1000;
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const STAGING: &str = "CREATE TABLE IF NOT EXISTS byte_staging (id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, store TEXT NOT NULL, operation TEXT NOT NULL, byte_length INTEGER NOT NULL CHECK(byte_length>0 AND byte_length<=9007199254740991), received INTEGER NOT NULL CHECK(received>=0), touched_at_ms INTEGER NOT NULL CHECK(touched_at_ms>=0 AND touched_at_ms<=9007199254740991)); CREATE INDEX IF NOT EXISTS byte_staging_by_store ON byte_staging(owner,store,touched_at_ms); CREATE TABLE IF NOT EXISTS byte_staging_parts (staging INTEGER NOT NULL, part INTEGER NOT NULL CHECK(part>=0), sha256 TEXT NOT NULL, bytes BLOB NOT NULL, PRIMARY KEY(staging,part))";

/// How many parts a payload of this length has.
pub fn parts_of(byte_length: u64) -> u64 {
    byte_length.div_ceil(PART_BYTES)
}

/// The length of one part of a payload of this length.
pub fn part_length(byte_length: u64, part: u64) -> u64 {
    byte_length
        .saturating_sub(part.saturating_mul(PART_BYTES))
        .min(PART_BYTES)
}

/// One call's share of a byte payload, as an operation carries it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Share {
    /// The whole payload's length.
    pub byte_length: u64,
    /// The part this call carries, from 0.
    #[serde(default)]
    pub part: u64,
    /// The staging the first part's answer named; every later part names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging: Option<u64>,
}

/// What one call of a payload brought.
pub enum Received {
    /// More parts are due; the store answers this and changes nothing else.
    Staged {
        staging: u64,
        received: u64,
        parts: u64,
    },
    /// The whole payload is here.
    Complete(Payload),
}
impl Received {
    /// The answer a store gives a call that staged a part.
    pub fn staged_answer(staging: u64, received: u64, parts: u64) -> serde_json::Value {
        serde_json::json!({"staged": {"staging": staging, "received": received, "parts": parts}})
    }
}

/// A complete payload: one part in memory, or every part in staging.
pub struct Payload {
    byte_length: u64,
    source: Source,
}
enum Source {
    Whole(Vec<u8>),
    Staged(i64),
}

/// Who a staged payload belongs to and how its refusals read.
#[derive(Clone, Copy)]
pub struct Stage<'a> {
    /// The store's refusal code.
    pub code: &'a str,
    pub owner: &'a str,
    pub store: &'a str,
}

fn refuse(code: &str, message: impl std::fmt::Display) -> String {
    format!("{code}: {message}")
}

impl Stage<'_> {
    /// Admit one call's part. `operation` is the store's canonical text of
    /// the operation without its part and staging, so a later part can only
    /// continue the payload it began. `max_length` is the store's bound.
    pub fn receive(
        &self,
        connection: &Connection,
        operation: &str,
        share: Share,
        bytes: Option<Vec<u8>>,
        max_length: u64,
        now: u64,
    ) -> Result<Received, String> {
        let code = self.code;
        let sql = |error: rusqlite::Error| refuse(code, error);
        let bytes = bytes.ok_or_else(|| refuse(code, "this operation carries its payload bytes"))?;
        if share.byte_length == 0 || share.byte_length > max_length {
            return Err(refuse(code, format!("a payload holds 1 to {max_length} bytes")));
        }
        if now > MAX_SAFE {
            return Err(refuse(code, "the time exceeds its bound"));
        }
        let parts = parts_of(share.byte_length);
        if share.part >= parts {
            return Err(refuse(code, "the part is past the payload's end"));
        }
        if bytes.len() as u64 != part_length(share.byte_length, share.part) {
            return Err(refuse(code, "a part carries exactly its share of the payload"));
        }
        if parts == 1 {
            if share.staging.is_some() {
                return Err(refuse(code, "a one-part payload is never staged"));
            }
            return Ok(Received::Complete(Payload {
                byte_length: share.byte_length,
                source: Source::Whole(bytes),
            }));
        }
        connection.execute_batch(STAGING).map_err(sql)?;
        let operation = sha256(operation.as_bytes());
        let staging = if share.part == 0 {
            if share.staging.is_some() {
                return Err(refuse(code, "the first part opens its own staging"));
            }
            let stale = now.saturating_sub(STAGING_TTL_MS) as i64;
            connection
                .execute(
                    "DELETE FROM byte_staging_parts WHERE staging IN (SELECT id FROM byte_staging WHERE owner=?1 AND store=?2 AND touched_at_ms<?3)",
                    params![self.owner, self.store, stale],
                )
                .map_err(sql)?;
            connection
                .execute(
                    "DELETE FROM byte_staging WHERE owner=?1 AND store=?2 AND touched_at_ms<?3",
                    params![self.owner, self.store, stale],
                )
                .map_err(sql)?;
            connection
                .execute(
                    "INSERT INTO byte_staging(owner,store,operation,byte_length,received,touched_at_ms) VALUES(?1,?2,?3,?4,0,?5)",
                    params![self.owner, self.store, operation, share.byte_length as i64, now as i64],
                )
                .map_err(sql)?;
            connection.last_insert_rowid()
        } else {
            let gone = || refuse(code, "the staged payload is gone or out of order; send it again from part 0");
            let id = share
                .staging
                .and_then(|id| i64::try_from(id).ok())
                .ok_or_else(gone)?;
            let held: Option<(String, String, String, i64, i64)> = connection
                .query_row(
                    "SELECT owner,store,operation,byte_length,received FROM byte_staging WHERE id=?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .optional()
                .map_err(sql)?;
            match held {
                Some((owner, store, held_operation, length, received))
                    if owner == self.owner
                        && store == self.store
                        && held_operation == operation
                        && length as u64 == share.byte_length
                        && received as u64 == share.part => {}
                _ => return Err(gone()),
            }
            id
        };
        connection
            .execute(
                "INSERT INTO byte_staging_parts(staging,part,sha256,bytes) VALUES(?1,?2,?3,?4)",
                params![staging, share.part as i64, sha256(&bytes), bytes],
            )
            .map_err(sql)?;
        connection
            .execute(
                "UPDATE byte_staging SET received=received+1,touched_at_ms=?2 WHERE id=?1",
                params![staging, now as i64],
            )
            .map_err(sql)?;
        if share.part + 1 < parts {
            return Ok(Received::Staged {
                staging: staging as u64,
                received: share.part + 1,
                parts,
            });
        }
        Ok(Received::Complete(Payload {
            byte_length: share.byte_length,
            source: Source::Staged(staging),
        }))
    }
}

impl Payload {
    pub fn byte_length(&self) -> u64 {
        self.byte_length
    }
    pub fn parts(&self) -> u64 {
        parts_of(self.byte_length)
    }
    /// The SHA-256 of the whole payload, read part by part.
    pub fn sha256(&self, connection: &Connection, code: &str) -> Result<String, String> {
        let mut hasher = Sha256::new();
        match &self.source {
            Source::Whole(bytes) => hasher.update(bytes),
            Source::Staged(staging) => {
                for part in 0..self.parts() {
                    hasher.update(staged_part(connection, code, *staging, part)?);
                }
            }
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
    /// Remove the staging once its store kept the payload.
    pub fn release(self, connection: &Connection, code: &str) -> Result<(), String> {
        if let Source::Staged(staging) = self.source {
            let sql = |error: rusqlite::Error| refuse(code, error);
            connection
                .execute("DELETE FROM byte_staging_parts WHERE staging=?1", params![staging])
                .map_err(sql)?;
            connection
                .execute("DELETE FROM byte_staging WHERE id=?1", params![staging])
                .map_err(sql)?;
        }
        Ok(())
    }
}

fn staged_part(connection: &Connection, code: &str, staging: i64, part: u64) -> Result<Vec<u8>, String> {
    connection
        .query_row(
            "SELECT bytes FROM byte_staging_parts WHERE staging=?1 AND part=?2",
            params![staging, part as i64],
            |row| row.get(0),
        )
        .map_err(|error| refuse(code, error))
}

/// One owning table pair and the refusal code of its store.
#[derive(Clone, Copy, Debug)]
pub struct Blobs {
    pub blobs: &'static str,
    pub parts: &'static str,
    pub code: &'static str,
    /// One object per owner and content: a store that shares content keeps
    /// `(owner, sha256)` unique and finds it through [`Blobs::find`].
    pub content_addressed: bool,
}

/// What a blobs row records of one object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub id: i64,
    pub sha256: String,
    pub byte_length: u64,
    pub parts: u64,
}

impl Blobs {
    fn sql(&self, error: rusqlite::Error) -> String {
        refuse(self.code, error)
    }
    pub fn schema(&self) -> String {
        let index = if self.content_addressed {
            format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS {b}_by_content ON {b}(owner,sha256);",
                b = self.blobs
            )
        } else {
            String::new()
        };
        format!(
            "CREATE TABLE IF NOT EXISTS {b} (id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, sha256 TEXT NOT NULL, byte_length INTEGER NOT NULL CHECK(byte_length>0 AND byte_length<=9007199254740991), parts INTEGER NOT NULL CHECK(parts>0)); {index} CREATE TABLE IF NOT EXISTS {p} (blob INTEGER NOT NULL, part INTEGER NOT NULL CHECK(part>=0), sha256 TEXT NOT NULL, bytes BLOB NOT NULL, PRIMARY KEY(blob,part))",
            b = self.blobs,
            p = self.parts
        )
    }

    fn insert(&self, connection: &Connection, owner: &str, sha256: &str, byte_length: u64) -> Result<i64, String> {
        connection
            .execute(
                &format!("INSERT INTO {}(owner,sha256,byte_length,parts) VALUES(?1,?2,?3,?4)", self.blobs),
                params![owner, sha256, byte_length as i64, parts_of(byte_length) as i64],
            )
            .map_err(|e| self.sql(e))?;
        Ok(connection.last_insert_rowid())
    }

    /// Keep a received payload as one object of `owner`; `sha256` is the
    /// payload's own digest ([`Payload::sha256`]).
    pub fn write(&self, connection: &Connection, owner: &str, payload: &Payload, sha256: &str) -> Result<Blob, String> {
        let id = self.insert(connection, owner, sha256, payload.byte_length)?;
        match &payload.source {
            Source::Whole(bytes) => connection.execute(
                &format!("INSERT INTO {}(blob,part,sha256,bytes) VALUES(?1,0,?2,?3)", self.parts),
                params![id, sha256, bytes],
            ),
            Source::Staged(staging) => connection.execute(
                &format!(
                    "INSERT INTO {}(blob,part,sha256,bytes) SELECT ?1,part,sha256,bytes FROM byte_staging_parts WHERE staging=?2 ORDER BY part",
                    self.parts
                ),
                params![id, staging],
            ),
        }
        .map_err(|e| self.sql(e))?;
        Ok(Blob {
            id,
            sha256: sha256.into(),
            byte_length: payload.byte_length,
            parts: payload.parts(),
        })
    }

    /// Copy a verified range of another table's object into a new object of
    /// `owner`, cut on this table's part boundaries; `None` when a part of
    /// the range is missing or corrupt. `sha256` is the range's digest.
    #[allow(clippy::too_many_arguments)]
    pub fn copy_range(
        &self,
        connection: &Connection,
        owner: &str,
        sha256_of_range: &str,
        from: &Blobs,
        source: &Blob,
        offset: u64,
        length: u64,
    ) -> Result<Option<Blob>, String> {
        if length == 0 {
            return Ok(None);
        }
        let id = self.insert(connection, owner, sha256_of_range, length)?;
        for part in 0..parts_of(length) {
            let size = part_length(length, part);
            let Some(bytes) = from.range(connection, source, offset + part * PART_BYTES, size)? else {
                self.delete(connection, id)?;
                return Ok(None);
            };
            connection
                .execute(
                    &format!("INSERT INTO {}(blob,part,sha256,bytes) VALUES(?1,?2,?3,?4)", self.parts),
                    params![id, part as i64, sha256(&bytes), bytes],
                )
                .map_err(|e| self.sql(e))?;
        }
        Ok(Some(Blob {
            id,
            sha256: sha256_of_range.into(),
            byte_length: length,
            parts: parts_of(length),
        }))
    }

    pub fn get(&self, connection: &Connection, id: i64) -> Result<Option<Blob>, String> {
        connection
            .query_row(
                &format!("SELECT sha256,byte_length,parts FROM {} WHERE id=?1", self.blobs),
                params![id],
                |row| {
                    Ok(Blob {
                        id,
                        sha256: row.get(0)?,
                        byte_length: row.get::<_, i64>(1)? as u64,
                        parts: row.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .optional()
            .map_err(|e| self.sql(e))
    }

    /// The object `owner` holds with this content, if any.
    pub fn find(&self, connection: &Connection, owner: &str, sha256: &str) -> Result<Option<Blob>, String> {
        let id: Option<i64> = connection
            .query_row(
                &format!("SELECT id FROM {} WHERE owner=?1 AND sha256=?2 ORDER BY id LIMIT 1", self.blobs),
                params![owner, sha256],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| self.sql(e))?;
        match id {
            Some(id) => self.get(connection, id),
            None => Ok(None),
        }
    }

    /// Whether every part is held at its recorded length; reads no bytes.
    pub fn intact(&self, connection: &Connection, blob: &Blob) -> Result<bool, String> {
        let (count, total): (i64, i64) = connection
            .query_row(
                &format!("SELECT COUNT(*),COALESCE(SUM(length(bytes)),0) FROM {} WHERE blob=?1", self.parts),
                params![blob.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| self.sql(e))?;
        Ok(count as u64 == blob.parts
            && total as u64 == blob.byte_length
            && blob.parts == parts_of(blob.byte_length))
    }

    /// One part, verified against its own digest and its place in the
    /// object; `None` when it is missing or corrupt.
    pub fn part(&self, connection: &Connection, blob: &Blob, part: u64) -> Result<Option<Vec<u8>>, String> {
        if part >= blob.parts {
            return Ok(None);
        }
        let held: Option<(String, Vec<u8>)> = connection
            .query_row(
                &format!("SELECT sha256,bytes FROM {} WHERE blob=?1 AND part=?2", self.parts),
                params![blob.id, part as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| self.sql(e))?;
        Ok(held
            .filter(|(digest, bytes)| {
                bytes.len() as u64 == part_length(blob.byte_length, part) && sha256(bytes) == *digest
            })
            .map(|(_, bytes)| bytes))
    }

    fn checked_range(&self, blob: &Blob, offset: u64, length: u64) -> Result<(), String> {
        if offset.checked_add(length).is_none_or(|end| end > blob.byte_length) {
            return Err(refuse(self.code, "the range is past the object's end"));
        }
        Ok(())
    }

    /// A range of an object, verified part by part; `None` when a part it
    /// touches is missing or corrupt.
    pub fn range(&self, connection: &Connection, blob: &Blob, offset: u64, length: u64) -> Result<Option<Vec<u8>>, String> {
        self.checked_range(blob, offset, length)?;
        let mut bytes = Vec::with_capacity(length as usize);
        let found = self.visit(connection, blob, offset, length, |slice| bytes.extend_from_slice(slice))?;
        Ok(found.then_some(bytes))
    }

    /// The SHA-256 of a range, verified part by part; `None` when a part it
    /// touches is missing or corrupt.
    pub fn range_sha256(&self, connection: &Connection, blob: &Blob, offset: u64, length: u64) -> Result<Option<String>, String> {
        self.checked_range(blob, offset, length)?;
        let mut hasher = Sha256::new();
        let found = self.visit(connection, blob, offset, length, |slice| hasher.update(slice))?;
        Ok(found.then(|| format!("{:x}", hasher.finalize())))
    }

    fn visit(
        &self,
        connection: &Connection,
        blob: &Blob,
        offset: u64,
        length: u64,
        mut take: impl FnMut(&[u8]),
    ) -> Result<bool, String> {
        let end = offset + length;
        let mut at = offset;
        while at < end {
            let part = at / PART_BYTES;
            let Some(bytes) = self.part(connection, blob, part)? else {
                return Ok(false);
            };
            let start = (at - part * PART_BYTES) as usize;
            let stop = ((end - part * PART_BYTES) as usize).min(bytes.len());
            if stop <= start {
                return Ok(false);
            }
            take(&bytes[start..stop]);
            at += (stop - start) as u64;
        }
        Ok(true)
    }

    pub fn delete(&self, connection: &Connection, id: i64) -> Result<(), String> {
        connection
            .execute(&format!("DELETE FROM {} WHERE blob=?1", self.parts), params![id])
            .map_err(|e| self.sql(e))?;
        connection
            .execute(&format!("DELETE FROM {} WHERE id=?1", self.blobs), params![id])
            .map_err(|e| self.sql(e))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "byte_parts_tests.rs"]
mod tests;
