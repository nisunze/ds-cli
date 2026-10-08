//! Solar's producer adapter for the shared Sync Center store.
//!
//! A completed prepared job SEALS its row into the store at completion
//! (`ds_sync_runtime::solar::seal`), under this host's local fence, with no
//! session and no gateway: the store is the queue, and a Solar completion is
//! queued the moment it is durable. That seal is the ONLY way a Solar row is
//! born (owner, 2026-09-20: "the legacy jobs in solar are not good — we
//! should remove them"): a completed job the store holds no row for — one
//! completed before the seal existed, or one whose seal the store could not
//! record — is simply not a publication. The producer's inventory offers
//! nothing; it retires such a job's result bytes on the next observation
//! (`Store::retire_job_result`, the job row stays as evidence) and the job
//! can be run again. Bytes are the job's `result` column; there is no
//! browser cache, temporary request file or second queue.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use ds_command_kernel::compute_jobs::{EngineKind, Job};
use ds_compute_runtime::{self as runtime, CompletionObserver, HostIdentity};
#[cfg(test)]
use ds_sync_runtime::LocalRow;
#[cfg(test)]
use ds_sync_runtime::Producer;
use ds_sync_runtime::{Producers, VerifiedReads, solar};

use crate::{auth, server_sync::sessions::ServerSessions};
use ds_sync_runtime::rows::now_ms;
#[cfg(test)]
use ds_sync_runtime::solar_producer::SolarProducer;
use ds_sync_runtime::solar_producer::solar_jobs_in;
use serde_json::Value;

/// What the Solar producer retired (job, bytes freed) and what it offered.
#[cfg(test)]
type SolarObservation = (Vec<(String, u64)>, Vec<LocalRow>);

pub struct SolarActivity {
    database: PathBuf,
    /// One session per authorized project, opened on first use. A Solar
    /// publication for one project and a report drain for another are two
    /// sessions on one host, not one session that switches.
    sessions: Arc<ServerSessions>,
    /// Shared Sync Center reader. It owns the object-ticket origin pin,
    /// redirect refusal, staging, and digest proof; this host only anchors its
    /// private cache below the protected server state directory.
    reads: VerifiedReads,
    wake: Arc<AtomicBool>,
    completion: ds_sync_runtime::solar_producer::SolarActivity,
    publication_failure: Mutex<Option<String>>,
}

/// One bounded background publisher for a server instance. It owns no durable
/// queue: wake-ups and periodic recovery only drain rows already held by the
/// shared Sync Center store.
pub struct SolarSyncPump {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

/// The shared runtime owns publication retry truth. This host only remembers the
/// last sealed producer observation and translates the runtime's typed wake
/// decision into the existing background thread's next trigger.
#[derive(Default)]
struct SyncWake {
    startup: bool,
    fingerprint: Option<String>,
    retry_eligible: bool,
    offline: bool,
    reconnect_pending: bool,
    wake_at_ms: Option<u64>,
}

impl SyncWake {
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
        let wake = Arc::new(AtomicBool::new(false));
        let completion = ds_sync_runtime::solar_producer::SolarActivity::new(
            database.clone(),
            sessions.identity().clone(),
            wake.clone(),
        );
        Ok(Arc::new(Self {
            sessions,
            reads: VerifiedReads::new(state_directory.join("sync-downloads")),
            database,
            wake,
            completion,
            publication_failure: Mutex::new(None),
        }))
    }

    /// The immutable common queue seal is durable before this wake is sent.
    pub fn local_publication_completed(&self) {
        self.wake.store(true, Ordering::Release);
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
        let mut projects: BTreeSet<String> =
            ds_sync_runtime::solar_producer::projects(&self.database, self.sessions.identity())?
                .into_iter()
                .collect();
        projects.extend(crate::server_reports::projects_with_publications(
            &self.database,
            &crate::server_sync::fence_of(self.sessions.identity()),
        )?);
        Ok(projects.into_iter().collect())
    }

