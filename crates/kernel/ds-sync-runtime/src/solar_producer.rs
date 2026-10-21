//! Native Solar producer and completion observer, shared by Server and Desktop.
//! Completed jobs seal the common queue under their captured identity without a
//! gateway session. Observations never adopt unsealed legacy Solar results.
use crate::{rows::now_ms, solar, ActivityRow, LocalRow, Producer, TransferReceipt};
use ds_command_kernel::compute_jobs::{EngineKind, Job};
use ds_compute_runtime::{
    self as runtime, CompletionObserver, HostIdentity, SolarPublication, SolarPublicationMetadata,
};
use std::{
    collections::BTreeSet,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

const PAGE: usize = 1000;
/// A bounded activity view. Jobs and inputs remain durable beyond this view.
const PROJECTION_PER_PROJECT: usize = 512;

pub fn fence_of(identity: &HostIdentity) -> crate::store::Fence {
    crate::store::Fence {
        account: identity.principal.uid.clone(),
        deployment: identity.principal.deployment.clone(),
        install_id: identity.principal.install_id.clone(),
    }
}

/// Projects owning durable Solar work under this captured connection.
pub fn projects(database: &Path, identity: &HostIdentity) -> Result<Vec<String>, String> {
    let store = runtime::open(database)?;
    projects_in_store(&store, identity, false)
}

/// Existing native Solar projects without opening or initializing a database.
pub fn projects_from_store(
    store: &ds_sync_store::Store,
    identity: &HostIdentity,
) -> Result<Vec<String>, String> {
    projects_in_store(store, identity, true)
}
fn projects_in_store(
    store: &ds_sync_store::Store,
    identity: &HostIdentity,
    install_fence: bool,
) -> Result<Vec<String>, String> {
    if !store.has_jobs().map_err(|error| error.to_string())? {
        return Ok(Vec::new());
    }
    let caller = identity.caller(None);
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
        for job in page.into_iter().filter(|job| {
            job.engine == EngineKind::SolarPrepared
                && (!install_fence || same_install(job, identity))
        }) {
            if let Some(context) = job.context {
                projects.insert(context.project);
            }
        }
    }
    Ok(projects.into_iter().collect())
}

/// The completion half of native Solar activity. Hosts provide only the captured
/// connection, durable database path and an event wake shared with their runner.
pub struct SolarActivity {
    database: PathBuf,
    identity: HostIdentity,
    wake: Arc<AtomicBool>,
}
impl SolarActivity {
    pub fn new(database: PathBuf, identity: HostIdentity, wake: Arc<AtomicBool>) -> Self {
        Self {
            database,
            identity,
            wake,
        }
    }
    /// Seal one completed prepared job's row into the sync store — the
    /// same acknowledged step as the completion, from the compute worker's
    /// own thread. The city's previous result on this machine is freed
    /// through `Store::retire_job_result` and told so with a receipt; a
    /// completion sealed BEHIND a later one of its city — a restart replays
    /// every completed job newest first — frees its own result the same
    /// way and the newer row stands. A result already reclaimed has nothing
    /// to seal. A seal the store could not record is returned as the error
    /// it is: the job is then not a publication — the next observation
    /// retires its result bytes, and the job can be run again.
    fn seal_completed(&self, job: &Job) -> Result<(), String> {
        let project = job
            .context
            .as_ref()
            .map(|context| context.project.as_str())
            .ok_or("completed Solar job carries no execution context to seal under")?;
        let Some(publication) = publication_metadata(&self.database, &self.identity, job)? else {
            return Ok(());
        };
        if publication.project_id != project {
            return Err("this job's sealed input names another project than its context".into());
        }
        let report = publication
            .outputs
            .iter()
            .find(|output| {
                output.output_id == "report_input.json" && output.content_type == "application/json"
            })
            .ok_or("completed Solar result has no sealed report input JSON")?;
        let request = solar::SealRequest {
            project_id: project,
            city_id: &publication.city_id,
            job_id: &job.id,
            completed_at_ms: job.updated_at_ms,
            engine_release: &publication.engine_release,
            engine_build_manifest_sha256: &publication.engine_build_manifest_sha256,
            input_base_fingerprint: publication
                .provenance
                .as_ref()
                .map(|provenance| provenance.input_base_fingerprint.as_str()),
            report_input_sha256: &report.sha256,
            report_input_size_bytes: report.size_bytes,
        };
        let store = crate::open_store(&self.database)?;
        let caller = self.identity.caller(Some(project));
        let free = |superseded: &str| -> Result<u64, String> {
            let mut store = store
                .lock()
                .map_err(|_| "The sync gate is unavailable".to_string())?;
            store
                .retire_job_result(&caller, superseded)
                .map_err(|error| error.to_string())
        };
        solar::seal(&store, &fence_of(&self.identity), &request, &free).map(|_| ())
    }
}
impl CompletionObserver for SolarActivity {
    fn completed(&self, job: &Job) -> Result<(), String> {
        if job.engine != EngineKind::SolarPrepared {
            return Ok(());
        }
        let sealed = self.seal_completed(job);
        self.wake.store(true, Ordering::Release);
        sealed
    }
    fn recover(&self) -> Result<(), String> {
        self.wake.store(true, Ordering::Release);
        Ok(())
    }
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
pub fn solar_jobs_in(
    database: &Path,
    identity: &HostIdentity,
    project: &str,
) -> Result<(Vec<Job>, bool), String> {
    let store = runtime::open(database)?;
    solar_jobs_in_store(&store, identity, project, false)
}

/// The same bounded activity query through an already opened read-only store.
pub fn solar_jobs_from_store(
    store: &ds_sync_store::Store,
    identity: &HostIdentity,
    project: &str,
) -> Result<(Vec<Job>, bool), String> {
    solar_jobs_in_store(store, identity, project, true)
}
fn solar_jobs_in_store(
    store: &ds_sync_store::Store,
    identity: &HostIdentity,
    project: &str,
    install_fence: bool,
) -> Result<(Vec<Job>, bool), String> {
    if !store.has_jobs().map_err(|error| error.to_string())? {
        return Ok((Vec::new(), false));
    }
    let caller = identity.caller(Some(project));
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
        for job in page.into_iter().filter(|job| {
            job.engine == EngineKind::SolarPrepared
                && (!install_fence || same_install(job, identity))
        }) {
            if jobs.len() == PROJECTION_PER_PROJECT {
                return Ok((jobs, true));
            }
            jobs.push(job);
        }
    }
}

