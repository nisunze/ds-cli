//! Application-owned source admission, bounded local import sessions and artifacts.
//! Native hosts bind paths and schedule these synchronous operations. Browser
//! file handles and project selection never supply filesystem authority.

use crate::columnar::{
    self, engine_message, ArtifactReplay, ArtifactSpool, ColumnarPlan, NativeColumnarConversion,
    ReadingParameters,
};
use crate::RequestContext;
use crate::native_source::{
    prepare_native_source, NativeLinkAccess, NativeSourceDisposition, NativeSourceFingerprint,
    NativeSourcePrepareRequest,
};
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::source_io::{SourceLayersV1, SourceReaderV1, SourceReadingV1};
#[cfg(test)]
use ds_network_contracts::engine::{ExecutionEnvironment, ExecutionRuntime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const TABULAR_CHUNK_BYTES: usize = 2 * 1024 * 1024;
const TABULAR_CHUNK_RECORDS: usize = 1024;
const TABULAR_PREFIX_MAX_BYTES: usize = 16 * 1024 * 1024;
const TABULAR_RECORD_MAX_BYTES: usize = 16 * 1024 * 1024;
const TABULAR_SESSION_LIMIT: usize = 8;

#[derive(Clone, Default)]
pub struct NativeTabularImportState {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<NativeTabularSession>>>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTabularImportRequest {
    pub source_path: PathBuf,
    pub file_name: String,
    #[serde(default)]
    pub expected_size_bytes: Option<u64>,
    #[serde(default)]
    pub expected_modified_unix_ms: Option<u64>,
    #[serde(default)]
    pub expected_quick_sha256: Option<String>,
    /// Physical source lines before the first data row, taken from the bounded
    /// preview. They are replayed in every batch so the Network engine's table
    /// reader sees exactly the same header band and column names.
    #[serde(default)]
    pub data_start: usize,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub z_column: Option<String>,
    #[serde(deserialize_with = "strict_reading")]
    pub input: SourceReadingV1,
}

/// The application admits only the declared reading contract, even when the
/// independently released IO engine later learns additional wire fields.
fn strict_reading<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<SourceReadingV1, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let object = value.as_object().ok_or_else(|| {
        <D::Error as serde::de::Error>::custom("source reading must be an object")
    })?;
    const KEYS: &[&str] = &[
        "sheetKey",
        "separator",
        "headerRow",
        "xColumn",
        "yColumn",
        "crs",
        "utmZone",
        "utmSouthern",
        "attributesOnly",
        "layerName",
        "columnRenames",
        "dropColumns",
    ];
    if let Some(key) = object.keys().find(|key| !KEYS.contains(&key.as_str())) {
        return Err(<D::Error as serde::de::Error>::custom(format!(
            "unknown source reading field: {key}"
        )));
    }
    if let Some(renames) = object.get("columnRenames").and_then(Value::as_array) {
        for rename in renames {
            if rename
                .as_object()
                .is_some_and(|row| row.keys().any(|key| key != "from" && key != "to"))
            {
                return Err(<D::Error as serde::de::Error>::custom(
                    "unknown column rename field",
                ));
            }
        }
    }
    serde_json::from_value(value).map_err(<D::Error as serde::de::Error>::custom)
}

impl NativeTabularImportRequest {
    fn validate(&self) -> Result<(), String> {
        crate::bounded_text(&self.file_name, "fileName", 4096)?;
        if self.columns.len() > 2048 {
            return Err("tabular columns exceed the 2048 column bound".into());
        }
        for column in &self.columns {
            crate::bounded_text(column, "column", 200)?;
        }
        if self.input.column_renames.len() > 2048 || self.input.drop_columns.len() > 2048 {
            return Err("tabular reading exceeds the 2048 column transform bound".into());
        }
        for rename in &self.input.column_renames {
            crate::bounded_text(&rename.from, "column rename source", 200)?;
            crate::bounded_text(&rename.to, "column rename target", 200)?;
        }
        for column in &self.input.drop_columns {
            crate::bounded_text(column, "dropped column", 200)?;
        }
        if let Some(column) = &self.z_column {
            crate::bounded_text(column, "zColumn", 200)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTabularImportStarted {
    pub session_id: String,
    pub file_bytes: u64,
    pub bytes_read: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTabularImportStatus {
    pub session_id: String,
    pub file_bytes: u64,
    pub bytes_read: u64,
    /// Progress in whatever this lane is actually reading — source bytes while
    /// converting, artifact rows while replaying one. Stated separately from
    /// `bytes_read` because a replay reads no source bytes at all, and a
    /// progress bar driven off `bytes_read` would sit at zero for the whole
    /// import rather than admit which file it is reading.
    pub progress_done: u64,
    pub progress_total: u64,
    pub valid_rows: u64,
    pub invalid_rows: u64,
    pub chunk_count: usize,
    pub done: bool,
    pub layer_name: Option<String>,
    pub geometry_type: Option<String>,
    pub columns: Vec<String>,
    pub elapsed_seconds: f64,
    /// The analytical artifact (plan §0.8a). Present once the conversion has
    /// completed, or immediately when an earlier one is being replayed.
    pub conversion: Option<NativeColumnarConversion>,
    /// Why the artifact was given up, when it was. The import is unaffected —
    /// losing the artifact costs analysis, not the operator's layer.
    pub conversion_error: Option<String>,
    /// True while this import is writing an artifact it has not finished.
    pub converting: bool,
}

/// Where an import's bounded batches come from.
///
/// Exactly one source is read per import. Either the original is parsed once
/// and its batches feed BOTH the layer and the artifact, or the artifact for
/// this exact reading already exists and the original is not read at all. There
/// is deliberately no third shape in which both happen (§0.8a).
enum SessionLane {
    Convert(Box<ConvertLane>),
    Replay(Box<ReplayLane>),
}

struct ConvertLane {
    reader: BufReader<File>,
    /// The Network engine that reads every batch of this import.
    engine: CapturedNetworkClient,
    header_prefix: Vec<u8>,
    input: SourceReadingV1,
    z_output_column: Option<String>,
    drop_z_output: bool,
    schema_locked: bool,
    data_records_read: u64,
    /// Where the artifact belongs. `None` when the store could not even be
    /// addressed, which is recorded and never fatal.
    plan: Option<ColumnarPlan>,
    /// The artifact being written: its append log until the source ends.
    /// `None` once it has been given up, which leaves the import running
    /// against the very same parsed batches.
    spool: Option<ArtifactSpool>,
}

struct ReplayLane {
    reader: ArtifactReplay,
    rows_total: u64,
    rows_read: u64,
}

struct NativeTabularSession {
    context: Option<RequestContext>,
    source_path: PathBuf,
    source_fingerprint: NativeSourceFingerprint,
    file_name: String,
    file_bytes: u64,
    lane: SessionLane,
    columns: Vec<String>,
    bytes_read: u64,
    valid_rows: u64,
    invalid_rows: u64,
    chunk_count: usize,
    done: bool,
    layer_name: Option<String>,
    geometry_type: Option<String>,
    started_at: Instant,
    conversion: Option<NativeColumnarConversion>,
    conversion_error: Option<String>,
}

/// Initialize Rayon's one process-wide pool before any native command runs.
/// Its width is exactly the logical CPU capacity the desktop host
/// (Windows, macOS, or Linux) exposes to this process. The installed application deliberately
/// ignores `RAYON_NUM_THREADS`: its native compute contract is all available
/// logical cores, while OS process affinity still defines availability.
#[cfg(test)]
pub fn initialize_native_host_pool() -> Result<ExecutionRuntime, String> {
    let logical_cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let wanted = logical_cpus;
    match rayon::ThreadPoolBuilder::new()
        .num_threads(wanted)
        .build_global()
    {
        Ok(()) => {}
        Err(error) => {
            // A dependency may have initialized Rayon first. Accept that only
            // when it created the exact capacity this host contract requires.
            let actual = rayon::current_num_threads();
            if actual != wanted {
                return Err(format!(
                    "native compute requested {wanted} Rayon threads but the existing pool has {actual}: {error}"
                ));
            }
        }
    }
    let runtime = ExecutionRuntime {
        environment: ExecutionEnvironment::Native,
        logical_cpus,
        internal_worker_threads: rayon::current_num_threads(),
    };
    if runtime.internal_worker_threads != wanted {
        return Err(format!(
            "native compute initialized {} Rayon threads; expected {wanted}",
            runtime.internal_worker_threads
        ));
    }
    Ok(runtime)
}

pub fn prepare_source(
    context: &RequestContext,
    managed_imports: &Path,
    request: crate::NativeSourcePrepareRequest,
) -> Result<crate::NativeSourcePrepareResult, String> {
    context.validate()?;
    if !request.source_path.is_absolute() {
        return Err("native source path must be absolute".into());
    }
    let destination = if request.disposition == NativeSourceDisposition::ManagedCopy {
        if !managed_imports.is_absolute() {
            return Err("managed import root must be absolute".into());
        }
        Some(managed_imports.to_path_buf())
    } else {
        if request
            .destination
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err("native destination path must be absolute".into());
        }
        request.destination
    };
    prepare_native_source(NativeSourcePrepareRequest {
        source_path: request.source_path,
        disposition: request.disposition,
        link_access: request.link_access,
        destination,
    })
}

/// Companion files of a shapefile sitting beside it on disk. A drop hands the
/// shell only the `.shp` the operator grabbed, and without its `.dbf` the
/// parse refuses (feedback ad44f3a0). Listed off the UI thread: a synchronous
/// command runs on the main thread, and a slow drive would freeze the window.
pub fn source_companions(
    context: &RequestContext,
    source_path: &Path,
) -> Result<Vec<PathBuf>, String> {
    context.validate()?;
    shapefile_companions(source_path)
}

const SHAPEFILE_SIDECARS: &[&str] = &[
    "dbf", "shx", "prj", "cpg", "qix", "sbn", "sbx", "fix", "shp.xml",
];

/// The same stem with a sidecar extension, in any letter case, as regular files.
fn shapefile_companions(shp: &Path) -> Result<Vec<PathBuf>, String> {
    let name = shp
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} is not a file path", shp.display()))?;
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".shp")
        .ok_or_else(|| format!("{name} is not a .shp file"))?;
    let dir = shp
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("could not read {}: {error}", dir.display()))?;
    let mut companions = Vec::new();
    for entry in entries.flatten() {
        let Some(other) = entry.file_name().to_str().map(str::to_ascii_lowercase) else {
            continue;
        };
        let is_sidecar = other
            .strip_prefix(stem)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|extension| SHAPEFILE_SIDECARS.contains(&extension));
        if is_sidecar && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            companions.push(entry.path());
        }
    }
    companions.sort();
    Ok(companions)
}