    /// One project's newest durable Solar rows, and whether it holds more
    /// than the projection covers.
    fn solar_jobs_in(&self, project: &str) -> Result<(Vec<Job>, bool), String> {
        solar_jobs_in(&self.database, self.sessions.identity(), project)
    }

    /// Whether one project holds more durable Solar work than a projection
    /// covers. A LOCAL fact about the rows on disk, so `/v1/activity` can
    /// report it whether or not that project's Sync Center could be read.
    pub fn projection_truncated(&self, project: &str) -> Result<bool, String> {
        Ok(self.solar_jobs_in(project)?.1)
    }

    /// This host's ONE producer set for a project — report + Solar, keyed by
    /// engine — with the pump's failure sentence projected into the Solar
    /// activity rows.
    fn with_producers<T>(
        &self,
        project: &str,
        f: impl FnOnce(&Producers<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        with_producers(
            &self.database,
            self.sessions.identity(),
            project,
            Some(&self.publication_failure),
            f,
        )
    }

    /// One project's Sync Center projection. The caller names the project;
    /// this host never answers for "the" project.
    pub fn store_read(&self, project: &str) -> Result<Value, String> {
        self.with_producers(project, |producers| {
            self.sessions.session(project)?.with_host_for_project(
                project,
                producers,
                &self.reads,
                |host| host.store_read(),
            )
        })
    }

    /// The Solar producer's observation of one project, for the host's own
    /// proof: what it retired (jobs completed with no row, with the bytes
    /// freed) and what it offered (always nothing).
    #[cfg(test)]
    pub(crate) fn observe_for_test(&self, project: &str) -> Result<SolarObservation, String> {
        let producer = SolarProducer {
            database: &self.database,
            identity: self.sessions.identity(),
            project: project.to_owned(),
            failure: None,
        };
        let retired = producer.retire_unsealed(project)?;
        let offered = producer.inventory(project)?;
        Ok((retired, offered))
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
        self.with_producers(&scope.project_id, |producers| {
            self.sessions
                .session(&scope.project_id)?
                .with_host_for_project(&scope.project_id, producers, &self.reads, |host| {
                    // The completion sealed its row, and that row is the
                    // publication that is cancelled; a job with no row is not
                    // a publication and has nothing to cancel. No remote work
                    // is opened or uploaded here.
                    ds_sync_runtime::SyncHost::local(host, &scope.project_id)?;
                    let row = host
                        .snapshot()?
                        .artifacts
                        .into_iter()
                        .find(|row| {
                            row.identity.engine == solar::ENGINE && row.replay_key == job.id
                        })
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
        })
    }

    /// Start the non-blocking publication pump after the HTTP server can
    /// accept requests. A completion seals its row and sets `wake`; it never
    /// uploads from a compute worker or delays durable replay at startup.
    pub fn start_pump(self: &Arc<Self>) -> SolarSyncPump {
        let activity = self.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let mut last_sync_recovery = Instant::now() - Duration::from_secs(30);
            // One publication scheduler per project. A project that is offline or
            // holding a retry keeps its own deadline; it never sets another
            // project's, and it never drains another project's queue.
            let mut projects: std::collections::BTreeMap<String, SyncWake> =
                std::collections::BTreeMap::new();
            let mut reconnect_generation = auth::gateway_reconnect_generation();
            while !worker_stop.load(Ordering::Acquire) {
                let observed_generation = auth::gateway_reconnect_generation();
                let generation_changed = observed_generation != reconnect_generation;
                reconnect_generation = observed_generation;
                let network_returned = generation_changed && auth::gateway_reachable();
                let woken = activity.wake.swap(false, Ordering::AcqRel) || network_returned;
                let sync_recovery_due = last_sync_recovery.elapsed() >= Duration::from_secs(30);
                let now = now_ms();

                let scopes = match activity.projects() {
                    Ok(scopes) => scopes,
                    Err(error) => {
                        activity.note_publication_failure(&error);
                        Vec::new()
                    }
                };
                let mut observed = false;
                for project in scopes {
                    let wake = projects
                        .entry(project.clone())
                        .or_insert_with(SyncWake::at_startup);
                    if network_returned {
                        wake.reconnect_pending = true;
                    }
                    if !woken && !wake.needs_observation(now, sync_recovery_due) {
                        continue;
                    }
                    observed = true;
                    let pass = activity.sessions.session(&project).and_then(|session| {
                        let inventory = crate::server_reports::inventory(&session)?;
                        let Some(trigger) =
                            wake.trigger(&inventory, now, sync_recovery_due || woken)
                        else {
                            return Ok(None);
                        };
                        activity
                            .with_producers(&project, |producers| {
                                crate::server_reports::drain(
                                    &session,
                                    producers,
                                    &activity.reads,
                                    trigger,
                                )
                            })
                            .map(Some)
                    });
                    match pass {
                        Ok(Some(pass)) => {
                            activity.clear_publication_failure();
                            wake.applied(pass);
                        }
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
                    last_sync_recovery = Instant::now();
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
    /// The seal, then the wake: the row is in the queue before the pump is
    /// asked to drain it. A seal the store refused still wakes the pump —
    /// its observation retires the unsealed result — and is reported.
    fn completed(&self, job: &Job) -> Result<(), String> {
        self.completion.completed(job)
    }

    fn recover(&self) -> Result<(), String> {
        self.completion.recover()
    }
}

/// This Server's paths into the one core producer set
/// (`ds_sync_runtime::with_producers`) that its Desktop runs too: report +
/// Solar, keyed by engine, so every host opened over the store (the report
/// drain, the Solar pump, a Sync Center read, a cancel, `ds report outbox
/// drain`) adopts, moves and frees a row through the producer that owns its
/// bytes, and a lost Solar row is never "freed" with 0 by the report producer.
pub fn with_producers<T>(
    database: &Path,
    identity: &HostIdentity,
    project: &str,
    failure: Option<&Mutex<Option<String>>>,
    f: impl FnOnce(&Producers<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let root = crate::server_reports::root(database)?;
    ds_sync_runtime::with_producers(database, &root, identity, project, failure, f)
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
        let wake = SyncWake {
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
        let mut wake = SyncWake::at_startup();
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
        let mut wake = SyncWake {
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
        let mut wake = SyncWake {
            startup: false,
            fingerprint: Some("same".into()),
            retry_eligible: false,
            offline: true,
            reconnect_pending: false,
            wake_at_ms: None,
        };
        wake.applied(crate::server_reports::Pass {
            inventory: inventory("same"),
            refreshed_remote: true,
            retry_eligible: false,
            offline: false,
            wake_at_ms: None,
            reclaimed: ds_sync_runtime::Reclaimed::default(),
            idle: None,
            summary: ds_sync_runtime::kernel_sync::Summary::default(),
            receipts: Vec::new(),
        });
        assert_eq!(
            wake.trigger(&inventory("same"), 10, false),
            Some(ds_sync_runtime::Trigger::Reconnect)
        );
    }

    #[test]
    fn a_proven_gateway_return_triggers_reconnect_without_waiting_for_recovery() {
        let wake = SyncWake {
            startup: false,
            fingerprint: Some("same".into()),
            retry_eligible: true,
            offline: true,
            reconnect_pending: true,
            wake_at_ms: None,
        };
        assert!(wake.needs_observation(10, false));
        assert_eq!(
            wake.trigger(&inventory("same"), 10, false),
            Some(ds_sync_runtime::Trigger::Reconnect)
        );
    }

    #[test]
    fn offline_recovery_and_kernel_deadline_are_explicit_typed_triggers() {
        let wake = SyncWake {
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
        let wake = SyncWake {
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
