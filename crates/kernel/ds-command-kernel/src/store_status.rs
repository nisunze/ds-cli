//! Store status and persistence (owner, 2026-10-24): DS is offline-first on
//! every edge, and what the Kernel holds locally is held forever. This module
//! answers `ds.store-status/v1`, the one typed record every platform reports,
//! asks the browser once to make its storage persistent, and names the one
//! refusal for a write the device has no room for.
//!
//! - Core and the Desktop's native store run the `store_status` durable-store
//!   call: usage from the database file and its journal, quota from the free
//!   disk where the platform can tell (otherwise `null`), `persisted: true`.
//! - The hosted Web builds the record from the browser's
//!   `navigator.storage.estimate()` and `persisted()` ([`browser`]), after the
//!   storage worker asked `persist()` once ([`Persistence`]).
//! - [`refine`] turns a full device or an exhausted browser quota into
//!   `store_quota_exhausted` with a remedy, wherever the store can tell.
use crate::durable_store::{Input, Outcome};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::Cell;

pub const STORE: &str = "store_status";
pub const SCHEMA: &str = "ds.store-status/v1";
/// A write the device or browser has no room for.
pub const QUOTA_EXHAUSTED: &str = "store_quota_exhausted";
/// The Desktop shell's native store file; any other file is Core's store.
pub const DESKTOP_FILE: &str = "kernel-store.sqlite";

/// Where the held data lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Opfs,
    NativeSqlite,
    CoreStore,
}

/// Whether the platform promised never to evict the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Persisted {
    Known(bool),
    Unknown(Unknown),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unknown {
    Unknown,
}
impl Persisted {
    pub const UNKNOWN: Persisted = Persisted::Unknown(Unknown::Unknown);
}

/// `ds.store-status/v1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreStatus {
    pub schema: String,
    pub persisted: Persisted,
    pub usage_bytes: u64,
    pub quota_bytes: Option<u64>,
    pub backend: Backend,
}
impl StoreStatus {
    pub fn new(
        persisted: Persisted,
        usage_bytes: u64,
        quota_bytes: Option<u64>,
        backend: Backend,
    ) -> Self {
        Self {
            schema: SCHEMA.into(),
            persisted,
            usage_bytes,
            quota_bytes,
            backend,
        }
    }
    fn into_map(self) -> Map<String, Value> {
        match serde_json::to_value(self) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        }
    }
}

/// The durable-store operation: one read, no parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Status,
}

/// Native hosts: the database file, its journal and the free disk.
pub fn execute(
    connection: &Connection,
    _input: Input<'_>,
    operation: Operation,
) -> Result<Outcome, String> {
    let Operation::Status = operation;
    let path: String = connection
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name='main'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("{}: {e}", crate::durable_store::INVALID))?;
    let (usage, quota) = if path.is_empty() {
        (pages(connection)?, None)
    } else {
        (
            ["", "-wal", "-journal"]
                .iter()
                .filter_map(|suffix| std::fs::metadata(format!("{path}{suffix}")).ok())
                .map(|meta| meta.len())
                .sum(),
            free_disk(&path),
        )
    };
    let desktop = std::path::Path::new(&path)
        .file_name()
        .is_some_and(|name| name == DESKTOP_FILE);
    let backend = if desktop {
        Backend::NativeSqlite
    } else {
        Backend::CoreStore
    };
    Ok(Outcome::read(
        StoreStatus::new(Persisted::Known(true), usage, quota, backend).into_map(),
    ))
}
fn pages(connection: &Connection) -> Result<u64, String> {
    let count: i64 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let size: i64 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(u64::try_from(count.saturating_mul(size)).unwrap_or(0))
}
/// Bytes available to this process on the file's volume, where the platform
/// tells; `None` otherwise.
#[cfg(unix)]
fn free_disk(path: &str) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let parent = std::path::Path::new(path).parent()?;
    let c_path = std::ffi::CString::new(parent.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `c_path` is a NUL-terminated path and `stat` is a valid out value.
    let status = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    // SAFETY: statvfs returned success, so it initialised the structure.
    let stat = unsafe { stat.assume_init() };
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}
#[cfg(not(unix))]
fn free_disk(_path: &str) -> Option<u64> {
    None
}