pub fn publication(
    database: &Path,
    identity: &HostIdentity,
    job: &Job,
) -> Result<SolarPublication, String> {
    let store = runtime::open(database)?;
    let caller = identity.caller(None);
    let input = store
        .job_input(&caller, &job.id)
        .map_err(|error| error.to_string())?
        .ok_or("completed Solar job lost its durable prepared input")?;
    let result = store
        .job_result(&caller, &job.id)
        .map_err(|error| error.to_string())?
        .ok_or("completed Solar job lost its durable result")?;
    runtime::solar_publication(job, &input, &result)
}

/// The publication one completed job stands for, or `None` when its
/// result bytes have been reclaimed: the job row stays as evidence of
/// the computation, but there is nothing here to publish or to read.
pub fn publication_metadata(
    database: &Path,
    identity: &HostIdentity,
    job: &Job,
) -> Result<Option<SolarPublicationMetadata>, String> {
    let store = runtime::open(database)?;
    let caller = identity.caller(None);
    // The result first: a reclaimed result is one NULL read, and its
    // input is not parsed for nothing on every pass.
    let Some(result) = store
        .job_result(&caller, &job.id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let input = store
        .job_input(&caller, &job.id)
        .map_err(|error| error.to_string())?
        .ok_or("completed Solar job lost its durable prepared input")?;
    runtime::solar_publication_metadata(job, &input, &result).map(Some)
}

/// The Solar publish ids (jobs) the store holds rows for in one project
/// under this host's fence. What the producer's observation skips without
/// reading a byte.
fn sealed_jobs(
    database: &Path,
    identity: &HostIdentity,
    project: &str,
) -> Result<BTreeSet<String>, String> {
    let store = ds_sync_store::Store::open(database).map_err(|error| error.to_string())?;
    let snapshot = store
        .snapshot(&fence_of(identity), &crate::rows::store_scope(project))
        .map_err(|error| error.to_string())?;
    Ok(snapshot
        .artifacts
        .into_iter()
        .filter(|row| row.identity.engine == solar::ENGINE)
        .map(|row| row.replay_key)
        .collect())
}

/// One project's Solar producer over this host's compute table. It is
/// constructed per pass, for exactly the project whose store lease the
/// pass holds.
pub struct SolarProducer<'a> {
    pub database: &'a Path,
    pub identity: &'a HostIdentity,
    pub project: String,
    /// The pump's failure sentence, projected into the activity rows when
    /// the producer runs inside the pump; a producer opened by the CLI's
    /// manual drain has none.
    pub failure: Option<&'a Mutex<Option<String>>>,
}

