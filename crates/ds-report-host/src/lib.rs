//! The native host of one transformer report run.
//!
//! The desktop shell, the `ds` executable and a headless server produce
//! individual transformer reports from the same governed inputs, through the
//! same engine, and record the same evidence. The decisions are the kernel's
//! (`ds_command_kernel::report_export`); this crate is the IO between those
//! decisions and a filesystem: it stages the exact documents the engine
//! reads, asks the host's engine adapter to run one closed task, proves the
//! bytes the engine declared are the bytes on disk, places the artifacts
//! under the operator's output root beside their receipt, and runs a batch of
//! transformers under a bounded number of resident engines.
//!
//! It starts no process and holds no credential. The [`ReportEngine`] adapter
//! and the input fetcher are the host's: `ds` reaches `ds-report` through its
//! audited process boundary, the desktop through its sidecar, and both hand
//! this crate a closed adapter with one method per task.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ds_command_kernel::printing::table_rows::PrintWarning;
use ds_command_kernel::report_export::{
    self as decisions, engine_identity, engine_request, plan_transformer_export, run_receipt,
    transformer_document_bytes, typed_failure, verify_result, BatchResult, EngineIdentity,
    InputReceipt, ResultRefusal, RunReceiptFacts, StagedInputs, VerifiedArtifact,
    BATCH_RECEIPT_FILE, MAX_RESULT_BYTES, MAX_SEMANTIC_REQUEST_BYTES, RUN_RECEIPT_FILE,
};
use ds_command_kernel::report_formats::DesignOutputSelection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub use ds_command_kernel::report_export::{
    batch_plan, blocker_summary, BatchPlan, DEFAULT_RESIDENT_LIMIT, MAX_RESIDENT_LIMIT,
};

pub mod preview;
pub use preview::*;

/// The kernel refused the inputs before any engine ran.
pub const CODE_INPUTS_INVALID: &str = "report_inputs_invalid";
/// The staging or output directory could not be prepared.
pub const CODE_STAGING_FAILED: &str = "report_staging_failed";
/// The engine failed before writing a typed result document.
pub const CODE_ENGINE_REFUSED: &str = "engine_refused";
/// The engine ran and refused, with typed blockers in `detail.blockers`.
pub const CODE_EXPORT_BLOCKED: &str = "export_blocked";
/// The result document, or the bytes it declared, are not this run's answer.
pub const CODE_RESULT_INVALID: &str = "report_result_invalid";
/// The output directory already holds this transformer's report.
pub const CODE_OUTPUT_EXISTS: &str = "report_output_exists";
/// The engine's build identity changed while the run was in flight.
pub const CODE_ENGINE_IDENTITY_CHANGED: &str = "report_engine_identity_changed";
/// The engine answered `build-info` with something other than its identity.
pub const CODE_ENGINE_IDENTITY_INVALID: &str = "callee_contract_mismatch";
/// A Rwanda project needs the installed administrative reference asset.
pub const CODE_ADMIN_BOUNDS_UNAVAILABLE: &str = "admin_bounds_unavailable";

/// The DSAB spatial-index header the reporter writes (`prepare-admin-bounds`).
const DSAB_MAGIC: &[u8; 8] = b"DSABIDX\0";
const DSAB_SCHEMA_VERSION: u16 = 1;
const DSAB_HEADER_BYTES: usize = 84;
const MAX_DSAB_BYTES: u64 = 256 * 1024 * 1024;
/// Where the desktop installs machine-shared reference assets.
pub const SHARED_ROOT_DIRECTORY: &str = "rw.datasolutions.desktop.shared";
const SHARED_ROOT_POINTER: &str = "reference-query-cache-location.json";
const REFERENCE_ASSET_ROOT: &str = "reference-assets";
pub const ADMIN_BOUNDS_ASSET_ID: &str = "rw-admin-villages";
const ADMIN_BOUNDS_FILE: &str = "asset.dsab";
const STAGING_TRANSFORMER_FILE: &str = "transformer.json";
const STAGING_CONFIG_FILE: &str = "config.json";
const STAGING_REQUEST_FILE: &str = "request.json";
const STAGING_RESULT_FILE: &str = "result.json";
const STAGING_OUT_DIRECTORY: &str = "out";
/// The `ds.print-context/v1` document a host assembled for this transformer,
/// staged beside its room and named to the engine digest-bound.
const STAGING_PRINT_CONTEXT_FILE: &str = "print-context.json";
/// The engine-schema media grant a host holds for this room's photos, staged
/// beside the room and named to the engine by path.
const STAGING_MEDIA_GRANT_FILE: &str = "media-grant.json";
/// The largest media grant document a run stages: one grant, or a bundle of
/// a few scopes, in the engine's own schema.
pub const MAX_MEDIA_GRANT_BYTES: u64 = 64 * 1024;

/// A typed failure with a stable code a host can document and match on.
#[derive(Debug, Clone, PartialEq)]
pub struct HostFailure {
    pub code: String,
    pub message: String,
    pub detail: Option<Value>,
}

impl HostFailure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl std::fmt::Display for HostFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HostFailure {}

/// How one engine invocation ended. The answer is the result document; this
/// is only the coarse signal and the engine's bounded own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineExit {
    pub succeeded: bool,
    pub summary: String,
}

/// The engine a host reaches, one closed method per task. An adapter names
/// the subcommand statically and never takes an argument vector from here.
pub trait ReportEngine: Sync {
    /// `ds-report build-info`, as the JSON document it prints.
    fn build_info(&self) -> Result<Value, HostFailure>;
    /// `ds-report export-transformer-report --request REQUEST --result RESULT`.
    fn export_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure>;
}

/// The external print context one transformer prints with: the exact
/// `ds.print-context/v1` bytes the kernel assembled (`print_context_document`),
/// their digest, and what they carry and omit — recorded in the run receipt.
#[derive(Debug, Clone)]
pub struct PrintContextBytes {
    pub bytes: Vec<u8>,
    /// Lowercase hex SHA-256 of `bytes`; the engine re-verifies it.
    pub sha256: String,
    /// Layer ids the document carries.
    pub layers: Vec<String>,
    /// Selected layer ids the host could not supply, named rather than dropped.
    pub omitted: Vec<String>,
}

/// One transformer's governed inputs, as the host fetched them.
#[derive(Debug, Clone)]
pub struct TransformerReportInputs {
    pub transformer: String,
    /// The saved document's server version — its publication revision.
    pub server_version: i64,
    pub layers: BTreeMap<String, Value>,
    /// ds-brain's `metadata.content_digest` when the room came from the
    /// service; recorded against the kernel's own digest of the same bytes.
    pub content_digest: Option<String>,
    pub selection: Option<DesignOutputSelection>,
    /// External print context, when the host assembled one. `None` prints the
    /// named outputs without it and records that in the receipt.
    pub print_context: Option<PrintContextBytes>,
    /// One-based position in the drawing set and the set's size, for the
    /// sheet numbering a title block prints; `None` is a set of one.
    pub sheet: Option<(u32, u32)>,
}

