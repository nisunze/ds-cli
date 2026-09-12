//! Atomic durable compute jobs in the same native database as artifact sync.
//!
//! Every row this build writes carries the kernel's [`ExecutionContext`]: the
//! one verified record that says what the job is about. This module performs
//! the I/O on that record and decides nothing — `execution_context::visible`
//! answers who may see a row, `execution_context::recover` answers what a row
//! written before the context existed is about, and `execution_context::
//! capacity` answers whether a submission may join the queue and which project
//! may take the next worker. Both capacity questions are asked **inside the
//! transaction that writes**, from a census read in that transaction: a
//! census read a moment earlier describes a pool that other workers have
//! already changed, and under a burst it hands one project every slot. An
//! invisible row is reported exactly as an absent one, so an id in a log
//! discloses nothing.
use crate::{Error, Result, Store};
use ds_command_kernel::compute_jobs::{self, Event, Job, Phase};
use ds_command_kernel::execution_context::{
    self, ExecutionContext, Fault, Intent, Limits, Principal, Provenance, Refusal, Slot,
    CONTEXT_UNRECOVERABLE,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

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
    ///
    /// The row's own `owner` and `lane` are compared here as well as in the
    /// SQL that selected it. That is deliberate redundancy, for the reason
    /// `submit_job` states: a row written before contexts existed has no
    /// principal to compare, so the only thing keeping two identities apart
    /// for it is that the owner digest is derived from the uid — and this
    /// answer must not depend on a caller having built its `owner` and `uid`
    /// from the same identity.
    fn sees(&self, job: &Job) -> bool {
        if job.owner != self.owner || job.lane != self.lane {
            return false;
        }
        match &job.context {
            Some(context) => execution_context::visible(context, &self.kernel(), self.project),
            None => self.project.is_none(),
        }
    }
}

/// The store's census of work in flight, as the kernel's capacity op wants it.
///
/// `running` is work a worker currently holds a live lease on; `queued` is
/// work waiting for one, including a running row whose lease has expired —
/// it is waiting to be claimed again, and counting it as running would let
/// a dead worker's row fill the pool for ever. A row no worker can ever claim
/// is in neither — one that predates execution contexts, and equally one whose
/// stored context does not survive `validate`, which `claim_job` skips for the
/// same reason. Such a row is not in flight, so counting it would spend a
/// project's queue bound on work that will never run and, worse, let it stand
/// for ever as "waiting work of another project" and refuse every project its
/// share.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Census {
    pub running: Vec<Slot>,
    pub queued: Vec<Slot>,
}

/// What reopening a queue found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Rows that already carried their context.
    pub stored: usize,
    /// Rows rebuilt from the sealed input (Solar names its own project).
    pub from_sealed_input: usize,
    /// Rows with no project of their own to be recovered into. They are left
    /// exactly as they are: readable, never claimed, never executed, never
    /// published — the owner reads such a job's stored INPUT (it never ran, so
    /// it has no result) and submits it again naming the project, which is a
    /// new job with its own id.
    pub unrecoverable: Vec<String>,
}