impl SolarProducer<'_> {
    /// This producer's own project, bounded. A producer is opened for one
    /// project and reads that project's rows; another project's queue is
    /// neither read nor able to fail this pass.
    fn solar_jobs(&self, project: &str) -> Result<Vec<Job>, String> {
        Ok(solar_jobs_in(self.database, self.identity, project)?.0)
    }

    /// The observation: NOTHING is offered. The seal at completion is the
    /// only way a Solar row is born; a completed job the store holds no
    /// row for — completed before the seal existed, or whose seal the store
    /// could not record — is not a publication, and its result bytes are
    /// retired here (`Store::retire_job_result`; the job row stays as
    /// evidence, and the job can be run again). A result already gone costs
    /// one NULL read. Answers the jobs retired and their bytes.
    pub fn retire_unsealed(&self, project: &str) -> Result<Vec<(String, u64)>, String> {
        if project != self.project {
            return Err("the Solar producer was opened for another project".into());
        }
        let sealed = sealed_jobs(self.database, self.identity, project)?;
        let unsealed: Vec<String> = self
            .solar_jobs(project)?
            .into_iter()
            .filter(|job| {
                job.phase == ds_command_kernel::compute_jobs::Phase::Completed
                    && job.engine == EngineKind::SolarPrepared
                    && !sealed.contains(&job.id)
            })
            .map(|job| job.id)
            .collect();
        let mut retired = Vec::new();
        if unsealed.is_empty() {
            return Ok(retired);
        }
        let mut store = runtime::open(self.database)?;
        let caller = self.identity.caller(Some(project));
        for job_id in unsealed {
            let freed = store
                .retire_job_result(&caller, &job_id)
                .map_err(|error| error.to_string())?;
            if freed > 0 {
                eprintln!(
                    "solar: job {job_id} completed with no row in the sync store (no completion sealed it); its {freed}-byte result was retired — run it again to publish it"
                );
                retired.push((job_id, freed));
            }
        }
        Ok(retired)
    }
}

