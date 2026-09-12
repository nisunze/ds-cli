//! Atomic durable compute jobs in the same native database as artifact sync.
//!
//! Every row this build writes carries the kernel's [`ExecutionContext`]: the
//! one verified record that says what the job is about. This module performs
//! the I/O on that record and decides nothing — `execution_context::visible`
//! answers who may see a row, `execution_context::recover` answers what a row
//! written before the context existed is about, and the caller's capacity gate
//! answers which project may take the next worker. An invisible row is
//! reported exactly as an absent one, so an id in a log discloses nothing.
use crate::{Error, Result, Store};
use ds_command_kernel::compute_jobs::{self, Event, Job, Phase};
use ds_command_kernel::execution_context::{self, ExecutionContext, Principal, Provenance, Slot};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS compute_jobs(
 id TEXT PRIMARY KEY, owner TEXT NOT NULL, lane TEXT NOT NULL,
 phase TEXT NOT NULL, lease_until INTEGER NOT NULL, created INTEGER NOT NULL,
 row TEXT NOT NULL, input BLOB NOT NULL, result BLOB);
 CREATE INDEX IF NOT EXISTS compute_jobs_pending ON compute_jobs(owner,lane,phase,created);";

/// Who is asking about durable compute work.
///
/// `owner` is the store's own durable fence — the digest of the native
/// account, lane and credential audience the row was written under, which is
/// what the SQL selects on. The remaining fields are the authenticated
/// identity the kernel compares a row's execution context against, and
/// `project` is the caller's optional narrowing: `Some(p)` asks only about
/// `p`, `None` asks about everything this connection may see.
#[derive(Clone, Copy, Debug)]
pub struct JobCaller<'a> {
    pub owner: &'a str,
    pub uid: &'a str,
    pub lane: &'a str,
    pub deployment: &'a str,
    pub project: Option<&'a str>,
}

impl<'a> JobCaller<'a> {
    /// The whole connection, unnarrowed.
    pub const fn connection(
        owner: &'a str,
        uid: &'a str,
        lane: &'a str,
        deployment: &'a str,
    ) -> Self {
        Self {
            owner,
            uid,
            lane,
            deployment,
            project: None,
        }
    }

    /// The same connection, asking about one project only.
    #[must_use]
    pub const fn about(mut self, project: Option<&'a str>) -> Self {
        self.project = project;
        self
    }

    fn kernel(&self) -> execution_context::Caller {
        execution_context::Caller {
            uid: self.uid.to_owned(),
            lane: self.lane.to_owned(),
            deployment: self.deployment.to_owned(),
        }
    }

    /// May this caller see this row at all?
    ///
    /// A row carrying a context is the kernel's decision. A row written
    /// before this slice that recovery could not reconstruct has no project
    /// to compare, so it stays readable by its own unnarrowed connection —
    /// the operator must be able to see the work the host refuses to guess
    /// about — and is invisible to any caller narrowing to a project, because
    /// it is not known to be in one.
    fn sees(&self, job: &Job) -> bool {
        match &job.context {
            Some(context) => execution_context::visible(context, &self.kernel(), self.project),
            None => self.project.is_none(),
        }
    }
}

/// The store's census of work in flight, as the kernel's capacity op wants it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Census {
    pub running: Vec<Slot>,
    pub queued: Vec<Slot>,
}

/// The project a row that predates execution contexts, and could not be
/// recovered, is counted under. It is not a project id: no directory returns
/// it, so nothing is admitted into it and fairness cannot be gamed with it.
pub const UNRECOVERED_PROJECT: &str = "ds:unrecovered-context";

/// What reopening a queue found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Rows that already carried their context.
    pub stored: usize,
    /// Rows rebuilt from the sealed input (Solar names its own project).
    pub from_sealed_input: usize,
    /// Rows rebuilt from the caller's saved selection (transformer work).
    pub from_saved_selection: usize,
    /// Rows with no honest source for a project. They are left exactly as
    /// they are: readable, never claimed, never executed.
    pub unrecoverable: Vec<String>,
}