/// The verified Rwanda villages reference the engine stamps artifacts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminBoundsAsset {
    pub path: PathBuf,
    /// SHA-256 of the exact file bytes — what the engine re-verifies.
    pub sha256: String,
    pub size_bytes: u64,
    /// The dataset snapshot identity the project receipt names.
    pub semantic_sha256: String,
}

/// What every run of one batch shares.
#[derive(Debug, Clone)]
pub struct RunSettings<'a> {
    pub project_id: &'a str,
    pub receipt: &'a InputReceipt,
    pub admin_bounds: Option<&'a AdminBoundsAsset>,
    /// Private scratch; each run creates one directory below it and removes
    /// it when done.
    pub staging_root: &'a Path,
    /// The operator-named output root; each run places
    /// `<out_root>/<transformer>/` beside the batch receipt.
    pub out_root: &'a Path,
}

/// What one run may carry beyond its governed inputs. Every member is
/// optional and the default carries none, so a caller that has nothing to add
/// runs exactly as before.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunExtras<'a> {
    /// One media grant in the engine's own schema, covering every photo the
    /// room can reference. The engine refuses a room whose spatial layers
    /// carry media references unless a grant is staged, for a print-only
    /// selection too; a room without photos needs none.
    pub media_grant: Option<&'a [u8]>,
}

/// One completed run: the artifacts are on disk under `artifact_dir`, the
/// receipt beside them.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub transformer: String,
    pub client_run_id: String,
    pub artifact_dir: PathBuf,
    pub receipt_path: PathBuf,
    pub receipt: Value,
    pub artifacts: Vec<VerifiedArtifact>,
    pub warnings: Vec<PrintWarning>,
    pub engine: EngineIdentity,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// `report-` plus 32 lowercase hex characters: the operating system's random
/// bytes where it offers them, otherwise a digest of the clock, the process
/// and a counter. It names scratch space and a receipt, never a secret.
pub fn random_run_id() -> String {
    let mut bytes = [0_u8; 16];
    let mut filled = false;
    #[cfg(unix)]
    {
        if let Ok(mut source) = File::open("/dev/urandom") {
            filled = source.read_exact(&mut bytes).is_ok();
        }
    }
    if !filled {
        let mut hasher = Sha256::new();
        hasher.update(now_ms().to_be_bytes());
        hasher.update(std::process::id().to_be_bytes());
        hasher.update(RUN_COUNTER.fetch_add(1, Ordering::Relaxed).to_be_bytes());
        hasher.update(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
                .to_be_bytes(),
        );
        bytes.copy_from_slice(&hasher.finalize()[..16]);
    }
    let mut id = String::with_capacity(39);
    id.push_str("report-");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

fn staging_failed(message: impl Into<String>) -> HostFailure {
    HostFailure::new(CODE_STAGING_FAILED, message)
}

fn create_private_dir(path: &Path) -> Result<(), HostFailure> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| staging_failed(format!("could not create {}: {error}", path.display())))?;
    Ok(())
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), HostFailure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| staging_failed(format!("could not create {}: {error}", path.display())))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| staging_failed(format!("could not write {}: {error}", path.display())))
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, HostFailure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(HostFailure::new(
                CODE_RESULT_INVALID,
                format!("could not inspect {}: {error}", path.display()),
            ))
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            format!("{} is not a regular file", path.display()),
        ));
    }
    if metadata.len() > max_bytes {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            format!("{} exceeds its {max_bytes}-byte bound", path.display()),
        ));
    }
    fs::read(path).map(Some).map_err(|error| {
        HostFailure::new(
            CODE_RESULT_INVALID,
            format!("could not read {}: {error}", path.display()),
        )
    })
}

/// Stream one regular file's digest, refusing a symlink and a size other
/// than the declared one.
fn verify_file(path: &Path, expected_size: u64, expected_sha256: &str) -> Result<(), HostFailure> {
    let invalid = |message: String| HostFailure::new(CODE_RESULT_INVALID, message);
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        invalid(format!(
            "could not inspect artifact {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(format!(
            "artifact {} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() != expected_size {
        return Err(invalid(format!(
            "artifact {} size does not match its declaration",
            path.display()
        )));
    }
    let mut file = File::open(path).map_err(|error| {
        invalid(format!(
            "could not open artifact {}: {error}",
            path.display()
        ))
    })?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            invalid(format!(
                "could not read artifact {}: {error}",
                path.display()
            ))
        })?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > expected_size {
            return Err(invalid(format!(
                "artifact {} changed while it was read",
                path.display()
            )));
        }
        digest.update(&buffer[..count]);
    }
    if total != expected_size || format!("{:x}", digest.finalize()) != expected_sha256 {
        return Err(invalid(format!(
            "artifact {} digest does not match its declaration",
            path.display()
        )));
    }
    Ok(())
}

/// Move one verified artifact into place: a rename on the same filesystem,
/// otherwise a copy that is re-verified before the staged copy goes.
fn place_artifact(from: &Path, to: &Path, size: u64, sha256: &str) -> Result<(), HostFailure> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(HostFailure::new(
            CODE_OUTPUT_EXISTS,
            format!("{} already exists", to.display()),
        ));
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)
        .map_err(|error| staging_failed(format!("could not place {}: {error}", to.display())))?;
    verify_file(to, size, sha256)?;
    fs::remove_file(from).map_err(|error| {
        staging_failed(format!(
            "could not remove staged {}: {error}",
            from.display()
        ))
    })?;
    Ok(())
}

// Flush verified artifacts and receipt before making the complete directory visible.
fn promote_report(staged: &Path, target: &Path) -> Result<(), HostFailure> {
    for entry in fs::read_dir(staged).map_err(|e| staging_failed(e.to_string()))? {
        let entry = entry.map_err(|e| staging_failed(e.to_string()))?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path())
            .and_then(|f| f.sync_all())
            .map_err(|e| staging_failed(e.to_string()))?;
    }
    sync_directory(staged)?;
    if fs::symlink_metadata(target).is_ok() {
        return Err(HostFailure::new(
            CODE_OUTPUT_EXISTS,
            format!("{} already exists", target.display()),
        ));
    }
    #[cfg(target_os = "linux")]
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staged,
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| {
        if e == rustix::io::Errno::EXIST {
            HostFailure::new(
                CODE_OUTPUT_EXISTS,
                format!("{} already exists", target.display()),
            )
        } else {
            staging_failed(format!("could not promote report: {e}"))
        }
    })?;
    #[cfg(not(target_os = "linux"))]
    fs::rename(staged, target)
        .map_err(|e| staging_failed(format!("could not promote report: {e}")))?;
    if let Some(parent) = target.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), HostFailure> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| staging_failed(e.to_string()))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

struct Staging {
    path: PathBuf,
}

