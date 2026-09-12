//! Portable native compute host shared by headless Server and Desktop.
//! Domain lifecycle/admission belongs to the kernel; domain engines run only
//! after their complete, durable input has been admitted.
//!
//! Admission means one thing here: `execution_context::admit` decided, from
//! the connection identity and the project the work named, what this operation
//! is about, and that decision was written with the job row. Nothing
//! downstream re-derives it. A read answers through
//! `execution_context::visible`; the queue bound and the fairness share are
//! `execution_context::capacity`, asked by the store inside the transaction
//! that inserts or claims, from a census read there — never from a census this
//! crate read a moment earlier; and a queue reopened from disk recovers its
//! rows through `execution_context::recover`. This crate performs the I/O; the
//! kernel decides.
//!
//! **Nothing here touches the network.** A claimed job runs exactly what was
//! admitted: there is no directory to re-check, no membership to have been
//! revoked and no upstream to be unavailable, so an outage cannot fail a job
//! and a job can fail only on its own execution. Entitlement is enforced by
//! the gateway where an effect leaves the machine — publication and sync —
//! exactly as the desktop behaves.
mod claim;
pub use claim::decode_solar_server_submission;

use base64::Engine;
use ds_command_kernel::compute_jobs::{self, EngineKind, Event, Job, Phase};
use ds_command_kernel::execution_context::{
    self, AdmitRequest, Admitted, Fault, Principal, Refusal, RetainEvent, SOLAR_PROCESSING,
    TRANSFORMER_PROCESSING,
};
use ds_network::network::native_fast_lv::{
    decode_native_fast_lv_request, encode_native_fast_lv_result, process_native_fast_lv,
};
use ds_solar_contracts::{PreparedSolarCityInput, SolarCalculationResult};
use ds_solar_io::output::{encode_outputs, OutputFile};
use ds_solar_runtime::{CalculationOptions, CalculationTimings, CancelToken, SolarRuntime};
use ds_sync_store::{JobCaller, Recovery, Store};
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

pub use ds_command_kernel::execution_context::Limits;

/// The Solar engine linked into this native host. Its dependency closure can
/// differ from a bundled standalone engine, so release admission must inspect
/// this identity directly rather than borrow the standalone manifest.
pub fn solar_engine_identity() -> ds_solar_contracts::EngineIdentity {
    let mut identity = ds_solar_contracts::EngineIdentity::current();
    identity.schemas = ds_solar_contracts::EngineIdentity::stamped_schema_inventory();
    identity
}

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

/// Resolve the protected native Server state directory shared by Server jobs
/// and report publication. A custom path is the exact `ds server serve
/// --state-dir` directory; callers place `store.sqlite`, `report-artifacts`,
/// and other Server-owned durable children below it.
///
/// This deliberately does not read `DS_CONFIG_HOME`: that namespace belongs
/// to a native user's credentials, while the Server's durable queue lives in
/// its XDG state namespace. Keeping this here prevents independent Server
/// consumers from silently selecting different queues.
pub fn server_state_directory(lane: &str, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if !matches!(lane, "stable" | "canary") {
        return Err("server lane must be stable or canary".into());
    }
    if let Some(path) = explicit {
        if !path.is_absolute() {
            return Err("server state directory must be absolute".into());
        }
        return Ok(path.to_path_buf());
    }
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/state")))
        .ok_or_else(|| "provide an absolute server state directory".to_string())?;
    if !root.is_absolute() {
        return Err("server state directory must be absolute".into());
    }
    Ok(root.join("ds/server").join(lane))
}

/// The connection an admission is made under: the store's durable owner
/// digest, plus the authenticated identity the kernel reasons about. It
/// deliberately carries no project — a project is decided per operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostIdentity {
    /// The digest of (uid, lane, credential audience) the durable rows are
    /// fenced by. The kernel holds no credential material to recompute it,
    /// so it stays the store's own fence.
    pub owner: String,
    pub principal: Principal,
}

impl HostIdentity {
    pub fn lane(&self) -> &str {
        &self.principal.lane
    }
    /// This connection asking about work, optionally narrowed to one project.
    pub fn caller<'a>(&'a self, project: Option<&'a str>) -> JobCaller<'a> {
        JobCaller::connection(
            &self.owner,
            &self.principal.uid,
            &self.principal.lane,
            &self.principal.deployment,
        )
        .about(project)
    }
}

