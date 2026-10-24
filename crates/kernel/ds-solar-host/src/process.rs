//! Native effects for fixed standalone Solar engine tasks.
//! No linked engine, acquisition, authorization or publication capability.

use crate::{
    cancel::CancelToken,
    files,
    output::{
        GeneratedDocument, MAX_OUTPUT_BYTES, MAX_OUTPUT_FILE_BYTES, MAX_OUTPUT_FILES, OutputFile,
    },
};
use ds_solar_contracts::{
    ArtifactEntry, EngineIdentity, PREPARED_CALCULATION_TASK_SCHEMA, PreparedCalculationRequest,
    PreparedCalculationTaskResult, digest::sha256_hex,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESULT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LOG_BYTES: usize = 64 * 1024;
const CALCULATION_DEADLINE: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessFailureKind {
    Unavailable,
    InvalidIdentity,
    Changed,
    Cancelled,
    Deadline,
    OutputBound,
    Refused,
    Io,
    Integrity,
}

#[derive(Debug, Clone)]
pub struct ProcessFailure {
    pub kind: ProcessFailureKind,
    pub message: String,
    /// Exact domain refusal from the fixed worker's bounded completion, when
    /// present. Hosts retain the owner code instead of parsing human messages.
    pub refusal: Option<ds_solar_contracts::SolarError>,
}
impl std::fmt::Display for ProcessFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for ProcessFailure {}
impl From<String> for ProcessFailure {
    fn from(message: String) -> Self {
        let kind = if message.starts_with("solar_engine_unavailable") {
            ProcessFailureKind::Unavailable
        } else if message.starts_with("solar_engine_identity")
            || message.starts_with("solar_engine_schema")
        {
            ProcessFailureKind::InvalidIdentity
        } else if message.starts_with("solar_engine_changed") {
            ProcessFailureKind::Changed
        } else if message.contains("cancelled") {
            ProcessFailureKind::Cancelled
        } else if message.contains("deadline") {
            ProcessFailureKind::Deadline
        } else if message.contains("bound_exceeded") || message.contains("too_large") {
            ProcessFailureKind::OutputBound
        } else if message.starts_with("solar_process_refused") {
            ProcessFailureKind::Refused
        } else if message.starts_with("solar_task_") {
            ProcessFailureKind::Integrity
        } else {
            ProcessFailureKind::Io
        };
        let refusal = if message.starts_with("solar_engine_update_required") {
            Some(ds_solar_contracts::SolarError::new(
                ds_solar_contracts::SolarErrorCode::SolarEngineUpdateRequired,
                message.clone(),
            ))
        } else if kind == ProcessFailureKind::Refused {
            message
                .split_once("; ")
                .and_then(|(_, completion)| {
                    serde_json::from_str::<serde_json::Value>(completion).ok()
                })
                .and_then(|completion| completion.get("error").cloned())
                .and_then(|error| serde_json::from_value(error).ok())
        } else {
            None
        };
        Self {
            kind,
            message,
            refusal,
        }
    }
}

#[derive(PartialEq, Eq)]
struct ExecutableStamp {
    length: u64,
    modified: std::time::SystemTime,
    sha256: [u8; 32],
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    device: u64,
}
impl ExecutableStamp {
    fn from_metadata(metadata: &fs::Metadata, sha256: [u8; 32]) -> Result<Self, String> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().map_err(|e| e.to_string())?,
            sha256,
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            device: metadata.dev(),
        })
    }
}
fn executable_stamp(path: &Path) -> Result<ExecutableStamp, String> {
    files::ordinary_file(path).map_err(|e| e.to_string())?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("solar_engine_executable_not_regular".into());
    }
    if metadata.len() > MAX_EXECUTABLE_BYTES {
        return Err("solar_engine_executable_too_large".into());
    }
    // Hash an ordinary bounded file without allocating the executable size.
    // Metadata alone cannot detect same-size edits with a restored timestamp.
    let mut digest = Sha256::new();
    let mut length = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    let mut reader = (&mut file).take(MAX_EXECUTABLE_BYTES + 1);
    loop {
        let count = reader.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        length += count as u64;
        digest.update(&chunk[..count]);
    }
    let sha256 = digest.finalize().into();
    let stamp = ExecutableStamp::from_metadata(&metadata, sha256)?;
    files::ordinary_file(path).map_err(|e| e.to_string())?;
    if length != stamp.length
        || ExecutableStamp::from_metadata(&file.metadata().map_err(|e| e.to_string())?, sha256)?
            != stamp
        || ExecutableStamp::from_metadata(&fs::metadata(path).map_err(|e| e.to_string())?, sha256)?
            != stamp
    {
        return Err("solar_engine_changed_during_job".into());
    }
    Ok(stamp)
}
fn verify_executable(path: &Path, stamp: &ExecutableStamp) -> Result<(), String> {
    if executable_stamp(path).as_ref() != Ok(stamp) {
        return Err("solar_engine_changed_during_job".into());
    }
    Ok(())
}

