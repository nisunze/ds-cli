//! Solar's producer adapter for the shared Sync Center store.
//!
//! It reconstructs rows and bytes from the durable compute record on every
//! call.  There is no browser cache, temporary request file or second queue.

use std::{
    collections::BTreeSet,
    io::Cursor,
    path::{Path, PathBuf},
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
    ActivityRow, LocalOutput, LocalRow, Producer, Reads, SolarPublishOutcome, SolarPublisher,
    TransferReceipt, inventory_digest,
};

use crate::{host::Connection, server_sync::ServerSyncSession};
use serde_json::Value;

pub struct SolarActivity {
    database: PathBuf,
    connection: Connection,
    session: Arc<ServerSyncSession>,
    /// Native engine registration and its following gateway pass are one
    /// release-attributed action, even when compute workers finish together.
    sync_gate: Mutex<()>,
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
    pub fn open(database: PathBuf, connection: Connection) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            session: Arc::new(ServerSyncSession::open(&database, &connection)?),
            database,
            connection,
            sync_gate: Mutex::new(()),
            wake: AtomicBool::new(false),
            publication_failure: Mutex::new(None),
        }))
    }

    fn publication(&self, job: &Job) -> Result<SolarPublication, String> {
        let store = runtime::open(&self.database)?;
        let input = store
            .job_input(&self.connection.owner, &self.connection.lane, &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable prepared input")?;
        let result = store
            .job_result(&self.connection.owner, &self.connection.lane, &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable result")?;
        runtime::solar_publication(job, &input, &result)
    }

    fn publication_metadata(&self, job: &Job) -> Result<SolarPublicationMetadata, String> {
        let store = runtime::open(&self.database)?;
        let input = store
            .job_input(&self.connection.owner, &self.connection.lane, &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable prepared input")?;
        let result = store
            .job_result(&self.connection.owner, &self.connection.lane, &job.id)
            .map_err(|error| error.to_string())?
            .ok_or("completed Solar job lost its durable result")?;
        runtime::solar_publication_metadata(job, &input, &result)
    }

    pub fn store_read(&self) -> Result<Value, String> {
        let producer = SolarProducer { activity: self };
        let reads = ServerReads;
        self.session
            .with_host_for_project(self.session.project(), &producer, &reads, |host| {
                host.store_read()
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
            while !worker_stop.load(Ordering::Acquire) {
                let woken = activity.wake.swap(false, Ordering::AcqRel);
                if woken || last_recovery.elapsed() >= Duration::from_secs(30) {
                    // The receipt records the actionable state. Keep stderr
                    // token-free and bounded if offline authority work fails.
                    match activity.publish_pending() {
                        Ok(()) => activity.clear_publication_failure(),
                        Err(error) => activity.note_publication_failure(&error),
                    }
                    last_recovery = Instant::now();
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

    fn publish_pending(&self) -> Result<(), String> {
        let _gate = self
            .sync_gate
            .lock()
            .map_err(|_| "Solar Sync Center activity gate is unavailable")?;
        let producer = SolarProducer { activity: self };
        let reads = ServerReads;
        let publisher = SolarComputeArtifactsPublisher { activity: self };
        let project = self.session.project().to_owned();
        self.session
            .with_host_for_project(&project, &producer, &reads, |host| {
                host.run_solar_publications(&publisher).map(|_| ())
            })
    }

    fn note_publication_failure(&self, error: &str) {
        // Compute/store errors can include implementation detail. Keep the
        // activity projection bounded and operator-actionable without copying
        // a path, response, or credential-derived value into it.
        let detail = if error.contains("exceeds 4096 durable jobs") {
            "Solar Sync Center inventory exceeds its 4096 completed-job bound".into()
        } else {
            "Solar Sync Center could not read or publish durable Solar work; it will retry".into()
        };
        if let Ok(mut failure) = self.publication_failure.lock() {
            *failure = Some(detail);
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

struct SolarProducer<'a> {
    activity: &'a SolarActivity,
}

impl SolarProducer<'_> {
    fn solar_jobs(&self) -> Result<Vec<Job>, String> {
        let store = runtime::open(&self.activity.database)?;
        let mut cursor: Option<(u64, String)> = None;
        let mut jobs = Vec::new();
        loop {
            let page = store
                .jobs_page(
                    &self.activity.connection.owner,
                    &self.activity.connection.lane,
                    cursor.as_ref().map(|(created, id)| (*created, id.as_str())),
                    1000,
                )
                .map_err(|error| error.to_string())?;
            let Some(last) = page.last() else { break };
            cursor = Some((last.created_at_ms, last.id.clone()));
            jobs.extend(
                page.into_iter()
                    .filter(|job| job.engine == EngineKind::SolarPrepared),
            );
            if jobs.len() > 4096 {
                return Err("Solar Sync Center inventory exceeds 4096 durable jobs".into());
            }
        }
        Ok(jobs)
    }
    fn rows(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        let mut rows = Vec::new();
        let mut identities = BTreeSet::new();
        for job in self.solar_jobs()?.into_iter().filter(|job| {
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
            .job(
                &self.activity.connection.owner,
                &self.activity.connection.lane,
                &row.client_publish_id,
            )
            .map_err(|error| error.to_string())?
            .ok_or("Solar publication is no longer durable")?;
        let publication = self.activity.publication(&job)?;
        if publication.project_id != self.activity.session.project() {
            return Err("Solar publication crosses the authenticated project fence".into());
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
        ds_cli_auth::transfer_sync_output(output_id, session_uri, output.size_bytes, &mut reader)
    }

    fn activity(&self, project: &str) -> Result<Vec<ActivityRow>, String> {
        let store = runtime::open(&self.activity.database)?;
        let mut rows = self
            .solar_jobs()?
            .into_iter()
            .filter_map(|job| {
                let input = store
                    .job_input(
                        &self.activity.connection.owner,
                        &self.activity.connection.lane,
                        &job.id,
                    )
                    .ok()??;
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
            .job(
                &self.activity.connection.owner,
                &self.activity.connection.lane,
                &row.client_publish_id,
            )
            .map_err(|error| error.to_string())?
            .ok_or("Solar publication is no longer durable")?;
        let publication = self.activity.publication(&job)?;
        let Some(provenance) = publication.provenance.as_ref() else {
            return Ok(SolarPublishOutcome::Blocked {
                detail: "Solar publication has no sealed snapshot provenance".into(),
            });
        };
        if publication.project_id != self.activity.session.project()
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
        match self
            .activity
            .session
            .register_solar_engine(version, &publication.engine_release)
        {
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
        let receipt = self.activity.session.publish_solar_calculation(
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

/// Solar's server pass only publishes its own completed rows. A remote head
/// is not silently downloaded into a compute job; such a request must use a
/// dedicated native read owner.
struct ServerReads;

impl Reads for ServerReads {
    fn head_root(&self, _project: &str, _identity: &Identity) -> Result<PathBuf, String> {
        Err("the Solar server publication host does not materialize remote heads".into())
    }

    fn download_verified(
        &self,
        _url: &str,
        _destination: &Path,
        _sha256: &str,
        _size_bytes: u64,
    ) -> Result<(), String> {
        Err("the Solar server publication host does not download remote heads".into())
    }
}
