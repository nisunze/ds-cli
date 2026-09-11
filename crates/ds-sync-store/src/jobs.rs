//! Atomic durable compute jobs in the same native database as artifact sync.
use crate::{Error, Result, Store};
use ds_command_kernel::compute_jobs::{self, Event, Job, Phase};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS compute_jobs(
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, lane TEXT NOT NULL,
 phase TEXT NOT NULL, lease_until INTEGER NOT NULL, created INTEGER NOT NULL,
 row TEXT NOT NULL, input BLOB NOT NULL, result BLOB);
 CREATE INDEX IF NOT EXISTS compute_jobs_pending ON compute_jobs(owner,lane,phase,created);";

impl Store {
    pub fn initialize_jobs(&self) -> Result<()> {
        // Acknowledged jobs survive power failure as well as process restarts.
        self.connection.pragma_update(None, "synchronous", "FULL")?;
        self.connection.execute_batch(SCHEMA)?;
        Ok(())
    }
    pub fn submit_job(&mut self, job: &Job, input: &[u8]) -> Result<Job> {
        if !compute_jobs::digest(&job.id)
            || !compute_jobs::digest(&job.input_sha256)
            || !valid_input_tag(&job.engine, &job.input_tag)
            || job.owner.is_empty()
            || job.owner.len() > 512
            || !matches!(job.lane.as_str(), "stable" | "canary")
            || job.phase != Phase::Queued
            || input.len() > 64 * 1024 * 1024
        {
            return Err(Error::Kernel("invalid_compute_job".into()));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<(String, Vec<u8>)> = transaction
            .query_row(
                "SELECT row,input FROM compute_jobs WHERE id=?1",
                [&job.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((row, old_input)) = old {
            let existing: Job = serde_json::from_str(&row)?;
            if existing.owner != job.owner
                || existing.lane != job.lane
                || existing.input_sha256 != job.input_sha256
                || old_input != input
            {
                return Err(Error::Kernel("compute_job_identity_conflict".into()));
            }
            return Ok(existing);
        }
        transaction.execute("INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,?2,?3,'queued',0,?4,?5,?6)", params![job.id,job.owner,job.lane,job.created_at_ms,serde_json::to_string(job)?,input])?;
        transaction.commit()?;
        Ok(job.clone())
    }
    pub fn jobs(&self, owner: &str, lane: &str, limit: usize) -> Result<Vec<Job>> {
        let mut statement = self.connection.prepare("SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 ORDER BY created DESC,id LIMIT ?3")?;
        let rows = statement.query_map(params![owner, lane, limit.clamp(1, 1000)], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn job(&self, owner: &str, lane: &str, id: &str) -> Result<Option<Job>> {
        let row: Option<String> = self
            .connection
            .query_row(
                "SELECT row FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3",
                params![id, owner, lane],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|r| Ok(serde_json::from_str(&r)?)).transpose()
    }
    pub fn claim_job(
        &mut self,
        owner: &str,
        lane: &str,
        worker: &str,
        now: u64,
        lease_ms: u64,
    ) -> Result<Option<(Job, Vec<u8>)>> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(String,Vec<u8>)> = transaction.query_row("SELECT row,input FROM compute_jobs WHERE owner=?1 AND lane=?2 AND (phase='queued' OR (phase='running' AND lease_until<=?3)) ORDER BY created,id LIMIT 1",params![owner,lane,now],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((row, input)) = found else {
            return Ok(None);
        };
        let current: Job = serde_json::from_str(&row)?;
        let job = compute_jobs::transition(&current, Event::Claim { worker, lease_ms }, now)
            .map_err(|e| Error::Kernel(e.into()))?;
        transaction.execute(
            "UPDATE compute_jobs SET row=?2,phase='running',lease_until=?3 WHERE id=?1",
            params![job.id, serde_json::to_string(&job)?, job.lease_until_ms],
        )?;
        transaction.commit()?;
        Ok(Some((job, input)))
    }
    pub fn update_job(
        &mut self,
        owner: &str,
        lane: &str,
        id: &str,
        event: Event<'_>,
        now: u64,
        result: Option<&[u8]>,
    ) -> Result<Job> {
        if result.is_some_and(|bytes| bytes.len() > 256 * 1024 * 1024) {
            return Err(Error::Kernel("compute_result_too_large".into()));
        }
        if matches!(event, Event::Complete { .. }) != result.is_some() {
            return Err(Error::Kernel("compute_result_missing_or_unexpected".into()));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: String = transaction.query_row(
            "SELECT row FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3",
            params![id, owner, lane],
            |r| r.get(0),
        )?;
        let current: Job = serde_json::from_str(&row)?;
        let job =
            compute_jobs::transition(&current, event, now).map_err(|e| Error::Kernel(e.into()))?;
        let phase = match job.phase {
            Phase::Queued => "queued",
            Phase::Running => "running",
            Phase::Completed => "completed",
            Phase::Failed => "failed",
            Phase::Cancelled => "cancelled",
        };
        transaction.execute("UPDATE compute_jobs SET row=?2,phase=?3,lease_until=?4,result=COALESCE(?5,result) WHERE id=?1",params![job.id,serde_json::to_string(&job)?,phase,job.lease_until_ms,result])?;
        transaction.commit()?;
        Ok(job)
    }
    pub fn job_result(&self, owner: &str, lane: &str, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.connection.query_row("SELECT result FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3 AND phase='completed'",params![id,owner,lane],|r|r.get(0)).optional()?.flatten())
    }
    /// The sealed request retained with a job, visible only to its native
    /// owner/lane. Hosts use it to reconstruct a publication after restart;
    /// it is never a caller-supplied file reference.
    pub fn job_input(&self, owner: &str, lane: &str, id: &str) -> Result<Option<Vec<u8>>> {
        self.connection
            .query_row(
                "SELECT input FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3",
                params![id, owner, lane],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// True only while this exact leased worker is still entitled to commit.
    /// Native engines use it for cooperative cancellation after `Cancel` or a
    /// lease takeover; it does not change job ownership.
    pub fn worker_still_current(
        &self,
        owner: &str,
        lane: &str,
        id: &str,
        worker: &str,
        now: u64,
    ) -> Result<bool> {
        let row: Option<String> = self
            .connection
            .query_row(
                "SELECT row FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3",
                params![id, owner, lane],
                |row| row.get(0),
            )
            .optional()?;
        Ok(row
            .map(|row| serde_json::from_str::<Job>(&row))
            .transpose()?
            .is_some_and(|job| {
                job.phase == Phase::Running
                    && job.worker.as_deref() == Some(worker)
                    && job.lease_until_ms > now
            }))
    }
}

fn valid_input_tag(engine: &compute_jobs::EngineKind, input_tag: &str) -> bool {
    matches!(
        (engine, input_tag),
        (compute_jobs::EngineKind::FastLv, "ds.fast-lv.request/v1")
            | (
                compute_jobs::EngineKind::SolarPrepared,
                "ds.solar.calculate.prepared/v1"
            )
    )
}
