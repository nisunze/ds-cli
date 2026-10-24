//! Read durable facts and persist the kernel's explicit recovery atomically.
use crate::{persist, scope_key, Error, Result, Store};
use ds_command_kernel::sync_store::{recovery, ArtifactRow, Fence, ReceiptRow, Scope};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

impl Store {
    pub fn artifact(&self, fence: &Fence, scope: &Scope, key: &str) -> Result<Option<ArtifactRow>> {
        let raw: Option<String> = self.connection.query_row(
            "SELECT row FROM artifacts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 AND replay_key=?5",
            params![fence.account, fence.deployment, fence.install_id, scope_key(scope), key], |row| row.get(0),
        ).optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(Error::Row))
            .transpose()
    }

    pub fn last_publication_attempt(
        &self,
        fence: &Fence,
        row: &ArtifactRow,
    ) -> Result<Option<ReceiptRow>> {
        Self::last_attempt_on(&self.connection, fence, row)
    }

    fn last_attempt_on(
        connection: &Connection,
        fence: &Fence,
        row: &ArtifactRow,
    ) -> Result<Option<ReceiptRow>> {
        let raw: Option<String> = connection.query_row(
            "SELECT row FROM receipts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 AND json_extract(row,'$.client_publish_id')=?5 AND json_extract(row,'$.action')='upload' ORDER BY sequence DESC LIMIT 1",
            params![fence.account, fence.deployment, fence.install_id, scope_key(&row.scope), row.client_publish_id], |row| row.get(0),
        ).optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(Error::Row))
            .transpose()
    }

    /// Why a queued row still waits, newest first by the append sequence: its
    /// own last upload attempt, a pass that failed before any step (`pass` /
    /// `failed`, carrying the transport's or the gateway's typed cause), or
    /// the session verdict (`session`: a refused or restored credential). A
    /// queued row is never retry-eligible, so this never opens a recovery.
    fn held_by_on(
        connection: &Connection,
        fence: &Fence,
        row: &ArtifactRow,
    ) -> Result<Option<ReceiptRow>> {
        let raw: Option<String> = connection.query_row(
            "SELECT row FROM receipts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 AND ((json_extract(row,'$.action')='upload' AND json_extract(row,'$.client_publish_id')=?5) OR (json_extract(row,'$.action')='pass' AND json_extract(row,'$.outcome')='failed') OR json_extract(row,'$.action')='session') ORDER BY sequence DESC LIMIT 1",
            params![fence.account, fence.deployment, fence.install_id, scope_key(&row.scope), row.client_publish_id], |row| row.get(0),
        ).optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(Error::Row))
            .transpose()
    }

    /// Status and apply share one snapshot shape; apply holds an IMMEDIATE transaction.
    /// The caller binds the local owner fence. No network, producer labels or file paths are accepted.
    pub fn recovery(
        &mut self,
        fence: &Fence,
        scope: &Scope,
        now_ms: u64,
        command: recovery::Command,
    ) -> Result<recovery::Result> {
        let behavior = if matches!(
            &command,
            recovery::Command::Status { .. }
                | recovery::Command::SanitizePreview { .. }
                | recovery::Command::VerifyArtifacts { .. }
        ) {
            TransactionBehavior::Deferred
        } else {
            TransactionBehavior::Immediate
        };
        let tx = self.connection.transaction_with_behavior(behavior)?;
        let snapshot = Self::snapshot_on(&tx, fence, scope)?;
        let standing: std::collections::BTreeSet<_> = snapshot
            .artifacts
            .iter()
            .map(|row| row.replay_key.as_str())
            .collect();
        let rows = Self::rows_on::<ArtifactRow>(&tx,
            "SELECT row FROM artifacts WHERE account=?1 AND deployment=?2 AND install_id=?3 AND scope=?4 ORDER BY replay_key LIMIT 4097",
            fence, &scope_key(scope))?;
        let mut entries = Vec::new();
        for row in rows {
            // A queued row has no verdict to recover from; what it shows is why
            // it still waits: the newest of its own attempt, a pass that could
            // not reach it, or a session verdict (`held_by_on`).
            let last_attempt = if matches!(
                row.state,
                ds_command_kernel::sync_store::ArtifactState::Held
                    | ds_command_kernel::sync_store::ArtifactState::Uploading
            ) {
                Self::held_by_on(&tx, fence, &row)?
            } else {
                Self::last_attempt_on(&tx, fence, &row)?
            };
            entries.push(recovery::Entry {
                standing: standing.contains(row.replay_key.as_str()),
                row,
                last_attempt,
            });
        }
        let result = recovery::resolve(recovery::Request {
            schema: recovery::SCHEMA.into(),
            fence: fence.clone(),
            scope: scope.clone(),
            now_ms,
            entries,
            lease: snapshot.lease,
            command,
        })
        .map_err(Error::Kernel)?;
        for change in &result.changes {
            persist(&tx, fence, change)?;
        }
        tx.commit()?;
        Ok(result)
    }
}
