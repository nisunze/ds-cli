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

mod jobs;

use ds_command_kernel::sync_store::{
    self as kernel, ArtifactRow, Event, Fence, GrantRow, HeadRow, LeaseRow, ReceiptRow, RowChange,
    Scope, Snapshot, Transition,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// The on-disk schema this build reads and writes.
pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the sync store could not be opened: {0}")]
    Open(String),
    #[error("the sync store on disk is schema version {found}; this build reads up to {supported}. Update the application; the store is left untouched.")]
    Incompatible { found: i64, supported: i64 },
    #[error("the sync store refused: {0}")]
    Kernel(String),
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
    PRIMARY KEY(account, deployment, install_id, scope, engine, operation, variant),
    UNIQUE(account, deployment, install_id, replay_key));
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
";

fn scope_key(scope: &Scope) -> String {
    match scope {
        Scope::Project { project } => format!("project:{project}"),
        Scope::Personal => "personal".to_string(),
    }
}

impl Store {
    /// Open (or create) the store at `path`. A newer schema on disk is
    /// refused, never rewritten.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| Error::Open(error.to_string()))?;
        }
        let connection = Connection::open(path).map_err(|error| Error::Open(error.to_string()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(SCHEMA)?;
        let found: Option<i64> = connection
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match found {
            Some(found) if found > SCHEMA_VERSION => {
                return Err(Error::Incompatible {
                    found,
                    supported: SCHEMA_VERSION,
                })
            }
            Some(_) => {}
            None => {
                connection.execute(
                    "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION],
                )?;
            }
        }
        Ok(Store { connection })
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

    /// Everything one scope holds under one fence.
    pub fn snapshot(&self, fence: &Fence, scope: &Scope) -> Result<Snapshot> {
        let key = scope_key(scope);
        let artifacts = self.rows::<ArtifactRow>(
            "SELECT row FROM artifacts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY engine, operation, variant",
            fence,
            &key,
        )?;
        let heads = self.rows::<HeadRow>(
            "SELECT row FROM heads WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY engine, operation, variant",
            fence,
            &key,
        )?;
        let grants = self.rows::<GrantRow>(
            "SELECT row FROM grants WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY grant_id",
            fence,
            &key,
        )?;
        let lease = self
            .rows::<LeaseRow>(
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
            "SELECT row FROM artifacts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND ?4 = ?4 ORDER BY scope, engine, operation, variant",
            fence,
            "",
        )
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
    /// refused transition (no changes) is returned as the kernel spelled it.
    pub fn apply(&mut self, fence: &Fence, now_ms: u64, event: Event) -> Result<Transition> {
        let scope = event_scope(&event);
        let snapshot = match &scope {
            Some(scope) => self.snapshot(fence, scope)?,
            // A handoff spans scopes: the kernel needs every artifact row of
            // this fence to keep the import idempotent.
            None => Snapshot {
                artifacts: self.artifacts_of_fence(fence)?,
                ..Snapshot::default()
            },
        };
        // A receipt is appended once: the same receipt applied again (a
        // retried host call, a replayed log) changes nothing.
        if let Event::ActionReceipt { receipt } = &event {
            let known: Option<i64> = self
                .connection
                .query_row(
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
        let tx = self.connection.transaction()?;
        for change in &transition.changes {
            persist(&tx, fence, change)?;
        }
        tx.commit()?;
        Ok(transition)
    }

    fn rows<T: serde::de::DeserializeOwned>(
        &self,
        sql: &str,
        fence: &Fence,
        scope: &str,
    ) -> Result<Vec<T>> {
        let mut statement = self.connection.prepare(sql)?;
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

fn event_scope(event: &Event) -> Option<Scope> {
    match event {
        Event::LocalProduced { row } => Some(row.scope.clone()),
        Event::HeadsRead { scope, .. }
        | Event::LeaseTake { scope, .. }
        | Event::LeaseRenew { scope, .. }
        | Event::LeaseRelease { scope, .. }
        | Event::Cancel { scope, .. } => Some(scope.clone()),
        Event::GrantOpened { grant } => Some(grant.scope.clone()),
        Event::ActionReceipt { receipt } => Some(receipt.scope.clone()),
        Event::HandoffLegacy { .. } => None,
    }
}

fn persist(tx: &rusqlite::Transaction<'_>, fence: &Fence, change: &RowChange) -> Result<()> {
    match change {
        RowChange::UpsertArtifact(row) => {
            tx.execute(
                "INSERT INTO artifacts(account, deployment, install_id, scope, engine, operation, variant, replay_key, state, updated_at_ms, row)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(account, deployment, install_id, scope, engine, operation, variant)
                 DO UPDATE SET replay_key=excluded.replay_key, state=excluded.state, updated_at_ms=excluded.updated_at_ms, row=excluded.row",
                params![
                    fence.account, fence.deployment, fence.install_id, scope_key(&row.scope),
                    row.identity.engine, row.identity.operation, row.identity.variant,
                    row.replay_key, row.state.as_str(), row.updated_at_ms as i64,
                    serde_json::to_string(row)?
                ],
            )?;
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