/// Everything a submission needs that is not the bytes.
///
/// `requested_project` is what the caller named; `saved_project` is the
/// caller's own default (the Server passes `None` — a saved selection is the
/// *client's* default and reaches the Server as a named project, never as
/// host state). The kernel decides between them, and that a sealed input's own
/// project outranks both. Nothing is fetched to allow any of them.
pub struct Admission<'a> {
    pub identity: &'a HostIdentity,
    /// Who asked, e.g. `server-connection:9f2c`. Bounded at 120 characters.
    pub client: &'a str,
    pub key: &'a str,
    pub requested_project: Option<&'a str>,
    pub saved_project: Option<&'a str>,
    pub limits: Limits,
    pub now_ms: u64,
}

/// Why a submission did not become a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// A typed decision the caller must be told, in the kernel's closed
    /// vocabulary. Hosts relay the code, the sentence and, for capacity, the
    /// retry guidance — never a translation of them.
    Refused(Refusal),
    /// The request itself is not admissible: bad key, bad bytes.
    Invalid(String),
    /// The host could not do its own part. Not the caller's answer.
    Host(String),
}

impl SubmitError {
    pub const fn code(&self) -> Option<&'static str> {
        match self {
            Self::Refused(refusal) => Some(refusal.code),
            Self::Invalid(_) | Self::Host(_) => None,
        }
    }
    pub fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Refused(refusal) => Some(refusal),
            Self::Invalid(_) | Self::Host(_) => None,
        }
    }
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => write!(formatter, "{}: {}", refusal.code, refusal.message),
            Self::Invalid(message) | Self::Host(message) => formatter.write_str(message),
        }
    }
}

impl From<Fault> for SubmitError {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Refused(refusal) => Self::Refused(refusal),
            Fault::Hard(message) => Self::Host(message),
        }
    }
}

pub fn submit(path: &Path, admission: &Admission<'_>, input: &[u8]) -> Result<Job, SubmitError> {
    decode_native_fast_lv_request(input)
        .map_err(|error| SubmitError::Invalid(format!("fast_lv_request_invalid: {error}")))?;
    submit_validated(
        path,
        admission,
        input,
        EngineKind::FastLv,
        "ds.fast-lv.request/v1",
        TRANSFORMER_PROCESSING,
        None,
    )
}