impl Drop for Staging {
    fn drop(&mut self) {
        // A developer replaying the engine over a run's exact inputs keeps
        // them; production removes every scratch on every path.
        if std::env::var_os("DS_REPORT_HOST_KEEP_STAGING").is_some_and(|v| v == "1") {
            return;
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn engine_identity_now(engine: &dyn ReportEngine) -> Result<EngineIdentity, HostFailure> {
    let document = engine.build_info()?;
    engine_identity(&document)
        .map_err(|error| HostFailure::new(CODE_ENGINE_IDENTITY_INVALID, error))
}

fn blocked(refusal: ResultRefusal, transformer: &str) -> HostFailure {
    match refusal {
        ResultRefusal::Blocked {
            status,
            blockers,
            artifacts,
        } => HostFailure::new(
            CODE_EXPORT_BLOCKED,
            format!(
                "the reporter ended {transformer} with status {status:?}: {}",
                blocker_summary(&blockers)
            ),
        )
        .with_detail(json!({
            "status": status,
            "blockers": blockers,
            "artifacts": artifacts,
        })),
        ResultRefusal::Invalid { message } => HostFailure::new(CODE_RESULT_INVALID, message),
    }
}

/// Run one transformer report end to end: plan, stage, run, prove, place,
/// receipt. Nothing is left under the output root unless every step held;
/// the staging directory is removed on every path.
pub fn run_transformer_report(
    engine: &dyn ReportEngine,
    settings: &RunSettings<'_>,
    inputs: &TransformerReportInputs,
) -> Result<RunOutcome, HostFailure> {
    run_transformer_report_with(engine, settings, inputs, &RunExtras::default())
}

/// [`run_transformer_report`] with what the host holds beyond the governed
/// inputs: a media grant is bounded, staged beside the room and named in the
/// engine request; the rest of the run is identical.
pub fn run_transformer_report_with(
    engine: &dyn ReportEngine,
    settings: &RunSettings<'_>,
    inputs: &TransformerReportInputs,
    extras: &RunExtras<'_>,
) -> Result<RunOutcome, HostFailure> {
    if let Some(grant) = extras.media_grant {
        if grant.is_empty() || grant.len() as u64 > MAX_MEDIA_GRANT_BYTES {
            return Err(HostFailure::new(
                CODE_INPUTS_INVALID,
                format!("a media grant is a document of 1 through {MAX_MEDIA_GRANT_BYTES} bytes"),
            ));
        }
    }
    let plan = plan_transformer_export(
        settings.project_id,
        settings.receipt,
        &inputs.transformer,
        inputs.server_version,
        &inputs.layers,
        inputs.selection.as_ref(),
    )
    .map_err(|error| HostFailure::new(CODE_INPUTS_INVALID, error))?;
    decisions::verify_content_digest(&plan, inputs.content_digest.as_deref())
        .map_err(|error| HostFailure::new(CODE_INPUTS_INVALID, error))?;
    if plan.requires_admin_bounds && settings.admin_bounds.is_none() {
        return Err(HostFailure::new(
            CODE_ADMIN_BOUNDS_UNAVAILABLE,
            "a Rwanda report requires the installed administrative reference asset",
        ));
    }
    let artifact_dir = settings.out_root.join(&plan.transformer);
    if fs::symlink_metadata(&artifact_dir).is_ok() {
        return Err(HostFailure::new(
            CODE_OUTPUT_EXISTS,
            format!("{} already exists", artifact_dir.display()),
        ));
    }

    let transformer_bytes = transformer_document_bytes(&inputs.layers)
        .map_err(|error| HostFailure::new(CODE_INPUTS_INVALID, error))?;
    let config_bytes = settings
        .receipt
        .network_config_bytes()
        .map_err(|error| HostFailure::new(CODE_INPUTS_INVALID, error))?;
    let semantic_bytes = transformer_bytes.len() as u64 + config_bytes.len() as u64;
    if semantic_bytes > MAX_SEMANTIC_REQUEST_BYTES {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            format!("the room and configuration exceed the {MAX_SEMANTIC_REQUEST_BYTES}-byte request bound"),
        ));
    }

    let client_run_id = random_run_id();
    fs::create_dir_all(settings.staging_root).map_err(|error| {
        staging_failed(format!(
            "could not create {}: {error}",
            settings.staging_root.display()
        ))
    })?;
    let staging = Staging {
        path: settings.staging_root.join(&client_run_id),
    };
    create_private_dir(&staging.path)?;
    let out_dir = staging.path.join(STAGING_OUT_DIRECTORY);
    create_private_dir(&out_dir)?;
    let transformer_path = staging.path.join(STAGING_TRANSFORMER_FILE);
    let config_path = staging.path.join(STAGING_CONFIG_FILE);
    let request_path = staging.path.join(STAGING_REQUEST_FILE);
    let result_path = staging.path.join(STAGING_RESULT_FILE);
    let print_context_path = staging.path.join(STAGING_PRINT_CONTEXT_FILE);
    let media_grant_path = staging.path.join(STAGING_MEDIA_GRANT_FILE);
    write_new_private(&transformer_path, &transformer_bytes)?;
    write_new_private(&config_path, &config_bytes)?;
    if let Some(context) = &inputs.print_context {
        write_new_private(&print_context_path, &context.bytes)?;
    }
    if let Some(grant) = extras.media_grant {
        write_new_private(&media_grant_path, grant)?;
    }
    let admin_bounds = settings
        .admin_bounds
        .map(|asset| (asset.path.as_path(), asset.sha256.as_str()));
    let print_context = inputs
        .print_context
        .as_ref()
        .map(|context| (print_context_path.as_path(), context.sha256.as_str()));
    let media_grant = extras
        .media_grant
        .is_some()
        .then_some(media_grant_path.as_path());
    let request = engine_request(
        &plan,
        &StagedInputs {
            transformer_document: &transformer_path,
            network_config: &config_path,
            out_dir: &out_dir,
            admin_bounds,
            media_grant,
            print_context,
            sheet: inputs.sheet,
        },
    )
    .map_err(|error| HostFailure::new(CODE_INPUTS_INVALID, error))?;
    write_new_private(
        &request_path,
        &serde_json::to_vec_pretty(&request).map_err(|error| staging_failed(error.to_string()))?,
    )?;

    let identity_before = engine_identity_now(engine)?;
    let exit = engine.export_transformer_report(&request_path, &result_path)?;
    let document = read_bounded(&result_path, MAX_RESULT_BYTES)?;
    if !exit.succeeded {
        if let Some(refusal) = document
            .as_deref()
            .and_then(|bytes| typed_failure(bytes, &plan))
        {
            return Err(blocked(refusal, &plan.transformer));
        }
        return Err(HostFailure::new(
            CODE_ENGINE_REFUSED,
            format!("the reporter did not complete {}", plan.transformer),
        )
        .with_detail(json!({ "engine": exit.summary })));
    }
    let document = document.ok_or_else(|| {
        HostFailure::new(
            CODE_RESULT_INVALID,
            "the reporter exited successfully without writing its result document",
        )
    })?;
    let result = verify_result(&document, &plan, &out_dir)
        .map_err(|refusal| blocked(refusal, &plan.transformer))?;
    verify_staged_artifacts(&out_dir, &result.artifacts)?;
    let identity_after = engine_identity_now(engine)?;
    if identity_after != identity_before {
        return Err(HostFailure::new(
            CODE_ENGINE_IDENTITY_CHANGED,
            "the reporter's build identity changed while the export was running",
        ));
    }

    fs::create_dir_all(settings.out_root).map_err(|error| {
        staging_failed(format!(
            "could not create {}: {error}",
            settings.out_root.display()
        ))
    })?;
    // Delivery is prepared on the destination filesystem. A crash before the
    // final rename leaves only private scratch, never a partial final report.
    let delivery = Staging {
        path: settings.out_root.join(format!(".{client_run_id}.partial")),
    };
    create_private_dir(&delivery.path)?;
    let placed = (|| -> Result<(), HostFailure> {
        for artifact in &result.artifacts {
            place_artifact(
                &out_dir.join(&artifact.filename),
                &delivery.path.join(&artifact.filename),
                artifact.size_bytes,
                &artifact.sha256,
            )?;
        }
        let receipt = run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: &client_run_id,
            engine: &identity_before,
            result: &result,
            artifact_dir: &plan.transformer,
            now_ms: now_ms(),
            expected_content_digest: inputs.content_digest.as_deref(),
            print_context_sha256: inputs
                .print_context
                .as_ref()
                .map(|context| context.sha256.as_str()),
            print_context_layers: inputs
                .print_context
                .as_ref()
                .map_or(&[][..], |context| context.layers.as_slice()),
            print_context_omitted: inputs
                .print_context
                .as_ref()
                .map_or(&[][..], |context| context.omitted.as_slice()),
        })
        .map_err(|error| HostFailure::new(CODE_RESULT_INVALID, error))?;
        write_new_private(
            &delivery.path.join(RUN_RECEIPT_FILE),
            &serde_json::to_vec_pretty(&receipt)
                .map_err(|error| staging_failed(error.to_string()))?,
        )?;
        Ok(())
    })();
    if let Err(failure) = placed {
        return Err(failure);
    }
    promote_report(&delivery.path, &artifact_dir)?;
    let receipt_path = artifact_dir.join(RUN_RECEIPT_FILE);
    let receipt: Value = serde_json::from_slice(
        &fs::read(&receipt_path).map_err(|error| staging_failed(error.to_string()))?,
    )
    .map_err(|error| staging_failed(error.to_string()))?;
    Ok(RunOutcome {
        transformer: plan.transformer.clone(),
        client_run_id,
        artifact_dir,
        receipt_path,
        receipt,
        artifacts: result.artifacts,
        warnings: result.warnings,
        engine: identity_before,
    })
}