/// Explicit override or the packaged sibling. Never search PATH or source trees.
pub fn installed_engine_path() -> Result<PathBuf, String> {
    let path = if let Some(path) = std::env::var_os("DS_SOLAR_ENGINE_BIN") {
        PathBuf::from(path)
    } else {
        std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(if cfg!(windows) { "solar.exe" } else { "solar" })
    };
    if !path.is_absolute() {
        return Err("solar_engine_path_must_be_absolute".into());
    }
    fs::canonicalize(path).map_err(|e| format!("solar_engine_unavailable: {e}"))
}

/// Read the actual addon identity without a calculation or writable install path.
pub fn installed_engine_identity() -> Result<EngineIdentity, ProcessFailure> {
    let path = installed_engine_path().map_err(ProcessFailure::from)?;
    selected_engine_identity(&path)
}

/// Probe the immutable program selected by the native host. This never resolves
/// a lane, environment override or a new active installation itself.
pub fn selected_engine_identity(path: &Path) -> Result<EngineIdentity, ProcessFailure> {
    engine_identity(path, &CancelToken::new()).map_err(ProcessFailure::from)
}

fn engine_identity(path: &Path, cancel: &CancelToken) -> Result<EngineIdentity, String> {
    Ok(engine_probe(path, cancel)?.identity)
}
fn engine_probe(
    path: &Path,
    cancel: &CancelToken,
) -> Result<ds_solar_contracts::engine_probe::EngineBuildProbe, String> {
    let stamp = executable_stamp(path)?;
    let stdout = run_task(
        path,
        path.parent().ok_or("solar_engine_path_invalid")?,
        1,
        Task::BuildInfo,
        cancel,
        Duration::from_secs(10),
    );
    verify_executable(path, &stamp)?;
    let stdout = stdout?;
    let probe: ds_solar_contracts::engine_probe::EngineBuildProbe = serde_json::from_slice(&stdout)
        .map_err(|e| format!("solar_engine_identity_invalid: {e}"))?;
    if probe.identity.name != "ds-solar-engine" || probe.identity.version.is_empty() {
        return Err("solar_engine_identity_invalid".into());
    }
    Ok(probe)
}

/// One immutable executable capture and one calculation, owned by the job lease.
/// The private directory also holds all bounded transient worker output.
pub struct PreparedEngine {
    directory: tempfile::TempDir,
    executable: PathBuf,
    _executable_file: File,
    stamp: ExecutableStamp,
    identity: EngineIdentity,
    workers: usize,
    completion_build_manifest_sha256: String,
}

mod batch;
mod budget;
mod encoding;
pub mod final_lint;
mod portfolio;
pub(crate) mod preparation;
pub use batch::{
    BatchEngine, BatchProgressSink, MAX_BATCH_TIMEOUT, VerifiedBatch, VerifiedBatchFile,
};
pub use portfolio::{PortfolioEngine, VerifiedPortfolio, VerifiedPortfolioFile};