/// Admit the existing hosted Solar calculation request as a durable native
/// job. The prepared input embeds every weather/reference byte the runtime
/// needs, so restart recovery never depends on a browser cache, a client path
/// or a temporary request file.
pub fn submit_solar(
    path: &Path,
    admission: &Admission<'_>,
    input: &[u8],
) -> Result<Job, SubmitError> {
    if input.len() > 64 * 1024 * 1024 {
        return Err(SubmitError::Invalid(
            "solar_prepared_request_too_large".into(),
        ));
    }
    let request: PreparedSolarRequest = serde_json::from_slice(input).map_err(|error| {
        SubmitError::Invalid(format!("solar_prepared_request_invalid: {error}"))
    })?;
    validate_solar_request(&request).map_err(SubmitError::Invalid)?;
    let sealed = request.prepared.city.identity.project_id.clone();
    submit_validated(
        path,
        admission,
        input,
        EngineKind::SolarPrepared,
        "ds.solar.calculate.prepared/v1",
        SOLAR_PROCESSING,
        Some(&sealed),
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
    admission: &Admission<'_>,
    input: &[u8],
    provenance: SolarSnapshotProvenance,
) -> Result<Job, SubmitError> {
    if input.len() > 64 * 1024 * 1024 {
        return Err(SubmitError::Invalid(
            "solar_prepared_request_too_large".into(),
        ));
    }
    let mut request: PreparedSolarRequest = serde_json::from_slice(input).map_err(|error| {
        SubmitError::Invalid(format!("solar_prepared_request_invalid: {error}"))
    })?;
    validate_solar_request(&request).map_err(SubmitError::Invalid)?;
    validate_provenance(&provenance, &request.prepared).map_err(SubmitError::Invalid)?;
    request.provenance = Some(provenance);
    let project = request.prepared.city.identity.project_id.clone();
    let sealed =
        serde_json::to_vec(&request).map_err(|error| SubmitError::Host(error.to_string()))?;
    submit_validated(
        path,
        admission,
        &sealed,
        EngineKind::SolarPrepared,
        "ds.solar.calculate.prepared/v1",
        SOLAR_PROCESSING,
        Some(&project),
    )
}

/// The durable id of one caller key, in one project, under one connection.
///
/// The project is part of the digest: two projects of one owner run the same
/// daily key as two pieces of work with two ids, and neither can reach the
/// other's. Within a project the key is still the caller's one handle on their
/// work — reusing it there for another operation or other bytes is
/// `scope_mismatch_for_key` / `payload_changed_for_key`, not a second job.
/// Rows written before this scheme keep the ids they have.
pub fn job_id(owner: &str, lane: &str, project: &str, key: &str) -> String {
    digest(&serde_json::to_vec(&(owner, lane, project, key)).expect("identity tuple encodes"))
}

fn submit_validated(
    path: &Path,
    admission: &Admission<'_>,
    input: &[u8],
    engine: EngineKind,
    input_tag: &str,
    operation: &str,
    sealed_project: Option<&str>,
) -> Result<Job, SubmitError> {
    let Admission {
        identity,
        client,
        key,
        requested_project,
        saved_project,
        limits,
        now_ms,
    } = *admission;
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(SubmitError::Invalid(
            "job_key_invalid: use 1..128 letters, digits, _ or -".into(),
        ));
    }
    // The project decides the id, so it is resolved first — by the kernel, from
    // the same three inputs `admit` will read again and answer identically.
    let project = execution_context::project_for(requested_project, saved_project, sealed_project)?;
    let id = job_id(&identity.owner, identity.lane(), &project, key);
    let mut store = open(path).map_err(SubmitError::Host)?;
    // Read the key's existing row through the whole connection, never through
    // a narrowed caller: the kernel must be able to answer
    // `scope_mismatch_for_key` for a key already used in this project for
    // another operation.
    let existing = store
        .job(&identity.caller(None), &id)
        .map_err(|error| SubmitError::Host(error.to_string()))?;
    let admitted = execution_context::admit(&AdmitRequest {
        now_ms,
        principal: identity.principal.clone(),
        client: client.to_owned(),
        operation: operation.to_owned(),
        job_id: id.clone(),
        idempotency_key: key.to_owned(),
        input_sha256: digest(input),
        requested_project: requested_project.map(str::to_owned),
        saved_project: saved_project.map(str::to_owned),
        sealed_project: sealed_project.map(str::to_owned),
        existing: existing.as_ref().and_then(|job| job.context.clone()),
    })?;
    if let Admitted::Existing(_) = &admitted {
        // An idempotent resubmit: the stored job, unchanged, and no capacity
        // consumed for work that is already queued or done.
        return existing
            .ok_or_else(|| SubmitError::Host("an existing context has no durable job row".into()));
    }
    let context = admitted.context().clone();
    let job = Job {
        id,
        owner: identity.owner.clone(),
        lane: identity.lane().to_owned(),
        input_sha256: context.input_sha256.clone(),
        engine,
        input_tag: input_tag.to_owned(),
        phase: Phase::Queued,
        attempts: 0,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
        context: Some(context),
    };
    // The queue bound is the store's to apply, inside the transaction that
    // inserts: a census read here would describe a queue other submissions
    // are changing. A refusal comes back typed and is relayed as the kernel
    // said it — including `context_unrecoverable` for a row written before
    // execution contexts, which is never adopted and never run.
    store
        .submit_job(&job, input, limits)
        .map_err(|error| match error {
            ds_sync_store::Error::Refused(refusal) => SubmitError::Refused(refusal),
            // The row's own statement of what the kernel decides from a stored
            // context. It is the only statement there is for a key whose row
            // predates contexts, or for two first submissions racing under one
            // key — and it is the caller's answer, not the host's fault.
            ds_sync_store::Error::Kernel(code) if code == "compute_job_identity_conflict" => {
                SubmitError::Refused(Refusal {
                    code: execution_context::PAYLOAD_CHANGED_FOR_KEY,
                    message: "that idempotency key was admitted with different input bytes".into(),
                    retry_after_ms: None,
                    scope: None,
                })
            }
            ds_sync_store::Error::Kernel(code) if code == "compute_job_scope_conflict" => {
                SubmitError::Refused(Refusal {
                    code: execution_context::SCOPE_MISMATCH_FOR_KEY,
                    message: "that idempotency key already names another project or operation"
                        .into(),
                    retry_after_ms: None,
                    scope: None,
                })
            }
            other => SubmitError::Host(other.to_string()),
        })
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

/// Everything a pool of workers needs to take work from one connection's
/// durable queue: which queue, whose it is, how much of the machine it may
/// use, how to re-authorize, and what to tell when a job completes.
pub struct WorkerContext {
    pub path: PathBuf,
    pub identity: HostIdentity,
    pub limits: Limits,
    pub auth: Arc<dyn Authorizer>,
    pub observer: Option<Arc<dyn CompletionObserver>>,
}

/// Give every row in this connection's queue the execution context it is
/// about, reconstructing the ones written before contexts existed. Solar rows
/// take their project from their own sealed input; a transformer row, which
/// carries none by design, is left alone and named — there is nothing outside
/// its own bytes that may name a project for it.
pub fn recover_contexts(path: &Path, identity: &HostIdentity) -> Result<Recovery, String> {
    open(path)?
        .recover_contexts(
            &identity.principal,
            &identity.owner,
            &|job, input| {
                solar_job_scope(job, input)
                    .ok()
                    .map(|scope| scope.project_id)
            },
            now_ms(),
        )
        .map_err(|error| error.to_string())
}

pub struct Workers {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Workers {
    pub fn start(context: Arc<WorkerContext>, count: usize) -> Result<Self, String> {
        if count == 0 || count > 256 {
            return Err("worker count must be 1..256".into());
        }
        open(&context.path)?;
        // Reopening a queue is the moment a row written by a released Server
        // gets the context it never had. It happens once, before any worker
        // can claim, and it never guesses.
        let recovery = recover_contexts(&context.path, &context.identity)?;
        if !recovery.unrecoverable.is_empty() {
            eprintln!(
                "compute queue: {} durable job(s) predate execution contexts and name no project; read each one's result and resubmit it under an explicit --project",
                recovery.unrecoverable.len()
            );
        }
        if let Some(observer) = context.observer.as_ref() {
            for job in open(&context.path)?
                .jobs(&context.identity.caller(None), 1000)
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
            let (context, stop) = (context.clone(), stop.clone());
            let pool = pool.clone();
            let worker = format!("{}-{}-{index}", std::process::id(), now_ms());
            threads.push(thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    if context.auth.authorize(&context.identity.owner).is_err() {
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    match pool.install(|| run_one(&context, &worker)) {
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

pub fn run_one(context: &WorkerContext, worker: &str) -> Result<bool, String> {
    let (path, identity) = (context.path.as_path(), &context.identity);
    let (owner, lane) = (identity.owner.as_str(), identity.lane());
    context.auth.authorize(owner)?;
    // Fairness is decided from the whole host's census, read by the store
    // inside the transaction that claims and asked of the kernel once per
    // candidate project: a project over its share while another waits is
    // skipped, so the next project's work starts instead of the pool idling,
    // and several workers claiming at once cannot each take the same room.
    let Some((job, input)) = open(path)?
        .claim_job(
            &identity.caller(None),
            worker,
            now_ms(),
            30_000,
            context.limits,
        )
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    // The store never hands out a claimable row without one.
    let job_context = job
        .context
        .clone()
        .ok_or("a claimed job has no execution context")?;
    // A claim is a lifecycle event, and the kernel's answer to a lifecycle
    // event is the context it was given. Stating it here is the point: what
    // runs now is exactly what was admitted, and there is nothing to ask
    // anyone about before it does.
    let job_context = execution_context::retain(&job_context, RetainEvent::Claim)
        .map_err(|fault| fault.to_string())?;
    let done = Arc::new(AtomicBool::new(false));
    let renewed = Arc::new(AtomicBool::new(true));
    let cancelled = CancelToken::new();
    let renew_thread = {
        let (path, owner, lane, worker, id, done, renewed, auth, cancelled, caller_project) = (
            path.to_owned(),
            owner.to_owned(),
            lane.to_owned(),
            worker.to_owned(),
            job.id.clone(),
            done.clone(),
            renewed.clone(),
            context.auth.clone(),
            cancelled.clone(),
            job_context.project.clone(),
        );
        let identity = identity.clone();
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
                            &identity.caller(Some(&caller_project)),
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
    context.auth.authorize(owner)?;
    let caller = identity.caller(Some(&job_context.project));
    match output {
        Ok(bytes) => {
            let sha = digest(&bytes);
            let completed = open(path)?
                .update_job(
                    &caller,
                    &job.id,
                    Event::Complete {
                        worker,
                        result_sha256: &sha,
                    },
                    now_ms(),
                    Some(&bytes),
                )
                .map_err(|e| e.to_string())?;
            // The result is this account's own and stays durable. Projecting
            // it into the project's activity and publication queue asks
            // nobody's permission: whether the publication itself may leave
            // the machine is the gateway's answer on that effect.
            if let Some(observer) = context.observer.as_deref() {
                if let Err(error) = observer.completed(&completed) {
                    eprintln!("compute activity projection: {error}");
                }
            }
        }
        Err(error) => {
            open(path)?
                .update_job(
                    &caller,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_state_directory_uses_an_explicit_absolute_server_root() {
        let root = Path::new("/var/lib/ds-server");
        assert_eq!(server_state_directory("stable", Some(root)).unwrap(), root);
        assert!(server_state_directory("stable", Some(Path::new("relative"))).is_err());
        assert!(server_state_directory("preview", Some(root)).is_err());
    }
}