fn verify_expected_fingerprint(
    fingerprint: &NativeSourceFingerprint,
    expected_size_bytes: Option<u64>,
    expected_modified_unix_ms: Option<u64>,
    expected_quick_sha256: Option<&str>,
) -> Result<(), String> {
    if let Some(expected) = expected_size_bytes {
        if expected != fingerprint.size_bytes {
            return Err(format!(
                "source changed after it was selected: expected {expected} bytes, found {}",
                fingerprint.size_bytes
            ));
        }
    }
    if expected_modified_unix_ms.is_some()
        && expected_modified_unix_ms != fingerprint.modified_unix_ms
    {
        return Err("source modification time changed after it was selected".to_string());
    }
    if let Some(expected) = expected_quick_sha256 {
        if expected != fingerprint.quick_sha256 {
            return Err("source fingerprint changed after it was selected".to_string());
        }
    }
    Ok(())
}

fn canonical_source_file(source_path: &Path) -> Result<PathBuf, String> {
    if !source_path.is_absolute() {
        return Err("file locations require an absolute desktop source path".to_string());
    }
    let source = std::fs::canonicalize(source_path)
        .map_err(|error| format!("could not locate source {}: {error}", source_path.display()))?;
    let metadata = std::fs::metadata(&source)
        .map_err(|error| format!("could not inspect source {}: {error}", source.display()))?;
    if !metadata.is_file() {
        return Err(format!("source is not a file: {}", source.display()));
    }
    Ok(source)
}

