//! Portable native compute host shared by headless Server and Desktop.
//! Domain lifecycle/admission belongs to the kernel; domain engines run only
//! after their complete, durable input has been admitted.
mod claim;
pub use claim::decode_solar_server_submission;

use base64::Engine;
use ds_command_kernel::compute_jobs::{self, EngineKind, Event, Job, Phase};
use ds_network::network::native_fast_lv::{
    decode_native_fast_lv_request, encode_native_fast_lv_result, process_native_fast_lv,
};
use ds_solar_contracts::{PreparedSolarCityInput, SolarCalculationResult};
use ds_solar_io::output::{encode_outputs, OutputFile};
use ds_solar_runtime::{CalculationOptions, CalculationTimings, CancelToken, SolarRuntime};
use ds_sync_store::Store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn open(path: &Path) -> Result<Store, String> {
    let store = Store::open(path).map_err(|e| e.to_string())?;
    store.initialize_jobs().map_err(|e| e.to_string())?;
    Ok(store)
}

pub fn submit(
    path: &Path,
    owner: &str,
    lane: &str,
    key: &str,
    input: &[u8],
) -> Result<Job, String> {
    decode_native_fast_lv_request(input)
        .map_err(|error| format!("fast_lv_request_invalid: {error}"))?;
    submit_validated(
        path,
        owner,
        lane,
        key,
        input,
        EngineKind::FastLv,
        "ds.fast-lv.request/v1",
    )
}

/// Admit the existing hosted Solar calculation request as a durable native
/// job. The prepared input embeds every weather/reference byte the runtime
/// needs, so restart recovery never depends on a browser cache, a client path
/// or a temporary request file.
pub fn submit_solar(
    path: &Path,
    owner: &str,
    lane: &str,
    key: &str,
    input: &[u8],
) -> Result<Job, String> {
    if input.len() > 64 * 1024 * 1024 {
        return Err("solar_prepared_request_too_large".into());
    }
    let request: PreparedSolarRequest = serde_json::from_slice(input)
        .map_err(|error| format!("solar_prepared_request_invalid: {error}"))?;
    validate_solar_request(&request)?;
    submit_validated(
        path,
        owner,
        lane,
        key,
        input,
        EngineKind::SolarPrepared,
        "ds.solar.calculate.prepared/v1",
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolarSnapshotProvenance {
    pub project_id: String,
    pub root: String,
    pub template_id: String,
    pub input_base_fingerprint: String,
    pub source_snapshot_sha256: String,
    pub snapshot_receipt_id: String,
}

pub fn submit_solar_with_provenance(
    path: &Path,
    owner: &str,
    lane: &str,
    key: &str,
    input: &[u8],
    provenance: SolarSnapshotProvenance,
) -> Result<Job, String> {
    if input.len() > 64 * 1024 * 1024 {
        return Err("solar_prepared_request_too_large".into());
    }
    let mut request: PreparedSolarRequest = serde_json::from_slice(input)
        .map_err(|error| format!("solar_prepared_request_invalid: {error}"))?;
    validate_solar_request(&request)?;
    validate_provenance(&provenance, &request.prepared)?;
    request.provenance = Some(provenance);
    let sealed = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    submit_validated(
        path,
        owner,
        lane,
        key,
        &sealed,
        EngineKind::SolarPrepared,
        "ds.solar.calculate.prepared/v1",
    )
}

fn submit_validated(
    path: &Path,
    owner: &str,
    lane: &str,
    key: &str,
    input: &[u8],
    engine: EngineKind,
    input_tag: &str,
) -> Result<Job, String> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("job_key_invalid: use 1..128 letters, digits, _ or -".into());
    }
    let id = digest(&serde_json::to_vec(&(owner, lane, key)).map_err(|e| e.to_string())?);
    let now = now_ms();
    let job = Job {
        id,
        owner: owner.into(),
        lane: lane.into(),
        input_sha256: digest(input),
        engine,
        input_tag: input_tag.to_owned(),
        phase: Phase::Queued,
        attempts: 0,
        created_at_ms: now,
        updated_at_ms: now,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
    };
    open(path)?
        .submit_job(&job, input)
        .map_err(|e| e.to_string())
}

fn valid_run_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

