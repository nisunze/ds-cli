//! Detached native host adapter; no Network computation is linked here.
mod captured;
mod design_snapshot;
mod grid;
mod grid_catalog;
mod profile;
mod report_engineering;
pub use captured::{CapturedNetworkClient, NetworkEnginePin};
pub use design_snapshot::{
    design_snapshot_engine_pin, process_design_snapshot, DesignSnapshotEnginePin,
    DesignSnapshotOutcome,
};
use ds_cli_exec::{addons::NETWORK, CapturedExternal};
use ds_network_contracts::{
    engine::{
        valid_hex, EngineIdentity, ExecutionEnvironment, ExecutionRuntime, NetworkBuildInfo,
        NetworkCapability, NetworkReceiptV1, NetworkTaskOutcome, NetworkTaskRefusalV1,
    },
    lv_processing::{
        decode_native_lv_request, NativeLvError, NativeLvRequestV1, NativeLvResultV1,
        MAX_NATIVE_LV_INPUT_BYTES, MAX_NATIVE_LV_OUTPUT_BYTES,
    },
};
pub use grid::GridOutcome;
pub use profile::{ProfileCheckpoint, ProfileOutcome, ProfileSession};
pub use report_engineering::ReportEngineeringOutcome;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
const MAX_IDENTITY_BYTES: usize = 64 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
const TASK_TIMEOUT: Duration = Duration::from_secs(3600);
#[derive(Debug)]
pub struct NetworkFailure {
    pub code: &'static str,
    pub message: String,
    pub owner_error: Option<Box<NativeLvError>>,
    pub owner_code: Option<String>,
    pub owner_detail: Option<Box<serde_json::Value>>,
    pub owner_remedy: Option<Box<str>>,
    pub owner_invalid_input: Option<bool>,
}
impl NetworkFailure {
    pub fn code(&self) -> &'static str {
        self.code
    }
    pub fn owner_error(&self) -> Option<&NativeLvError> {
        self.owner_error.as_deref()
    }
    pub fn owner_code(&self) -> Option<&str> {
        self.owner_code.as_deref()
    }
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            owner_error: None,
            owner_code: None,
            owner_detail: None,
            owner_remedy: None,
            owner_invalid_input: None,
        }
    }
    fn native(error: NativeLvError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
            owner_error: Some(Box::new(error)),
            owner_code: None,
            owner_detail: None,
            owner_remedy: None,
            owner_invalid_input: None,
        }
    }
}
impl fmt::Display for NetworkFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for NetworkFailure {}
#[derive(Debug)]
pub struct LvOutcome {
    pub bytes: Vec<u8>,
    pub result: NativeLvResultV1,
    pub runtime: ExecutionRuntime,
    pub identity: EngineIdentity,
    pub executable_sha256: String,
    pub cancelled: bool,
    pub development_override: bool,
}
/// Discovery reads metadata only. Execution separately admits the candidate.
pub fn available() -> ds_cli_contract::spec::Availability {
    NETWORK.availability()
}
/// Original standalone sorted-map wire profile.
pub fn process_lv(
    input: &[u8],
    workers: usize,
    scratch: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<LvOutcome, NetworkFailure> {
    process_lv_with_order(
        input,
        workers,
        scratch,
        cancel,
        ds_network_contracts::engine::JsonObjectOrder::Sorted,
    )
}
/// The caller pins its captured legacy wire profile; no repair or fallback.
pub fn process_lv_with_order(
    input: &[u8],
    workers: usize,
    scratch: &Path,
    cancel: &dyn Fn() -> bool,
    object_order: ds_network_contracts::engine::JsonObjectOrder,
) -> Result<LvOutcome, NetworkFailure> {
    let request = decode_native_lv_request(input).map_err(NetworkFailure::native)?;
    let captured = CapturedNetworkClient::capture(workers, cancel)?;
    process_lv_captured(&captured, input, request, scratch, cancel, object_order)
}

fn process_lv_captured(
    captured: &CapturedNetworkClient,
    input: &[u8],
    request: NativeLvRequestV1,
    scratch: &Path,
    cancel: &dyn Fn() -> bool,
    object_order: ds_network_contracts::engine::JsonObjectOrder,
) -> Result<LvOutcome, NetworkFailure> {
    let expected_result_schema =
        ds_network_contracts::lv_processing::native_lv_result_schema_for_request(&request.schema)
            .map_err(NetworkFailure::native)?;
    let engine = &captured.engine;
    let identity = &captured.identity;
    let executable_sha256 = &captured.executable_sha256;
    let development_override = captured.development_override;
    if identity.json_object_order != object_order {
        return Err(NetworkFailure::new(
            "network_serialization_incompatible",
            "the captured engine JSON-map profile differs from the consumer's pinned byte contract",
        ));
    }
    let width = identity.runtime.internal_worker_threads;
    let input_sha256 = digest_bytes(input);
    let sm = fs::symlink_metadata(scratch)
        .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
    if !sm.is_dir() || sm.file_type().is_symlink() {
        return Err(NetworkFailure::new(
            "network_scratch_invalid",
            "scratch must be an ordinary directory",
        ));
    }
    let run = tempfile::Builder::new()
        .prefix("ds-network-run-")
        .tempdir_in(scratch)
        .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700))
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
    }
    let request_path = run.path().join("request.json");
    write_request(&request_path, input)?;
    let output = run.path().join("output");
    create_output(&output)?;
    let args: Vec<OsString> = vec![
        "--request".into(),
        request_path.as_os_str().to_owned(),
        "--out".into(),
        output.as_os_str().to_owned(),
        "--workers".into(),
        width.to_string().into(),
    ];
    let completed = engine
        .call_cancelled("lv-process", &args, TASK_TIMEOUT, width, cancel)
        .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
    if completed.truncated || completed.stdout.len() > MAX_IDENTITY_BYTES {
        return Err(NetworkFailure::new(
            "network_receipt_bounded",
            "task exceeded its receipt output bound",
        ));
    }
    if !completed.succeeded() {
        return Err(read_refusal(&completed.stdout, identity)?);
    }
    let receipt: NetworkReceiptV1 = serde_json::from_str(&completed.stdout)
        .map_err(|e| NetworkFailure::new("network_receipt_invalid", e.to_string()))?;
    let cancelled = validate_receipt(&receipt, identity, &input_sha256, expected_result_schema)?;
    let om = fs::symlink_metadata(&output)
        .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
    if !om.is_dir() || om.file_type().is_symlink() {
        return Err(NetworkFailure::new(
            "network_result_invalid",
            "result directory changed",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if om.file_attributes() & 0x400 != 0 {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "result directory became a reparse point",
            ));
        }
    }
    let bytes = read_regular(
        &output.join("result.json"),
        MAX_NATIVE_LV_OUTPUT_BYTES,
        "network_result_invalid",
    )?;
    if bytes.len() != receipt.artifact.byte_count || digest_bytes(&bytes) != receipt.artifact.sha256
    {
        return Err(NetworkFailure::new(
            "network_result_digest_mismatch",
            "result byte count or digest disagrees with receipt",
        ));
    }
    let result: NativeLvResultV1 = serde_json::from_slice(&bytes)
        .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
    validate_result(&result, &request, identity)?;
    if digest_file(
        &request_path,
        MAX_NATIVE_LV_INPUT_BYTES as u64,
        "network_input_changed",
    )? != input_sha256
    {
        return Err(NetworkFailure::new(
            "network_input_changed",
            "typed request changed during task",
        ));
    }
    if digest_file(
        engine.executable(),
        MAX_EXECUTABLE_BYTES,
        "network_engine_changed",
    )? != *executable_sha256
    {
        return Err(NetworkFailure::new(
            "network_engine_changed",
            "captured executable changed during task",
        ));
    }
    // Completion after a ignored cooperative cancel cannot be imported as success.
    if cancel() && !cancelled {
        return Err(NetworkFailure::new(
            "callee_cancelled",
            "the native task completed after its host cancelled it",
        ));
    }
    Ok(LvOutcome {
        bytes,
        result,
        runtime: identity.runtime,
        identity: identity.clone(),
        executable_sha256: executable_sha256.clone(),
        cancelled,
        development_override,
    })
}
/// Intentional inspection probes the captured owner, never the host Rayon pool.
pub fn engine_identity(workers: usize) -> Result<EngineIdentity, NetworkFailure> {
    capture_identity(workers, &|| false).map(|(_, identity, _, _)| identity)
}
fn capture_identity(
    workers: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<(CapturedExternal, EngineIdentity, String, bool), NetworkFailure> {
    if workers == 0 {
        return Err(NetworkFailure::new(
            "network_workers_invalid",
            "worker budget must be positive",
        ));
    }
    let width = workers
        .min(std::thread::available_parallelism().map_or(1, usize::from))
        .max(1);
    let engine = NETWORK
        .capture()
        .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
    let development_override = admit_candidate(&engine)?;
    capture_resolved_identity(engine, width, development_override, cancel)
}

fn capture_resolved_identity(
    engine: CapturedExternal,
    width: usize,
    development_override: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<(CapturedExternal, EngineIdentity, String, bool), NetworkFailure> {
    let executable_sha256 = digest_file(
        engine.executable(),
        MAX_EXECUTABLE_BYTES,
        "network_engine_invalid",
    )?;
    let args: Vec<OsString> = vec!["--workers".into(), width.to_string().into()];
    let probe = engine
        .call_cancelled("build-info", &args, Duration::from_secs(20), width, cancel)
        .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
    if !probe.succeeded() || probe.truncated || probe.stdout.len() > MAX_IDENTITY_BYTES {
        return Err(NetworkFailure::new(
            "network_engine_identity_invalid",
            "build-info did not return one bounded successful identity",
        ));
    }
    let build: NetworkBuildInfo = serde_json::from_str(&probe.stdout)
        .map_err(|e| NetworkFailure::new("network_engine_identity_invalid", e.to_string()))?;
    if build.schema != "ds.engine-build/v1" || build.module != "network" {
        return Err(NetworkFailure::new(
            "network_engine_incompatible",
            "captured binary is not the Network module",
        ));
    }
    let identity = build.engine_identity;
    validate_identity(&identity, width)?;
    if !development_override && !identity.is_qualified() {
        return Err(NetworkFailure::new("network_engine_unqualified", "the selected Network executable must report qualified source and dependency closure pins"));
    }
    if digest_file(
        engine.executable(),
        MAX_EXECUTABLE_BYTES,
        "network_engine_changed",
    )? != executable_sha256
    {
        return Err(NetworkFailure::new(
            "network_engine_changed",
            "executable changed during identity probe",
        ));
    }
    Ok((engine, identity, executable_sha256, development_override))
}
// Selection does not install or assert signature trust. Normal captures require
// qualified source/closure pins; absolute developer overrides are labeled.
fn admit_candidate(engine: &CapturedExternal) -> Result<bool, NetworkFailure> {
    if let Some(value) = std::env::var_os("DS_NETWORK_ENGINE_BIN") {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(NetworkFailure::new(
                "network_engine_path_invalid",
                "explicit engine path must be absolute",
            ));
        }
        let path = fs::canonicalize(path)
            .map_err(|e| NetworkFailure::new("network_engine_missing", e.to_string()))?;
        if path == engine.executable() {
            return Ok(true);
        }
        return Err(NetworkFailure::new(
            "network_engine_changed",
            "explicit override changed during capture",
        ));
    }
    // Existing External selection captures the fixed sibling/lane/PATH owner.
    // Actual source and closure qualification is verified by the probe.
    Ok(false)
}
fn validate_identity(identity: &EngineIdentity, width: usize) -> Result<(), NetworkFailure> {
    let pins_valid = match (&identity.source_sha, &identity.dependency_closure_sha256) {
        (None, None) => true,
        (Some(source), Some(closure)) => valid_hex(source, 40) && valid_hex(closure, 64),
        _ => false,
    };
    if identity.schema != "ds.network-engine/v1"
        || identity.protocol_major != 1
        || !matches!(identity.environment, ExecutionEnvironment::Native)
        || !matches!(identity.runtime.environment, ExecutionEnvironment::Native)
        || identity.runtime.logical_cpus == 0
        || identity.runtime.logical_cpus > 65536
        || identity.runtime.internal_worker_threads != width
        || width > identity.runtime.logical_cpus
        || identity.core_version.is_empty()
        || identity.core_version.len() > 128
        || identity.capabilities.len() > 32
        || !identity
            .capabilities
            .iter()
            .any(|cap| matches!(cap, NetworkCapability::LvProcessingV1))
        || !pins_valid
    {
        return Err(NetworkFailure::new(
            "network_engine_incompatible",
            "incompatible protocol, runtime, budget, capability or source pin",
        ));
    }
    Ok(())
}
fn same_identity(left: &EngineIdentity, right: &EngineIdentity) -> Result<bool, NetworkFailure> {
    let a = serde_json::to_value(left)
        .map_err(|e| NetworkFailure::new("network_engine_identity_invalid", e.to_string()))?;
    let b = serde_json::to_value(right)
        .map_err(|e| NetworkFailure::new("network_engine_identity_invalid", e.to_string()))?;
    Ok(a == b)
}
fn validate_receipt(
    receipt: &NetworkReceiptV1,
    identity: &EngineIdentity,
    input_sha256: &str,
    expected_result_schema: &str,
) -> Result<bool, NetworkFailure> {
    if receipt.schema != "ds.network.task-result/v1"
        || receipt.input_sha256 != input_sha256
        || !same_identity(&receipt.engine_identity, identity)?
        || receipt.artifact.file != "result.json"
        || receipt.artifact.schema != expected_result_schema
        || receipt.artifact.byte_count == 0
        || receipt.artifact.byte_count > MAX_NATIVE_LV_OUTPUT_BYTES
        || !valid_hex(&receipt.artifact.sha256, 64)
        || receipt.result_input_sha256.is_some()
    {
        return Err(NetworkFailure::new(
            "network_receipt_invalid",
            "receipt disagrees with frozen input, identity or fixed artifact",
        ));
    }
    Ok(matches!(receipt.outcome, NetworkTaskOutcome::Cancelled))
}
fn validate_result(
    result: &NativeLvResultV1,
    request: &NativeLvRequestV1,
    identity: &EngineIdentity,
) -> Result<(), NetworkFailure> {
    let expected_result_schema =
        ds_network_contracts::lv_processing::native_lv_result_schema_for_request(&request.schema)
            .map_err(NetworkFailure::native)?;
    if result.schema != expected_result_schema
        || result.engine_core_version != identity.core_version
        || result.jobs.len() != request.jobs.len()
        || result
            .jobs
            .iter()
            .zip(&request.jobs)
            .any(|(a, b)| a.transformer_name != b.transformer_name)
        || result.jobs.iter().any(|j| {
            if j.ok {
                j.output.is_none() || j.error.is_some() || j.error_code.is_some()
            } else {
                j.output.is_some() || j.error.is_none()
            }
        })
    {
        return Err(NetworkFailure::new(
            "network_result_invalid",
            "incompatible schema, version, job order or outcome",
        ));
    }
    Ok(())
}
fn read_refusal(bytes: &str, identity: &EngineIdentity) -> Result<NetworkFailure, NetworkFailure> {
    let refusal: NetworkTaskRefusalV1 = serde_json::from_str(bytes)
        .map_err(|e| NetworkFailure::new("network_refusal_invalid", e.to_string()))?;
    if refusal.schema != "ds.network.task-refusal/v1"
        || refusal.code.is_empty()
        || refusal.code.len() > 128
        || refusal.error.len() > 4096
        || refusal
            .remedy
            .as_ref()
            .is_some_and(|s| s.len() > MAX_IDENTITY_BYTES)
        || !same_identity(&refusal.engine_identity, identity)?
    {
        return Err(NetworkFailure::new(
            "network_refusal_invalid",
            "malformed refusal or changed identity",
        ));
    }
    Ok(NetworkFailure {
        code: static_code(&refusal.code),
        message: refusal.error,
        owner_error: None,
        owner_code: Some(refusal.code),
        owner_detail: refusal.detail.map(Box::new),
        owner_remedy: refusal.remedy.map(String::into_boxed_str),
        owner_invalid_input: refusal.invalid_input,
    })
}
fn static_code(code: &str) -> &'static str {
    match code {
        "network_engine_missing" => "network_engine_missing",
        "callee_cancelled" => "callee_cancelled",
        "network_task_cancelled" => "network_task_cancelled",
        "network_control_unavailable" => "network_control_unavailable",
        "design_snapshot_processing_failed" => "design_snapshot_processing_failed",
        "callee_timed_out" => "callee_timed_out",
        "callee_wait_failed" => "callee_wait_failed",
        "callee_supervision_failed" => "callee_supervision_failed",
        "callee_stdin_failed" => "callee_stdin_failed",
        "fast_lv_input_too_large" => "fast_lv_input_too_large",
        "fast_lv_input_invalid" => "fast_lv_input_invalid",
        "fast_lv_schema_unsupported" => "fast_lv_schema_unsupported",
        "fast_lv_bound_refused" => "fast_lv_bound_refused",
        "fast_lv_output_too_large" => "fast_lv_output_too_large",
        "fast_lv_result_encoding_failed" => "fast_lv_result_encoding_failed",
        "fast_lv_publication_invalid" => "fast_lv_publication_invalid",
        "network_control_invalid" => "network_control_invalid",
        "network_input_invalid" => "network_input_invalid",
        "network_output_invalid" => "network_output_invalid",
        "network_artifact_io" => "network_artifact_io",
        "network_workers_invalid" => "network_workers_invalid",
        _ => "network_engine_refused",
    }
}
fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn open_regular(path: &Path, code: &'static str) -> Result<File, NetworkFailure> {
    let before =
        fs::symlink_metadata(path).map_err(|e| NetworkFailure::new(code, e.to_string()))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(NetworkFailure::new(
            code,
            "artifact must be an ordinary file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|e| NetworkFailure::new(code, e.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|e| NetworkFailure::new(code, e.to_string()))?;
    if !metadata.is_file() {
        return Err(NetworkFailure::new(code, "opened artifact is not ordinary"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != metadata.dev() || before.ino() != metadata.ino() {
            return Err(NetworkFailure::new(code, "artifact changed while opening"));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(NetworkFailure::new(code, "artifact is a reparse point"));
        }
    }
    Ok(file)
}
fn read_regular(
    path: &Path,
    maximum: usize,
    code: &'static str,
) -> Result<Vec<u8>, NetworkFailure> {
    let file = open_regular(path, code)?;
    let len = file
        .metadata()
        .map_err(|e| NetworkFailure::new(code, e.to_string()))?
        .len();
    if len > maximum as u64 {
        return Err(NetworkFailure::new(code, "artifact exceeds byte bound"));
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| NetworkFailure::new(code, e.to_string()))?;
    if bytes.len() > maximum || bytes.len() as u64 != len {
        return Err(NetworkFailure::new(code, "artifact changed during read"));
    }
    Ok(bytes)
}
fn digest_file(path: &Path, maximum: u64, code: &'static str) -> Result<String, NetworkFailure> {
    let mut file = open_regular(path, code)?;
    let len = file
        .metadata()
        .map_err(|e| NetworkFailure::new(code, e.to_string()))?
        .len();
    if len > maximum {
        return Err(NetworkFailure::new(code, "file exceeds byte bound"));
    }
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|e| NetworkFailure::new(code, e.to_string()))?;
        if read == 0 {
            break;
        }
        count = count.saturating_add(read as u64);
        if count > maximum {
            return Err(NetworkFailure::new(code, "file exceeds byte bound"));
        }
        digest.update(&chunk[..read]);
    }
    if count != len {
        return Err(NetworkFailure::new(code, "file changed during hash"));
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn write_request(path: &Path, bytes: &[u8]) -> Result<(), NetworkFailure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|e| NetworkFailure::new("network_request_io", e.to_string()))?;
    file.write_all(bytes)
        .map_err(|e| NetworkFailure::new("network_request_io", e.to_string()))
}
fn create_output(path: &Path) -> Result<(), NetworkFailure> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))
}