/// Reveal one previously selected import in the platform file manager. This
/// never opens/executes the source itself, and relative browser provenance is
/// refused before it reaches the opener plugin.
pub fn source_location(context: &RequestContext, source_path: &Path) -> Result<PathBuf, String> {
    context.validate()?;
    canonical_source_file(source_path)
}

/// Bridge ordinary bounded files back to the existing shared web importer.
/// Large sources are deliberately refused here: they stay linked and use a
/// native streaming command instead of crossing IPC as an eager byte buffer.
pub fn read_source(
    context: &RequestContext,
    source_path: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    context.validate()?;
    const HARD_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
    let limit = max_bytes.min(HARD_LIMIT_BYTES);
    if limit == 0 {
        return Err("native source read limit must be greater than zero".to_string());
    }
    let source = canonical_source_file(source_path)?;
    let file = File::open(&source)
        .map_err(|error| format!("could not read source {}: {error}", source.display()))?;
    let size = file
        .metadata()
        .map_err(|error| format!("could not inspect source {}: {error}", source.display()))?
        .len();
    if size > limit {
        return Err(format!(
            "source is {size} bytes; eager desktop reads are limited to {limit} bytes"
        ));
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(size).map_err(|_| "source size cannot be represented on this host")?,
    );
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            format!(
                "could not finish reading source {}: {error}",
                source.display()
            )
        })?;
    if bytes.len() as u64 > limit {
        return Err("source grew beyond the native read limit".into());
    }
    Ok(bytes)
}

/// Read only the leading bytes of a source, regardless of its total size.
/// This is the desktop equivalent of `File.slice(0, preview_limit)`: it gives
/// the shared preview worker a bounded sample without fabricating an eager
/// multi-gigabyte browser File.
pub fn read_source_prefix(
    context: &RequestContext,
    source_path: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    context.validate()?;
    let limit = max_bytes.min(4 * 1024 * 1024);
    if limit == 0 {
        return Err("native source preview limit must be greater than zero".into());
    }
    let source = canonical_source_file(source_path)?;
    let file = File::open(&source)
        .map_err(|error| format!("could not read source {}: {error}", source.display()))?;
    let mut bytes = Vec::with_capacity(limit as usize);
    file.take(limit).read_to_end(&mut bytes).map_err(|error| {
        format!(
            "could not read source preview {}: {error}",
            source.display()
        )
    })?;
    Ok(bytes)
}