/// The established Cloud Run calculation request shape. It is deliberately
/// path-free: server recovery replays the sealed prepared input itself.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedSolarRequest {
    prepared: PreparedSolarCityInput,
    #[serde(default = "default_render_charts")]
    render_charts: bool,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    provenance: Option<SolarSnapshotProvenance>,
}

fn validate_solar_request(request: &PreparedSolarRequest) -> Result<(), String> {
    request
        .prepared
        .validate()
        .map_err(|error| format!("solar_prepared_request_invalid: {error}"))?;
    if request
        .run_id
        .as_deref()
        .is_some_and(|run_id| !valid_run_id(run_id))
    {
        return Err(
            "solar_prepared_request_invalid: run_id must be 1..128 letters, digits, _ or -".into(),
        );
    }
    if let Some(provenance) = &request.provenance {
        validate_provenance(provenance, &request.prepared)?;
    }
    Ok(())
}
fn validate_provenance(
    provenance: &SolarSnapshotProvenance,
    prepared: &PreparedSolarCityInput,
) -> Result<(), String> {
    let digest = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    let receipt = provenance.snapshot_receipt_id.as_bytes();
    let uuid = receipt.len() == 36
        && [8, 13, 18, 23].into_iter().all(|i| receipt[i] == b'-')
        && matches!(receipt[14], b'1'..=b'5')
        && matches!(receipt[19], b'8' | b'9' | b'a' | b'b')
        && receipt.iter().enumerate().all(|(i, b)| {
            [8, 13, 18, 23].contains(&i) || b.is_ascii_digit() || (b'a'..=b'f').contains(b)
        });
    if provenance.project_id != prepared.city.identity.project_id
        || provenance.root != prepared.city.identity.root
        || provenance.template_id != prepared.city.identity.city_id
        || prepared.city.revision.source != "ds-brain"
        || prepared.city.revision.revision.as_deref()
            != Some(provenance.input_base_fingerprint.as_str())
        || !digest(&provenance.input_base_fingerprint)
        || !digest(&provenance.source_snapshot_sha256)
        || !uuid
    {
        return Err("solar_snapshot_provenance_invalid_or_stale".into());
    }
    Ok(())
}

const fn default_render_charts() -> bool {
    true
}

/// The established Cloud Run response shape, retained verbatim when the same
/// runtime is placed behind the durable native owner.
#[derive(Debug, Serialize, Deserialize)]
struct PreparedSolarResponse {
    #[serde(flatten)]
    result: SolarCalculationResult,
    timings: CalculationTimings,
    plots_generated: bool,
    outputs: Vec<OutputFile>,
}

