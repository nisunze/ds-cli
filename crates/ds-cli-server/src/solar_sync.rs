//! Solar's producer adapter for the shared Sync Center store.
//!
//! It reconstructs rows and bytes from the durable compute record on every
//! call.  There is no browser cache, temporary request file or second queue.

use std::{
    collections::BTreeSet,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ds_command_kernel::{
    compute_jobs::{EngineKind, Job},
    sync::{Identity, Trigger},
};
use ds_compute_runtime::{
    self as runtime, CompletionObserver, SolarPublication, SolarPublicationMetadata,
};
use ds_sync_runtime::{
    inventory_digest, run, ActivityRow, LocalOutput, LocalRow, Producer, Reads, TransferReceipt,
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
}

impl SolarActivity {
    pub fn open(database: PathBuf, connection: Connection) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            session: Arc::new(ServerSyncSession::open(&database, &connection)?),
            database,
            connection,
            sync_gate: Mutex::new(()),
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
}

impl CompletionObserver for SolarActivity {
    fn completed(&self, job: &Job) -> Result<(), String> {
        if job.engine != EngineKind::SolarPrepared {
            return Ok(());
        }
        let publication = self.publication(job)?;
        let _gate = self
            .sync_gate
            .lock()
            .map_err(|_| "Solar Sync Center activity gate is unavailable")?;
        let (_, version) = publication
            .engine_release
            .rsplit_once('@')
            .ok_or("Solar publication has no engine release version")?;
        self.session
            .register_solar_engine(version, &publication.engine_release)?;
        let producer = SolarProducer { activity: self };
        let reads = ServerReads;
        self.session
            .with_host_for_project(&publication.project_id, &producer, &reads, |host| {
                run(host, &publication.project_id, Trigger::LocalChange).map(|_| ())
            })
    }

    fn recover(&self) -> Result<(), String> {
        let producer = SolarProducer { activity: self };
        for job in producer
            .solar_jobs()?
            .into_iter()
            .filter(|job| job.phase == ds_command_kernel::compute_jobs::Phase::Completed)
        {
            self.completed(&job)?;
        }
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
            let outputs: Vec<LocalOutput> = publication
                .outputs
                .iter()
                .map(|output| LocalOutput {
                    output_id: output.output_id.clone(),
                    format: output
                        .output_id
                        .rsplit_once('.')
                        .map_or("bin", |(_, extension)| extension)
                        .to_owned(),
                    content_type: output.content_type.clone(),
                    sha256: output.sha256.clone(),
                    size_bytes: output.size_bytes,
                })
                .collect();
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
                base_revision: None,
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
        let output = publication
            .outputs
            .into_iter()
            .find(|output| output.output_id == output_id)
            .ok_or("requested Solar output is not part of the sealed publication")?;
        if output.size_bytes > 16 * 1024 * 1024 || runtime::digest(&output.bytes) != output.sha256 {
            return Err("Solar output bytes no longer match their sealed declaration".into());
        }
        let mut reader = Cursor::new(output.bytes);
        ds_cli_auth::transfer_sync_output(output_id, session_uri, output.size_bytes, &mut reader)
    }

    fn activity(&self, project: &str) -> Result<Vec<ActivityRow>, String> {
        let store = runtime::open(&self.activity.database)?;
        Ok(self
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
            .collect::<Vec<_>>())
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