/// Every declared artifact is a regular file with the declared bytes, and
/// nothing else is in the staged output directory.
fn verify_staged_artifacts(
    out_dir: &Path,
    artifacts: &[VerifiedArtifact],
) -> Result<(), HostFailure> {
    let mut expected = HashSet::new();
    for artifact in artifacts {
        verify_file(
            &out_dir.join(&artifact.filename),
            artifact.size_bytes,
            &artifact.sha256,
        )?;
        expected.insert(artifact.filename.as_str());
    }
    let mut found = HashSet::new();
    let entries = fs::read_dir(out_dir).map_err(|error| {
        HostFailure::new(
            CODE_RESULT_INVALID,
            format!("could not list {}: {error}", out_dir.display()),
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| HostFailure::new(CODE_RESULT_INVALID, error.to_string()))?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            HostFailure::new(
                CODE_RESULT_INVALID,
                "the reporter output directory contains a non-Unicode name",
            )
        })?;
        if !expected.contains(name) || !found.insert(name.to_string()) {
            return Err(HostFailure::new(
                CODE_RESULT_INVALID,
                format!("the reporter output directory contains an undeclared file {name:?}"),
            ));
        }
    }
    if found.len() != expected.len() {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            "the reporter output directory is incomplete",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Batches
// ---------------------------------------------------------------------------

/// A whole batch's settings.
#[derive(Debug, Clone)]
pub struct BatchSettings<'a> {
    pub run: RunSettings<'a>,
    pub lane: &'a str,
    /// How many engines may be resident at once; the kernel's `batch_plan`
    /// decides it from the scope, the processors and the resident limit.
    pub concurrency: usize,
}

/// What a batch left behind: one result per requested transformer in the
/// kernel's order, the receipt at the output root, and the runs that
/// completed.
#[derive(Debug)]
pub struct BatchOutcome {
    pub receipt: Value,
    pub receipt_path: PathBuf,
    pub results: Vec<BatchResult>,
    pub runs: Vec<RunOutcome>,
    pub engine: EngineIdentity,
}

fn failure_result(transformer: &str, failure: &HostFailure) -> BatchResult {
    BatchResult::error(
        transformer,
        &failure.code,
        &failure.message,
        failure.detail.clone(),
    )
}

/// Run every named transformer with at most `concurrency` engines resident.
///
/// Inputs are fetched one at a time on the calling thread — a host's
/// credential and its network are serial resources — and handed to a worker
/// as soon as one is free, so at most `concurrency` rooms plus one are ever
/// held in memory. A transformer that cannot be fetched or that the engine
/// refuses is recorded with its typed failure; the batch continues.
pub fn run_batch<F>(
    engine: &dyn ReportEngine,
    settings: &BatchSettings<'_>,
    names: &[String],
    mut fetch: F,
) -> Result<BatchOutcome, HostFailure>
where
    F: FnMut(&str) -> Result<TransformerReportInputs, HostFailure>,
{
    if names.is_empty() {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "a report batch names at least one transformer",
        ));
    }
    if settings.concurrency == 0 || settings.concurrency > MAX_RESIDENT_LIMIT {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            format!("batch concurrency must be from 1 through {MAX_RESIDENT_LIMIT}"),
        ));
    }
    let out_root = settings.run.out_root;
    fs::create_dir_all(out_root).map_err(|error| {
        staging_failed(format!("could not create {}: {error}", out_root.display()))
    })?;
    let receipt_path = out_root.join(BATCH_RECEIPT_FILE);
    if fs::symlink_metadata(&receipt_path).is_ok() {
        return Err(HostFailure::new(
            CODE_OUTPUT_EXISTS,
            format!(
                "{} already exists; choose a fresh output directory",
                receipt_path.display()
            ),
        ));
    }
    let identity = engine_identity_now(engine)?;

    let workers = settings.concurrency.min(names.len());
    let (job_tx, job_rx) = mpsc::sync_channel::<(String, TransformerReportInputs)>(0);
    let job_rx = Mutex::new(job_rx);
    let (done_tx, done_rx) = mpsc::channel::<(String, Result<RunOutcome, HostFailure>)>();
    let mut results: Vec<BatchResult> = Vec::with_capacity(names.len());
    let mut runs = Vec::new();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let job_rx = &job_rx;
            let done_tx = done_tx.clone();
            let run_settings = &settings.run;
            scope.spawn(move || loop {
                let job = match job_rx.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => break,
                };
                let Ok((name, inputs)) = job else { break };
                let outcome = run_transformer_report(engine, run_settings, &inputs);
                drop(inputs);
                if done_tx.send((name, outcome)).is_err() {
                    break;
                }
            });
        }
        for name in names {
            match fetch(name) {
                Ok(inputs) => {
                    if job_tx.send((name.clone(), inputs)).is_err() {
                        break;
                    }
                }
                Err(failure) => results.push(failure_result(name, &failure)),
            }
        }
        drop(job_tx);
        drop(done_tx);
        for (name, outcome) in done_rx {
            match outcome {
                Ok(run) => {
                    results.push(BatchResult::ok(
                        &name,
                        format!("{}/{}", run.transformer, RUN_RECEIPT_FILE),
                        run.artifacts.len(),
                    ));
                    runs.push(run);
                }
                Err(failure) => results.push(failure_result(&name, &failure)),
            }
        }
    });

    let receipt = decisions::batch_receipt(
        settings.run.project_id,
        settings.lane,
        Some(&identity),
        settings.concurrency,
        names,
        &results,
        now_ms(),
    )
    .map_err(|error| HostFailure::new(CODE_RESULT_INVALID, error))?;
    write_new_private(
        &receipt_path,
        &serde_json::to_vec_pretty(&receipt).map_err(|error| staging_failed(error.to_string()))?,
    )?;
    Ok(BatchOutcome {
        receipt,
        receipt_path,
        results,
        runs,
        engine: identity,
    })
}