/// A native host's answer to the `store_status` call (the Desktop shell's
/// `kernelStoreCall`), checked into the typed record. The answer's store
/// bookkeeping (`generation`, `owner`, `effects`) is not part of the record.
pub fn from_native_answer(answer: &str) -> Result<StoreStatus, String> {
    let mut value: Value = serde_json::from_str(answer)
        .map_err(|e| format!("{}: {e}", crate::durable_store::INVALID))?;
    if let Some(map) = value.as_object_mut() {
        for key in ["generation", "owner", "effects"] {
            map.remove(key);
        }
    }
    let status: StoreStatus = serde_json::from_value(value)
        .map_err(|e| format!("{}: {e}", crate::durable_store::INVALID))?;
    if status.schema != SCHEMA || status.backend == Backend::Opfs {
        return Err(format!(
            "{}: not a native {SCHEMA} record",
            crate::durable_store::INVALID
        ));
    }
    Ok(status)
}

/// The refusal for a write the store has no room for, with its remedy.
pub fn quota_refusal(detail: &str) -> String {
    format!(
        "{QUOTA_EXHAUSTED}: the device has no room to keep this ({detail}). Remedy: free disk space or browser storage, then repeat the action; nothing already held was removed"
    )
}
/// What the store itself can tell: SQLite's full-database message (ENOSPC
/// surfaces as `database or disk is full`), the OS message and a browser's
/// `QuotaExceededError`. Anything else passes through unchanged.
pub fn refine(error: String) -> String {
    if error.starts_with(QUOTA_EXHAUSTED) {
        return error;
    }
    let lowered = error.to_ascii_lowercase();
    const MARKERS: [&str; 4] = [
        "database or disk is full",
        "no space left on device",
        "quotaexceedederror",
        "quota has been exceeded",
    ];
    if MARKERS.iter().any(|marker| lowered.contains(marker)) {
        return quota_refusal(error.trim());
    }
    error
}
/// An opaque OPFS write failure counts as exhausted only when the browser's
/// own estimate says the origin is at its quota.
pub fn at_quota(usage: u64, quota: Option<u64>) -> bool {
    quota.is_some_and(|quota| quota > 0 && usage.saturating_mul(100) >= quota.saturating_mul(99))
}

/// The browser's answer to `persist()`, recorded so the page never asks in a
/// loop: one request per worker, whatever the answer.
#[derive(Default)]
pub struct Persistence {
    asked: Cell<bool>,
    answer: Cell<Option<Persisted>>,
}
/// The browser storage calls, so a test supplies its own.
#[allow(async_fn_in_trait)]
pub trait StorageApi {
    /// `navigator.storage.persist()`; `None` when the browser lacks it or refuses to answer.
    async fn persist(&self) -> Option<bool>;
    async fn persisted(&self) -> Option<bool>;
    /// `(usage, quota)` from `navigator.storage.estimate()`.
    async fn estimate(&self) -> Option<(u64, Option<u64>)>;
}
impl Persistence {
    /// Ask `persist()` the first time only and record the answer.
    pub async fn ensure(&self, api: &impl StorageApi) -> Persisted {
        if !self.asked.replace(true) {
            let answer = api
                .persist()
                .await
                .map_or(Persisted::UNKNOWN, Persisted::Known);
            self.answer.set(Some(answer));
        }
        self.answer.get().unwrap_or(Persisted::UNKNOWN)
    }
    pub fn recorded(&self) -> Option<Persisted> {
        self.answer.get()
    }
}
/// The hosted Web's record. `persisted()` is the browser's current truth; the
/// recorded `persist()` answer stands in only when it cannot be read.
pub async fn web_status(api: &impl StorageApi, recorded: Option<Persisted>) -> StoreStatus {
    let persisted = api
        .persisted()
        .await
        .map(Persisted::Known)
        .or(recorded)
        .unwrap_or(Persisted::UNKNOWN);
    let (usage, quota) = api.estimate().await.unwrap_or((0, None));
    StoreStatus::new(persisted, usage, quota, Backend::Opfs)
}

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub mod browser;

#[cfg(test)]
#[path = "store_status_tests.rs"]
mod tests;