/// Open a large linked delimited-text source for bounded materialization AND
/// conversion (plan §0.8a).
///
/// One read, two consumers: every `next` call parses complete lines with the
/// Network engine's table reader, returns one Arrow IPC batch to the import, and appends
/// that same batch to the GeoParquet artifact. When the artifact for this exact
/// reading already exists, no source line is read at all — the chunks come back
/// out of the artifact instead.
pub fn start_import(
    context: RequestContext,
    state: &NativeTabularImportState,
    columnar_root: &Path,
    request: NativeTabularImportRequest,
) -> Result<NativeTabularImportStarted, String> {
    context.validate()?;
    request.validate()?;
    let mut guard = state
        .sessions
        .lock()
        .map_err(|_| "native tabular session lock was poisoned")?;
    if guard.len() >= TABULAR_SESSION_LIMIT {
        return Err(format!("native tabular session limit ({TABULAR_SESSION_LIMIT}) reached; cancel an existing import"));
    }
    let session_id = context.request_id().to_string();
    if guard.contains_key(&session_id) {
        return Err("native tabular request already exists".into());
    }
    let mut session = open_tabular_session(columnar_root, request)?;
    session.context = Some(context);
    let started = NativeTabularImportStarted {
        session_id: session_id.clone(),
        file_bytes: session.file_bytes,
        bytes_read: session.bytes_read,
    };
    guard.insert(session_id, Arc::new(Mutex::new(session)));
    Ok(started)
}

pub fn next_import_chunk(
    state: &NativeTabularImportState,
    session_id: &str,
) -> Result<Vec<u8>, String> {
    let session = held_session(state, session_id)?;
    let mut session = session
        .lock()
        .map_err(|_| "native tabular session was poisoned")?;
    validate_session_scope(session_id, &session)?;
    next_tabular_chunk(&mut session, TABULAR_CHUNK_BYTES).map(|chunk| chunk.unwrap_or_default())
}

pub fn import_status(
    state: &NativeTabularImportState,
    session_id: &str,
) -> Result<NativeTabularImportStatus, String> {
    let session = held_session(state, session_id)?;
    let session = session
        .lock()
        .map_err(|_| "native tabular session was poisoned")?;
    validate_session_scope(session_id, &session)?;
    Ok(tabular_status(session_id, &session))
}

pub fn cancel_import(state: &NativeTabularImportState, session_id: &str) -> Result<(), String> {
    crate::validate_request_id(session_id)?;
    let mut guard = state
        .sessions
        .lock()
        .map_err(|_| "native tabular session lock was poisoned")?;
    if let Some(session) = guard.get(session_id) {
        let session = session
            .lock()
            .map_err(|_| "native tabular session was poisoned")?;
        validate_session_scope(session_id, &session)?;
    }
    guard.remove(session_id);
    Ok(())
}

fn held_session(
    state: &NativeTabularImportState,
    session_id: &str,
) -> Result<Arc<Mutex<NativeTabularSession>>, String> {
    crate::validate_request_id(session_id)?;
    state
        .sessions
        .lock()
        .map_err(|_| "native tabular session lock was poisoned")?
        .get(session_id)
        .cloned()
        .ok_or_else(|| "native tabular session was not found".into())
}

fn validate_session_scope(session_id: &str, session: &NativeTabularSession) -> Result<(), String> {
    let context = session
        .context
        .as_ref()
        .ok_or("native tabular session has no captured scope")?;
    context.validate()?;
    if context.request_id() != session_id {
        return Err("native tabular session scope mismatch".into());
    }
    Ok(())
}