impl Store {
    pub fn initialize_jobs(&self) -> Result<()> {
        // Acknowledged jobs survive power failure as well as process restarts.
        self.connection.pragma_update(None, "synchronous", "FULL")?;
        self.connection.execute_batch(SCHEMA)?;
        Ok(())
    }
    /// Persist one admitted job, bounded by `limits`. The execution context
    /// is not optional here: a row this build writes is always about a
    /// verified project, and the row's own identity must agree with it.
    ///
    /// The queue bound (`execution_context::capacity`, `Intent::Admit`) is
    /// asked inside the write transaction from a census read there, so two
    /// submissions racing for the last place cannot both take it. A refusal
    /// is [`Error::Refused`] with the kernel's own code, sentence and retry
    /// guidance, and writes nothing.
    ///
    /// A row already present under this id is returned unchanged when it
    /// carries a context (an idempotent resubmit; no capacity is consumed).
    /// A row under this id that predates execution contexts is
    /// `context_unrecoverable` and is left exactly as it is: this build's ids
    /// digest the project, so a released Server's id cannot be re-derived and
    /// such a row can only be reached deliberately. It is readable and never
    /// executed; the owner submits its stored input again naming the project,
    /// which is a new job with its own id.
    pub fn submit_job(&mut self, job: &Job, input: &[u8], limits: Limits) -> Result<Job> {
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
                || existing.engine != job.engine
                || existing.input_tag != job.input_tag
                || old_input != input
            {
                return Err(Error::Kernel("compute_job_identity_conflict".into()));
            }
            // The kernel already refused a key whose scope or operation moved;
            // the durable row is the second, independent statement of it, so a
            // host that forgot to ask cannot re-scope work by resubmitting.
            //
            // The principal is part of that statement. A row is invisible to
            // another authenticated identity, so `admit` is asked with no
            // existing context and cannot answer `principal_mismatch` for it;
            // the only thing that keeps two principals' keys apart is that the
            // owner digest is derived from the uid. That is true today and
            // this row must not depend on its staying true.
            match existing.context.as_ref() {
                Some(stored)
                    if stored.project != context.project
                        || stored.operation != context.operation
                        || stored.principal_uid != context.principal_uid =>
                {
                    return Err(Error::Kernel("compute_job_scope_conflict".into()));
                }
                Some(_) => return Ok(existing),
                None => {
                    return Err(Error::Refused(Refusal {
                        code: CONTEXT_UNRECOVERABLE,
                        message:
                            format!(
                                "that job was stored before execution contexts and names no project; {}",
                                execution_context::UNRECOVERABLE_REMEDY
                            ),
                        retry_after_ms: None,
                        scope: None,
                    }));
                }
            }
        }
        let census = census_in(&transaction, &job.owner, &job.lane, job.created_at_ms)?;
        decided(execution_context::capacity(
            limits,
            Intent::Admit,
            &census.running,
            &census.queued,
            &context.project,
        ))?;
        transaction.execute("INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,?2,?3,'queued',0,?4,?5,?6)", params![job.id,job.owner,job.lane,job.created_at_ms,serde_json::to_string(job)?,input])?;
        transaction.commit()?;
        Ok(job.clone())
    }
    /// This caller's newest durable jobs. Rows it may not see are absent, not
    /// refused. A caller narrowed to one project is narrowed in the query
    /// itself, so a page holds that project's newest rows up to the limit
    /// however much of the connection's other work is interleaved.
    pub fn jobs(&self, caller: &JobCaller<'_>, limit: usize) -> Result<Vec<Job>> {
        let mut statement = self.connection.prepare(
            "SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 AND (?3 IS NULL OR json_extract(row,'$.context.project')=?3) ORDER BY created DESC,id DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![
                caller.owner,
                caller.lane,
                caller.project,
                limit.clamp(1, 1000)
            ],
            |r| r.get::<_, String>(0),
        )?;
        visible(caller, rows)
    }
    /// Stable newest-first keyset page over this caller's durable jobs.  The
    /// cursor is the last `(created_at_ms,id)` returned; callers can walk past
    /// unrelated engines without making old recoverable work disappear. A
    /// narrowed caller pages over its project's rows alone.
    pub fn jobs_page(
        &self,
        caller: &JobCaller<'_>,
        before: Option<(u64, &str)>,
        limit: usize,
    ) -> Result<Vec<Job>> {
        // The first page starts above every row. SQLite integers are i64, so
        // the sentinel must be one it can hold: `u64::MAX` fails to bind.
        let (created, id) = before.unwrap_or((i64::MAX as u64, "~"));
        let mut statement = self.connection.prepare(
            "SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 AND (created<?3 OR (created=?3 AND id<?4)) AND (?5 IS NULL OR json_extract(row,'$.context.project')=?5) ORDER BY created DESC,id DESC LIMIT ?6",
        )?;
        let rows = statement.query_map(
            params![
                caller.owner,
                caller.lane,
                created,
                id,
                caller.project,
                limit.clamp(1, 1000)
            ],
            |row| row.get::<_, String>(0),
        )?;
        visible(caller, rows)
    }
    pub fn job(&self, caller: &JobCaller<'_>, id: &str) -> Result<Option<Job>> {
        Ok(self.visible_row(caller, id)?.map(|(job, _)| job))
    }
    /// Take the oldest claimable job whose project the kernel's capacity
    /// decision admits under `limits`, or `None` — and change nothing — when
    /// no project may take a worker right now.
    ///
    /// The census is read inside the claiming transaction and
    /// `execution_context::capacity` (`Intent::Claim`) is asked once per
    /// candidate row, in queue order: a project over its fair share is skipped,
    /// not refused, so another project's work starts instead of the machine
    /// idling; a full pool ends the search. Because the census and the claim
    /// are one transaction, two workers claiming at once cannot both read a
    /// pool with room and both take it.
    pub fn claim_job(
        &mut self,
        caller: &JobCaller<'_>,
        worker: &str,
        now: u64,
        lease_ms: u64,
        limits: Limits,
    ) -> Result<Option<(Job, Vec<u8>)>> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let census = census_in(&transaction, caller.owner, caller.lane, now)?;
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
                if !caller.sees(&current) {
                    continue;
                }
                match execution_context::capacity(
                    limits,
                    Intent::Claim,
                    &census.running,
                    &census.queued,
                    &context.project,
                ) {
                    Ok(()) => {}
                    Err(Fault::Refused(refusal)) if refusal.scope == Some("global") => break,
                    Err(Fault::Refused(_)) => continue,
                    Err(Fault::Hard(message)) => return Err(Error::Kernel(message)),
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

    /// What is running and what is waiting at `now`, named only by project —
    /// the census `execution_context::capacity` takes (see [`Census`] for
    /// what counts). Unnarrowed by design: a fairness decision is about the
    /// whole host, not about one project. This is the operator's view; the
    /// decisions themselves read the census inside their own transaction.
    pub fn census(&self, owner: &str, lane: &str, now: u64) -> Result<Census> {
        census_in(&self.connection, owner, lane, now)
    }

    /// Give every row this connection owns the execution context it is about,
    /// reconstructing the ones written before contexts existed.
    ///
    /// `sealed_project` reads the project out of a row's own sealed input —
    /// only the host that understands the engine can do that, so it is a
    /// callback and not a decision here. `recover` in the kernel decides what
    /// a row is about; this loop only persists the answer and counts what it
    /// could not honestly reconstruct. A row that already carries a context is
    /// never rewritten and never read — reopening a live queue counts it and
    /// touches nothing, so a Server the owner leaves running for weeks does
    /// not pay its whole queue in memory each time it restarts. Nothing
    /// outside a row's own bytes is offered: no selection of the moment can
    /// name a project for work admitted before this build.
    pub fn recover_contexts(
        &mut self,
        principal: &Principal,
        owner: &str,
        sealed_project: &dyn Fn(&Job, &[u8]) -> Option<String>,
        now_ms: u64,
    ) -> Result<Recovery> {
        // Only the rows that need a context are read, and their sealed bytes
        // one row at a time. A long-lived queue holds hundreds of megabytes of
        // sealed Solar inputs; reading every one of them in order to skip it
        // would make reopening the queue cost the whole queue in memory at
        // once — the unbounded allocation the contract forbids, paid on every
        // restart of a Server that has nothing to recover.
        let mut recovery = Recovery {
            stored: self.connection.query_row(
                "SELECT COUNT(*) FROM compute_jobs WHERE owner=?1 AND lane=?2 AND json_extract(row,'$.context') IS NOT NULL",
                params![owner, principal.lane],
                |row| row.get::<_, i64>(0),
            )? as usize,
            ..Recovery::default()
        };
        let contextless: Vec<String> = {
            let mut statement = self.connection.prepare(
                "SELECT id FROM compute_jobs WHERE owner=?1 AND lane=?2 AND json_extract(row,'$.context') IS NULL ORDER BY created,id",
            )?;
            let rows = statement.query_map(params![owner, principal.lane], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in contextless {
            let Some((row, input)) = self
                .connection
                .query_row(
                    "SELECT row,input FROM compute_jobs WHERE id=?1 AND owner=?2 AND lane=?3",
                    params![id, owner, principal.lane],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()?
            else {
                continue;
            };
            let job: Job = serde_json::from_str(&row)?;
            if job.context.is_some() {
                recovery.stored += 1;
                continue;
            }
            let sealed = sealed_project(&job, &input);
            drop(input);
            let row = serde_json::to_value(&job)?;
            match execution_context::recover(row, principal, sealed.as_deref(), now_ms) {
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

/// The census as seen through one connection or transaction — the same
/// query for the operator's view and for the decisions, so they cannot
/// disagree about what counts.
fn census_in(connection: &Connection, owner: &str, lane: &str, now: u64) -> Result<Census> {
    let mut statement = connection.prepare(
        "SELECT row FROM compute_jobs WHERE owner=?1 AND lane=?2 AND phase IN ('queued','running')",
    )?;
    let rows = statement.query_map(params![owner, lane], |row| row.get::<_, String>(0))?;
    let mut census = Census::default();
    for row in rows {
        let job: Job = serde_json::from_str(&row?)?;
        // Exactly what `claim_job` will accept: a context that is there and
        // that reads. Anything else is work no worker can take, and the
        // capacity decision must not be made about it.
        let Some(context) = job
            .context
            .as_ref()
            .filter(|context| context.validate().is_ok())
        else {
            continue;
        };
        let slot = Slot {
            project: context.project.clone(),
        };
        match job.phase {
            Phase::Running if job.lease_until_ms > now => census.running.push(slot),
            Phase::Running | Phase::Queued => census.queued.push(slot),
            _ => {}
        }
    }
    Ok(census)
}

/// A kernel decision as the store's answer: a refusal is relayed with its
/// code, sentence and guidance intact; a hard fault is the host's own bug.
fn decided(decision: std::result::Result<(), Fault>) -> Result<()> {
    match decision {
        Ok(()) => Ok(()),
        Err(Fault::Refused(refusal)) => Err(Error::Refused(refusal)),
        Err(Fault::Hard(message)) => Err(Error::Kernel(message)),
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