impl Producer for SolarProducer<'_> {
    fn amend_open_declaration(
        &self,
        project: &str,
        declaration: &mut serde_json::Value,
    ) -> Result<(), String> {
        if project != self.project {
            return Err("Solar declaration crosses its producer project".into());
        }
        let job_id = declaration["client_run_id"]
            .as_str()
            .ok_or("publication has no replay key")?;
        let shared = crate::open_store(self.database)?;
        let fence = fence_of(self.identity);
        if shared
            .lock()
            .map_err(|_| "The sync gate is unavailable")?
            .publication_payload(&fence, project, job_id)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return crate::retained_publication::RetainedProducer {
                store: &shared,
                fence: &fence,
                project,
            }
            .amend_open_declaration(project, declaration);
        }
        let store = runtime::open(self.database)?;
        let job = store
            .job(&self.identity.caller(Some(project)), job_id)
            .map_err(|error| error.to_string())?
            .ok_or("Solar publication is no longer durable")?;
        let publication = publication(self.database, self.identity, &job)?;
        let provenance = publication
            .provenance
            .as_ref()
            .ok_or("Solar publication has no sealed snapshot provenance")?;
        if publication.project_id != project
            || provenance.project_id != project
            || provenance.template_id != publication.city_id
            || declaration["operation"] != format!("calculate-{}", publication.city_id)
            || declaration["engine_version"] != publication.engine_release
            || declaration["engine_build_manifest_sha256"]
                != publication.engine_build_manifest_sha256
            || declaration["input_base_fingerprint"] != provenance.input_base_fingerprint
        {
            return Err("Solar declaration differs from its sealed provenance".into());
        }
        let output = publication
            .outputs
            .iter()
            .find(|output| output.output_id == "report_input.json")
            .ok_or("Solar result has no sealed report input")?;
        let outputs = declaration["outputs"]
            .as_array()
            .ok_or("Solar declaration has no inventory")?;
        if outputs.len() != 1
            || outputs[0]["output_id"] != solar::OUTPUT_ID
            || outputs[0]["sha256"] != output.sha256
            || outputs[0]["size_bytes"] != output.size_bytes
            || outputs[0]["content_type"] != output.content_type
        {
            return Err("Solar declaration differs from its sealed output".into());
        }
        declaration["source_snapshot_sha256"] = provenance.source_snapshot_sha256.clone().into();
        declaration["snapshot_receipt_id"] = provenance.snapshot_receipt_id.clone().into();
        ds_command_kernel::sync::declaration::validate(project, declaration).map(|_| ())
    }

    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        self.retire_unsealed(project)?;
        Ok(Vec::new())
    }

    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        if crate::retained_publication::owns(row) {
            let shared = crate::open_store(self.database)?;
            let fence = fence_of(self.identity);
            return crate::retained_publication::RetainedProducer {
                store: &shared,
                fence: &fence,
                project: &self.project,
            }
            .transfer(row, output_id, session_uri);
        }
        let store = runtime::open(self.database)?;
        let job = store
            .job(&self.identity.caller(None), &row.client_publish_id)
            .map_err(|error| error.to_string())?
            .ok_or("Solar publication is no longer durable")?;
        let publication = publication(self.database, self.identity, &job)?;
        if publication.project_id != self.project {
            return Err("Solar publication crosses the project fence of this pass".into());
        }
        if output_id != solar::OUTPUT_ID {
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
        crate::transfer_verified_output(
            output_id,
            session_uri,
            output.size_bytes,
            &output.sha256,
            &mut reader,
        )
    }

    /// Free the bytes of a Solar publication that lost. Solar's bytes are
    /// the completed job's `result` column in this host's own SQLite — the
    /// sealed response with its `report_input.json` body — so the reclaim
    /// is `Store::retire_job_result`: that column becomes NULL, and the job
    /// row (phase, `result_sha256`, context) stays as evidence, exactly as a
    /// report batch's directory leaves and its store row stays. Fenced by
    /// this pass's project; a result already gone answers 0. The same
    /// pattern on both sides — no `Ok(0)` because the bytes live elsewhere.
    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
        if project != self.project {
            return Err("the Solar producer was opened for another project".into());
        }
        if crate::retained_publication::owns(row) {
            let shared = crate::open_store(self.database)?;
            let fence = fence_of(self.identity);
            return crate::retained_publication::RetainedProducer {
                store: &shared,
                fence: &fence,
                project,
            }
            .reclaim(project, row);
        }
        if row.client_publish_id.is_empty() {
            // A replica of the head, not a publication of this producer.
            return Ok(0);
        }
        let mut store = runtime::open(self.database)?;
        let caller = self.identity.caller(Some(project));
        store
            .retire_job_result(&caller, &row.client_publish_id)
            .map_err(|error| error.to_string())
    }

    fn activity(&self, project: &str) -> Result<Vec<ActivityRow>, String> {
        let store = runtime::open(self.database)?;
        let mut rows = activity_from_store(&store, self.identity, project)?;
        if let Some(detail) = self
            .failure
            .and_then(|failure| failure.lock().ok().and_then(|held| held.clone()))
        {
            rows.push(ActivityRow {
                id: "solar-sync-pump".into(),
                engine: solar::ENGINE.into(),
                state: "failed".into(),
                created_at_ms: 0,
                updated_at_ms: now_ms(),
                detail: Some(detail),
            });
        }
        Ok(rows)
    }
}

fn same_install(job: &Job, identity: &HostIdentity) -> bool {
    job.context
        .as_ref()
        .is_some_and(|context| context.install_id == identity.principal.install_id)
}

/// Project the native retained Solar jobs with the producer's existing mapping.
pub fn activity_from_store(
    store: &ds_sync_store::Store,
    identity: &HostIdentity,
    project: &str,
) -> Result<Vec<ActivityRow>, String> {
    let caller = identity.caller(None);
    let rows = solar_jobs_from_store(store, identity, project)?
        .0
        .into_iter()
        .map(|job| {
            let input = store
                .job_input(&caller, &job.id)
                .map_err(|error| error.to_string())?
                .ok_or("retained Solar activity lost its durable input")?;
            let scope = runtime::solar_job_scope(&job, &input)?;
            if scope.project_id != project {
                return Err("retained Solar activity crosses its project fence".into());
            }
            Ok(ActivityRow {
                id: job.id,
                engine: solar::ENGINE.into(),
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
        .collect::<Result<Vec<_>, String>>()?;
    Ok(rows)
}