fn open_tabular_session(
    columnar_root: &Path,
    request: NativeTabularImportRequest,
) -> Result<NativeTabularSession, String> {
    let started_at = Instant::now();
    let prepared = prepare_native_source(NativeSourcePrepareRequest {
        source_path: request.source_path.clone(),
        disposition: NativeSourceDisposition::Linked,
        link_access: Some(NativeLinkAccess::ReadOnly),
        destination: None,
    })?;
    let source = prepared.prepared_path;
    let fingerprint = prepared.fingerprint;
    let file_bytes = fingerprint.size_bytes;
    verify_expected_fingerprint(
        &fingerprint,
        request.expected_size_bytes,
        request.expected_modified_unix_ms,
        request.expected_quick_sha256.as_deref(),
    )?;
    if request.file_name.to_ascii_lowercase().ends_with(".xlsx")
        || request.file_name.to_ascii_lowercase().ends_with(".xlsm")
    {
        return Err(
            "native streaming currently accepts delimited text, not Excel workbooks".to_string(),
        );
    }

    let z_output_column = request.z_column.as_ref().map(|column| {
        request
            .input
            .column_renames
            .iter()
            .find(|rename| rename.from == *column)
            .map(|rename| rename.to.clone())
            .unwrap_or_else(|| column.clone())
    });
    let drop_z_output = request.z_column.as_ref().is_some_and(|column| {
        request
            .input
            .drop_columns
            .iter()
            .any(|dropped| dropped == column)
    });
    let mut input = request.input;
    if let Some(z_column) = request.z_column.as_ref() {
        // The reader normally drops properties before this bridge can promote Z.
        // Retain it just for conversion, then remove it below when requested.
        input.drop_columns.retain(|column| column != z_column);
    }
    let schema_locked = !request.columns.is_empty();
    let columns = output_columns(
        &request.columns,
        &input,
        request.z_column.as_deref(),
        drop_z_output,
    );

    let reading = ReadingParameters {
        data_start: request.data_start,
        columns: request.columns.clone(),
        z_column: request.z_column.clone(),
    };
    // Where this exact reading of these exact bytes belongs. Best effort by
    // design: an artifact store that cannot be addressed or written costs the
    // analysis, never the operator's layer, so it is recorded and the import
    // proceeds on the source alone.
    let plan = columnar::plan(
        columnar_root,
        &source,
        &request.file_name,
        &input,
        input.layer_name.as_deref(),
        &reading,
    );

    // The artifact decides the lane. Opening it IS the completeness check: an
    // interrupted write never reached this path (it lives under `.partial`
    // until it is whole), and a file the engine cannot read converts again.
    let opened = plan
        .as_ref()
        .ok()
        .filter(|plan| plan.artifact_path.is_file())
        .and_then(|plan| {
            let engine = capture_reader().ok()?;
            ArtifactReplay::open(&plan.artifact_path, engine).ok()
        });

    // The replay lane never opens the source at all — not to read it, and not
    // to hold a handle on it.
    if let (Ok(plan), Some(reader)) = (plan.as_ref(), opened) {
        let summary = reader.summary().clone();
        let conversion = columnar::receipt_for_artifact(
            plan,
            &plan.artifact_path,
            &summary,
            &columnar::fallback_layer_name(&input, &request.file_name),
            started_at.elapsed().as_secs_f64(),
        );
        return Ok(NativeTabularSession {
            context: None,
            source_path: source,
            source_fingerprint: fingerprint,
            file_name: request.file_name,
            file_bytes,
            lane: SessionLane::Replay(Box::new(ReplayLane {
                reader,
                rows_total: u64::try_from(summary.feature_count).unwrap_or(u64::MAX),
                rows_read: 0,
            })),
            columns: summary.columns.clone(),
            bytes_read: 0,
            valid_rows: 0,
            invalid_rows: 0,
            chunk_count: 0,
            done: false,
            layer_name: Some(conversion.layer_name.clone()),
            geometry_type: summary.geometry_type.clone(),
            started_at,
            conversion: Some(conversion),
            conversion_error: None,
        });
    }

    let engine = capture_reader()?;
    let mut reader = BufReader::new(
        open_tabular_source(&source)
            .map_err(|error| format!("could not read source {}: {error}", source.display()))?,
    );
    let prefix_lines = input.header_row.map_or(request.data_start, |row| {
        request.data_start.max(row.saturating_add(1))
    });
    let header_prefix = read_header_prefix(&mut reader, prefix_lines)?;
    let bytes_read = u64::try_from(header_prefix.len()).unwrap_or(u64::MAX);
    let mut conversion_error = plan.as_ref().err().cloned();
    let spool = match plan
        .as_ref()
        .map_err(String::clone)
        .and_then(ArtifactSpool::create)
    {
        Ok(spool) => Some(spool),
        Err(error) => {
            conversion_error = Some(error);
            None
        }
    };

    Ok(NativeTabularSession {
        context: None,
        source_path: source,
        source_fingerprint: fingerprint,
        file_name: request.file_name,
        file_bytes,
        lane: SessionLane::Convert(Box::new(ConvertLane {
            reader,
            engine,
            header_prefix,
            input,
            z_output_column,
            drop_z_output,
            schema_locked,
            data_records_read: 0,
            plan: plan.ok(),
            spool,
        })),
        columns,
        bytes_read,
        valid_rows: 0,
        invalid_rows: 0,
        chunk_count: 0,
        done: false,
        layer_name: None,
        geometry_type: None,
        started_at,
        conversion: None,
        conversion_error,
    })
}

fn open_tabular_source(path: &std::path::Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep the selected snapshot readable by other processes, but prevent
        // writes, truncation, rename, or delete until this session is closed.
        options.share_mode(0x0000_0001); // FILE_SHARE_READ
    }
    options.open(path)
}