impl PreparedEngine {
    pub fn capture(
        engine: &Path,
        scratch: &Path,
        workers: usize,
        cancel: &CancelToken,
    ) -> Result<Self, ProcessFailure> {
        Self::capture_inner(engine, scratch, workers, cancel).map_err(ProcessFailure::from)
    }

    fn capture_inner(
        engine: &Path,
        scratch: &Path,
        workers: usize,
        cancel: &CancelToken,
    ) -> Result<Self, String> {
        let captured = Self::capture_transport(engine, scratch, workers, cancel)?;
        validate_identity(&captured.identity)?;
        Ok(captured)
    }

    fn capture_transport(
        engine: &Path,
        scratch: &Path,
        workers: usize,
        cancel: &CancelToken,
    ) -> Result<Self, String> {
        check_cancel(cancel)?;
        let engine =
            fs::canonicalize(engine).map_err(|e| format!("solar_engine_unavailable: {e}"))?;
        files::ordinary_file(&engine).map_err(|e| e.to_string())?;
        let scratch = fs::canonicalize(scratch)
            .map_err(|e| format!("solar_engine_scratch_unavailable: {e}"))?;
        let directory = tempfile::Builder::new()
            .prefix("solar-process-")
            .tempdir_in(&scratch)
            .map_err(|e| format!("solar_engine_scratch_unavailable: {e}"))?;
        let executable = engine;
        let executable_file = File::open(&executable).map_err(|e| e.to_string())?;
        let stamp = executable_stamp(&executable)?;
        let workers = workers.max(1).min(crate::compute_parallelism());
        let probe = engine_probe(&executable, cancel)?;
        verify_executable(&executable, &stamp)?;
        Ok(Self {
            directory,
            executable,
            _executable_file: executable_file,
            stamp,
            identity: probe.identity,
            completion_build_manifest_sha256: probe.completion_build_manifest_sha256,
            workers,
        })
    }

    pub fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    /// Consumes the captured worker so one admitted job executes it once.
    pub fn calculate(
        self,
        request: &PreparedCalculationRequest,
        cancel: &CancelToken,
    ) -> Result<VerifiedCalculation, ProcessFailure> {
        self.calculate_inner(request, cancel)
            .map_err(ProcessFailure::from)
    }

    fn calculate_inner(
        self,
        request: &PreparedCalculationRequest,
        cancel: &CancelToken,
    ) -> Result<VerifiedCalculation, String> {
        request.prepared.validate().map_err(|e| e.to_string())?;
        check_cancel(cancel)?;
        let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("solar_prepared_request_too_large".into());
        }
        let input = self.directory.path().join("request.json");
        files::write(&input, &bytes).map_err(|e| e.to_string())?;
        let out = self.directory.path().join("out");
        let _budget = budget::acquire(self.workers, cancel)?;
        verify_executable(&self.executable, &self.stamp)?;
        let result = run_task(
            &self.executable,
            self.directory.path(),
            _budget.workers(),
            Task::Calculate {
                request: &input,
                out: &out,
            },
            cancel,
            CALCULATION_DEADLINE,
        );
        verify_executable(&self.executable, &self.stamp)?;
        result?;
        check_cancel(cancel)?;
        ordinary_directory(&out)?;
        let result = files::read(&out.join("task-result.json"), MAX_RESULT_BYTES)
            .map_err(|e| format!("solar_task_result_invalid: {e}"))?;
        let task: PreparedCalculationTaskResult = serde_json::from_slice(&result)
            .map_err(|e| format!("solar_task_result_invalid: {e}"))?;
        let verified = verify_task(task, request, &bytes, &self.identity, &out, cancel)?;
        let outputs = encoding::encode(
            &self.executable,
            self.directory.path(),
            &verified.outputs,
            cancel,
        )?;
        Ok(VerifiedCalculation {
            result: verified.result,
            timings: verified.timings,
            plots_generated: verified.plots_generated,
            outputs,
        })
    }
}