impl Store {
    pub fn initialize_jobs(&self) -> Result<()> {
        // Acknowledged jobs survive power failure as well as process restarts.
        self.connection.pragma_update(None, "synchronous", "FULL")?;
        self.connection.execute_batch(SCHEMA)?;
        Ok(())
    }
    /// Persist one admitted job. The execution context is not optional here:
    /// a row this build writes is always about a verified project, and the
    /// row's own identity must agree with it.
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
        let Some(context) = job.context.as_ref() else {
            return Err(Error::Kernel(
                "compute_job_without_execution_context".into(),
            ));
        };
        context
            .validate()
            .map_err(|fault| Error::Kernel(fault.to_string()))?;
        if context.job_id != job.id
            || context.input_sha256 != job.input_sha256
            || context.lane != job.lane
        {
            return Err(Error::Kernel("compute_job_context_mismatch".into()));
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
            // The kernel already refused a key whose scope or operation moved;
            // the durable row is the second, independent statement of it, so a
            // host that forgot to ask cannot re-scope work by resubmitting.
            if existing.context.as_ref().is_some_and(|stored| {
                stored.project != context.project || stored.operation != context.operation
            }) {
                return Err(Error::Kernel("compute_job_scope_conflict".into()));
            }
            return Ok(existing);
        }
        transaction.execute("INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,?2,?3,'queued',0,?4,?5,?6)", params![job.id,job.owner,job.lane,job.created_at_ms,serde_json::to_string(job)?,input])?;
        transaction.commit()?;
        Ok(job.clone())
    }
    /// This caller's newest durable jobs. Rows it may not see are absent, not
    /// refused; the limit bounds the rows read, so a page can come back
    /// shorter than the limit when another project's work is interleaved.
    pub fn jobs(&self, caller: &JobCaller<'_>, limit: usize) -> Result<Vec<Job>> {
        let mut statement = self.connection.prepare("SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 ORDER BY created DESC,id LIMIT ?3")?;
        let rows = statement.query_map(
            params![caller.owner, caller.lane, limit.clamp(1, 1000)],
            |r| r.get::<_, String>(0),
        )?;
        visible(caller, rows)
    }
    /// Stable newest-first keyset page over this caller's durable jobs.  The
    /// cursor is the last `(created_at_ms,id)` returned; callers can walk past
    /// unrelated engines without making old recoverable work disappear.
    pub fn jobs_page(
        &self,
        caller: &JobCaller<'_>,
        before: Option<(u64, &str)>,
        limit: usize,
    ) -> Result<Vec<Job>> {
        let (created, id) = before.unwrap_or((u64::MAX, "~"));
        let mut statement = self.connection.prepare(
            "SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 AND (created<?3 OR (created=?3 AND id<?4)) ORDER BY created DESC,id DESC LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![caller.owner, caller.lane, created, id, limit.clamp(1, 1000)],
            |row| row.get::<_, String>(0),
        )?;
        visible(caller, rows)
    }
    pub fn job(&self, caller: &JobCaller<'_>, id: &str) -> Result<Option<Job>> {
        Ok(self.visible_row(caller, id)?.map(|(job, _)| job))
    }
    /// Take the oldest claimable job whose project the caller's own capacity
    /// gate admits. `admit` is asked once per candidate row, in queue order,
    /// and its answer is the kernel's `capacity` decision: a project over its
    /// fair share is skipped, not refused, so another project's work starts
    /// instead of the machine idling.
    pub fn claim_job(
        &mut self,
        caller: &JobCaller<'_>,
        worker: &str,
        now: u64,
        lease_ms: u64,
        admit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Option<(Job, Vec<u8>)>> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claimed = {
            let mut statement = transaction.prepare("SELECT row,input FROM compute_jobs WHERE owner=?1 AND lane=?2 AND (phase='queued' OR (phase='running' AND lease_until<=?3)) ORDER BY created,id")?;
            let mut rows = statement.query(params![caller.owner, caller.lane, now])?;
            let mut found = None;
            while let Some(row) = rows.next()? {
                let current: Job = serde_json::from_str(&row.get::<_, String>(0)?)?;
                // A row whose context could not be recovered is never
                // executed: the host must be told, not allowed to guess a
                // project for work that will publish somewhere.
                let Some(context) = current.context.as_ref() else {
                    continue;
                };
                if !caller.sees(&current) || !admit(&context.project) {
                    continue;
                }
                found = Some((current, row.get::<_, Vec<u8>>(1)?));
                break;
            }
            found
        };
        let Some((current, input)) = claimed else {
            return Ok(None);
        };
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
        caller: &JobCaller<'_>,
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
            params![id, caller.owner, caller.lane],
            |r| r.get(0),
        )?;
        let current: Job = serde_json::from_str(&row)?;
        if !caller.sees(&current) {
            // Exactly the answer an unknown id gets, from the same code path.
            return Err(Error::Storage(rusqlite::Error::QueryReturnedNoRows));
        }
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
    pub fn job_result(&self, caller: &JobCaller<'_>, id: &str) -> Result<Option<Vec<u8>>> {
        let Some((job, _)) = self.visible_row(caller, id)? else {
            return Ok(None);
        };
        if job.phase != Phase::Completed {
            return Ok(None);
        }
        Ok(self
            .connection
            .query_row(
                "SELECT result FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3 AND phase='completed'",
                params![id, caller.owner, caller.lane],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }
    /// The sealed request retained with a job, visible only to a caller the
    /// job's context admits. Hosts use it to reconstruct a publication after
    /// restart; it is never a caller-supplied file reference.
    pub fn job_input(&self, caller: &JobCaller<'_>, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.visible_row(caller, id)?.map(|(_, input)| input))
    }

    /// What is running and what is waiting, named only by project — the
    /// census `execution_context::capacity` takes. Unnarrowed by design: a
    /// fairness decision is about the whole host, not about one project.
    pub fn census(&self, owner: &str, lane: &str) -> Result<Census> {
        let mut statement = self.connection.prepare(
            "SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 AND phase IN ('queued','running')",
        )?;
        let rows = statement.query_map(params![owner, lane], |row| row.get::<_, String>(0))?;
        let mut census = Census::default();
        for row in rows {
            let job: Job = serde_json::from_str(&row?)?;
            let slot = Slot {
                project: job.context.as_ref().map_or_else(
                    || UNRECOVERED_PROJECT.to_owned(),
                    |context| context.project.clone(),
                ),
            };
            match job.phase {
                Phase::Running => census.running.push(slot),
                Phase::Queued => census.queued.push(slot),
                _ => {}
            }
        }
        Ok(census)
    }

    /// Give every row this connection owns the execution context it is about,
    /// reconstructing the ones written before contexts existed.
    ///
    /// `sealed_project` reads the project out of a row's own sealed input —
    /// only the host that understands the engine can do that, so it is a
    /// callback and not a decision here. `recover` in the kernel decides what
    /// a row is about; this loop only persists the answer and counts what it
    /// could not honestly reconstruct. A row that already carries a context is
    /// never rewritten, so reopening a live queue writes nothing.
    pub fn recover_contexts(
        &mut self,
        principal: &Principal,
        owner: &str,
        saved_project: Option<&str>,
        sealed_project: &dyn Fn(&Job, &[u8]) -> Option<String>,
        now_ms: u64,
    ) -> Result<Recovery> {
        let pending: Vec<(Job, Vec<u8>)> = {
            let mut statement = self
                .connection
                .prepare("SELECT row,input FROM compute_jobs WHERE owner=?1 AND lane=?2")?;
            let rows = statement.query_map(params![owner, principal.lane], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            rows.map(|row| {
                let (row, input) = row?;
                Ok((serde_json::from_str::<Job>(&row)?, input))
            })
            .collect::<Result<Vec<_>>>()?
        };
        let mut recovery = Recovery::default();
        for (job, input) in pending {
            if job.context.is_some() {
                recovery.stored += 1;
                continue;
            }
            let sealed = sealed_project(&job, &input);
            let row = serde_json::to_value(&job)?;
            match execution_context::recover(
                row,
                principal,
                saved_project,
                sealed.as_deref(),
                now_ms,
            ) {
                Ok((context, provenance)) => {
                    let mut recovered = job.clone();
                    recovered.context = Some(context);
                    self.connection.execute(
                        "UPDATE compute_jobs SET row=?2 WHERE id=?1",
                        params![recovered.id, serde_json::to_string(&recovered)?],
                    )?;
                    match provenance {
                        Provenance::Stored => recovery.stored += 1,
                        Provenance::SealedInput => recovery.from_sealed_input += 1,
                        Provenance::SavedSelection => recovery.from_saved_selection += 1,
                    }
                }
                Err(_) => recovery.unrecoverable.push(job.id.clone()),
            }
        }
        Ok(recovery)
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

    fn visible_row(&self, caller: &JobCaller<'_>, id: &str) -> Result<Option<(Job, Vec<u8>)>> {
        let found: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT row,input FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3",
                params![id, caller.owner, caller.lane],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((row, input)) = found else {
            return Ok(None);
        };
        let job: Job = serde_json::from_str(&row)?;
        Ok(caller.sees(&job).then_some((job, input)))
    }
}

/// Decode a page of stored rows and drop the ones this caller may not see. A
/// row that will not decode is still an error: an unreadable queue is a fault,
/// not an invisible job.
fn visible(
    caller: &JobCaller<'_>,
    rows: impl Iterator<Item = rusqlite::Result<String>>,
) -> Result<Vec<Job>> {
    let mut jobs = Vec::new();
    for row in rows {
        let job: Job = serde_json::from_str(&row?)?;
        if caller.sees(&job) {
            jobs.push(job);
        }
    }
    Ok(jobs)
}

/// The execution context a durable job row carries, re-exported so a host does
/// not need the kernel's module path to name it.
pub type JobContext = ExecutionContext;

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
