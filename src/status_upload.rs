//! Project design-intake upload state machine.
//!
//! The kernel owns admission, upload-before-processing phase order, bounded
//! concurrency, process request shape, result reconciliation, progress and
//! cancellation. Hosts own only effects: browser `File` reads, native file
//! reads, resumable byte transfer and the fixed `/process` call.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const SCHEMA: &str = "ds.design-status-upload/v1";
pub const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_FILES: usize = 2_000;
pub const MAX_PARALLEL_EFFECTS: usize = 100;

const SHAPEFILE_COMPONENT_ERROR: &str =
    "Upload shapefiles as one .zip containing the .shp, .shx, .dbf, and .prj files.";
const NO_PROCESS_RESULT: &str = "No processing result returned for uploaded file";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadMode {
    LvDrafting,
    SketchLv,
    LvProcess,
}

impl UploadMode {
    fn wire(self) -> &'static str {
        match self {
            Self::LvDrafting => "lv_drafting",
            Self::SketchLv => "sketch_lv",
            Self::LvProcess => "lv_process",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileInput {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    content_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRequest {
    project_id: String,
    mode: UploadMode,
    files: Vec<FileInput>,
    #[serde(default = "default_parallel_limit")]
    parallel_limit: usize,
    #[serde(default)]
    upload_chain: Vec<String>,
    #[serde(default)]
    use_firestore_design_data: bool,
    #[serde(default)]
    process_settings: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Uploading,
    Processing,
    Complete,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FileState {
    QueuedUpload,
    Uploading,
    Uploaded,
    Processing,
    Complete,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadFile {
    index: usize,
    name: String,
    size: u64,
    content_type: String,
    state: FileState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob_path: Option<String>,
    uploaded: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct UploadResult {
    pub name: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UploadState {
    project_id: String,
    mode: UploadMode,
    phase: Phase,
    files: Vec<UploadFile>,
    parallel_limit: usize,
    upload_chain: Vec<String>,
    use_firestore_design_data: bool,
    process_settings: Map<String, Value>,
    results: Vec<UploadResult>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    UploadSucceeded {
        file_index: usize,
        blob_path: String,
    },
    UploadFailed {
        file_index: usize,
        error: String,
    },
    ProcessSucceeded {
        file_index: usize,
        response: Value,
    },
    ProcessFailed {
        file_index: usize,
        error: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Start(StartRequest),
    Advance {
        state: UploadState,
        event: Event,
    },
    Cancel {
        state: UploadState,
        reason: String,
    },
    ValidateTarget {
        project_id: String,
        mode: UploadMode,
        response: Value,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Effect {
    Upload {
        file_index: usize,
        project_id: String,
        file_name: String,
        size: u64,
        content_type: String,
        upload_domain: &'static str,
    },
    Process {
        file_index: usize,
        project_id: String,
        action: &'static str,
        files: Vec<ProcessFile>,
        chain: Vec<String>,
        extra: Value,
    },
}

#[derive(Debug, Clone, Serialize)]
struct ProcessFile {
    file_name: String,
    file_path: String,
}

#[derive(Debug, Serialize)]
struct Progress {
    started: usize,
    completed: usize,
    total: usize,
    uploaded: usize,
    phase: Phase,
    current_file: String,
}

#[derive(Debug, Serialize)]
struct Reply {
    schema: &'static str,
    state: UploadState,
    effects: Vec<Effect>,
    new_results: Vec<UploadResult>,
    progress: Progress,
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("status upload request exceeds 2 MiB".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|error| error.to_string())?;
    let reply = match request {
        Request::ValidateTarget {
            project_id,
            mode,
            response,
        } => {
            return serde_json::to_string(&validate_upload_target(&project_id, mode, &response)?)
                .map_err(|error| error.to_string());
        }
        Request::Start(request) => start(request)?,
        Request::Advance { mut state, event } => {
            validate_state(&state)?;
            let current_file = event_file_name(&state, &event)?;
            let new_results = apply_event(&mut state, event)?;
            settle(&mut state, new_results, current_file)?
        }
        Request::Cancel { mut state, reason } => {
            validate_state(&state)?;
            cancel(&mut state, &reason)?
        }
    };
    serde_json::to_string(&reply).map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatedUploadTarget {
    pub schema: &'static str,
    pub session_uri: String,
    pub blob_path: String,
}

/// Admit a server-issued resumable target before any host sends file bytes.
pub fn validate_upload_target(
    project_id: &str,
    mode: UploadMode,
    response: &Value,
) -> Result<ValidatedUploadTarget, String> {
    validate_project(project_id)?;
    if let Some(error) = trimmed_string(response.get("error")) {
        return Err(error);
    }
    let descriptor = response
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(response);
    let invalid = || "Upload session did not include the expected server-issued target".to_string();
    let session_uri = descriptor
        .get("session_uri")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 8_192)
        .ok_or_else(invalid)?;
    let blob_path = descriptor
        .get("blob_path")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 2_048)
        .ok_or_else(invalid)?;
    if descriptor.get("upload_domain").and_then(Value::as_str) != Some(mode.wire()) {
        return Err(invalid());
    }
    let url = url::Url::parse(session_uri).map_err(|_| invalid())?;
    let receipt = descriptor
        .get("upload_receipt")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .filter(|value| {
            value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
        .ok_or_else(invalid)?;
    let root = match mode {
        UploadMode::LvDrafting => "lv_drafting",
        UploadMode::SketchLv => "lv_sketches",
        UploadMode::LvProcess => "lv_process",
    };
    let prefix = format!("{project_id}/{root}/uploads/{receipt}/");
    let file_name = blob_path.strip_prefix(&prefix).ok_or_else(invalid)?;
    if url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/upload/storage/v1/b/")
        || !url
            .query_pairs()
            .any(|(key, value)| key == "upload_id" && !value.is_empty())
        || file_name.is_empty()
        || file_name.contains('/')
        || matches!(file_name, "." | "..")
    {
        return Err(invalid());
    }
    Ok(ValidatedUploadTarget {
        schema: SCHEMA,
        session_uri: session_uri.into(),
        blob_path: blob_path.into(),
    })
}

fn start(request: StartRequest) -> Result<Reply, String> {
    validate_project(&request.project_id)?;
    if request.files.is_empty() || request.files.len() > MAX_FILES {
        return Err(format!("status upload needs 1..={MAX_FILES} files"));
    }
    validate_chain(&request.upload_chain)?;
    validate_settings(&request.process_settings)?;
    let mut results = Vec::new();
    let mut files = Vec::with_capacity(request.files.len());
    for (index, file) in request.files.into_iter().enumerate() {
        validate_file(&file)?;
        let refused = is_shapefile_component(&file.name);
        if refused {
            results.push(UploadResult {
                name: file.name.clone(),
                ok: false,
                error: Some(SHAPEFILE_COMPONENT_ERROR.into()),
                detail: None,
            });
        }
        files.push(UploadFile {
            index,
            name: file.name,
            size: file.size,
            content_type: normalized_content_type(&file.content_type),
            state: if refused {
                FileState::Complete
            } else {
                FileState::QueuedUpload
            },
            blob_path: None,
            uploaded: false,
        });
    }
    let mut state = UploadState {
        project_id: request.project_id,
        mode: request.mode,
        phase: Phase::Uploading,
        files,
        parallel_limit: request.parallel_limit.clamp(1, MAX_PARALLEL_EFFECTS),
        upload_chain: request.upload_chain,
        use_firestore_design_data: request.use_firestore_design_data,
        process_settings: request.process_settings,
        results: results.clone(),
    };
    settle(&mut state, results, String::new())
}

fn settle(
    state: &mut UploadState,
    new_results: Vec<UploadResult>,
    current_file: String,
) -> Result<Reply, String> {
    let effects = schedule(state)?;
    let progress = progress(state, current_file);
    Ok(Reply {
        schema: SCHEMA,
        state: state.clone(),
        effects,
        new_results,
        progress,
    })
}

fn schedule(state: &mut UploadState) -> Result<Vec<Effect>, String> {
    if matches!(state.phase, Phase::Complete | Phase::Cancelled) {
        return Ok(Vec::new());
    }
    if state.phase == Phase::Uploading {
        let active = state
            .files
            .iter()
            .filter(|file| file.state == FileState::Uploading)
            .count();
        let mut available = state.parallel_limit.saturating_sub(active);
        let mut effects = Vec::new();
        for file in &mut state.files {
            if available == 0 {
                break;
            }
            if file.state != FileState::QueuedUpload {
                continue;
            }
            file.state = FileState::Uploading;
            available -= 1;
            effects.push(Effect::Upload {
                file_index: file.index,
                project_id: state.project_id.clone(),
                file_name: file.name.clone(),
                size: file.size,
                content_type: file.content_type.clone(),
                upload_domain: state.mode.wire(),
            });
        }
        if state
            .files
            .iter()
            .all(|file| !matches!(file.state, FileState::QueuedUpload | FileState::Uploading))
        {
            state.phase = Phase::Processing;
        } else {
            return Ok(effects);
        }
    }
    if state.phase == Phase::Processing {
        let active = state
            .files
            .iter()
            .filter(|file| file.state == FileState::Processing)
            .count();
        let mut available = state.parallel_limit.saturating_sub(active);
        let chain = effective_chain(state);
        let extra = process_extra(state);
        let mut effects = Vec::new();
        for file in &mut state.files {
            if available == 0 {
                break;
            }
            if file.state != FileState::Uploaded {
                continue;
            }
            let blob_path = file
                .blob_path
                .clone()
                .ok_or_else(|| "uploaded status file has no blob path".to_string())?;
            file.state = FileState::Processing;
            available -= 1;
            effects.push(Effect::Process {
                file_index: file.index,
                project_id: state.project_id.clone(),
                action: state.mode.wire(),
                files: vec![ProcessFile {
                    file_name: file.name.clone(),
                    file_path: blob_path,
                }],
                chain: chain.clone(),
                extra: extra.clone(),
            });
        }
        if state
            .files
            .iter()
            .all(|file| !matches!(file.state, FileState::Uploaded | FileState::Processing))
        {
            state.phase = Phase::Complete;
        }
        return Ok(effects);
    }
    Ok(Vec::new())
}

fn apply_event(state: &mut UploadState, event: Event) -> Result<Vec<UploadResult>, String> {
    let (index, expected) = match &event {
        Event::UploadSucceeded { file_index, .. } | Event::UploadFailed { file_index, .. } => {
            (*file_index, FileState::Uploading)
        }
        Event::ProcessSucceeded { file_index, .. } | Event::ProcessFailed { file_index, .. } => {
            (*file_index, FileState::Processing)
        }
    };
    let file = state
        .files
        .get_mut(index)
        .filter(|file| file.index == index)
        .ok_or_else(|| "status upload event names an unknown file".to_string())?;
    if file.state != expected {
        return Err("status upload event does not match the file's active effect".into());
    }
    let result = match event {
        Event::UploadSucceeded { blob_path, .. } => {
            let path = blob_path.trim();
            if path.is_empty() || path.len() > 2_048 || path.chars().any(char::is_control) {
                return Err("upload effect returned an invalid blob path".into());
            }
            file.blob_path = Some(path.to_owned());
            file.uploaded = true;
            file.state = FileState::Uploaded;
            return Ok(Vec::new());
        }
        Event::UploadFailed { error, .. } => UploadResult {
            name: file.name.clone(),
            ok: false,
            error: Some(nonempty_error(error, "Upload failed")),
            detail: None,
        },
        Event::ProcessSucceeded { response, .. } => process_result(&file.name, &response),
        Event::ProcessFailed { error, .. } => UploadResult {
            name: file.name.clone(),
            ok: false,
            error: Some(nonempty_error(error, "Processing failed")),
            detail: None,
        },
    };
    file.state = FileState::Complete;
    state.results.push(result.clone());
    Ok(vec![result])
}

fn cancel(state: &mut UploadState, reason: &str) -> Result<Reply, String> {
    if state.phase == Phase::Complete {
        return Err("a completed status upload cannot be cancelled".into());
    }
    let message = nonempty_error(reason.to_owned(), "Upload cancelled");
    let mut new_results = Vec::new();
    for file in &mut state.files {
        if matches!(file.state, FileState::Complete | FileState::Cancelled) {
            continue;
        }
        file.state = FileState::Cancelled;
        let result = UploadResult {
            name: file.name.clone(),
            ok: false,
            error: Some(message.clone()),
            detail: None,
        };
        state.results.push(result.clone());
        new_results.push(result);
    }
    state.phase = Phase::Cancelled;
    settle(state, new_results, String::new())
}

fn process_result(file_name: &str, response: &Value) -> UploadResult {
    if let Some(error) = trimmed_string(response.get("error")) {
        return failed(file_name, error);
    }
    let Some(results) = response.get("results").and_then(Value::as_array) else {
        return failed(file_name, NO_PROCESS_RESULT.into());
    };
    if results.is_empty() {
        return failed(file_name, NO_PROCESS_RESULT.into());
    }
    let row = results.iter().find(|row| {
        trimmed_string(row.get("file_name")).as_deref() == Some(file_name)
            || trimmed_string(row.get("name")).as_deref() == Some(file_name)
    });
    let Some(row) = row else {
        return failed(file_name, NO_PROCESS_RESULT.into());
    };
    let error = process_error(row);
    let submitted = js_number(row.get("submitted"));
    UploadResult {
        name: file_name.into(),
        ok: error.is_empty() && submitted > 0.0,
        error: if !error.is_empty() {
            Some(error)
        } else if submitted > 0.0 {
            None
        } else {
            Some("Processing was not submitted for this file".into())
        },
        detail: None,
    }
}

fn process_error(row: &Value) -> String {
    for key in ["error", "error_message"] {
        if let Some(error) = trimmed_string(row.get(key)) {
            return error;
        }
    }
    row.get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .filter_map(js_string)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_default()
}

fn event_file_name(state: &UploadState, event: &Event) -> Result<String, String> {
    let index = match event {
        Event::UploadSucceeded { file_index, .. }
        | Event::UploadFailed { file_index, .. }
        | Event::ProcessSucceeded { file_index, .. }
        | Event::ProcessFailed { file_index, .. } => *file_index,
    };
    state
        .files
        .get(index)
        .filter(|file| file.index == index)
        .map(|file| file.name.clone())
        .ok_or_else(|| "status upload event names an unknown file".into())
}

fn progress(state: &UploadState, current_file: String) -> Progress {
    Progress {
        started: state
            .files
            .iter()
            .filter(|file| file.state != FileState::QueuedUpload)
            .count(),
        completed: state.results.len(),
        total: state.files.len(),
        uploaded: state.files.iter().filter(|file| file.uploaded).count(),
        phase: state.phase.clone(),
        current_file,
    }
}

fn effective_chain(state: &UploadState) -> Vec<String> {
    if state.mode == UploadMode::LvProcess || state.use_firestore_design_data {
        Vec::new()
    } else {
        state.upload_chain.clone()
    }
}

fn process_extra(state: &UploadState) -> Value {
    if state.mode == UploadMode::LvProcess {
        json!({"settings": state.process_settings})
    } else {
        json!({})
    }
}

fn validate_state(state: &UploadState) -> Result<(), String> {
    validate_project(&state.project_id)?;
    if state.files.is_empty() || state.files.len() > MAX_FILES {
        return Err("status upload state has an invalid file count".into());
    }
    if state.results.len() > state.files.len() {
        return Err("status upload state has more results than files".into());
    }
    if !(1..=MAX_PARALLEL_EFFECTS).contains(&state.parallel_limit) {
        return Err("status upload state has an invalid parallel limit".into());
    }
    validate_chain(&state.upload_chain)?;
    validate_settings(&state.process_settings)?;
    let completed_files = state
        .files
        .iter()
        .filter(|file| matches!(file.state, FileState::Complete | FileState::Cancelled))
        .count();
    if completed_files != state.results.len() {
        return Err("status upload state has inconsistent completion evidence".into());
    }
    let active = state
        .files
        .iter()
        .filter(|file| matches!(file.state, FileState::Uploading | FileState::Processing))
        .count();
    if active > state.parallel_limit {
        return Err("status upload state exceeds its parallel limit".into());
    }
    for (index, file) in state.files.iter().enumerate() {
        if file.index != index {
            return Err("status upload file indices are not contiguous".into());
        }
        validate_file(&FileInput {
            name: file.name.clone(),
            size: file.size,
            content_type: file.content_type.clone(),
        })?;
        if file.uploaded != file.blob_path.is_some() {
            return Err("status upload file has inconsistent upload evidence".into());
        }
        if matches!(file.state, FileState::QueuedUpload | FileState::Uploading) && file.uploaded {
            return Err("status upload file has evidence before upload completion".into());
        }
        if matches!(file.state, FileState::Uploaded | FileState::Processing) && !file.uploaded {
            return Err("status upload file lacks completed upload evidence".into());
        }
    }
    let valid_phase = match state.phase {
        Phase::Uploading => state
            .files
            .iter()
            .all(|file| file.state != FileState::Processing),
        Phase::Processing => state
            .files
            .iter()
            .all(|file| !matches!(file.state, FileState::QueuedUpload | FileState::Uploading)),
        Phase::Complete => state
            .files
            .iter()
            .all(|file| file.state == FileState::Complete),
        Phase::Cancelled => state
            .files
            .iter()
            .all(|file| matches!(file.state, FileState::Complete | FileState::Cancelled)),
    };
    if !valid_phase {
        return Err("status upload phase disagrees with its file states".into());
    }
    Ok(())
}

fn validate_project(project: &str) -> Result<(), String> {
    if project.is_empty()
        || project.trim() != project
        || project.len() > 500
        || project.chars().any(|character| character.is_control())
    {
        return Err("status upload project identity is invalid".into());
    }
    Ok(())
}

fn validate_file(file: &FileInput) -> Result<(), String> {
    if file.name.is_empty()
        || file.name.trim() != file.name
        || file.name.len() > 500
        || file.name.chars().any(char::is_control)
        || file.content_type.len() > 200
        || file.content_type.chars().any(char::is_control)
    {
        return Err("status upload file declaration is invalid".into());
    }
    Ok(())
}

fn validate_chain(chain: &[String]) -> Result<(), String> {
    if chain.len() > 16
        || chain.iter().any(|step| {
            step != "sketch"
                || step.trim() != step
                || step.len() > 100
                || step.chars().any(char::is_control)
        })
        || chain
            .iter()
            .filter(|step| step.as_str() == "sketch")
            .count()
            > 1
    {
        return Err("status upload chain is invalid".into());
    }
    Ok(())
}

fn validate_settings(settings: &Map<String, Value>) -> Result<(), String> {
    if settings.len() > 512
        || settings.iter().any(|(key, value)| {
            key.is_empty()
                || key.len() > 128
                || key.chars().any(char::is_control)
                || !matches!(value, Value::Bool(_) | Value::Number(_))
        })
    {
        return Err("status upload process settings are invalid".into());
    }
    Ok(())
}

fn is_shapefile_component(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    let extension = normalized
        .rfind('.')
        .map(|index| &normalized[index..])
        .unwrap_or_default();
    matches!(
        extension,
        ".shp" | ".shx" | ".dbf" | ".prj" | ".cpg" | ".sbn" | ".sbx" | ".qix" | ".fix"
    )
}

fn normalized_content_type(content_type: &str) -> String {
    let content_type = content_type.trim();
    if content_type.is_empty() {
        "application/octet-stream".into()
    } else {
        content_type.into()
    }
}

const fn default_parallel_limit() -> usize {
    MAX_PARALLEL_EFFECTS
}

fn trimmed_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn js_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some(String::new()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => Some(
            values
                .iter()
                .filter_map(js_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        Value::Object(_) => Some("[object Object]".into()),
    }
}

fn js_number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(value)) => value.trim().parse().unwrap_or(f64::NAN),
        Some(Value::Bool(true)) => 1.0,
        Some(Value::Bool(false) | Value::Null) | None => 0.0,
        _ => f64::NAN,
    }
}

fn failed(name: &str, error: String) -> UploadResult {
    UploadResult {
        name: name.into(),
        ok: false,
        error: Some(error),
        detail: None,
    }
}

fn nonempty_error(error: String, fallback: &str) -> String {
    let error = error.trim();
    if error.is_empty() {
        fallback.into()
    } else {
        error.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(value: Value) -> Value {
        serde_json::from_str(&evaluate(value.to_string().as_bytes()).unwrap()).unwrap()
    }

    fn start(files: Value, mode: &str, firestore: bool, chain: Value) -> Value {
        run(json!({
            "op":"start", "project_id":"gisagara", "mode":mode,
            "files":files, "upload_chain":chain,
            "use_firestore_design_data":firestore,
            "process_settings":{"assign_earthing":true}
        }))
    }

    fn advance(reply: &Value, event: Value) -> Value {
        run(json!({"op":"advance", "state":reply["state"], "event":event}))
    }

    #[test]
    fn rejects_loose_shapefile_components_before_any_host_effect() {
        let reply = start(
            json!([
                {"name":"roads.shp","size":2,"content_type":""},
                {"name":"roads.zip","size":3,"content_type":"application/zip"}
            ]),
            "lv_drafting",
            false,
            json!([]),
        );
        assert_eq!(reply["new_results"][0]["name"], "roads.shp");
        assert_eq!(reply["new_results"][0]["error"], SHAPEFILE_COMPONENT_ERROR);
        assert_eq!(reply["effects"].as_array().unwrap().len(), 1);
        assert_eq!(reply["effects"][0]["file_name"], "roads.zip");
        assert_eq!(reply["effects"][0]["content_type"], "application/zip");
    }

    #[test]
    fn upload_target_is_project_domain_and_storage_origin_fenced() {
        let valid = json!({
            "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=secret",
            "blob_path":"gisagara/lv_process/uploads/receipt-1/a.zip",
            "upload_domain":"lv_process",
            "upload_receipt":"receipt-1"
        });
        let target = validate_upload_target("gisagara", UploadMode::LvProcess, &valid).unwrap();
        assert_eq!(target.blob_path, valid["blob_path"]);
        for bad in [
            json!({"session_uri":"https://evil.test/upload/storage/v1/b/b/o?upload_id=x","blob_path":valid["blob_path"],"upload_domain":"lv_process"}),
            json!({"session_uri":valid["session_uri"],"blob_path":"other/lv_process/uploads/receipt-1/a.zip","upload_domain":"lv_process"}),
            json!({"session_uri":valid["session_uri"],"blob_path":valid["blob_path"],"upload_domain":"lv_drafting"}),
            json!({"session_uri":valid["session_uri"],"blob_path":"gisagara/lv_process/uploads/wrong/a.zip","upload_domain":"lv_process","upload_receipt":"receipt-1"}),
        ] {
            assert!(validate_upload_target("gisagara", UploadMode::LvProcess, &bad).is_err());
        }
        let via_wasm_contract = run(json!({
            "op":"validate_target", "project_id":"gisagara", "mode":"lv_process",
            "response":{"data":valid}
        }));
        assert_eq!(via_wasm_contract["schema"], SCHEMA);
    }

    #[test]
    fn every_upload_finishes_before_any_one_file_process_effect() {
        let mut reply = start(
            json!([
                {"name":"a.zip","size":1},
                {"name":"b.zip","size":1}
            ]),
            "lv_process",
            false,
            json!(["sketch"]),
        );
        assert_eq!(reply["effects"][0]["kind"], "upload");
        reply = advance(
            &reply,
            json!({"kind":"upload_succeeded","file_index":0,"blob_path":"projects/gisagara/process/a.zip"}),
        );
        assert!(reply["effects"].as_array().unwrap().is_empty());
        assert_eq!(reply["progress"]["phase"], "uploading");
        reply = advance(
            &reply,
            json!({"kind":"upload_succeeded","file_index":1,"blob_path":"projects/gisagara/process/b.zip"}),
        );
        assert_eq!(reply["progress"]["phase"], "processing");
        assert_eq!(reply["effects"].as_array().unwrap().len(), 2);
        for effect in reply["effects"].as_array().unwrap() {
            assert_eq!(effect["kind"], "process");
            assert_eq!(effect["files"].as_array().unwrap().len(), 1);
            assert_eq!(effect["chain"], json!([]));
            assert_eq!(effect["extra"]["settings"]["assign_earthing"], true);
        }
    }

    #[test]
    fn firestore_and_process_modes_own_chain_and_settings_shape() {
        let cases = [
            (
                "lv_drafting",
                false,
                json!(["sketch"]),
                json!(["sketch"]),
                json!({}),
            ),
            ("lv_drafting", true, json!(["sketch"]), json!([]), json!({})),
            (
                "lv_process",
                false,
                json!(["sketch"]),
                json!([]),
                json!({"settings":{"assign_earthing":true}}),
            ),
        ];
        for (mode, firestore, chain, expected_chain, expected_extra) in cases {
            let first = start(json!([{"name":"a.zip","size":1}]), mode, firestore, chain);
            let next = advance(
                &first,
                json!({"kind":"upload_succeeded","file_index":0,"blob_path":"projects/gisagara/process/a.zip"}),
            );
            assert_eq!(next["effects"][0]["chain"], expected_chain);
            assert_eq!(next["effects"][0]["extra"], expected_extra);
        }
    }

    #[test]
    fn reconciles_each_backend_shape_and_completes_in_event_order() {
        let first = start(
            json!([{"name":"a.zip","size":1},{"name":"b.zip","size":1}]),
            "lv_drafting",
            false,
            json!([]),
        );
        let second = advance(
            &first,
            json!({"kind":"upload_succeeded","file_index":0,"blob_path":"p/a"}),
        );
        let mut reply = advance(
            &second,
            json!({"kind":"upload_succeeded","file_index":1,"blob_path":"p/b"}),
        );
        reply = advance(
            &reply,
            json!({"kind":"process_succeeded","file_index":1,"response":{"error":"worker failed"}}),
        );
        assert_eq!(reply["new_results"][0]["name"], "b.zip");
        assert_eq!(reply["new_results"][0]["error"], "worker failed");
        reply = advance(
            &reply,
            json!({"kind":"process_succeeded","file_index":0,"response":{"results":[{"file_name":"a.zip","submitted":"1"}]}}),
        );
        assert_eq!(reply["progress"]["phase"], "complete");
        assert_eq!(reply["state"]["results"][0]["name"], "b.zip");
        assert_eq!(
            reply["state"]["results"][1],
            json!({"name":"a.zip","ok":true})
        );
    }

    #[test]
    fn concurrency_is_kernel_bounded_and_refilled_by_events() {
        let files = (0..105)
            .map(|index| json!({"name":format!("{index}.zip"),"size":1}))
            .collect::<Vec<_>>();
        let reply = start(json!(files), "lv_drafting", false, json!([]));
        assert_eq!(
            reply["effects"].as_array().unwrap().len(),
            MAX_PARALLEL_EFFECTS
        );
        let next = advance(
            &reply,
            json!({"kind":"upload_failed","file_index":4,"error":"network"}),
        );
        assert_eq!(next["effects"].as_array().unwrap().len(), 1);
        assert_eq!(next["effects"][0]["file_index"], 100);
    }

    #[test]
    fn host_capacity_is_clamped_and_persisted_by_the_kernel() {
        let files = (0..5)
            .map(|index| json!({"name":format!("{index}.zip"),"size":1}))
            .collect::<Vec<_>>();
        let reply = run(json!({
            "op":"start", "project_id":"gisagara", "mode":"lv_drafting",
            "files":files, "parallel_limit":2, "upload_chain":[],
            "process_settings":{}
        }));
        assert_eq!(reply["state"]["parallel_limit"], 2);
        assert_eq!(reply["effects"].as_array().unwrap().len(), 2);
        let clamped = run(json!({
            "op":"start", "project_id":"gisagara", "mode":"lv_drafting",
            "files":[{"name":"a.zip","size":1}], "parallel_limit":999,
            "upload_chain":[], "process_settings":{}
        }));
        assert_eq!(clamped["state"]["parallel_limit"], MAX_PARALLEL_EFFECTS);
    }

    #[test]
    fn tampered_async_state_is_refused_before_scheduling_more_work() {
        let reply = start(
            json!([{"name":"a.zip","size":1},{"name":"b.zip","size":1}]),
            "lv_drafting",
            false,
            json!([]),
        );
        let mut state = reply["state"].clone();
        state["files"][1]["state"] = json!("processing");
        state["files"][1]["uploaded"] = json!(true);
        state["files"][1]["blob_path"] = json!("p/b");
        assert!(evaluate(
            json!({
                "op":"advance", "state":state,
                "event":{"kind":"upload_failed","file_index":0,"error":"offline"}
            })
            .to_string()
            .as_bytes()
        )
        .is_err());
    }

    #[test]
    fn duplicate_stale_events_and_post_completion_cancellation_refuse() {
        let first = start(
            json!([{"name":"a.zip","size":1}]),
            "lv_drafting",
            false,
            json!([]),
        );
        let event = json!({"kind":"upload_failed","file_index":0,"error":"offline"});
        let done = advance(&first, event.clone());
        assert_eq!(done["progress"]["phase"], "complete");
        assert!(evaluate(
            json!({"op":"advance","state":done["state"],"event":event})
                .to_string()
                .as_bytes()
        )
        .is_err());
        assert!(evaluate(
            json!({"op":"cancel","state":done["state"],"reason":"stop"})
                .to_string()
                .as_bytes()
        )
        .is_err());
    }
}