// ---------------------------------------------------------------------------
// The Rwanda administrative reference asset
// ---------------------------------------------------------------------------

/// The machine-shared geographic data root the desktop installs reference
/// assets under: the platform's local data directory, unless the pointer file
/// the Geographic Data settings page writes names another root.
pub fn shared_root() -> Result<PathBuf, String> {
    let default = platform_local_data_dir()
        .ok_or("the local data directory is unavailable")?
        .join(SHARED_ROOT_DIRECTORY);
    match fs::read(default.join(SHARED_ROOT_POINTER)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default),
        Err(error) => Err(format!(
            "could not read geographic data storage setting: {error}"
        )),
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "geographic data storage setting is invalid".to_string())?;
            let path = value["root"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| "geographic data storage setting has no root".to_string())?;
            if !path.is_absolute() {
                return Err("geographic data storage root is not absolute".into());
            }
            Ok(path)
        }
    }
}

fn platform_local_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(root));
        }
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(|home| PathBuf::from(home).join(".local").join("share"))
    }
}

/// Where the desktop installs the villages index for one dataset snapshot.
pub fn installed_admin_bounds_path(shared_root: &Path, semantic_sha256: &str) -> PathBuf {
    shared_root
        .join(REFERENCE_ASSET_ROOT)
        .join(ADMIN_BOUNDS_ASSET_ID)
        .join(semantic_sha256)
        .join(ADMIN_BOUNDS_FILE)
}