pub struct VerifiedCalculation {
    pub result: ds_solar_contracts::SolarCalculationResult,
    pub timings: ds_solar_contracts::CalculationTimings,
    pub plots_generated: bool,
    pub outputs: Vec<OutputFile>,
}

fn validate_identity(identity: &EngineIdentity) -> Result<(), String> {
    if identity.name != "ds-solar-engine"
        || identity.version.is_empty()
        || !identity
            .schemas
            .iter()
            .any(|s| s == PREPARED_CALCULATION_TASK_SCHEMA)
        || !identity.schemas.iter().any(|s| s == "ds-solar.result/v1")
        || !identity
            .schemas
            .iter()
            .any(|s| s == "ds-solar.prepared-city-input/v1")
        || !identity
            .schemas
            .iter()
            .any(|s| s == "ds-solar.worker-budget/v1")
    {
        return Err("solar_engine_schema_incompatible".into());
    }
    Ok(())
}

struct VerifiedArtifacts {
    result: ds_solar_contracts::SolarCalculationResult,
    timings: ds_solar_contracts::CalculationTimings,
    plots_generated: bool,
    outputs: Vec<GeneratedDocument>,
}

fn verify_task(
    task: PreparedCalculationTaskResult,
    request: &PreparedCalculationRequest,
    request_bytes: &[u8],
    identity: &EngineIdentity,
    out: &Path,
    cancel: &CancelToken,
) -> Result<VerifiedArtifacts, String> {
    check_cancel(cancel)?;
    let mut result_identity = task.result.engine.clone();
    // Development semantic results omit schemas; admission always stamps them.
    if identity.schema.is_empty() && result_identity.schemas.is_empty() {
        result_identity.schemas = identity.schemas.clone();
    }
    if task.schema_version != PREPARED_CALCULATION_TASK_SCHEMA
        || task.engine != *identity
        || result_identity != *identity
        || task.request_sha256 != sha256_hex(request_bytes)
        || task.prepared_input_digest != request.prepared.input_digest
        || task.result.identity != request.prepared.city.identity
        || task.result.input_digest != request.prepared.input_digest
        || task.result.weather_digest != request.prepared.weather.content_digest
        || task.result.run_id != request.run_id
        || task.plots_generated != request.render_charts
        || task.result.artifacts.schema_version
            != ds_solar_contracts::version::MANIFEST_SCHEMA_VERSION
    {
        return Err("solar_task_identity_or_request_mismatch".into());
    }
    task.result
        .validate()
        .map_err(|e| format!("solar_task_result_digest_invalid: {e}"))?;
    if task.outputs.len() > MAX_OUTPUT_FILES {
        return Err("solar_task_output_bound_exceeded".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0usize;
    let mut artifacts = Vec::new();
    for entry in &task.outputs {
        check_cancel(cancel)?;
        if entry.name.is_empty()
            || entry.name.len() > 512
            || entry.name.contains(['\\', ':'])
            || entry
                .name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !Path::new(&entry.name)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || !names.insert(entry.name.clone())
            || entry.byte_count > MAX_OUTPUT_FILE_BYTES as u64
        {
            return Err("solar_task_artifact_descriptor_invalid".into());
        }
        // Every ancestor is a private ordinary directory, never a worker link.
        let mut path = out.join("artifacts");
        ordinary_directory(&path)?;
        let parts: Vec<_> = Path::new(&entry.name).components().collect();
        for part in &parts[..parts.len() - 1] {
            path.push(part.as_os_str());
            ordinary_directory(&path)?;
        }
        path.push(parts.last().expect("nonempty name").as_os_str());
        let bytes = files::read(&path, MAX_OUTPUT_FILE_BYTES as u64).map_err(|e| e.to_string())?;
        total = total
            .checked_add(bytes.len())
            .ok_or("solar_task_output_bound_exceeded")?;
        if total > MAX_OUTPUT_BYTES
            || ArtifactEntry::from_bytes(&entry.name, &entry.media_type, &bytes) != *entry
        {
            return Err("solar_task_artifact_digest_or_size_mismatch".into());
        }
        artifacts.push(GeneratedDocument {
            entry: entry.clone(),
            bytes,
        });
    }
    // The result manifest must describe the same closed artifacts.
    let mut manifested = task
        .outputs
        .iter()
        .filter(|e| e.name != "report_input.json")
        .cloned()
        .collect::<Vec<_>>();
    manifested.sort_by(|a, b| a.name.cmp(&b.name));
    if !names.contains("report_input.json") || task.result.artifacts.artifacts != manifested {
        return Err("solar_task_artifact_manifest_mismatch".into());
    }
    check_cancel(cancel)?;
    Ok(VerifiedArtifacts {
        result: task.result,
        timings: task.timings,
        plots_generated: task.plots_generated,
        outputs: artifacts,
    })
}

fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("solar_task_output_directory_invalid".into());
    }
    Ok(())
}

