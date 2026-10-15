//! The native host of the sync store — SQLite behind the kernel's boundary.
//!
//! Contract: `docs/contracts/ds-sync-store.md`. The kernel module
//! `ds_command_kernel::sync_store` owns every row shape and every transition;
//! this crate opens one database per execution context, reads the snapshot a
//! transition needs, asks the kernel, and persists the `RowChange`s it returns
//! in one transaction. It decides nothing.
//!
//! Desktop (Linux, Windows), the `ds` CLI and a server worker all hold their
//! rows here. Two processes on one machine share one file: SQLite serialises
//! the writes and the kernel's lease serialises the work.
//!
//! An incompatible schema version is refused explicitly: an upgrade never
//! strands or erases a queue silently (`ds-headless-application-runtime.md`
//! §6).

pub mod cache_health;
mod jobs;
mod profile_sources;
mod publications;
pub use profile_sources::{
    ProfileSourceHeader, ProfileSourceScope, MAX_PROFILE_SOURCES, MAX_PROFILE_SOURCE_BYTES,
};
mod recovery;
pub use publications::{PublicationBytes, PublicationPayload};

pub use jobs::{Census, JobCaller, JobContext, Recovery};

use ds_command_kernel::survey_moments::{MediaRecord, SyncState};
use ds_command_kernel::sync_store::{
    self as kernel, ArtifactRow, ArtifactState, Event, Fence, GrantRow, HeadRow, LeaseRow,
    QueueStatus, ReceiptRow, RowChange, Scope, Snapshot, Transition,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::Path;

/// The on-disk schema this build reads and writes.
pub const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the sync store could not be opened: {0}")]
    Open(String),
    #[error("the sync store on disk is schema version {found}; this build reads up to {supported}. Update the application; the store is left untouched.")]
    Incompatible { found: i64, supported: i64 },
    #[error("the sync store refused: {0}")]
    Kernel(String),
    /// A typed decision of the kernel's a caller must be told — today the
    /// capacity of the compute queue — with its code, sentence and guidance
    /// intact for the host to relay. Nothing was written.
    #[error("{}: {}", .0.code, .0.message)]
    Refused(ds_command_kernel::execution_context::Refusal),
    #[error("the sync store could not be read or written: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("a stored row is unreadable: {0}")]
    Row(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    connection: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS artifacts(
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    scope TEXT NOT NULL, engine TEXT NOT NULL, operation TEXT NOT NULL, variant TEXT NOT NULL,
    replay_key TEXT NOT NULL, state TEXT NOT NULL, updated_at_ms INTEGER NOT NULL, row TEXT NOT NULL,
    PRIMARY KEY(account, deployment, install_id, replay_key));
CREATE TABLE IF NOT EXISTS heads(
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    scope TEXT NOT NULL, engine TEXT NOT NULL, operation TEXT NOT NULL, variant TEXT NOT NULL,
    read_at_ms INTEGER NOT NULL, row TEXT NOT NULL,
    PRIMARY KEY(account, deployment, install_id, scope, engine, operation, variant));
CREATE TABLE IF NOT EXISTS head_reads(
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    scope TEXT NOT NULL, read_at_ms INTEGER NOT NULL,
    PRIMARY KEY(account, deployment, install_id, scope));
CREATE TABLE IF NOT EXISTS grants(
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    scope TEXT NOT NULL, grant_id TEXT NOT NULL, row TEXT NOT NULL,
    PRIMARY KEY(account, deployment, install_id, scope, grant_id));
CREATE TABLE IF NOT EXISTS receipts(
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    scope TEXT NOT NULL, receipt_id TEXT NOT NULL, at_ms INTEGER NOT NULL, row TEXT NOT NULL,
    UNIQUE(account, deployment, install_id, receipt_id));
CREATE TABLE IF NOT EXISTS leases(
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    scope TEXT NOT NULL, row TEXT NOT NULL,
    PRIMARY KEY(account, deployment, install_id, scope));
CREATE TABLE IF NOT EXISTS survey_media(
    account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
    project TEXT NOT NULL, path TEXT NOT NULL, state TEXT NOT NULL,
    size INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, row TEXT NOT NULL,
    PRIMARY KEY(account, deployment, install_id, project, path));
";

fn schema_version(connection: &Connection) -> Result<Option<i64>> {
    let has_meta: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if has_meta.is_none() {
        return Ok(None);
    }
    Ok(connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

// Version 2 retains earlier publication rows while the kernel sees one standing
// row per identity. Unknown layouts refuse before a schema/version write.
fn validate_artifacts_v2(connection: &Connection) -> Result<()> {
    let columns = connection
        .prepare("PRAGMA table_info(artifacts)")?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = [
        ("account", "TEXT", 1, 1),
        ("deployment", "TEXT", 1, 2),
        ("install_id", "TEXT", 1, 3),
        ("scope", "TEXT", 1, 0),
        ("engine", "TEXT", 1, 0),
        ("operation", "TEXT", 1, 0),
        ("variant", "TEXT", 1, 0),
        ("replay_key", "TEXT", 1, 4),
        ("state", "TEXT", 1, 0),
        ("updated_at_ms", "INTEGER", 1, 0),
        ("row", "TEXT", 1, 0),
    ];
    if columns.len() != expected.len()
        || columns.iter().zip(expected).any(|(actual, expected)| {
            (actual.0.as_str(), actual.1.as_str(), actual.2, actual.3) != expected
        })
    {
        return Err(Error::Open(
            "schema 2 artifacts layout is unknown; all stored rows are left untouched".into(),
        ));
    }
    let unique = connection
        .prepare("PRAGMA index_list(artifacts)")?
        .query_map([], |row| {
            Ok((row.get::<_, i64>(2)?, row.get::<_, String>(3)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if unique.iter().filter(|row| row.0 != 0).count() != 1
        || !unique.iter().any(|row| row.0 != 0 && row.1 == "pk")
    {
        return Err(Error::Open(
            "schema 2 artifact key is unknown; all stored rows are left untouched".into(),
        ));
    }
    Ok(())
}

fn migrate_artifacts_v1(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let columns = transaction
        .prepare("PRAGMA table_info(artifacts)")?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = [
        ("account", "TEXT", 1, 1),
        ("deployment", "TEXT", 1, 2),
        ("install_id", "TEXT", 1, 3),
        ("scope", "TEXT", 1, 4),
        ("engine", "TEXT", 1, 5),
        ("operation", "TEXT", 1, 6),
        ("variant", "TEXT", 1, 7),
        ("replay_key", "TEXT", 1, 0),
        ("state", "TEXT", 1, 0),
        ("updated_at_ms", "INTEGER", 1, 0),
        ("row", "TEXT", 1, 0),
    ];
    if columns.len() != expected.len()
        || columns.iter().zip(expected).any(|(actual, expected)| {
            (actual.0.as_str(), actual.1.as_str(), actual.2, actual.3) != expected
        })
    {
        return Err(Error::Open(
            "schema 1 artifacts layout is unknown; migration left all rows untouched".into(),
        ));
    }
    transaction.execute_batch("ALTER TABLE artifacts RENAME TO artifacts_v1;")?;
    transaction.execute_batch(SCHEMA)?;
    transaction.execute_batch("INSERT INTO artifacts SELECT account,deployment,install_id,scope,engine,operation,variant,replay_key,state,updated_at_ms,row FROM artifacts_v1; DROP TABLE artifacts_v1;")?;
    validate_artifacts_v2(&transaction)?;
    transaction.execute(
        "UPDATE meta SET value=?1 WHERE key='schema_version'",
        params![SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(())
}

fn scope_key(scope: &Scope) -> String {
    match scope {
        Scope::Project { project } => format!("project:{project}"),
        Scope::Personal => "personal".to_string(),
    }
}

/// The store holds signed work grants and queued user data, so its directory
/// is created `0700` and the database file is `0600` before SQLite opens it
/// (SQLite creates `-wal`/`-shm` with the database's mode). Existing Unix
/// files are tightened through metadata/path operations, never a raw file
/// descriptor; unsafe ownership, links, parent write permissions or failed
/// privacy admission refuse. No additional storage owner or dependency.
#[cfg(unix)]
fn preparation_guard() -> std::io::Result<std::sync::MutexGuard<'static, ()>> {
    static PREPARING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    PREPARING
        .lock()
        .map_err(|_| std::io::Error::other("private Store file preparation lock is poisoned"))
}

fn prepare_private(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
        // POSIX record locks belong to a process: closing ANY descriptor for
        // an existing SQLite inode cancels that process's SQLite locks. Only
        // initial creation may use a Rust descriptor, and all same-process
        // Store preparers must wait until that fresh descriptor is closed.
        let _preparing = preparation_guard()?;
        if let Some(parent) = parent {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        let parent_metadata = std::fs::symlink_metadata(parent.unwrap_or_else(|| Path::new(".")))?;
        if !parent_metadata.is_dir()
            || parent_metadata.file_type().is_symlink()
            || parent_metadata.permissions().mode() & 0o022 != 0
        {
            return Err(std::io::Error::other(
                "Store parent must be an ordinary owner-controlled directory",
            ));
        }
        let private = std::fs::Permissions::from_mode(0o600);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(file) => {
                file.set_permissions(private.clone())?;
                drop(file); // No other Store preparer may reach SQLite yet.
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        // Metadata/chmod do not open or close a descriptor for the existing
        // database. SQLite's VFS remains its sole descriptor/lock owner.
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.uid() != parent_metadata.uid()
        {
            return Err(std::io::Error::other(
                "Store must be an ordinary single-link file owned by its private directory owner",
            ));
        }
        std::fs::set_permissions(path, private.clone())?;
        let after = std::fs::symlink_metadata(path)?;
        let parent_after = std::fs::symlink_metadata(parent.unwrap_or_else(|| Path::new(".")))?;
        if !after.is_file()
            || after.file_type().is_symlink()
            || after.nlink() != 1
            || after.uid() != metadata.uid()
            || after.ino() != metadata.ino()
            || after.dev() != metadata.dev()
            || after.permissions().mode() & 0o077 != 0
            || parent_after.ino() != parent_metadata.ino()
            || parent_after.dev() != parent_metadata.dev()
            || parent_after.uid() != parent_metadata.uid()
            || !parent_after.is_dir()
            || parent_after.file_type().is_symlink()
            || parent_after.permissions().mode() & 0o022 != 0
        {
            return Err(std::io::Error::other(
                "Store identity or owner-controlled parent changed during metadata preparation",
            ));
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            match std::fs::symlink_metadata(&sidecar) {
                Ok(meta) => {
                    if !meta.is_file()
                        || meta.file_type().is_symlink()
                        || meta.nlink() != 1
                        || meta.uid() != parent_metadata.uid()
                    {
                        return Err(std::io::Error::other("Store sidecar must be an ordinary single-link file with the same private owner"));
                    }
                    if meta.permissions().mode() & 0o077 != 0 {
                        std::fs::set_permissions(&sidecar, private.clone())?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    #[cfg(not(unix))]
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

impl Store {
    /// Open (or create) the store at `path`. A newer schema on disk is
    /// refused, never rewritten.
    pub fn open(path: &Path) -> Result<Self> {
        prepare_private(path).map_err(|error| Error::Open(error.to_string()))?;
        let flags = OpenFlags::default();
        #[cfg(unix)]
        let flags = flags | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let mut connection = Connection::open_with_flags(path, flags)
            .map_err(|error| Error::Open(error.to_string()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        // The power-loss guarantee, decided (owner, 2026-09-20, after a
        // power cut): an acknowledged seal survives a sudden power cut. Under
        // WAL, `NORMAL` syncs the log only at checkpoints, so a transaction
        // acknowledged to the caller could be lost with the last few
        // milliseconds of writes; `FULL` fsyncs the WAL on every commit. The
        // trade is write latency — one fsync per transition, on a store
        // that sees a few transitions per pass — against the durability of
        // every acknowledged row, receipt and lease. Durability wins.
        connection.pragma_update(None, "synchronous", "FULL")?;
        let found = schema_version(&connection)?;
        match found {
            Some(found) if found > SCHEMA_VERSION => {
                return Err(Error::Incompatible {
                    found,
                    supported: SCHEMA_VERSION,
                })
            }
            Some(2) => validate_artifacts_v2(&connection)?,
            Some(1) => migrate_artifacts_v1(&mut connection)?,
            Some(found) => {
                return Err(Error::Open(format!(
                    "unknown sync store schema {found}; store left untouched"
                )))
            }
            None => {
                let existing: Option<i64> = connection
                    .query_row(
                        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='artifacts'",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;
                if existing.is_some() {
                    return Err(Error::Open(
                        "an unversioned artifact store is unknown; all rows are left untouched"
                            .into(),
                    ));
                }
                connection.execute_batch(SCHEMA)?;
                connection.execute(
                    "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION],
                )?;
            }
        }
        Ok(Store { connection })
    }

    /// Open an existing store for reading only: no schema is created, no
    /// version is written, no lease is taken. `Ok(None)` when there is no
    /// store at `path` — a machine that never sealed anything holds an
    /// empty queue, not an error. A newer schema on disk is refused exactly
    /// as [`Store::open`] refuses it.
    ///
    /// This is what a status command uses: it must answer on a machine with
    /// no session and no running host, and it must not leave state behind.
    pub fn open_read_only(path: &Path) -> Result<Option<Self>> {
        #[cfg(unix)]
        let preparing = preparation_guard().map_err(|error| Error::Open(error.to_string()))?;
        if !path.is_file() {
            return Ok(None);
        }
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| Error::Open(error.to_string()))?;
        #[cfg(unix)]
        drop(preparing); // Any fresh creator descriptor closed before SQLite opened.
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let has_meta: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if has_meta.is_none() {
            // A file with none of this store's tables holds no queue.
            return Ok(None);
        }
        if let Some(found) = schema_version(&connection)? {
            if found > SCHEMA_VERSION {
                return Err(Error::Incompatible {
                    found,
                    supported: SCHEMA_VERSION,
                });
            }
            if found == 2 {
                validate_artifacts_v2(&connection)?;
            } else if found != 1 {
                return Err(Error::Open(format!(
                    "unknown sync store schema {found}; store left untouched"
                )));
            }
        }
        Ok(Some(Store { connection }))
    }

    /// Open an in-memory store (tests and dry runs).
    pub fn in_memory() -> Result<Self> {
        let connection =
            Connection::open_in_memory().map_err(|error| Error::Open(error.to_string()))?;
        connection.execute_batch(SCHEMA)?;
        connection.execute(
            "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
        Ok(Store { connection })
    }

    /// The raw connection, for tests that pin a pragma. Not a door for
    /// hosts: every write goes through [`Store::apply`].
    #[doc(hidden)]
    pub fn connection_for_test(&self) -> &Connection {
        &self.connection
    }

    /// Everything one scope holds under one fence.
    pub fn snapshot(&self, fence: &Fence, scope: &Scope) -> Result<Snapshot> {
        Self::snapshot_on(&self.connection, fence, scope)
    }

    fn snapshot_on(connection: &Connection, fence: &Fence, scope: &Scope) -> Result<Snapshot> {
        let key = scope_key(scope);
        let artifacts = Self::rows_on::<ArtifactRow>(
            connection,
            "SELECT row FROM artifacts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 AND NOT EXISTS (SELECT 1 FROM artifacts newer WHERE newer.account=artifacts.account AND newer.deployment=artifacts.deployment AND newer.install_id=artifacts.install_id AND newer.scope=artifacts.scope AND newer.engine=artifacts.engine AND newer.operation=artifacts.operation AND newer.variant=artifacts.variant AND newer.rowid>artifacts.rowid) ORDER BY engine, operation, variant",
            fence,
            &key,
        )?;
        let heads = Self::rows_on::<HeadRow>(
            connection,
            "SELECT row FROM heads WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY engine, operation, variant",
            fence,
            &key,
        )?;
        let grants = Self::rows_on::<GrantRow>(
            connection,
            "SELECT row FROM grants WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY grant_id",
            fence,
            &key,
        )?;
        let lease = Self::rows_on::<LeaseRow>(
                connection,
                "SELECT row FROM leases WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4",
                fence,
                &key,
            )?
            .into_iter()
            .next();
        Ok(Snapshot {
            artifacts,
            heads,
            grants,
            lease,
        })
    }

    /// When this scope's heads were last read from the record, if ever.
    pub fn heads_read_at_ms(&self, fence: &Fence, scope: &Scope) -> Result<Option<u64>> {
        let at: Option<i64> = self
            .connection
            .query_row(
                "SELECT read_at_ms FROM head_reads WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4",
                params![fence.account, fence.deployment, fence.install_id, scope_key(scope)],
                |row| row.get(0),
            )
            .optional()?;
        Ok(at.map(|value| value.max(0) as u64))
    }

    /// Every artifact row of one fence, across scopes.
    pub fn artifacts_of_fence(&self, fence: &Fence) -> Result<Vec<ArtifactRow>> {
        self.rows::<ArtifactRow>(
            "SELECT row FROM artifacts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND ?4 = ?4 AND NOT EXISTS (SELECT 1 FROM artifacts newer WHERE newer.account=artifacts.account AND newer.deployment=artifacts.deployment AND newer.install_id=artifacts.install_id AND newer.scope=artifacts.scope AND newer.engine=artifacts.engine AND newer.operation=artifacts.operation AND newer.variant=artifacts.variant AND newer.rowid>artifacts.rowid) ORDER BY scope, engine, operation, variant",
            fence,
            "",
        )
    }

    /// Every lease under one fence, across scopes, live or not — the
    /// caller's clock decides which are live.
    pub fn leases_of_fence(&self, fence: &Fence) -> Result<Vec<LeaseRow>> {
        self.rows::<LeaseRow>(
            "SELECT row FROM leases WHERE account=?1 AND deployment=?2 AND install_id=?3 AND ?4 = ?4 ORDER BY scope",
            fence,
            "",
        )
    }

    /// The queue as this fence holds it: the kernel's reading over every
    /// artifact row and lease of the fence, narrowed to `project` when one
    /// is named. Read-only; nothing on disk beyond the store is consulted.
    pub fn queue(&self, fence: &Fence, project: Option<&str>, now_ms: u64) -> Result<QueueStatus> {
        let rows = self.artifacts_of_fence(fence)?;
        let leases = self.leases_of_fence(fence)?;
        let mut receipts = self.latest_delivery_receipts(fence, &rows, project)?;
        for row in rows
            .iter()
            .filter(|row| matches!(row.state, ArtifactState::Conflict | ArtifactState::Refused))
        {
            if let Some(attempt) = self.last_publication_attempt(fence, row)? {
                receipts.push(attempt);
            }
        }
        Ok(kernel::queue_status(
            &rows, &leases, &receipts, project, now_ms,
        ))
    }

    /// The queue as this whole file holds it, every fence together: what a
    /// status command reads on a machine with no native identity to name a
    /// fence. One store is one owner's (a Server serves exactly one), so
    /// this is that owner's queue across the lanes and installs it has run
    /// under — a local diagnostic, never an execution: the fence still
    /// decides what a worker may drain.
    pub fn queue_all(&self, project: Option<&str>, now_ms: u64) -> Result<QueueStatus> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT account, deployment, install_id FROM artifacts ORDER BY account, deployment, install_id",
        )?;
        let fences = statement.query_map([], |row| {
            Ok(Fence {
                account: row.get(0)?,
                deployment: row.get(1)?,
                install_id: row.get(2)?,
            })
        })?;
        let mut statuses = Vec::new();
        for fence in fences {
            statuses.push(self.queue(&fence?, project, now_ms)?);
        }
        Ok(kernel::combine_queue_statuses(&statuses))
    }

    /// Only the newest delivery receipt for each queued project under this
    /// exact fence. The append sequence orders attempts; wall clocks do not.
    fn latest_delivery_receipts(
        &self,
        fence: &Fence,
        rows: &[ArtifactRow],
        project: Option<&str>,
    ) -> Result<Vec<ReceiptRow>> {
        let projects: std::collections::BTreeSet<&str> = rows
            .iter()
            .filter(|row| matches!(row.state, ArtifactState::Held | ArtifactState::Uploading))
            .map(|row| row.scope.project())
            .filter(|id| !id.is_empty() && project.is_none_or(|wanted| *id == wanted))
            .collect();
        let mut statement = self.connection.prepare(
            "SELECT row FROM receipts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 \
             AND json_extract(row, '$.action') IN ('upload', 'download', 'session') \
             ORDER BY sequence DESC LIMIT 1",
        )?;
        let mut receipts = Vec::new();
        for id in projects {
            let raw: Option<String> = statement
                .query_row(
                    params![
                        fence.account,
                        fence.deployment,
                        fence.install_id,
                        scope_key(&Scope::Project { project: id.into() })
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(raw) = raw {
                receipts.push(serde_json::from_str(&raw)?);
            }
        }
        Ok(receipts)
    }

    /// The most recent receipts of one scope, newest first.
    pub fn receipts(&self, fence: &Fence, scope: &Scope, limit: usize) -> Result<Vec<ReceiptRow>> {
        let mut statement = self.connection.prepare(
            "SELECT row FROM receipts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY sequence DESC LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![
                fence.account,
                fence.deployment,
                fence.install_id,
                scope_key(scope),
                limit.min(10_000) as i64
            ],
            |row| row.get::<_, String>(0),
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    /// Ask the kernel for the transition `event` causes from the stored
    /// snapshot, persist its row changes atomically, and return it. A
    /// refusal is returned as the kernel spelled it, including any audit
    /// receipt that records the refused attempt without restating its row.
    pub fn apply(&mut self, fence: &Fence, now_ms: u64, event: Event) -> Result<Transition> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transition = Self::apply_on(&tx, fence, now_ms, event)?;
        tx.commit()?;
        Ok(transition)
    }

    // Both ordinary receipts and atomic producer-byte seals use this one transition.
    fn apply_on(
        tx: &rusqlite::Transaction<'_>,
        fence: &Fence,
        now_ms: u64,
        event: Event,
    ) -> Result<Transition> {
        let scope = event_scope(&event);
        let snapshot = Self::snapshot_on(tx, fence, &scope)?;
        // A receipt is appended once: the same receipt applied again (a
        // retried host call, a replayed log) changes nothing.
        if let Event::ActionReceipt { receipt } = &event {
            let known: Option<i64> = tx.query_row(
                    "SELECT 1 FROM receipts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND receipt_id=?4",
                    params![fence.account, fence.deployment, fence.install_id, receipt.receipt_id],
                    |row| row.get(0),
                )
                .optional()?;
            if known.is_some() {
                return Ok(Transition {
                    schema: kernel::SCHEMA.to_string(),
                    changes: Vec::new(),
                    refusals: vec![kernel::Refusal {
                        code: "receipt_already_recorded".into(),
                        detail: receipt.receipt_id.clone(),
                    }],
                    plan_trigger: None,
                });
            }
        }
        let request = kernel::Request {
            schema: kernel::SCHEMA.to_string(),
            fence: fence.clone(),
            now_ms,
            snapshot,
            event,
        };
        let transition = kernel::apply(&request).map_err(Error::Kernel)?;
        for change in &transition.changes {
            persist(tx, fence, change)?;
        }
        Ok(transition)
    }

    /// How many rows of every kind this file holds for one project, across
    /// fences: what a local-data status counts as "the sync store's rows".
    pub fn rows_of_project(&self, project: &str) -> Result<u64> {
        let scope = scope_key(&Scope::Project {
            project: project.to_string(),
        });
        let mut total: i64 = 0;
        for table in ["artifacts", "heads", "grants", "receipts", "leases"] {
            let count: i64 = self.connection.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE scope=?1"),
                params![scope],
                |row| row.get(0),
            )?;
            total += count;
        }
        Ok(total.max(0) as u64)
    }

    /// Whether this file holds the survey-media table: a store written by a
    /// build before the table existed, opened read-only, holds no media.
    fn has_survey_media(&self) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='survey_media'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some())
    }

    /// Every survey-media record this file holds for one project, across
    /// fences — one store is one owner's, so this is that owner's holdings
    /// on this machine. Ordered by path for a stable answer.
    pub fn survey_media_of_project(&self, project: &str) -> Result<Vec<MediaRecord>> {
        if !self.has_survey_media()? {
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare("SELECT row FROM survey_media WHERE project=?1 ORDER BY path")?;
        let rows = statement.query_map(params![project], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    /// One project's record for one object path, if held.
    pub fn survey_media_record(&self, project: &str, path: &str) -> Result<Option<MediaRecord>> {
        if !self.has_survey_media()? {
            return Ok(None);
        }
        let row: Option<String> = self
            .connection
            .query_row(
                "SELECT row FROM survey_media WHERE project=?1 AND path=?2 ORDER BY updated_at_ms DESC LIMIT 1",
                params![project, path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match row {
            Some(row) => Some(serde_json::from_str(&row)?),
            None => None,
        })
    }

    /// Write or replace one record under this fence. The record's own
    /// `project`/`path` are the key; nothing here decides a state.
    pub fn survey_media_put(&mut self, fence: &Fence, record: &MediaRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO survey_media(account, deployment, install_id, project, path, state, size, updated_at_ms, row)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(account, deployment, install_id, project, path)
             DO UPDATE SET state=excluded.state, size=excluded.size, updated_at_ms=excluded.updated_at_ms, row=excluded.row",
            params![
                fence.account,
                fence.deployment,
                fence.install_id,
                record.project,
                record.path,
                record.state.as_str(),
                record.size as i64,
                record.updated_at_ms as i64,
                serde_json::to_string(record)?
            ],
        )?;
        Ok(())
    }

    /// Remove one project's records for the given paths (every fence).
    /// Answers how many rows went.
    pub fn survey_media_remove(&mut self, project: &str, paths: &[String]) -> Result<usize> {
        if !self.has_survey_media()? {
            return Ok(0);
        }
        let tx = self.connection.transaction()?;
        let mut removed = 0;
        for path in paths {
            removed += tx.execute(
                "DELETE FROM survey_media WHERE project=?1 AND path=?2",
                params![project, path],
            )?;
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Remove every record of one project in one state (every fence),
    /// answering the paths removed so the host can free their bytes.
    pub fn survey_media_remove_state(
        &mut self,
        project: &str,
        state: SyncState,
    ) -> Result<Vec<String>> {
        if !self.has_survey_media()? {
            return Ok(Vec::new());
        }
        let tx = self.connection.transaction()?;
        let paths: Vec<String> = {
            let mut statement = tx.prepare(
                "SELECT path FROM survey_media WHERE project=?1 AND state=?2 ORDER BY path",
            )?;
            let rows = statement.query_map(params![project, state.as_str()], |row| {
                row.get::<_, String>(0)
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };
        tx.execute(
            "DELETE FROM survey_media WHERE project=?1 AND state=?2",
            params![project, state.as_str()],
        )?;
        tx.commit()?;
        Ok(paths)
    }

    fn rows<T: serde::de::DeserializeOwned>(
        &self,
        sql: &str,
        fence: &Fence,
        scope: &str,
    ) -> Result<Vec<T>> {
        Self::rows_on(&self.connection, sql, fence, scope)
    }

    fn rows_on<T: serde::de::DeserializeOwned>(
        connection: &Connection,
        sql: &str,
        fence: &Fence,
        scope: &str,
    ) -> Result<Vec<T>> {
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map(
            params![fence.account, fence.deployment, fence.install_id, scope],
            |row| row.get::<_, String>(0),
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }
}

fn event_scope(event: &Event) -> Scope {
    match event {
        Event::LocalProduced { row } => row.scope.clone(),
        Event::HeadsRead { scope, .. }
        | Event::LeaseTake { scope, .. }
        | Event::LeaseRenew { scope, .. }
        | Event::LeaseRelease { scope, .. }
        | Event::Cancel { scope, .. } => scope.clone(),
        Event::GrantOpened { grant } => grant.scope.clone(),
        Event::ActionReceipt { receipt } => receipt.scope.clone(),
    }
}

fn persist(tx: &rusqlite::Transaction<'_>, fence: &Fence, change: &RowChange) -> Result<()> {
    match change {
        RowChange::UpsertArtifact(row) => {
            let changed = tx.execute(
                "INSERT INTO artifacts(account, deployment, install_id, scope, engine, operation, variant, replay_key, state, updated_at_ms, row)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(account, deployment, install_id, replay_key)
                 DO UPDATE SET state=excluded.state, updated_at_ms=excluded.updated_at_ms, row=excluded.row
                 WHERE artifacts.scope=excluded.scope AND artifacts.engine=excluded.engine AND artifacts.operation=excluded.operation AND artifacts.variant=excluded.variant",
                params![
                    fence.account, fence.deployment, fence.install_id, scope_key(&row.scope),
                    row.identity.engine, row.identity.operation, row.identity.variant,
                    row.replay_key, row.state.as_str(), row.updated_at_ms as i64,
                    serde_json::to_string(row)?
                ],
            )?;
            if changed != 1 {
                return Err(Error::Kernel("replay_key_taken".into()));
            }
        }
        RowChange::ReplaceHeads {
            scope,
            heads,
            read_at_ms,
        } => {
            tx.execute(
                "DELETE FROM heads WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4",
                params![fence.account, fence.deployment, fence.install_id, scope_key(scope)],
            )?;
            for head in heads {
                upsert_head(tx, fence, head)?;
            }
            tx.execute(
                "INSERT INTO head_reads(account, deployment, install_id, scope, read_at_ms) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account, deployment, install_id, scope) DO UPDATE SET read_at_ms=excluded.read_at_ms",
                params![fence.account, fence.deployment, fence.install_id, scope_key(scope), *read_at_ms as i64],
            )?;
        }
        RowChange::UpsertHead(head) => {
            upsert_head(tx, fence, head)?;
            tx.execute(
                "INSERT INTO head_reads(account, deployment, install_id, scope, read_at_ms) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account, deployment, install_id, scope) DO UPDATE SET read_at_ms=MAX(read_at_ms, excluded.read_at_ms)",
                params![fence.account, fence.deployment, fence.install_id, scope_key(&head.scope), head.read_at_ms as i64],
            )?;
        }
        RowChange::UpsertGrant(grant) => {
            tx.execute(
                "INSERT INTO grants(account, deployment, install_id, scope, grant_id, row) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(account, deployment, install_id, scope, grant_id) DO UPDATE SET row=excluded.row",
                params![
                    fence.account, fence.deployment, fence.install_id, scope_key(&grant.scope),
                    grant.grant_id, serde_json::to_string(grant)?
                ],
            )?;
        }
        RowChange::DropGrant { scope, grant_id } => {
            tx.execute(
                "DELETE FROM grants WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 AND grant_id=?5",
                params![fence.account, fence.deployment, fence.install_id, scope_key(scope), grant_id],
            )?;
        }
        RowChange::AppendReceipt(receipt) => {
            tx.execute(
                "INSERT OR IGNORE INTO receipts(account, deployment, install_id, scope, receipt_id, at_ms, row) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    fence.account, fence.deployment, fence.install_id, scope_key(&receipt.scope),
                    receipt.receipt_id, receipt.at_ms as i64, serde_json::to_string(receipt)?
                ],
            )?;
        }
        RowChange::SetLease(lease) => {
            tx.execute(
                "INSERT INTO leases(account, deployment, install_id, scope, row) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account, deployment, install_id, scope) DO UPDATE SET row=excluded.row",
                params![
                    fence.account, fence.deployment, fence.install_id, scope_key(&lease.scope),
                    serde_json::to_string(lease)?
                ],
            )?;
        }
        RowChange::ClearLease { scope } => {
            tx.execute(
                "DELETE FROM leases WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4",
                params![fence.account, fence.deployment, fence.install_id, scope_key(scope)],
            )?;
        }
    }
    Ok(())
}

fn upsert_head(tx: &rusqlite::Transaction<'_>, fence: &Fence, head: &HeadRow) -> Result<()> {
    tx.execute(
        "INSERT INTO heads(account, deployment, install_id, scope, engine, operation, variant, read_at_ms, row)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(account, deployment, install_id, scope, engine, operation, variant)
         DO UPDATE SET read_at_ms=excluded.read_at_ms, row=excluded.row",
        params![
            fence.account, fence.deployment, fence.install_id, scope_key(&head.scope),
            head.identity.engine, head.identity.operation, head.identity.variant,
            head.read_at_ms as i64, serde_json::to_string(head)?
        ],
    )?;
    Ok(())
}

#[cfg(all(test, unix))]
mod preparation_tests {
    #[test]
    fn read_only_open_waits_until_fresh_creator_descriptor_is_closed() {
        use std::os::unix::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("fresh.sqlite");
        let guard = super::preparation_guard().unwrap();
        let fresh = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx
                .send(super::Store::open_read_only(&path).unwrap().is_none())
                .unwrap();
        });
        started_rx.recv().unwrap();
        assert!(matches!(
            done_rx.recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(fresh);
        drop(guard);
        assert!(done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap());
        reader.join().unwrap();
    }
}