/// Durable publication facts derived only from a sealed prepared request and
/// its retained completed response.  Hosts map this to their activity store;
/// this runtime deliberately does not depend on Sync Center types or transport.
#[derive(Debug, Clone)]
pub struct SolarPublication {
    pub project_id: String,
    pub city_id: String,
    pub engine_release: String,
    pub engine_build_manifest_sha256: String,
    pub outputs: Vec<SolarOutput>,
    pub provenance: Option<SolarSnapshotProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolarJobScope {
    pub project_id: String,
    pub city_id: String,
}

#[derive(Debug, Clone)]
pub struct SolarOutput {
    pub output_id: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SolarPublicationMetadata {
    pub project_id: String,
    pub city_id: String,
    pub engine_release: String,
    pub engine_build_manifest_sha256: String,
    pub outputs: Vec<SolarOutputMetadata>,
    pub provenance: Option<SolarSnapshotProvenance>,
}
#[derive(Debug, Clone)]
pub struct SolarOutputMetadata {
    pub output_id: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// Read the sealed project scope before completion so a host can project the
/// same durable queued/running/cancelled lifecycle into Sync Center.
pub fn solar_job_scope(job: &Job, input: &[u8]) -> Result<SolarJobScope, String> {
    if job.engine != EngineKind::SolarPrepared || job.input_tag != "ds.solar.calculate.prepared/v1"
    {
        return Err("job is not a prepared Solar computation".into());
    }
    if digest(input) != job.input_sha256 {
        return Err("durable Solar input digest mismatch".into());
    }
    let request: PreparedSolarRequest = serde_json::from_slice(input)
        .map_err(|error| format!("durable Solar request scope is invalid: {error}"))?;
    validate_solar_request(&request)?;
    Ok(SolarJobScope {
        project_id: request.prepared.city.identity.project_id,
        city_id: request.prepared.city.identity.city_id,
    })
}

/// Decode publication facts without ever consulting a caller path, browser
/// cache or temporary file.  The payload is re-verified against every output
/// declaration before a host may offer it to a Sync Center transfer.
pub fn solar_publication(
    job: &Job,
    input: &[u8],
    result: &[u8],
) -> Result<SolarPublication, String> {
    let scope = solar_job_scope(job, input)?;
    if job.phase != Phase::Completed
        || digest(result) != job.result_sha256.as_deref().unwrap_or_default()
    {
        return Err("Solar publication result is not a completed durable result".into());
    }
    let request: PreparedSolarRequest = serde_json::from_slice(input)
        .map_err(|error| format!("durable Solar request cannot be published: {error}"))?;
    validate_solar_request(&request)?;
    let response: PreparedSolarResponse = serde_json::from_slice(result)
        .map_err(|error| format!("durable Solar response cannot be published: {error}"))?;
    let outputs = response
        .outputs
        .into_iter()
        .map(|output| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&output.body_base64)
                .map_err(|_| "durable Solar output is not valid base64".to_string())?;
            let declared = output.entry;
            if declared.byte_count != bytes.len() as u64
                || declared.content_digest != format!("sha256:{}", digest(&bytes))
            {
                return Err("durable Solar output does not match its sealed declaration".into());
            }
            Ok(SolarOutput {
                output_id: declared.name,
                content_type: declared.media_type,
                sha256: digest(&bytes),
                size_bytes: bytes.len() as u64,
                bytes,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if outputs.is_empty() {
        return Err("completed Solar result has no publishable outputs".into());
    }
    Ok(SolarPublication {
        project_id: scope.project_id,
        city_id: scope.city_id,
        engine_release: format!(
            "{}@{}",
            response.result.engine.name, response.result.engine.version
        ),
        engine_build_manifest_sha256: response.result.engine.build_manifest_sha256,
        outputs,
        provenance: request.provenance,
    })
}

/// Publication inventory without materialising output byte bodies. The bytes
/// are decoded and verified only by `solar_publication` during a requested
/// transfer.
pub fn solar_publication_metadata(
    job: &Job,
    input: &[u8],
    result: &[u8],
) -> Result<SolarPublicationMetadata, String> {
    let scope = solar_job_scope(job, input)?;
    let request: PreparedSolarRequest = serde_json::from_slice(input)
        .map_err(|error| format!("durable Solar request cannot be published: {error}"))?;
    validate_solar_request(&request)?;
    if job.phase != Phase::Completed
        || digest(result) != job.result_sha256.as_deref().unwrap_or_default()
    {
        return Err("Solar publication result is not a completed durable result".into());
    }
    let response: PreparedSolarResponse = serde_json::from_slice(result)
        .map_err(|error| format!("durable Solar response cannot be published: {error}"))?;
    let outputs = response
        .outputs
        .into_iter()
        .map(|output| {
            let entry = output.entry;
            let sha256 = entry
                .content_digest
                .strip_prefix("sha256:")
                .filter(|value| value.len() == 64)
                .ok_or("durable Solar output has an invalid declared digest")?;
            Ok(SolarOutputMetadata {
                output_id: entry.name,
                content_type: entry.media_type,
                sha256: sha256.into(),
                size_bytes: entry.byte_count,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(SolarPublicationMetadata {
        project_id: scope.project_id,
        city_id: scope.city_id,
        engine_release: format!(
            "{}@{}",
            response.result.engine.name, response.result.engine.version
        ),
        engine_build_manifest_sha256: response.result.engine.build_manifest_sha256,
        outputs,
        provenance: request.provenance,
    })
}

/// Performs live native credential validation. Implemented by the native auth
/// host; no credential, browser token or backend route enters the compute host.
pub trait Authorizer: Send + Sync + 'static {
    fn authorize(&self, expected_owner: &str) -> Result<(), String>;
}

/// A host-owned projection hook after a durable terminal commit.  It never
/// decides compute lifecycle and failures are retried by recovery; hosts use
/// it to project completed work into their existing durable activity owner.
pub trait CompletionObserver: Send + Sync + 'static {
    fn completed(&self, job: &Job) -> Result<(), String>;
    /// Reconcile existing terminal work after restart and after transient
    /// publication failures. The observer owns its durable activity store.
    fn recover(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct Workers {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Workers {
    pub fn start(
        path: PathBuf,
        owner: String,
        lane: String,
        count: usize,
        auth: Arc<dyn Authorizer>,
        observer: Option<Arc<dyn CompletionObserver>>,
    ) -> Result<Self, String> {
        if count == 0 || count > 256 {
            return Err("worker count must be 1..256".into());
        }
        open(&path)?;
        if let Some(observer) = observer.as_ref() {
            for job in open(&path)?
                .jobs(&owner, &lane, 1000)
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|job| job.phase == Phase::Completed)
            {
                if let Err(error) = observer.completed(&job) {
                    eprintln!("compute activity recovery: {error}");
                }
            }
            if let Err(error) = observer.recover() {
                eprintln!("compute activity recovery: {error}");
            }
        }
        // All batches share this bounded pool, including the engine's nested
        // parallel transformer work. A batch cannot escape memory admission by
        // starting another machine-wide pool for its individual transformers.
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(count)
                .thread_name(|i| format!("ds-compute-{i}"))
                .build()
                .map_err(|e| e.to_string())?,
        );
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::with_capacity(count);
        for index in 0..count {
            let (path, owner, lane, auth, stop, observer) = (
                path.clone(),
                owner.clone(),
                lane.clone(),
                auth.clone(),
                stop.clone(),
                observer.clone(),
            );
            let pool = pool.clone();
            let worker = format!("{}-{}-{index}", std::process::id(), now_ms());
            threads.push(thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    if auth.authorize(&owner).is_err() {
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    match pool.install(|| {
                        run_one(
                            &path,
                            &owner,
                            &lane,
                            &worker,
                            auth.clone(),
                            observer.as_deref(),
                        )
                    }) {
                        Ok(true) => {}
                        Ok(false) => {
                            thread::sleep(Duration::from_millis(200));
                        }
                        Err(error) => {
                            eprintln!("compute worker: {error}");
                            thread::sleep(Duration::from_secs(1));
                        }
                    }
                }
            }));
        }
        Ok(Self { stop, threads })
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.stop();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

pub fn run_one(
    path: &Path,
    owner: &str,
    lane: &str,
    worker: &str,
    auth: Arc<dyn Authorizer>,
    observer: Option<&dyn CompletionObserver>,
) -> Result<bool, String> {
    auth.authorize(owner)?;
    let Some((job, input)) = open(path)?
        .claim_job(owner, lane, worker, now_ms(), 30_000)
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let done = Arc::new(AtomicBool::new(false));
    let renewed = Arc::new(AtomicBool::new(true));
    let cancelled = CancelToken::new();
    let renew_thread = {
        let (path, owner, lane, worker, id, done, renewed, auth, cancelled) = (
            path.to_owned(),
            owner.to_owned(),
            lane.to_owned(),
            worker.to_owned(),
            job.id.clone(),
            done.clone(),
            renewed.clone(),
            auth.clone(),
            cancelled.clone(),
        );
        thread::spawn(move || {
            let mut ticks = 0;
            while !done.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(100));
                let current = open(&path).and_then(|store| {
                    store
                        .worker_still_current(&owner, &lane, &id, &worker, now_ms())
                        .map_err(|error| error.to_string())
                });
                if !matches!(current, Ok(true)) {
                    cancelled.cancel();
                    if current.is_err() {
                        renewed.store(false, Ordering::Release);
                    }
                    break;
                }
                ticks += 1;
                if ticks < 50 {
                    continue;
                }
                ticks = 0;
                let result = auth.authorize(&owner).and_then(|_| {
                    open(&path)?
                        .update_job(
                            &owner,
                            &lane,
                            &id,
                            Event::Renew {
                                worker: &worker,
                                lease_ms: 30_000,
                            },
                            now_ms(),
                            None,
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                });
                if result.is_err() {
                    renewed.store(false, Ordering::Release);
                    cancelled.cancel();
                    break;
                }
            }
        })
    };
    let output = std::panic::catch_unwind(|| {
        if digest(&input) != job.input_sha256 {
            return Err("durable input digest mismatch".to_string());
        }
        execute(&input, &job, Some(&cancelled))
    })
    .unwrap_or_else(|_| Err("native compute panicked; input retained".into()));
    done.store(true, Ordering::Release);
    let _ = renew_thread.join();
    if !renewed.load(Ordering::Acquire) {
        return Err("job lease or authorization lost; input retained for recovery".into());
    }
    if cancelled.is_cancelled() {
        // `Cancel` (or a recovered worker) already committed the durable
        // lifecycle state. The engine observed the token at a stage boundary;
        // this worker must not overwrite that terminal/fenced state.
        if !open(path)?
            .worker_still_current(owner, lane, &job.id, worker, now_ms())
            .map_err(|error| error.to_string())?
        {
            return Ok(true);
        }
    }
    auth.authorize(owner)?;
    match output {
        Ok(bytes) => {
            let sha = digest(&bytes);
            let completed = open(path)?
                .update_job(
                    owner,
                    lane,
                    &job.id,
                    Event::Complete {
                        worker,
                        result_sha256: &sha,
                    },
                    now_ms(),
                    Some(&bytes),
                )
                .map_err(|e| e.to_string())?;
            if let Some(observer) = observer {
                if let Err(error) = observer.completed(&completed) {
                    eprintln!("compute activity projection: {error}");
                }
            }
        }
        Err(error) => {
            open(path)?
                .update_job(
                    owner,
                    lane,
                    &job.id,
                    Event::Fail {
                        worker,
                        error: &error,
                    },
                    now_ms(),
                    None,
                )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

fn execute(input: &[u8], job: &Job, cancel: Option<&CancelToken>) -> Result<Vec<u8>, String> {
    if job.engine == EngineKind::FastLv {
        if job.input_tag != "ds.fast-lv.request/v1" {
            return Err("durable Fast LV input tag mismatch".into());
        }
        let request = decode_native_fast_lv_request(input)
            .map_err(|error| format!("durable Fast LV input invalid: {error}"))?;
        let result = process_native_fast_lv(request).map_err(|e| e.to_string())?;
        return encode_native_fast_lv_result(&result).map_err(|e| e.to_string());
    }
    if job.engine != EngineKind::SolarPrepared || job.input_tag != "ds.solar.calculate.prepared/v1"
    {
        return Err("durable compute engine or input tag is unsupported".into());
    }
    let request: PreparedSolarRequest = serde_json::from_slice(input)
        .map_err(|error| format!("durable input is not a supported compute request: {error}"))?;
    validate_solar_request(&request)?;
    let run_id = request.run_id.unwrap_or_else(|| job.id.clone());
    let cancel = cancel.cloned().unwrap_or_else(CancelToken::new);
    let runtime = SolarRuntime::new().with_cancel(&cancel);
    let options = CalculationOptions::default()
        .with_charts(request.render_charts)
        .with_run_id(run_id);
    let outcome = runtime
        .calculate(&request.prepared, &options)
        .map_err(|error| error.to_string())?;
    let mut files = vec![(
        outcome.report_input.name(),
        outcome.report_input.entry.media_type.as_str(),
        outcome.report_input.bytes.as_slice(),
    )];
    if let Some(artifacts) = &outcome.artifacts {
        for chart in &artifacts.charts {
            files.push((
                chart.name.as_str(),
                chart.media_type,
                chart.bytes.as_slice(),
            ));
        }
    }
    for document in &outcome.documents {
        files.push((
            document.name(),
            document.entry.media_type.as_str(),
            document.bytes.as_slice(),
        ));
    }
    let response = PreparedSolarResponse {
        result: outcome.result,
        timings: outcome.timings,
        plots_generated: request.render_charts,
        outputs: encode_outputs(files).map_err(|error| error.to_string())?,
    };
    serde_json::to_vec(&response).map_err(|error| format!("Solar result encoding failed: {error}"))
}

pub fn capacity() -> usize {
    let cpus = thread::available_parallelism().map_or(1, usize::from);
    #[cfg(target_os = "linux")]
    let memory = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
        s.lines().find_map(|l| {
            l.strip_prefix("MemAvailable:")
                .and_then(|r| r.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok())
                .and_then(|n| n.checked_mul(1024))
        })
    });
    #[cfg(not(target_os = "linux"))]
    let memory = None;
    compute_jobs::concurrent_jobs(cpus, memory).min(256)
}