fn check_cancel(cancel: &CancelToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("solar_process_cancelled".into())
    } else {
        Ok(())
    }
}

enum Task<'a> {
    BuildInfo,
    FinalLint { request: &'a Path, out: &'a Path },
    Preparation { request: &'a Path, out: &'a Path },
    Encoding { request: &'a Path, out: &'a Path },
    Calculate { request: &'a Path, out: &'a Path },
    Batch { request: &'a Path, out: &'a Path },
    Portfolio { request: &'a Path, out: &'a Path },
}

struct ReapedChild {
    child: Child,
    cleaned: bool,
}
impl ReapedChild {
    fn kill_and_wait(&mut self) {
        if self.cleaned {
            return;
        }
        #[cfg(unix)]
        unsafe {
            // The child owns a new process group. Kill any worker descendants too.
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.cleaned = true;
    }
}
impl Drop for ReapedChild {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

fn run_task(
    executable: &Path,
    root: &Path,
    workers: usize,
    task: Task<'_>,
    cancel: &CancelToken,
    deadline: Duration,
) -> Result<Vec<u8>, String> {
    run_task_observed(executable, root, workers, task, cancel, deadline, None)
}

#[allow(clippy::too_many_arguments)]
fn run_task_observed(
    executable: &Path,
    root: &Path,
    workers: usize,
    task: Task<'_>,
    cancel: &CancelToken,
    deadline: Duration,
    progress: Option<&batch::ProgressContext<'_>>,
) -> Result<Vec<u8>, String> {
    check_cancel(cancel)?;
    let bounds = match &task {
        Task::BuildInfo => None,
        Task::FinalLint { .. } => Some(FileBounds::final_lint()),
        Task::Preparation { .. } => Some(FileBounds::preparation()),
        Task::Encoding { .. } => Some(FileBounds::encoding()),
        Task::Calculate { .. } => Some(FileBounds::prepared()),
        Task::Batch { .. } => Some(FileBounds::batch()),
        Task::Portfolio { .. } => Some(FileBounds::portfolio()),
    };
    let mut command = Command::new(executable);
    match task {
        Task::BuildInfo => {
            command.arg("build-info");
        }
        Task::FinalLint { request, out } => {
            command
                .arg(ds_cli_exec::solar::Task::FinalLint.subcommand())
                .arg("--request")
                .arg(request)
                .arg("--out")
                .arg(out);
        }
        Task::Preparation { request, out } => {
            command
                .arg(ds_cli_exec::solar::Task::PrepareReference.subcommand())
                .arg("--request")
                .arg(request)
                .arg("--out")
                .arg(out);
        }
        Task::Encoding { request, out } => {
            command
                .arg(ds_cli_exec::solar::Task::EncodeOutputs.subcommand())
                .arg("--request")
                .arg(request)
                .arg("--out")
                .arg(out);
        }
        Task::Calculate { request, out } => {
            command
                .arg("calculate-prepared")
                .arg("--request")
                .arg(request)
                .arg("--out")
                .arg(out);
        }
        Task::Batch { request, out } => {
            command
                .arg("calculate-batch")
                .arg("--request")
                .arg(request)
                .arg("--out")
                .arg(out)
                .arg("--progress-jsonl");
        }
        Task::Portfolio { request, out } => {
            command
                .arg("calculate-portfolio")
                .arg("--request")
                .arg(request)
                .arg("--out")
                .arg(out);
        }
    }
    command
        .current_dir(root)
        .env_clear()
        .env("DS_SOLAR_WORKER_THREADS", workers.to_string())
        .env("RAYON_NUM_THREADS", workers.to_string())
        .env("OMP_NUM_THREADS", workers.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setpgid is async-signal-safe and touches no Rust state.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    let mut child = ReapedChild {
        child: command
            .spawn()
            .map_err(|e| format!("solar_engine_start_failed: {e}"))?,
        cleaned: false,
    };
    thread::scope(|scope| {
        let overflow = Arc::new(AtomicBool::new(false));
        let pipe_failure = Arc::new(Mutex::new(None));
        let stdout_stream = child.child.stdout.take().expect("piped stdout");
        let stderr_stream = child.child.stderr.take().expect("piped stderr");
        let stdout_overflow = overflow.clone();
        let stdout_failure = pipe_failure.clone();
        let stdout = scope.spawn(move || {
            let result = if let Some(context) = progress {
                batch::read_progress(stdout_stream, context, cancel)
            } else {
                read_log(stdout_stream, stdout_overflow)
            };
            if let Err(error) = &result {
                *stdout_failure
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = Some(error.clone());
            }
            result
        });
        let stderr_overflow = overflow.clone();
        let stderr = scope.spawn(move || read_log(stderr_stream, stderr_overflow));
        let started = Instant::now();
        let status = loop {
            let refusal = if cancel.is_cancelled() {
                Some("solar_process_cancelled".to_string())
            } else if started.elapsed() >= deadline {
                Some("solar_process_deadline_exceeded".to_string())
            } else if overflow.load(Ordering::Acquire) {
                Some("solar_process_log_bound_exceeded".to_string())
            } else {
                pipe_failure
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clone()
            };
            if let Some(refusal) = refusal {
                child.kill_and_wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(refusal);
            }
            if let Some(bounds) = &bounds
                && let Err(error) = check_output_bound_with(root, bounds)
            {
                child.kill_and_wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(error);
            }
            match child.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    child.kill_and_wait();
                    let _ = stdout.join();
                    let _ = stderr.join();
                    return Err(error.to_string());
                }
            }
        };
        // Reap/kill before joining drains, including descendants retaining pipes.
        child.kill_and_wait();
        let stdout = stdout
            .join()
            .map_err(|_| "solar_process_stdout_reader_failed")??;
        let _stderr = stderr
            .join()
            .map_err(|_| "solar_process_stderr_reader_failed")??;
        check_cancel(cancel)?;
        if overflow.load(Ordering::Acquire) {
            return Err("solar_process_log_bound_exceeded".into());
        }
        if !status.success() {
            return Err(format!(
                "solar_process_refused: {status}; {}",
                String::from_utf8_lossy(&stdout)
            ));
        }
        if let Some(bounds) = &bounds {
            check_output_bound_with(root, bounds)?;
        }
        Ok(stdout)
    })
}

fn read_log(mut stream: impl Read, overflow: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    let mut kept = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(kept);
        }
        let retain = n.min(MAX_LOG_BYTES.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..retain]);
        if retain < n {
            overflow.store(true, Ordering::Release);
        }
    }
}

