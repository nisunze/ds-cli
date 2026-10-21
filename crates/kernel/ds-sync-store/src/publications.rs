//! Immutable producer bytes. Artifact rows remain the sole publication queue.
use crate::{Error, Result, Store};
use ds_command_kernel::sync_store::{ArtifactRow, Event, Fence, Scope, Transition};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS publication_payloads(
 account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, replay_key TEXT NOT NULL, receipt TEXT NOT NULL, retained INTEGER NOT NULL,
 PRIMARY KEY(account,deployment,install_id,project,replay_key));
 CREATE TABLE IF NOT EXISTS publication_outputs(
 account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, replay_key TEXT NOT NULL, output_id TEXT NOT NULL, bytes BLOB NOT NULL,
 PRIMARY KEY(account,deployment,install_id,project,replay_key,output_id));";

pub struct PublicationBytes {
    pub output_id: String,
    pub bytes: Vec<u8>,
}
pub struct PublicationPayload {
    pub receipt: String,
    pub outputs: Vec<PublicationBytes>,
}

impl Store {
    /// Commit the producer closure and LocalProduced through the same transaction.
    /// A refused transition writes no payload; replay never replaces immutable bytes.
    pub fn seal_publication_payload(
        &mut self,
        fence: &Fence,
        now_ms: u64,
        row: ArtifactRow,
        payload: &PublicationPayload,
    ) -> Result<Transition> {
        self.connection.execute_batch(SCHEMA)?;
        let project = match &row.scope {
            Scope::Project { project } => project.clone(),
            _ => return Err(Error::Kernel("producer payload requires a project".into())),
        };
        let key = row.replay_key.clone();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx.query_row("SELECT receipt FROM publication_payloads WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4 AND replay_key=?5", params![fence.account,fence.deployment,fence.install_id,project,key],|row|row.get(0)).optional()?;
        if previous
            .as_ref()
            .is_some_and(|receipt| receipt != &payload.receipt)
        {
            return Err(Error::Kernel("immutable producer receipt changed".into()));
        }
        let transition = Self::apply_on(&tx, fence, now_ms, Event::LocalProduced { row })?;
        if transition.refusals.is_empty() && previous.is_none() {
            tx.execute(
                "INSERT INTO publication_payloads VALUES(?1,?2,?3,?4,?5,?6,1)",
                params![
                    fence.account,
                    fence.deployment,
                    fence.install_id,
                    project,
                    key,
                    payload.receipt
                ],
            )?;
            for output in &payload.outputs {
                tx.execute(
                    "INSERT INTO publication_outputs VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        fence.account,
                        fence.deployment,
                        fence.install_id,
                        project,
                        key,
                        output.output_id,
                        output.bytes
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(transition)
    }
    fn has_publication_payloads(&self) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='publication_payloads'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some())
    }
    pub fn publication_payload(
        &self,
        fence: &Fence,
        project: &str,
        key: &str,
    ) -> Result<Option<String>> {
        if !self.has_publication_payloads()? {
            return Ok(None);
        }
        Ok(self.connection.query_row("SELECT receipt FROM publication_payloads WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4 AND replay_key=?5 AND retained=1",params![fence.account,fence.deployment,fence.install_id,project,key],|row|row.get(0)).optional()?)
    }
    pub fn publication_output(
        &self,
        fence: &Fence,
        project: &str,
        key: &str,
        output: &str,
    ) -> Result<Option<Vec<u8>>> {
        if self.publication_payload(fence, project, key)?.is_none() {
            return Ok(None);
        }
        Ok(self.connection.query_row("SELECT bytes FROM publication_outputs WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4 AND replay_key=?5 AND output_id=?6",params![fence.account,fence.deployment,fence.install_id,project,key,output],|row|row.get(0)).optional()?)
    }
    /// Stream one retained output through SQLite's incremental blob reader.
    /// The caller holds a read snapshot. Neither a SQL handle nor a full blob
    /// allocation leaves the storage owner.
    pub fn visit_publication_output(
        &self,
        fence: &Fence,
        project: &str,
        key: &str,
        output: &str,
        mut visit: impl FnMut(&[u8]),
    ) -> Result<Option<u64>> {
        use std::io::Read;
        if !self.has_publication_payloads()? {
            return Ok(None);
        }
        let rowid: Option<i64> = self.connection.query_row(
            "SELECT o.rowid FROM publication_outputs o JOIN publication_payloads p USING(account,deployment,install_id,project,replay_key) WHERE o.account=?1 AND o.deployment=?2 AND o.install_id=?3 AND o.project=?4 AND o.replay_key=?5 AND o.output_id=?6 AND p.retained=1",
            params![fence.account, fence.deployment, fence.install_id, project, key, output],
            |row| row.get(0),
        ).optional()?;
        let Some(rowid) = rowid else {
            return Ok(None);
        };
        let mut blob = self.connection.blob_open(
            rusqlite::MAIN_DB,
            "publication_outputs",
            "bytes",
            rowid,
            true,
        )?;
        let length = blob.len() as u64;
        if length == 0 || length > 64 * 1024 * 1024 {
            return Err(Error::Kernel(
                "publication output exceeds its byte bound".into(),
            ));
        }
        let mut buffer = [0u8; 65536];
        loop {
            let count = blob
                .read(&mut buffer)
                .map_err(|_| Error::Kernel("publication output cannot be read".into()))?;
            if count == 0 {
                break;
            }
            visit(&buffer[..count]);
        }
        Ok(Some(length))
    }
    /// Retire only derived output bytes. The immutable closure remains as evidence.
    pub fn retire_publication_payload(
        &mut self,
        fence: &Fence,
        project: &str,
        key: &str,
    ) -> Result<u64> {
        if !self.has_publication_payloads()? {
            return Ok(0);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let bytes:i64=tx.query_row("SELECT COALESCE(SUM(length(bytes)),0) FROM publication_outputs WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4 AND replay_key=?5",params![fence.account,fence.deployment,fence.install_id,project,key],|row|row.get(0))?;
        tx.execute("DELETE FROM publication_outputs WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4 AND replay_key=?5",params![fence.account,fence.deployment,fence.install_id,project,key])?;
        tx.execute("UPDATE publication_payloads SET retained=0 WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4 AND replay_key=?5",params![fence.account,fence.deployment,fence.install_id,project,key])?;
        tx.commit()?;
        Ok(bytes.max(0) as u64)
    }
}