fn read_header_prefix<R: BufRead>(reader: &mut R, records: usize) -> Result<Vec<u8>, String> {
    let mut prefix = Vec::new();
    for _ in 0..records {
        let Some(record) = read_logical_record(reader, TABULAR_RECORD_MAX_BYTES)? else {
            return Err("tabular data start is beyond the end of the source".to_string());
        };
        if prefix.len().saturating_add(record.len()) > TABULAR_PREFIX_MAX_BYTES {
            return Err("tabular header exceeds the 16 MiB native safety bound".to_string());
        }
        prefix.extend_from_slice(&record);
    }
    Ok(prefix)
}

/// Read one CSV-style logical record. Newlines inside quoted fields remain in
/// the same record, and the cap is checked before extending the allocation.
/// Toggling on each quote correctly leaves escaped `""` pairs in the prior
/// state while avoiding delimiter-specific parsing in this transport layer.
fn read_logical_record<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut record = Vec::new();
    let mut in_quotes = false;
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| format!("could not stream tabular source: {error}"))?;
        if available.is_empty() {
            return if record.is_empty() {
                Ok(None)
            } else {
                Ok(Some(record))
            };
        }
        let mut take = 0usize;
        let mut complete = false;
        for byte in available {
            take += 1;
            if *byte == b'"' {
                in_quotes = !in_quotes;
            }
            if *byte == b'\n' && !in_quotes {
                complete = true;
                break;
            }
        }
        if record.len().saturating_add(take) > max_bytes {
            return Err(format!(
                "a tabular logical record exceeds the {} MiB native safety bound",
                max_bytes / (1024 * 1024)
            ));
        }
        record.extend_from_slice(&available[..take]);
        reader.consume(take);
        if complete {
            return Ok(Some(record));
        }
    }
}

fn read_complete_records<R: BufRead>(
    reader: &mut R,
    target_bytes: usize,
    max_records: usize,
) -> Result<(Vec<u8>, usize, bool), String> {
    let mut bytes = Vec::with_capacity(target_bytes.saturating_add(4096));
    let mut records = 0usize;
    let mut eof = false;
    while bytes.len() < target_bytes && records < max_records.max(1) {
        match read_logical_record(reader, TABULAR_RECORD_MAX_BYTES)? {
            Some(record) => {
                bytes.extend_from_slice(&record);
                records += 1;
            }
            None => {
                eof = true;
                break;
            }
        }
    }
    Ok((bytes, records, eof))
}

fn next_tabular_chunk(
    session: &mut NativeTabularSession,
    target_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    if session.done {
        return Ok(None);
    }
    match session.lane {
        SessionLane::Replay(_) => next_replayed_chunk(session),
        SessionLane::Convert(_) => next_converted_chunk(session, target_bytes),
    }
}

/// One bounded chunk out of an artifact converted earlier.
///
/// The original is not opened, not read and not parsed. This is what "the
/// conversion becomes the analytical source of record for the import" means in
/// practice on a re-import (§0.8a).
fn next_replayed_chunk(session: &mut NativeTabularSession) -> Result<Option<Vec<u8>>, String> {
    let chunk = {
        let SessionLane::Replay(lane) = &mut session.lane else {
            return Ok(None);
        };
        lane.reader.next_chunk()?
    };
    let Some(chunk) = chunk else {
        session.done = true;
        if let SessionLane::Replay(lane) = &mut session.lane {
            lane.rows_read = lane.rows_total;
        }
        return Ok(None);
    };
    let rows = u64::try_from(chunk.rows).unwrap_or(u64::MAX);
    if let SessionLane::Replay(lane) = &mut session.lane {
        lane.rows_read = lane.rows_read.saturating_add(rows);
    }
    session.valid_rows = session.valid_rows.saturating_add(rows);
    session.chunk_count += 1;
    Ok(Some(chunk.ipc))
}

/// One bounded chunk parsed out of the source — and appended, unchanged, to the
/// artifact being written. ONE parse feeds both.
fn next_converted_chunk(
    session: &mut NativeTabularSession,
    target_bytes: usize,
) -> Result<Option<Vec<u8>>, String> {
    let SessionLane::Convert(lane) = &mut session.lane else {
        return Ok(None);
    };
    let rows_before = lane.data_records_read;
    let (data, logical_records, eof) =
        read_complete_records(&mut lane.reader, target_bytes.max(1), TABULAR_CHUNK_RECORDS)?;
    lane.data_records_read = lane
        .data_records_read
        .saturating_add(u64::try_from(logical_records).unwrap_or(u64::MAX));
    let parsed = if data.is_empty() {
        None
    } else {
        let mut batch = Vec::with_capacity(lane.header_prefix.len().saturating_add(data.len()));
        batch.extend_from_slice(&lane.header_prefix);
        batch.extend_from_slice(&data);
        Some(read_batch(
            &lane.engine,
            &session.file_name,
            batch,
            &lane.input,
        ))
    };
    let data_len = u64::try_from(data.len()).unwrap_or(u64::MAX);
    drop(data);

    if eof {
        verify_tabular_source_unchanged(session)?;
        session.done = true;
    }
    session.bytes_read = session.bytes_read.saturating_add(data_len);

    let mut ipc = None;
    match parsed {
        None => {}
        Some(Err(error)) if error.contains("no valid coordinate rows") => {
            session.invalid_rows = session
                .invalid_rows
                .saturating_add(u64::try_from(logical_records).unwrap_or(u64::MAX));
            // Return control after every bounded record batch. The frontend can
            // observe progress and cancel even when the selected columns are bad.
            ipc = Some(Vec::new());
        }
        Some(Err(error)) => {
            abandon_conversion(session, &error);
            return Err(error);
        }
        Some(Ok(result)) => {
            ipc = Some(absorb_parsed_batch(session, result, rows_before)?);
        }
    }

    if session.done {
        finish_conversion(session);
    }
    Ok(ipc)
}