struct FileBounds {
    request: u64,
    result: u64,
    output: u64,
    files: usize,
    file: u64,
    depth: usize,
}
impl FileBounds {
    fn final_lint() -> Self {
        use ds_solar_contracts::final_lint::*;
        Self {
            request: MAX_FINAL_LINT_REQUEST_BYTES,
            result: MAX_FINAL_LINT_RESULT_BYTES,
            output: 0,
            files: 3,
            file: 0,
            depth: 2,
        }
    }
    fn preparation() -> Self {
        use ds_solar_contracts::preparation_task::*;
        Self {
            request: MAX_REFERENCE_PREPARATION_REQUEST_BYTES,
            result: MAX_REFERENCE_PREPARATION_RESULT_BYTES,
            output: 64 * 1024 * 1024,
            files: 12,
            file: 16 * 1024 * 1024,
            depth: 4,
        }
    }
    fn encoding() -> Self {
        use ds_solar_contracts::output_task::*;
        Self {
            request: OUTPUT_ENCODING_MAX_REQUEST_BYTES,
            result: OUTPUT_ENCODING_MAX_RESULT_BYTES,
            output: MAX_OUTPUT_BYTES as u64,
            files: MAX_OUTPUT_FILES * 17 + 16,
            file: MAX_OUTPUT_FILE_BYTES as u64,
            depth: 19,
        }
    }
    fn prepared() -> Self {
        Self {
            request: MAX_REQUEST_BYTES as u64,
            result: MAX_RESULT_BYTES,
            output: MAX_OUTPUT_BYTES as u64,
            files: 2048,
            file: MAX_OUTPUT_FILE_BYTES as u64,
            depth: 16,
        }
    }
    fn batch() -> Self {
        use ds_solar_contracts::prepared_batch::*;
        Self {
            request: BATCH_MAX_REQUEST_BYTES,
            result: BATCH_MAX_RESULT_BYTES as u64,
            output: BATCH_MAX_OUTPUT_BYTES,
            files: BATCH_MAX_OUTPUT_FILES * 2 + 16,
            file: BATCH_MAX_FILE_BYTES,
            depth: BATCH_MAX_PATH_DEPTH + 3,
        }
    }
    fn portfolio() -> Self {
        use ds_solar_contracts::portfolio_task::*;
        Self {
            request: PORTFOLIO_TASK_MAX_REQUEST_BYTES,
            result: PORTFOLIO_TASK_MAX_RESULT_BYTES as u64,
            output: PORTFOLIO_TASK_MAX_OUTPUT_BYTES,
            files: PORTFOLIO_TASK_MAX_OUTPUT_FILES * 17 + 16,
            file: PORTFOLIO_TASK_MAX_FILE_BYTES,
            depth: 19,
        }
    }
}
fn check_output_bound_with(root: &Path, bounds: &FileBounds) -> Result<(), String> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut count = 0usize;
    let mut total = 0u64;
    while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            count += 1;
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            if depth == 0 && entry.file_name() == "request.json" && metadata.len() > bounds.request
            {
                return Err("solar_process_output_bound_exceeded".into());
            }
            if count > bounds.files || depth >= bounds.depth || metadata.file_type().is_symlink() {
                return Err("solar_process_output_bound_exceeded".into());
            }
            if metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if metadata.is_file() {
                let cap = if depth == 0 && entry.file_name() == "request.json" {
                    bounds.request
                } else if entry.file_name() == "task-result.json" {
                    bounds.result
                } else {
                    bounds.file
                };
                if metadata.len() > cap {
                    return Err("solar_process_output_bound_exceeded".into());
                }
                total = total.saturating_add(metadata.len());
            } else {
                return Err("solar_process_output_nonregular".into());
            }
            if total > bounds.output + bounds.result + bounds.request {
                return Err("solar_process_output_bound_exceeded".into());
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
