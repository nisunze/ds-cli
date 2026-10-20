//! A legacy source too large for one call imports in batches. Each batch
//! writes its entries (never replacing a newer row) and adds them to this
//! identity's running tally; the finishing call records the one receipt
//! through [`crate::durable_store::import_once`] and clears the tally. A batch
//! after the receipt refuses, so the import runs once per identity, store and
//! source; a batch repeated after an interruption writes nothing new.
use crate::durable_store::{held_receipt, import_once, Identity, ImportReceipt};
use rusqlite::{params, Connection, OptionalExtension};

pub const INVALID: &str = "durable_store_invalid";
/// The most entries one batch carries.
pub const MAX_ENTRIES: usize = 4096;
pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS durable_import_batches (owner TEXT NOT NULL, store TEXT NOT NULL, source TEXT NOT NULL, count INTEGER NOT NULL CHECK(count>=0), skipped INTEGER NOT NULL CHECK(skipped>=0), chain TEXT NOT NULL, PRIMARY KEY(owner,store,source))";

fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}

/// One batch's entries as the host handed them over, and what they wrote.
pub struct Batch<'a> {
    pub entries: &'a str,
    pub count: u64,
    pub skipped: u64,
}

/// Refuse a batch of a source this identity already imported.
pub fn open(connection: &Connection, identity: &Identity, store: &str, source: &str) -> Result<(), String> {
    if held_receipt(connection, identity, store, source)?.is_some() {
        return Err(format!("{INVALID}: {store} already imported {source} for this identity"));
    }
    Ok(())
}

/// Add one written batch to the tally: counts, and the SHA-256 chain over
/// every batch's entries in the order they arrived.
pub fn tally(connection: &Connection, identity: &Identity, store: &str, source: &str, batch: Batch) -> Result<(), String> {
    connection.execute_batch(TABLE).map_err(sql)?;
    let owner = identity.owner();
    let chain: String = connection
        .query_row(
            "SELECT chain FROM durable_import_batches WHERE owner=?1 AND store=?2 AND source=?3",
            params![owner, store, source],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?
        .unwrap_or_default();
    let next = crate::durable_store::sha256(format!("{chain}\n{}", crate::durable_store::sha256(batch.entries.as_bytes())).as_bytes());
    connection
        .execute(
            "INSERT INTO durable_import_batches(owner,store,source,count,skipped,chain) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(owner,store,source) DO UPDATE SET count=count+excluded.count,skipped=skipped+excluded.skipped,chain=excluded.chain",
            params![owner, store, source, batch.count as i64, batch.skipped as i64, next],
        )
        .map_err(sql)?;
    Ok(())
}

/// Record the receipt from the tally (an import with no batch records an
/// empty one) and clear the tally. `pruned` written entries the finish
/// removed again count as skipped.
pub fn finish(connection: &Connection, identity: &Identity, store: &str, source: &str, now: u64, pruned: u64) -> Result<(ImportReceipt, bool), String> {
    connection.execute_batch(TABLE).map_err(sql)?;
    let owner = identity.owner();
    let held: (i64, i64, String) = connection
        .query_row(
            "SELECT count,skipped,chain FROM durable_import_batches WHERE owner=?1 AND store=?2 AND source=?3",
            params![owner, store, source],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sql)?
        .unwrap_or((0, 0, crate::durable_store::sha256(b"")));
    let (count, skipped, chain) = (held.0 as u64, held.1 as u64, held.2);
    let moved = pruned.min(count);
    let receipt = import_once(connection, identity, store, source, &chain, now, || Ok((count - moved, skipped + moved)))?;
    connection
        .execute(
            "DELETE FROM durable_import_batches WHERE owner=?1 AND store=?2 AND source=?3",
            params![owner, store, source],
        )
        .map_err(sql)?;
    Ok(receipt)
}