/// Post-process one parsed batch into the import's Arrow chunk, and append the
/// very same features to the artifact.
fn absorb_parsed_batch(
    session: &mut NativeTabularSession,
    result: SourceLayersV1,
    rows_before: u64,
) -> Result<Vec<u8>, String> {
    let invalid_coordinates = result.skipped_coordinate_rows.unwrap_or(0);
    let Some(layer) = result.layers.into_iter().next() else {
        let error = "native tabular parser returned no layer".to_string();
        abandon_conversion(session, &error);
        return Err(error);
    };
    let SessionLane::Convert(lane) = &mut session.lane else {
        return Err("the import is not converting".to_string());
    };
    let mut features = layer.features;
    if features.is_empty() {
        session.invalid_rows = session
            .invalid_rows
            .saturating_add(u64::try_from(invalid_coordinates).unwrap_or(u64::MAX));
        let mut give_up: Option<String> = None;
        if let Some(spool) = lane.spool.as_mut() {
            if let Err(error) = spool.append(&[], &[], invalid_coordinates) {
                give_up = Some(error);
            }
        }
        if let Some(error) = give_up {
            abandon_conversion(session, &error);
        }
        return Ok(Vec::new());
    }

    for feature in &mut features {
        let elevation = if let Some(properties) =
            feature.get_mut("properties").and_then(Value::as_object_mut)
        {
            if let Some(row) = properties.get_mut("_ds_import_row") {
                if let Some(local) = row.as_u64() {
                    *row = Value::from(local.saturating_add(rows_before));
                }
            }
            properties.remove("_ds_import_skipped_rows");
            let elevation = lane.z_output_column.as_deref().and_then(|z_column| {
                let value = properties
                    .get(z_column)
                    .and_then(value_as_f64)
                    .filter(|value| value.is_finite());
                if let Some(elevation) = value {
                    properties.insert(
                        "elevation_m".to_string(),
                        Value::String(elevation.to_string()),
                    );
                }
                if lane.drop_z_output {
                    properties.remove(z_column);
                }
                value
            });
            if lane.schema_locked {
                properties.retain(|column, _| session.columns.contains(column));
            }
            elevation
        } else {
            None
        };
        if let Some(elevation) = elevation {
            if let Some(coordinates) = feature
                .get_mut("geometry")
                .and_then(|geometry| geometry.get_mut("coordinates"))
                .and_then(Value::as_array_mut)
            {
                if coordinates.len() == 2 {
                    coordinates.push(Value::from(elevation));
                }
            }
        }
    }

    let valid = features.len();
    if !lane.schema_locked {
        for column in ds_columnar::columns_for_features(&features) {
            if !session.columns.contains(&column) {
                session.columns.push(column);
            }
        }
        lane.schema_locked = true;
    }

    // The artifact is written from the SAME features the import receives, so
    // the two can never describe different data and the source is read once.
    let mut give_up: Option<String> = None;
    if let Some(spool) = lane.spool.as_mut() {
        if let Err(error) = spool.append(&features, &session.columns, invalid_coordinates) {
            give_up = Some(error);
        }
    }
    if let Some(error) = give_up {
        abandon_conversion(session, &error);
    }

    session.valid_rows = session
        .valid_rows
        .saturating_add(u64::try_from(valid).unwrap_or(u64::MAX));
    session.invalid_rows = session
        .invalid_rows
        .saturating_add(u64::try_from(invalid_coordinates).unwrap_or(u64::MAX));
    session.chunk_count += 1;
    session.layer_name = Some(layer.name);
    session.geometry_type = layer.geometry_type;
    // The engine encodes the browser chunk from the same features.
    let SessionLane::Convert(lane) = &session.lane else {
        return Err("the import is not converting".to_string());
    };
    lane.engine
        .features_ipc(&features, &session.columns, &|| false)
        .map_err(engine_message)
}

