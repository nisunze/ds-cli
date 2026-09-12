//! Solar's producer adapter for the shared Sync Center store.
//!
//! It reconstructs rows and bytes from the durable compute record on every
//! call.  There is no browser cache, temporary request file or second queue.

use std::{
    collections::BTreeSet,
    io::Cursor,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use ds_command_kernel::{
    compute_jobs::{EngineKind, Job},
    sync::Identity,
};
use ds_compute_runtime::{
    self as runtime, CompletionObserver, SolarPublication, SolarPublicationMetadata,
};
use ds_sync_runtime::{
    ActivityRow, LocalOutput, LocalRow, Producer, SolarPublishOutcome, SolarPublisher,
    TransferReceipt, VerifiedReads, inventory_digest,
};

use crate::server_sync::sessions::ServerSessions;
use serde_json::Value;

/// One page of the durable queue, the store's own maximum.
const PAGE: usize = 1000;

/// How many of a project's newest durable Solar rows one projection covers.
///
/// A Sync Center projection is a working view, not an archive: it exists so
/// the owner can see and publish what this host has been doing. Bounding it
/// per project is what keeps a long-lived host — the owner may leave one
/// running for months — answering in constant time, and keeps one busy
/// project from deciding what another project's answer costs. A project with
/// more rows than this says so (`more` in the activity envelope); nothing is
/// lost, because the rows themselves stay durable and readable job by job.
///
/// The retention question this bound makes visible — when, if ever, a durable
/// Solar row is removed — is an OWNER decision and is recorded as one in
/// `docs-routes.md` §7. Nothing here deletes anything.
const PROJECTION_PER_PROJECT: usize = 512;

pub struct SolarActivity {
    database: PathBuf,
    /// One session per authorized project, opened on first use. A Solar
    /// publication for one project and a report drain for another are two
    /// sessions on one host, not one session that switches.
    sessions: Arc<ServerSessions>,
    /// Native engine registration and its following gateway pass are one
    /// release-attributed action, even when compute workers finish together.
    sync_gate: Mutex<()>,
    /// Shared Sync Center reader. It owns the object-ticket origin pin,
    /// redirect refusal, staging, and digest proof; this host only anchors its
    /// private cache below the protected server state directory.
    reads: VerifiedReads,
    wake: AtomicBool,
    publication_failure: Mutex<Option<String>>,
}

/// One bounded background publisher for a server instance. It owns no durable
/// queue: wake-ups and periodic recovery only drain rows already held by the
/// shared Sync Center store.
pub struct SolarSyncPump {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

/// The shared runtime owns report retry truth. This host only remembers the
/// last sealed producer observation and translates the runtime's typed wake
/// decision into the existing background thread's next trigger.
#[derive(Default)]
struct ReportWake {
    startup: bool,
    fingerprint: Option<String>,
    retry_eligible: bool,
    offline: bool,
    reconnect_pending: bool,
    wake_at_ms: Option<u64>,
}

impl ReportWake {
    /// A project's scheduler before its first observation: one startup pass
    /// is owed, and nothing else is known yet.
    fn at_startup() -> Self {
        Self {
            startup: true,
            ..Self::default()
        }
    }

    fn needs_observation(&self, now_ms: u64, recovery_due: bool) -> bool {
        // Inventory scans are local-only and retain the established 30-second
        // recovery cadence. An actual kernel deadline or a proven online
        // transition is an event, so it may wake the same thread sooner.
        recovery_due
            || self.reconnect_pending
            || self.wake_at_ms.is_some_and(|at_ms| at_ms <= now_ms)
    }

    fn trigger(
        &self,
        inventory: &crate::server_reports::Inventory,
        now_ms: u64,
        recovery_due: bool,
    ) -> Option<ds_sync_runtime::Trigger> {
        if self.reconnect_pending {
            return Some(ds_sync_runtime::Trigger::Reconnect);
        }
        if self.startup {
            return recovery_due.then_some(ds_sync_runtime::Trigger::Startup);
        }
        let deadline_due = self.wake_at_ms.is_some_and(|at_ms| at_ms <= now_ms);
        let changed = self.fingerprint.as_deref() != Some(inventory.fingerprint.as_str());
        if !deadline_due && !recovery_due {
            return None;
        }
        // `Offline` is a kernel observation, not a timer reason. Retry the
        // pending local work at the recovery boundary; only a later successful
        // pass can prove that a `Reconnect` record read is warranted.
        (changed || self.retry_eligible || self.offline || deadline_due)
            .then_some(ds_sync_runtime::Trigger::LocalChange)
    }

    fn applied(&mut self, pass: crate::server_reports::Pass) {
        let reconnected = self.offline && !pass.offline && !self.reconnect_pending;
        self.startup = false;
        self.fingerprint = Some(pass.inventory.fingerprint);
        self.retry_eligible = pass.retry_eligible;
        self.offline = pass.offline;
        self.reconnect_pending = reconnected;
        self.wake_at_ms = pass.wake_at_ms;
    }

    fn failed(&mut self) {
        // Consume an attempted startup/deadline until the next local recovery
        // boundary. A failed request cannot spin the background thread.
        self.fingerprint = None;
        self.reconnect_pending = false;
        self.wake_at_ms = None;
    }
}

impl SolarSyncPump {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Drop for SolarSyncPump {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl SolarActivity {
    /// The connection is not a parameter: the sessions carry it, and with it
    /// the one account whose work this host projects.
    pub fn open(database: PathBuf, sessions: Arc<ServerSessions>) -> Result<Arc<Self>, String> {
        let state_directory = database
            .parent()
            .ok_or("server database path has no protected state directory")?;
        Ok(Arc::new(Self {
            sessions,
            reads: VerifiedReads::new(state_directory.join("sync-downloads")),
            database,
            sync_gate: Mutex::new(()),
            wake: AtomicBool::new(false),
            publication_failure: Mutex::new(None),
        }))
    }

    fn caller(&self) -> ds_sync_store::JobCaller<'_> {
        self.sessions.identity().caller(None)
    }

    /// The projects this host holds durable Solar or report work in. Read from
    /// the work itself, never from a selection.
    ///
    /// It pages the whole queue and keeps only the distinct names, so a host
    /// left running for months answers this in bounded memory however much
    /// work it has done. A long queue is not an error; it is a long queue.
    fn projects(&self) -> Result<Vec<String>, String> {
        let store = runtime::open(&self.database)?;
        let caller = self.caller();
        let mut projects = BTreeSet::new();
        let mut cursor: Option<(u64, String)> = None;
        loop {
            let page = store
                .jobs_page(
                    &caller,
                    cursor.as_ref().map(|(created, id)| (*created, id.as_str())),
                    PAGE,
                )
                .map_err(|error| error.to_string())?;
            let Some(last) = page.last() else { break };
            cursor = Some((last.created_at_ms, last.id.clone()));
            for job in page
                .into_iter()
                .filter(|job| job.engine == EngineKind::SolarPrepared)
            {
                if let Some(context) = job.context {
                    projects.insert(context.project);
                }
            }
        }
        projects.extend(crate::server_reports::projects_with_publications(
            &self.database,
        )?);
        Ok(projects.into_iter().collect())
    }

    /// One project's newest durable Solar rows, and whether it holds more
    /// than the projection covers.
    ///
    /// A projection is a bounded read of ONE project, narrowed in the query
    /// itself: a project with a hundred thousand rows costs the same as a
    /// project with ten, and no project's queue length can fail another
    /// project's answer. It never refuses for being long — the previous
    /// bound turned a busy host into a host with no Sync Center at all — it
    /// reports the truncation and the caller is told in the envelope.
    fn solar_jobs_in(&self, project: &str) -> Result<(Vec<Job>, bool), String> {
        let store = runtime::open(&self.database)?;
        let caller = self.sessions.identity().caller(Some(project));
        let mut cursor: Option<(u64, String)> = None;
        let mut jobs = Vec::new();
        loop {
            let page = store
                .jobs_page(
                    &caller,
                    cursor.as_ref().map(|(created, id)| (*created, id.as_str())),
                    PAGE,
                )
                .map_err(|error| error.to_string())?;
            let Some(last) = page.last() else {
                return Ok((jobs, false));
            };
            cursor = Some((last.created_at_ms, last.id.clone()));
            for job in page
                .into_iter()
                .filter(|job| job.engine == EngineKind::SolarPrepared)
            {
                if jobs.len() == PROJECTION_PER_PROJECT {
                    return Ok((jobs, true));
                }
                jobs.push(job);
            }
        }
    }

    /// Whether one project holds more durable Solar work than a projection
    /// covers. A LOCAL fact about the rows on disk, so `/v1/activity` can
    /// report it whether or not that project's Sync Center could be read.
    pub fn projection_truncated(&self, project: &str) -> Result<bool, String> {
        Ok(self.solar_jobs_in(project)?.1)
    }

    fn publication(&self, job: &Job) -> Result<SolarPublication, String> {
        let store = runtime::open(&self.database)?;
        let input = store
            .job_input(&self.caller(), &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable prepared input")?;
        let result = store
            .job_result(&self.caller(), &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable result")?;
        runtime::solar_publication(job, &input, &result)
    }

    fn publication_metadata(&self, job: &Job) -> Result<SolarPublicationMetadata, String> {
        let store = runtime::open(&self.database)?;
        let input = store
            .job_input(&self.caller(), &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable prepared input")?;
        let result = store
            .job_result(&self.caller(), &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable result")?;
        runtime::solar_publication_metadata(job, &input, &result)
    }

    /// One project's Sync Center projection. The caller names the project;
    /// this host never answers for "the" project.
    pub fn store_read(&self, project: &str) -> Result<Value, String> {
        let producer = SolarProducer {
            activity: self,
            project: project.to_owned(),
        };
        self.sessions.session(project)?.with_host_for_project(
            project,
            &producer,
            &self.reads,
            |host| host.store_read(),
        )
    }

    /// Cancel only the shared publication owned by this completed Solar job.
    /// The compute result remains durable and readable; StoreHost records the
    /// publication transition and refuses an already committed artifact.
    pub fn cancel_publication(&self, job: &Job) -> Result<Value, String> {
        if job.engine != EngineKind::SolarPrepared
            || job.phase != ds_command_kernel::compute_jobs::Phase::Completed
        {
            return Err("only a completed prepared Solar job has a publication to cancel".into());
        }
        let store = runtime::open(&self.database)?;
        let input = store
            .job_input(&self.caller(), &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable prepared input")?;
        let scope = runtime::solar_job_scope(job, &input)?;
        // The job's own context is the authority on what it is about; the
        // sealed input must agree with it, and a row that disagrees is not
        // cancelled under either name.
        if job
            .context
            .as_ref()
            .is_some_and(|context| context.project != scope.project_id)
        {
            return Err("this job's sealed input names another project than its context".into());
        }
        let producer = SolarProducer {
            activity: self,
            project: scope.project_id.clone(),
        };
        self.sessions
            .session(&scope.project_id)?
            .with_host_for_project(&scope.project_id, &producer, &self.reads, |host| {
                // A completion can be cancelled before the background pump's
                // first pass. Record the existing local row first, without
                // opening or uploading any remote work.
                ds_sync_runtime::SyncHost::local(host, &scope.project_id)?;
                let row = producer
                    .rows(&scope.project_id)?
                    .into_iter()
                    .find(|row| row.client_publish_id == job.id)
                    .ok_or("Solar job no longer owns the current publication for this city")?;
                let transition = host.cancel_publication(&row.identity, &job.id)?;
                let state = if transition.refusals.is_empty() {
                    "cancelled"
                } else {
                    "refused"
                };
                Ok(serde_json::json!({
                    "state": state,
                    "identity": row.identity,
                    "transition": transition,
                }))
            })
    }

    /// Start the non-blocking publication pump after the HTTP server can
    /// accept requests. Completion observers only set `wake`; they never
    /// upload from a compute worker or delay durable replay at startup.
    pub fn start_pump(self: &Arc<Self>) -> SolarSyncPump {
        let activity = self.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let mut last_recovery = Instant::now() - Duration::from_secs(30);
            let mut last_report_recovery = Instant::now() - Duration::from_secs(30);
            // One report scheduler per project. A project that is offline or
            // holding a retry keeps its own deadline; it never sets another
            // project's, and it never drains another project's queue.
            let mut reports: std::collections::BTreeMap<String, ReportWake> =
                std::collections::BTreeMap::new();
            while !worker_stop.load(Ordering::Acquire) {
                let woken = activity.wake.swap(false, Ordering::AcqRel);
                let recovery_due = last_recovery.elapsed() >= Duration::from_secs(30);
                let report_recovery_due = last_report_recovery.elapsed() >= Duration::from_secs(30);
                let now = now_ms();

                if woken || recovery_due {
                    // The receipt records the actionable Solar state. Keep
                    // stderr token-free and bounded if its authority work fails.
                    match activity.publish_pending() {
                        Ok(()) => activity.clear_publication_failure(),
                        Err(error) => activity.note_publication_failure(&error),
                    }
                    last_recovery = Instant::now();
                }

                // Reports are a separate shared-runtime producer. A completed
                // Solar job never turns into a report `Manual` sync or a remote
                // head poll; only report inventory and runtime wake facts can.
                let scopes = match activity.projects() {
                    Ok(scopes) => scopes,
                    Err(error) => {
                        activity.note_publication_failure(&error);
                        Vec::new()
                    }
                };
                let mut observed = false;
                for project in scopes {
                    let wake = reports
                        .entry(project.clone())
                        .or_insert_with(ReportWake::at_startup);
                    if !wake.needs_observation(now, report_recovery_due) {
                        continue;
                    }
                    observed = true;
                    let pass = activity.sessions.session(&project).and_then(|session| {
                        let inventory =
                            crate::server_reports::inventory(&activity.database, &session)?;
                        let Some(trigger) = wake.trigger(&inventory, now, report_recovery_due)
                        else {
                            return Ok(None);
                        };
                        crate::server_reports::drain(
                            &activity.database,
                            &session,
                            &activity.reads,
                            trigger,
                        )
                        .map(Some)
                    });
                    match pass {
                        Ok(Some(pass)) => wake.applied(pass),
                        Ok(None) => {}
                        Err(error) => {
                            wake.failed();
                            activity.note_publication_failure(&error);
                        }
                    }
                }
                if observed {
                    // Every actual observation consumes this recovery slot. A
                    // failed startup or expired deadline retries at the next
                    // bounded local recovery, never in the next 100ms loop.
                    last_report_recovery = Instant::now();
                }
                for _ in 0..10 {
                    if worker_stop.load(Ordering::Acquire) || activity.wake.load(Ordering::Acquire)
                    {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });
        SolarSyncPump {
            stop,
            worker: Some(worker),
        }
    }

    /// Drain every project's pending Solar publications, each through its own
    /// session and its own store lease. One project's failure is recorded and
    /// the next project is still drained: a held publication in one project
    /// never stops another's.
    fn publish_pending(&self) -> Result<(), String> {
        let _gate = self
            .sync_gate
            .lock()
            .map_err(|_| "Solar Sync Center activity gate is unavailable")?;
        let mut first_error = None;
        for project in self.projects()? {
            let producer = SolarProducer {
                activity: self,
                project: project.clone(),
            };
            let publisher = SolarComputeArtifactsPublisher {
                activity: self,
                project: project.clone(),
            };
            let pass = self.sessions.session(&project).and_then(|session| {
                session.with_host_for_project(&project, &producer, &self.reads, |host| {
                    host.run_solar_publications(&publisher).map(|_| ())
                })
            });
            if let Err(error) = pass {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// One bounded sentence for the operator, whatever went wrong.
    ///
    /// The error itself is deliberately not carried: compute and store errors
    /// can include a path, a response or a credential-derived value, and this
    /// sentence is projected. There is no longer a second sentence for an
    /// over-long inventory, because a long inventory is no longer a failure —
    /// a projection is bounded per project and says so.
    fn note_publication_failure(&self, _error: &str) {
        if let Ok(mut failure) = self.publication_failure.lock() {
            *failure = Some(
                "Sync Center could not read or publish durable server work; it will retry".into(),
            );
        }
    }

    fn clear_publication_failure(&self) {
        if let Ok(mut failure) = self.publication_failure.lock() {
            *failure = None;
        }
    }
}

impl CompletionObserver for SolarActivity {
    fn completed(&self, job: &Job) -> Result<(), String> {
        if job.engine != EngineKind::SolarPrepared {
            return Ok(());
        }
        self.wake.store(true, Ordering::Release);
        Ok(())
    }

    fn recover(&self) -> Result<(), String> {
        self.wake.store(true, Ordering::Release);
        Ok(())
    }
}

/// One project's producer. It is constructed per pass, for exactly the
/// project whose session and store lease the pass holds.
struct SolarProducer<'a> {
    activity: &'a SolarActivity,
    project: String,
}

impl SolarProducer<'_> {
    /// This producer's own project, bounded. A producer is opened for one
    /// project and reads that project's rows; another project's queue is
    /// neither read nor able to fail this pass.
    fn solar_jobs(&self, project: &str) -> Result<Vec<Job>, String> {
        Ok(self.activity.solar_jobs_in(project)?.0)
    }
    fn rows(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        if project != self.project {
            return Err("the Solar producer was opened for another project".into());
        }
        let mut rows = Vec::new();
        let mut identities = BTreeSet::new();
        for job in self.solar_jobs(project)?.into_iter().filter(|job| {
            job.phase == ds_command_kernel::compute_jobs::Phase::Completed
                && job.engine == EngineKind::SolarPrepared
        }) {
            let publication = self.activity.publication_metadata(&job)?;
            if publication.project_id != project {
                continue;
            }
            let identity_key = format!("calculate-{}", publication.city_id);
            if !identities.insert(identity_key.clone()) {
                continue;
            }
            // ds-brain admits exactly this calculation payload. Other Solar
            // response files stay under the sealed compute result and cannot
            // become undeclared Sync Center artifacts.
            let report = publication
                .outputs
                .iter()
                .find(|output| {
                    output.output_id == "report_input.json"
                        && output.content_type == "application/json"
                })
                .ok_or("completed Solar result has no sealed report input JSON")?;
            let outputs = vec![LocalOutput {
                output_id: "report-input".into(),
                format: "json".into(),
                content_type: "application/json".into(),
                sha256: report.sha256.clone(),
                size_bytes: report.size_bytes,
            }];
            let mut pairs: Vec<(String, String)> = outputs
                .iter()
                .map(|output| (output.output_id.clone(), output.sha256.clone()))
                .collect();
            let size_bytes = outputs.iter().try_fold(0_u64, |total, output| {
                total
                    .checked_add(output.size_bytes)
                    .ok_or("Solar outputs overflow their size bound")
            })?;
            rows.push(LocalRow {
                identity: Identity {
                    engine: "solar".into(),
                    operation: identity_key,
                    variant: "default".into(),
                },
                sha256: inventory_digest(&mut pairs),
                size_bytes,
                produced_at_ms: job.updated_at_ms,
                // The shared StoreHost writes a published head from this
                // revision. It is the sealed authority fingerprint, never a
                // digest of prepared bytes or a locally chosen replacement.
                base_revision: publication
                    .provenance
                    .as_ref()
                    .map(|provenance| provenance.input_base_fingerprint.clone()),
                readable: true,
                engine_release: publication.engine_release.clone(),
                engine_build_manifest_sha256: publication.engine_build_manifest_sha256.clone(),
                grant_engine: "solar".into(),
                resource: publication.city_id.clone(),
                client_publish_id: job.id.clone(),
                outputs,
            });
        }
        Ok(rows)
    }
}

impl Producer for SolarProducer<'_> {
    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        self.rows(project)
    }

    fn locator(&self, row: &LocalRow) -> String {
        format!("solar:prepared:job:{}", row.client_publish_id)
    }

    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        let store = runtime::open(&self.activity.database)?;
        let job = store
            .job(&self.activity.caller(), &row.client_publish_id)
            .map_err(|error| error.to_string())?
            .ok_or("Solar publication is no longer durable")?;
        let publication = self.activity.publication(&job)?;
        if publication.project_id != self.project {
            return Err("Solar publication crosses the project fence of this pass".into());
        }
        if output_id != "report-input" {
            return Err("requested Solar output is not part of the closed publication".into());
        }
        let output = publication
            .outputs
            .into_iter()
            .find(|output| {
                output.output_id == "report_input.json" && output.content_type == "application/json"
            })
            .ok_or("requested Solar output is not part of the sealed publication")?;
        if output.size_bytes > 16 * 1024 * 1024 || runtime::digest(&output.bytes) != output.sha256 {
            return Err("Solar output bytes no longer match their sealed declaration".into());
        }
        let mut reader = Cursor::new(output.bytes);
        ds_sync_runtime::transfer_verified_output(
            output_id,
            session_uri,
            output.size_bytes,
            &output.sha256,
            &mut reader,
        )
    }

    fn activity(&self, project: &str) -> Result<Vec<ActivityRow>, String> {
        let store = runtime::open(&self.activity.database)?;
        let caller = self.activity.caller();
        let mut rows = self
            .solar_jobs(project)?
            .into_iter()
            .filter_map(|job| {
                let input = store.job_input(&caller, &job.id).ok()??;
                let scope = runtime::solar_job_scope(&job, &input).ok()?;
                (scope.project_id == project).then(|| ActivityRow {
                    id: job.id,
                    engine: "solar".into(),
                    state: match job.phase {
                        ds_command_kernel::compute_jobs::Phase::Queued => "queued",
                        ds_command_kernel::compute_jobs::Phase::Running => "running",
                        ds_command_kernel::compute_jobs::Phase::Completed => "completed",
                        ds_command_kernel::compute_jobs::Phase::Failed => "failed",
                        ds_command_kernel::compute_jobs::Phase::Cancelled => "cancelled",
                    }
                    .into(),
                    created_at_ms: job.created_at_ms,
                    updated_at_ms: job.updated_at_ms,
                    detail: job.error,
                })
            })
            .collect::<Vec<_>>();
        if let Ok(Some(detail)) = self
            .activity
            .publication_failure
            .lock()
            .map(|failure| failure.clone())
        {
            rows.push(ActivityRow {
                id: "solar-sync-pump".into(),
                engine: "solar".into(),
                state: "failed".into(),
                created_at_ms: 0,
                updated_at_ms: now_ms(),
                detail: Some(detail),
            });
        }
        Ok(rows)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |value| value.as_millis() as u64)
}

/// The only native adapter allowed to turn a sealed Solar result into a
/// compute-artifact publication. The store owns leases, retries and receipts;
/// this object only declares the fixed Solar shape and streams its one output.
struct SolarComputeArtifactsPublisher<'a> {
    activity: &'a SolarActivity,
    project: String,
}

impl SolarPublisher for SolarComputeArtifactsPublisher<'_> {
    fn publish_solar(
        &self,
        row: &LocalRow,
        guard: &dyn Fn() -> Result<(), String>,
        transfer: &dyn Fn(&str, &str) -> Result<TransferReceipt, String>,
    ) -> Result<SolarPublishOutcome, String> {
        let store = runtime::open(&self.activity.database)?;
        let job = store
            .job(&self.activity.caller(), &row.client_publish_id)
            .map_err(|error| error.to_string())?
            .ok_or("Solar publication is no longer durable")?;
        let publication = self.activity.publication(&job)?;
        let Some(provenance) = publication.provenance.as_ref() else {
            return Ok(SolarPublishOutcome::Blocked {
                detail: "Solar publication has no sealed snapshot provenance".into(),
            });
        };
        if publication.project_id != self.project
            || provenance.project_id != publication.project_id
            || provenance.template_id != publication.city_id
            || row.identity.operation != format!("calculate-{}", publication.city_id)
            || row.outputs.len() != 1
            || row.outputs[0].output_id != "report-input"
            || row.outputs[0].content_type != "application/json"
        {
            return Ok(SolarPublishOutcome::Blocked {
                detail: "Solar publication no longer matches its sealed project, city, or output"
                    .into(),
            });
        }
        let (_, version) = publication
            .engine_release
            .rsplit_once('@')
            .ok_or("Solar publication has no engine release version")?;
        let session = self
            .activity
            .sessions
            .session(&self.project)
            .map_err(|error| format!("this project has no Sync Center session: {error}"))?;
        match session.register_solar_engine(version, &publication.engine_release) {
            Ok(()) => {}
            Err(ds_cli_auth::sync::SolarPublicationError::Blocked(detail)) => {
                return Ok(SolarPublishOutcome::Blocked { detail });
            }
            Err(ds_cli_auth::sync::SolarPublicationError::StoredStale(detail)) => {
                return Ok(SolarPublishOutcome::StoredStale {
                    work_id: None,
                    detail,
                });
            }
            Err(ds_cli_auth::sync::SolarPublicationError::Retryable(detail)) => {
                return Err(detail);
            }
        }
        let receipt = session.publish_solar_calculation(
            ds_cli_auth::SolarCalculationArtifactOpen {
                project_id: &publication.project_id,
                client_run_id: &row.client_publish_id,
                city_id: &publication.city_id,
                engine_version: &publication.engine_release,
                engine_build_manifest_sha256: &publication.engine_build_manifest_sha256,
                input_base_fingerprint: &provenance.input_base_fingerprint,
                source_snapshot_sha256: &provenance.source_snapshot_sha256,
                snapshot_receipt_id: &provenance.snapshot_receipt_id,
                output_sha256: &row.outputs[0].sha256,
                output_size_bytes: row.outputs[0].size_bytes,
            },
            guard,
            |session_uri| transfer("report-input", session_uri),
        );
        match receipt {
            Ok(receipt) if receipt.state == "published" => Ok(SolarPublishOutcome::Published {
                work_id: receipt.work_id,
                head_revision: i64::try_from(receipt.head_revision)
                    .map_err(|_| "Solar artifact head revision exceeds native range")?,
            }),
            Ok(receipt) if receipt.state == "stored_stale" => {
                Ok(SolarPublishOutcome::StoredStale {
                    work_id: Some(receipt.work_id),
                    detail: "Solar snapshot changed before publication finalized".into(),
                })
            }
            Ok(_) => Ok(SolarPublishOutcome::Blocked {
                detail: "Solar compute artifact authority returned an invalid terminal state"
                    .into(),
            }),
            Err(ds_cli_auth::sync::SolarPublicationError::Blocked(detail)) => {
                Ok(SolarPublishOutcome::Blocked { detail })
            }
            Err(ds_cli_auth::sync::SolarPublicationError::StoredStale(detail)) => {
                Ok(SolarPublishOutcome::StoredStale {
                    work_id: None,
                    detail,
                })
            }
            Err(ds_cli_auth::sync::SolarPublicationError::Retryable(detail)) => Err(detail),
        }
    }
}

#[cfg(test)]
mod report_wake_tests {
    use super::*;

    fn inventory(value: &str) -> crate::server_reports::Inventory {
        crate::server_reports::Inventory {
            fingerprint: value.into(),
        }
    }

    #[test]
    fn event_wake_does_not_rescan_an_idle_report_inventory() {
        let wake = ReportWake {
            startup: false,
            fingerprint: Some("same".into()),
            retry_eligible: false,
            offline: false,
            reconnect_pending: false,
            wake_at_ms: None,
        };
        assert!(!wake.needs_observation(10, false));
        assert!(wake.needs_observation(10, true));
    }

    #[test]
    fn startup_is_once_then_idle_report_inventory_never_creates_a_gateway_trigger() {
        let mut wake = ReportWake::at_startup();
        assert_eq!(
            wake.trigger(&inventory("first"), 10, true),
            Some(ds_sync_runtime::Trigger::Startup)
        );
        wake.startup = false;
        wake.fingerprint = Some("first".into());
        assert_eq!(wake.trigger(&inventory("first"), 11, true), None);
    }

    #[test]
    fn sealed_inventory_change_and_retained_work_are_local_change_triggers() {
        let mut wake = ReportWake {
            startup: false,
            fingerprint: Some("old".into()),
            retry_eligible: false,
            offline: false,
            reconnect_pending: false,
            wake_at_ms: None,
        };
        assert_eq!(
            wake.trigger(&inventory("new"), 10, true),
            Some(ds_sync_runtime::Trigger::LocalChange)
        );
        wake.fingerprint = Some("new".into());
        wake.retry_eligible = true;
        assert_eq!(
            wake.trigger(&inventory("new"), 11, true),
            Some(ds_sync_runtime::Trigger::LocalChange)
        );
    }

    #[test]
    fn reconnect_requires_a_successful_post_offline_observation() {
        let mut wake = ReportWake {
            startup: false,
            fingerprint: Some("same".into()),
            retry_eligible: false,
            offline: true,
            reconnect_pending: false,
            wake_at_ms: None,
        };
        wake.applied(crate::server_reports::Pass {
            inventory: inventory("same"),
            retry_eligible: false,
            offline: false,
            wake_at_ms: None,
        });
        assert_eq!(
            wake.trigger(&inventory("same"), 10, false),
            Some(ds_sync_runtime::Trigger::Reconnect)
        );
    }

    #[test]
    fn offline_recovery_and_kernel_deadline_are_explicit_typed_triggers() {
        let wake = ReportWake {
            startup: false,
            fingerprint: Some("same".into()),
            retry_eligible: false,
            offline: true,
            reconnect_pending: false,
            wake_at_ms: None,
        };
        assert_eq!(
            wake.trigger(&inventory("same"), 10, true),
            Some(ds_sync_runtime::Trigger::LocalChange)
        );
        let wake = ReportWake {
            startup: false,
            fingerprint: Some("same".into()),
            retry_eligible: false,
            offline: false,
            reconnect_pending: false,
            wake_at_ms: Some(10),
        };
        assert_eq!(
            wake.trigger(&inventory("same"), 10, false),
            Some(ds_sync_runtime::Trigger::LocalChange)
        );
    }
}