/// Prove one DSAB file is the villages index of the named dataset snapshot:
/// the magic and schema, the payload length, the source digest in its header
/// and the content digest over its payload. Returns the exact byte identity
/// the engine re-verifies.
pub fn verify_admin_bounds_asset(
    path: &Path,
    expected_semantic_sha256: &str,
) -> Result<AdminBoundsAsset, HostFailure> {
    let unavailable = |message: String| {
        HostFailure::new(CODE_ADMIN_BOUNDS_UNAVAILABLE, message)
            .with_detail(json!({ "path": path.display().to_string(), "semantic_sha256": expected_semantic_sha256 }))
    };
    if !decisions::exact_lower_sha256(expected_semantic_sha256) {
        return Err(unavailable(
            "reference asset semantic digest is invalid".into(),
        ));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| unavailable(format!("reference asset is not installed: {error}")))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(unavailable("reference asset is not a regular file".into()));
    }
    let size = metadata.len();
    if !(DSAB_HEADER_BYTES as u64..=MAX_DSAB_BYTES).contains(&size) {
        return Err(unavailable(
            "reference asset size is outside the DSAB limit".into(),
        ));
    }
    let mut file = File::open(path)
        .map_err(|error| unavailable(format!("could not open reference asset: {error}")))?;
    let mut header = [0_u8; DSAB_HEADER_BYTES];
    file.read_exact(&mut header)
        .map_err(|error| unavailable(format!("could not read reference asset header: {error}")))?;
    if &header[..8] != DSAB_MAGIC {
        return Err(unavailable("reference asset has invalid DSAB magic".into()));
    }
    if u16::from_le_bytes([header[8], header[9]]) != DSAB_SCHEMA_VERSION
        || u16::from_le_bytes([header[10], header[11]]) != 0
    {
        return Err(unavailable(
            "reference asset has an unsupported DSAB schema".into(),
        ));
    }
    let payload_size = u64::from_le_bytes(
        header[12..20]
            .try_into()
            .map_err(|_| unavailable("reference asset payload header is malformed".into()))?,
    );
    if payload_size != size - DSAB_HEADER_BYTES as u64 {
        return Err(unavailable(
            "reference asset has an invalid DSAB payload length".into(),
        ));
    }
    let source_sha256 = header[20..52]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if source_sha256 != expected_semantic_sha256 {
        return Err(unavailable(
            "reference asset does not match the required dataset snapshot".into(),
        ));
    }
    let expected_content: [u8; 32] = header[52..84]
        .try_into()
        .map_err(|_| unavailable("reference asset content header is malformed".into()))?;
    let mut content = Sha256::new();
    content.update(&header[20..52]);
    let mut file_digest = Sha256::new();
    file_digest.update(header);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| unavailable(format!("could not read reference asset: {error}")))?;
        if count == 0 {
            break;
        }
        content.update(&buffer[..count]);
        file_digest.update(&buffer[..count]);
    }
    let actual_content: [u8; 32] = content.finalize().into();
    if actual_content != expected_content {
        return Err(unavailable(
            "reference asset DSAB content digest mismatch".into(),
        ));
    }
    Ok(AdminBoundsAsset {
        path: path.to_path_buf(),
        size_bytes: size,
        sha256: format!("{:x}", file_digest.finalize()),
        semantic_sha256: expected_semantic_sha256.to_string(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    const SHEETS: &str =
        r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx"]}]}"#;

    pub(crate) fn receipt(country: &str) -> InputReceipt {
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: country.into(),
            sheets_json: SHEETS.into(),
            sheets_sha256: format!("{:x}", Sha256::digest(SHEETS.as_bytes())),
            reference_semantic_sha256: "7".repeat(64),
        }
    }

    fn inputs(name: &str) -> TransformerReportInputs {
        let mut layers = BTreeMap::new();
        layers.insert(
            "lv_poles".to_string(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"pole_number":"P1"},"geometry":{"type":"Point","coordinates":[30.06,-1.94]}}]}),
        );
        TransformerReportInputs {
            transformer: name.into(),
            server_version: 7,
            layers,
            content_digest: None,
            selection: None,
            print_context: None,
            sheet: None,
        }
    }

    fn build_info(source_sha: &str) -> Value {
        json!({
            "schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
            "package_version": "0.1.0", "source_sha": source_sha,
            "engine_version": format!("ds-network-reporter@0.1.0+{source_sha}"),
            "cargo_lock_sha256": "d".repeat(64), "features": [], "rustc": "rustc 1.95.0",
            "target": "x86_64-unknown-linux-gnu", "profile": "release",
            "schemas": ["ds-network-reporter.task-result/v1", "ds.engine-build/v1"],
            "build_manifest_sha256": "f".repeat(64)
        })
    }

    /// A fake engine that answers exactly like `ds-report` would: it reads the
    /// typed request, writes the artifacts it is asked for into `out_dir`, and
    /// writes its result document. Its behaviour is scripted per test.
    #[derive(Default)]
    pub(crate) struct FakeEngine {
        identities: Mutex<Vec<String>>,
        pub(crate) calls: AtomicUsize,
        concurrent: AtomicUsize,
        peak: AtomicUsize,
        /// What to do with a transformer: `partial`, `crash`, `tamper`,
        /// `extra`, `slow`; for a page, `not_svg`, `two_pages`, `oversized`.
        pub(crate) behaviour: Mutex<BTreeMap<String, &'static str>>,
        /// The media grant bytes the last request named, read from the
        /// staged path, or `None` when the request named no grant.
        pub(crate) media_grant_seen: Mutex<Option<Vec<u8>>>,
    }

    impl FakeEngine {
        pub(crate) fn release() -> Self {
            let engine = Self::default();
            engine.identities.lock().unwrap().push("a".repeat(40));
            engine
        }
    }

    /// One `svg__<id>` page as the engine writes it, its bytes hashed as they
    /// go so an oversized page never lives in memory.
    fn write_page(path: &Path, behaviour: &str) -> (u64, String) {
        let mut file = File::create(path).unwrap();
        let mut digest = Sha256::new();
        let mut total = 0_u64;
        let mut push = |bytes: &[u8]| {
            file.write_all(bytes).unwrap();
            digest.update(bytes);
            total += bytes.len() as u64;
            total
        };
        match behaviour {
            "not_svg" => push(b"%PDF-1.7 not a page"),
            "oversized" => {
                push(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><!--");
                let filler = [b'x'; 64 * 1024];
                while push(&filler) <= preview::MAX_PREVIEW_BYTES {}
                push(b"--></svg>")
            }
            _ => push(
                b"\n  <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 420 297\"><path d=\"M0 0h420v297H0z\"/></svg>",
            ),
        };
        (total, format!("{:x}", digest.finalize()))
    }

    impl ReportEngine for FakeEngine {
        fn build_info(&self) -> Result<Value, HostFailure> {
            let mut identities = self.identities.lock().unwrap();
            let current = identities[0].clone();
            if identities.len() > 1 {
                identities.remove(0);
            }
            Ok(build_info(&current))
        }

        fn export_transformer_report(
            &self,
            request: &Path,
            result: &Path,
        ) -> Result<EngineExit, HostFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let live = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(live, Ordering::SeqCst);
            let request: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
            let transformer = request["transformer"].as_str().unwrap().to_string();
            let behaviour = self
                .behaviour
                .lock()
                .unwrap()
                .get(&transformer)
                .copied()
                .unwrap_or("ok");
            if behaviour == "slow" {
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            // The engine consumes exactly the staged plain documents.
            let room: Value = serde_json::from_slice(
                &fs::read(request["transformer_document"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            assert!(
                room["layers"]["lv_poles"]["features"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 1
            );
            let config: Value = serde_json::from_slice(
                &fs::read(request["network_config"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            assert_eq!(
                config["sheets"]["project_settings"][0]["parameter"],
                "design_export_format"
            );
            assert_eq!(request["input_shape"], "plain_local");
            *self.media_grant_seen.lock().unwrap() = request["media_grant"]
                .as_str()
                .map(|path| fs::read(path).unwrap());
            let out_dir = PathBuf::from(request["out_dir"].as_str().unwrap());
            let country = request["country"].as_str().unwrap();
            let mut artifacts = Vec::new();
            let mut print_diagnostics = Vec::new();
            for format in request["formats"].as_array().unwrap() {
                let format = format.as_str().unwrap();
                if let Some(id) = format.strip_prefix("svg__") {
                    let filename = format!("{transformer}.{id}.svg");
                    let path = out_dir.join(&filename);
                    let (size, sha256) = write_page(&path, behaviour);
                    artifacts.push(json!({"format": format, "filename": filename, "path": path, "bytes": size, "sha256": sha256}));
                    if behaviour == "two_pages" {
                        let twin = format!("{transformer}.{id}.twin.svg");
                        let path = out_dir.join(&twin);
                        let (size, sha256) = write_page(&path, "ok");
                        artifacts.push(json!({"format": format, "filename": twin, "path": path, "bytes": size, "sha256": sha256}));
                    }
                    print_diagnostics.push(
                        json!({"output_id": format, "findings": [{"code": "label_overlap"}]}),
                    );
                    continue;
                }
                let filename = match format {
                    "shp" => format!("{transformer}.shp.zip"),
                    "xlsx" => format!("{transformer}.xlsx"),
                    other => panic!("{other}"),
                };
                let bytes = format!("{transformer}:{format}:bytes").into_bytes();
                let path = out_dir.join(&filename);
                fs::write(&path, &bytes).unwrap();
                let declared_sha = if behaviour == "tamper" && format == "xlsx" {
                    "0".repeat(64)
                } else {
                    format!("{:x}", Sha256::digest(&bytes))
                };
                artifacts.push(json!({"format": format, "filename": filename, "path": path, "bytes": bytes.len(), "sha256": declared_sha}));
            }
            if behaviour == "extra" {
                fs::write(out_dir.join("debug.log"), b"noise").unwrap();
            }
            if behaviour == "crash" {
                self.concurrent.fetch_sub(1, Ordering::SeqCst);
                return Ok(EngineExit {
                    succeeded: false,
                    summary: "segmentation fault".into(),
                });
            }
            let (status, blockers, exit) = if behaviour == "partial" {
                artifacts.pop();
                (
                    "partial",
                    json!([{"code": "format_build_failed", "message": "xlsx exceeded memory", "format": "xlsx"}]),
                    false,
                )
            } else {
                ("completed", json!([]), true)
            };
            let document = json!({
                "task": "export_transformer_report", "status": status, "transformer": transformer,
                "country": country, "layers_decoded": 1, "features_decoded": 1,
                "admin_bounds": {"configured": request.get("admin_bounds_asset").is_some(), "villages_loaded": if request.get("admin_bounds_asset").is_some() {14920} else {0}, "features_matched": 0, "features_requested": 0},
                "geometry_changed": false, "formats_requested": request["formats"],
                "artifacts": artifacts, "blockers": blockers, "elapsed_ms": 5,
                "print_diagnostics": print_diagnostics
            });
            fs::write(result, serde_json::to_vec(&document).unwrap()).unwrap();
            self.concurrent.fetch_sub(1, Ordering::SeqCst);
            Ok(EngineExit {
                succeeded: exit,
                summary: String::new(),
            })
        }
    }

    fn settings<'a>(
        receipt: &'a InputReceipt,
        root: &'a Path,
        staging: &'a Path,
    ) -> RunSettings<'a> {
        RunSettings {
            project_id: "proj-1",
            receipt,
            admin_bounds: None,
            staging_root: staging,
            out_root: root,
        }
    }

    #[test]
    fn one_run_places_verified_artifacts_beside_their_receipt_and_cleans_staging() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let outcome = run_transformer_report(
            &engine,
            &settings(&receipt, &out, &staging),
            &inputs("tx_a"),
        )
        .unwrap();
        assert_eq!(outcome.artifacts.len(), 2);
        assert!(out.join("tx_a").join("tx_a.shp.zip").is_file());
        assert!(out.join("tx_a").join("tx_a.xlsx").is_file());
        assert_eq!(
            fs::read(out.join("tx_a").join("tx_a.xlsx")).unwrap(),
            b"tx_a:xlsx:bytes"
        );
        let written: Value =
            serde_json::from_slice(&fs::read(&outcome.receipt_path).unwrap()).unwrap();
        assert_eq!(written["schema"], "ds.report-run/v1");
        assert_eq!(written["outputs"][1]["path"], "tx_a/tx_a.xlsx");
        assert_eq!(
            written["outputs"][1]["sha256"],
            format!("{:x}", Sha256::digest(b"tx_a:xlsx:bytes"))
        );
        assert_eq!(written["publication"]["state"], "pending");
        assert_eq!(written["transformer_revision"], 7);
        assert!(decisions::valid_run_id(
            written["client_run_id"].as_str().unwrap()
        ));
        // Staging is gone; only the placed report remains.
        assert!(fs::read_dir(&staging).unwrap().next().is_none());
        let mut names: Vec<_> = fs::read_dir(out.join("tx_a"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["report-run.json", "tx_a.shp.zip", "tx_a.xlsx"]);
        // A second run for the same transformer refuses to overwrite.
        let again = run_transformer_report(
            &engine,
            &settings(&receipt, &out, &staging),
            &inputs("tx_a"),
        )
        .unwrap_err();
        assert_eq!(again.code, CODE_OUTPUT_EXISTS);
    }

    #[test]
    fn a_media_grant_is_staged_beside_the_room_and_named_in_the_request() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let grant = br#"{"schema":"ds.media-grant/v1","bucket":"b","prefix":"p/"}"#;
        let outcome = run_transformer_report_with(
            &engine,
            &settings(&receipt, &out, &staging),
            &inputs("tx_a"),
            &RunExtras {
                media_grant: Some(&grant[..]),
            },
        )
        .unwrap();
        assert_eq!(outcome.artifacts.len(), 2);
        assert_eq!(
            engine.media_grant_seen.lock().unwrap().as_deref(),
            Some(&grant[..])
        );
        // The grant is staging, not delivery: it goes with the scratch.
        assert!(fs::read_dir(&staging).unwrap().next().is_none());
        let mut names: Vec<_> = fs::read_dir(out.join("tx_a"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["report-run.json", "tx_a.shp.zip", "tx_a.xlsx"]);
        // The plain wrapper names no grant.
        run_transformer_report(
            &engine,
            &settings(&receipt, &out, &staging),
            &inputs("tx_b"),
        )
        .unwrap();
        assert!(engine.media_grant_seen.lock().unwrap().is_none());
        // An empty or oversized grant is refused before the engine runs.
        let calls = engine.calls.load(Ordering::SeqCst);
        for grant in [Vec::new(), vec![b'{'; MAX_MEDIA_GRANT_BYTES as usize + 1]] {
            let refused = run_transformer_report_with(
                &engine,
                &settings(&receipt, &out, &staging),
                &inputs("tx_c"),
                &RunExtras {
                    media_grant: Some(grant.as_slice()),
                },
            )
            .unwrap_err();
            assert_eq!(refused.code, CODE_INPUTS_INVALID);
        }
        assert_eq!(engine.calls.load(Ordering::SeqCst), calls);
        assert!(!out.join("tx_c").exists());
    }

    #[test]
    fn interrupted_delivery_is_private_and_does_not_block_a_fresh_run() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        let abandoned = out.join(".report-interrupted.partial");
        fs::create_dir_all(&abandoned).unwrap();
        fs::write(abandoned.join("tx_a.xlsx"), b"partial bytes").unwrap();
        assert!(!out.join("tx_a").exists());
        let receipt = receipt("Benin");
        let run = run_transformer_report(
            &FakeEngine::release(),
            &settings(&receipt, &out, &temp.path().join("staging")),
            &inputs("tx_a"),
        )
        .unwrap();
        assert!(run.receipt_path.is_file());
        assert_eq!(
            fs::read(run.artifact_dir.join("tx_a.xlsx")).unwrap(),
            b"tx_a:xlsx:bytes"
        );
        // Never delete another process's scratch to recover our own run.
        assert!(abandoned.is_dir());
        let candidate = out.join("candidate");
        fs::create_dir(&candidate).unwrap();
        fs::write(candidate.join("report-run.json"), b"replacement").unwrap();
        assert_eq!(
            promote_report(&candidate, &run.artifact_dir)
                .unwrap_err()
                .code,
            CODE_OUTPUT_EXISTS
        );
        assert_eq!(
            fs::read(run.artifact_dir.join("tx_a.xlsx")).unwrap(),
            b"tx_a:xlsx:bytes"
        );
    }

    #[test]
    fn saved_content_mismatch_refuses_before_engine_or_staging() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        let receipt = receipt("Benin");
        let mut room = inputs("tx_a");
        room.content_digest = Some("f".repeat(64));
        let failure = run_transformer_report(
            &FakeEngine::release(),
            &settings(&receipt, &out, &temp.path().join("staging")),
            &room,
        )
        .unwrap_err();
        assert_eq!(failure.code, CODE_INPUTS_INVALID);
        assert!(!out.exists());
        assert!(!temp.path().join("staging").exists());
    }

    #[test]
    fn refusals_are_typed_and_leave_nothing_behind() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        {
            let mut behaviour = engine.behaviour.lock().unwrap();
            behaviour.insert("tx_partial".into(), "partial");
            behaviour.insert("tx_crash".into(), "crash");
            behaviour.insert("tx_tamper".into(), "tamper");
            behaviour.insert("tx_extra".into(), "extra");
        }
        let run = |name: &str| {
            run_transformer_report(&engine, &settings(&receipt, &out, &staging), &inputs(name))
                .unwrap_err()
        };
        let partial = run("tx_partial");
        assert_eq!(partial.code, CODE_EXPORT_BLOCKED);
        assert_eq!(
            partial.detail.as_ref().unwrap()["blockers"][0]["code"],
            "format_build_failed"
        );
        assert!(partial.message.contains("xlsx exceeded memory"));
        assert_eq!(run("tx_crash").code, CODE_ENGINE_REFUSED);
        assert_eq!(run("tx_tamper").code, CODE_RESULT_INVALID);
        assert!(run("tx_extra").message.contains("undeclared"));
        assert_eq!(run("collisions").code, CODE_INPUTS_INVALID);
        assert!(fs::read_dir(&staging).unwrap().next().is_none());
        assert!(!out.exists() || fs::read_dir(&out).unwrap().next().is_none());
        // Rwanda without the installed asset is refused before the engine runs.
        let rwanda = super::tests::receipt("Rwanda");
        let calls = engine.calls.load(Ordering::SeqCst);
        let refused =
            run_transformer_report(&engine, &settings(&rwanda, &out, &staging), &inputs("tx_a"))
                .unwrap_err();
        assert_eq!(refused.code, CODE_ADMIN_BOUNDS_UNAVAILABLE);
        assert_eq!(engine.calls.load(Ordering::SeqCst), calls);
    }

    #[test]
    fn an_engine_that_changes_identity_mid_run_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let receipt = receipt("Benin");
        let engine = FakeEngine::default();
        engine
            .identities
            .lock()
            .unwrap()
            .extend(["a".repeat(40), "b".repeat(40)]);
        let failure = run_transformer_report(
            &engine,
            &settings(
                &receipt,
                &temp.path().join("out"),
                &temp.path().join("staging"),
            ),
            &inputs("tx_a"),
        )
        .unwrap_err();
        assert_eq!(failure.code, CODE_ENGINE_IDENTITY_CHANGED);
        assert!(!temp.path().join("out").join("tx_a").exists());
    }

    #[test]
    fn a_batch_runs_under_its_concurrency_and_records_every_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        engine
            .behaviour
            .lock()
            .unwrap()
            .insert("tx_c".into(), "partial");
        for name in ["tx_a", "tx_b", "tx_d", "tx_e"] {
            engine.behaviour.lock().unwrap().insert(name.into(), "slow");
        }
        let names: Vec<String> = ["tx_a", "tx_b", "tx_c", "tx_d", "tx_e", "tx_missing"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let fetched = Mutex::new(Vec::new());
        let outcome = run_batch(
            &engine,
            &BatchSettings {
                run: settings(&receipt, &out, &staging),
                lane: "stable",
                concurrency: 2,
            },
            &names,
            |name| {
                fetched.lock().unwrap().push(name.to_string());
                if name == "tx_missing" {
                    Err(HostFailure::new(
                        "transformer_not_found",
                        "no such transformer",
                    ))
                } else {
                    Ok(inputs(name))
                }
            },
        )
        .unwrap();
        assert_eq!(fetched.into_inner().unwrap(), names);
        assert!(
            engine.peak.load(Ordering::SeqCst) <= 2,
            "peak {}",
            engine.peak.load(Ordering::SeqCst)
        );
        assert_eq!(engine.calls.load(Ordering::SeqCst), 5);
        assert_eq!(outcome.runs.len(), 4);
        assert_eq!(outcome.receipt["status"], "partial");
        assert_eq!(outcome.receipt["completed"], 4);
        assert_eq!(outcome.receipt["failed"], 2);
        assert_eq!(outcome.receipt["concurrency"], 2);
        let rows = outcome.receipt["results"].as_array().unwrap();
        let ordered: Vec<&str> = rows
            .iter()
            .map(|row| row["transformer"].as_str().unwrap())
            .collect();
        assert_eq!(
            ordered,
            ["tx_a", "tx_b", "tx_c", "tx_d", "tx_e", "tx_missing"]
        );
        assert_eq!(rows[2]["error"]["code"], "export_blocked");
        assert_eq!(rows[5]["error"]["code"], "transformer_not_found");
        assert_eq!(rows[0]["receipt"], "tx_a/report-run.json");
        assert!(out.join("report-batch.json").is_file());
        assert!(out.join("tx_e").join("report-run.json").is_file());
        assert!(!out.join("tx_c").exists());
        // The batch receipt is written once; a second batch needs a fresh root.
        let again = run_batch(
            &engine,
            &BatchSettings {
                run: settings(&receipt, &out, &staging),
                lane: "stable",
                concurrency: 1,
            },
            &names[..1],
            |name| Ok(inputs(name)),
        )
        .unwrap_err();
        assert_eq!(again.code, CODE_OUTPUT_EXISTS);
    }

    #[test]
    fn the_admin_bounds_asset_is_proven_by_its_header_and_content() {
        let temp = tempfile::tempdir().unwrap();
        let payload = b"villages".to_vec();
        let mut source = [0_u8; 32];
        for (index, byte) in source.iter_mut().enumerate() {
            *byte = (index as u8 % 16) * 0x11;
        }
        let semantic = source
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut content = Sha256::new();
        content.update(source);
        content.update(&payload);
        let mut header = Vec::new();
        header.extend_from_slice(DSAB_MAGIC);
        header.extend_from_slice(&1_u16.to_le_bytes());
        header.extend_from_slice(&0_u16.to_le_bytes());
        header.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        header.extend_from_slice(&source);
        header.extend_from_slice(&content.finalize());
        assert_eq!(header.len(), DSAB_HEADER_BYTES);
        let root = temp.path();
        let path = installed_admin_bounds_path(root, &semantic);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = header.clone();
        bytes.extend_from_slice(&payload);
        fs::write(&path, &bytes).unwrap();
        let asset = verify_admin_bounds_asset(&path, &semantic).unwrap();
        assert_eq!(asset.size_bytes, bytes.len() as u64);
        assert_eq!(asset.sha256, format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(asset.semantic_sha256, semantic);
        assert_eq!(
            verify_admin_bounds_asset(&path, &"8".repeat(64))
                .unwrap_err()
                .code,
            CODE_ADMIN_BOUNDS_UNAVAILABLE
        );
        let mut corrupt = bytes.clone();
        corrupt[DSAB_HEADER_BYTES] ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert!(verify_admin_bounds_asset(&path, &semantic)
            .unwrap_err()
            .message
            .contains("content digest"));
        assert!(
            verify_admin_bounds_asset(&root.join("absent.dsab"), &semantic)
                .unwrap_err()
                .message
                .contains("not installed")
        );
    }

    #[test]
    fn run_ids_are_unique_and_well_formed() {
        let first = random_run_id();
        let second = random_run_id();
        assert!(decisions::valid_run_id(&first));
        assert!(decisions::valid_run_id(&second));
        assert_ne!(first, second);
    }
}