/// Publish the artifact, once the source has been read to its end.
fn finish_conversion(session: &mut NativeTabularSession) {
    let elapsed = session.started_at.elapsed().as_secs_f64();
    let fallback = session.file_name.clone();
    let layer_name = session.layer_name.clone().unwrap_or(fallback);
    let SessionLane::Convert(lane) = &mut session.lane else {
        return;
    };
    let Some(spool) = lane.spool.take() else {
        return;
    };
    let Some(plan) = lane.plan.clone() else {
        spool.abandon();
        return;
    };
    match spool.finish(&lane.engine, &layer_name) {
        Ok(conversion) => {
            session.conversion = Some(columnar::receipt_for_conversion(
                &plan,
                &conversion,
                elapsed,
            ));
        }
        Err(error) => session.conversion_error = Some(error),
    }
}

/// Give up on the artifact and say why. The import is untouched: it keeps the
/// bounded Arrow chunks it has already received and keeps receiving more, which
/// is the difference between losing analysis and losing the operator's layer.
fn abandon_conversion(session: &mut NativeTabularSession, reason: &str) {
    if let SessionLane::Convert(lane) = &mut session.lane {
        if let Some(spool) = lane.spool.take() {
            spool.abandon();
            session.conversion_error = Some(reason.to_string());
        }
    }
}

fn verify_tabular_source_unchanged(session: &NativeTabularSession) -> Result<(), String> {
    let prepared = prepare_native_source(NativeSourcePrepareRequest {
        source_path: session.source_path.clone(),
        disposition: NativeSourceDisposition::Linked,
        link_access: Some(NativeLinkAccess::ReadOnly),
        destination: None,
    })?;
    if prepared.fingerprint != session.source_fingerprint {
        return Err(
            "source changed while it was being imported; partial output was discarded".to_string(),
        );
    }
    Ok(())
}

fn output_columns(
    preview_columns: &[String],
    input: &SourceReadingV1,
    z_column: Option<&str>,
    drop_z_output: bool,
) -> Vec<String> {
    let dropped = input
        .drop_columns
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut columns = Vec::new();
    for source in preview_columns {
        if dropped.contains(source.as_str()) || (drop_z_output && Some(source.as_str()) == z_column)
        {
            continue;
        }
        let output = input
            .column_renames
            .iter()
            .find(|rename| rename.from == *source)
            .map(|rename| rename.to.clone())
            .unwrap_or_else(|| source.clone());
        if !columns.contains(&output) {
            columns.push(output);
        }
    }
    for internal in ["_ds_import_row", "_ds_import_sheet"] {
        columns.push(internal.to_string());
    }
    if !input.attributes_only && input.crs != "none" {
        for internal in ["_ds_import_x", "_ds_import_y", "_ds_import_crs"] {
            columns.push(internal.to_string());
        }
    }
    if z_column.is_some() {
        columns.push("elevation_m".to_string());
    }
    columns
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|text| text.trim().parse::<f64>().ok())
    })
}

fn tabular_status(session_id: &str, session: &NativeTabularSession) -> NativeTabularImportStatus {
    let bytes_read = session.bytes_read.min(session.file_bytes);
    let (progress_done, progress_total) = match &session.lane {
        SessionLane::Convert(_) => (bytes_read, session.file_bytes),
        SessionLane::Replay(lane) => (lane.rows_read.min(lane.rows_total), lane.rows_total),
    };
    NativeTabularImportStatus {
        session_id: session_id.to_string(),
        file_bytes: session.file_bytes,
        bytes_read,
        progress_done,
        progress_total,
        valid_rows: session.valid_rows,
        invalid_rows: session.invalid_rows,
        chunk_count: session.chunk_count,
        done: session.done,
        layer_name: session.layer_name.clone(),
        geometry_type: session.geometry_type.clone(),
        columns: session.columns.clone(),
        elapsed_seconds: session.started_at.elapsed().as_secs_f64(),
        conversion: session.conversion.clone(),
        conversion_error: session.conversion_error.clone(),
        converting: matches!(&session.lane, SessionLane::Convert(lane) if lane.spool.is_some()),
    }
}

#[cfg(test)]
#[path = "compute_tests.rs"]
mod tests;

/// The Network engine that reads the source's batches (DECOUPLING-013).
fn capture_reader() -> Result<CapturedNetworkClient, String> {
    CapturedNetworkClient::capture(1, &|| false).map_err(engine_message)
}

/// One bounded batch read by the engine's table reader. A reader refusal
/// keeps the reader's own words; an engine failure names its code.
fn read_batch(
    engine: &CapturedNetworkClient,
    file_name: &str,
    batch: Vec<u8>,
    reading: &SourceReadingV1,
) -> Result<SourceLayersV1, String> {
    engine
        .source_layers(
            file_name,
            batch,
            SourceReaderV1::Table(Box::new(reading.clone())),
            &|| false,
        )
        .map_err(engine_message)
}
