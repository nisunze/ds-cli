//! Bounded process exchange for the Solar-owned final-document lint.
use super::*;
use ds_solar_contracts::final_lint::*;

/// Preserve a declared task refusal separately from process or local IO errors.
#[derive(Debug)]
pub enum Error {
    Task(Box<ds_core_contracts::Failure>),
    Local(String),
}
impl Error {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Task(failure) => Some(failure.code()),
            Self::Local(_) => None,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Task(failure) => write!(f, "{}: {failure}", failure.code()),
            Self::Local(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for Error {}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Local(message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Local(message.into())
    }
}

/// Run only the declared lint task on the caller's captured addon. The host
/// verifies provenance and request identity; it carries no lint algorithm.
pub fn invoke(
    engine: &ds_cli_exec::solar::Solar,
    scratch: &Path,
    draft: &str,
    final_markdown: &str,
    cancel: &CancelToken,
) -> Result<FinalLintTaskResult, Error> {
    check_cancel(cancel)?;
    if draft.len() as u64 > MAX_FINAL_LINT_DOCUMENT_BYTES
        || final_markdown.len() as u64 > MAX_FINAL_LINT_DOCUMENT_BYTES
    {
        return Err("solar_final_lint_document_too_large".into());
    }
    engine
        .with_task_executable(ds_cli_exec::solar::Task::FinalLint, |path| {
            invoke_inner(path, scratch, draft, final_markdown, cancel)
        })
        .map_err(|failure| Error::Task(Box::new(failure)))?
        .map_err(Error::Local)
}

fn invoke_inner(
    engine: &Path,
    scratch: &Path,
    draft: &str,
    final_markdown: &str,
    cancel: &CancelToken,
) -> Result<FinalLintTaskResult, String> {
    let captured = PreparedEngine::capture_transport(engine, scratch, 1, cancel)?;
    preparation::validate_completion_stamp(
        &captured.identity,
        &captured.completion_build_manifest_sha256,
    )?;
    let request = FinalLintRequest {
        schema_version: FINAL_LINT_REQUEST_SCHEMA.into(),
        draft: draft.into(),
        final_markdown: final_markdown.into(),
    };
    let bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_FINAL_LINT_REQUEST_BYTES {
        return Err("solar_final_lint_request_too_large".into());
    }
    let input = captured.directory.path().join("request.json");
    let out = captured.directory.path().join("out");
    files::write(&input, &bytes).map_err(|error| error.to_string())?;
    let budget = budget::acquire(1, cancel)?;
    verify_executable(&captured.executable, &captured.stamp)?;
    let reply = run_task(
        &captured.executable,
        captured.directory.path(),
        budget.workers(),
        Task::FinalLint {
            request: &input,
            out: &out,
        },
        cancel,
        Duration::from_secs(60),
    );
    verify_executable(&captured.executable, &captured.stamp)?;
    let stdout = reply?;
    check_cancel(cancel)?;
    ordinary_directory(&out)?;
    if fs::read_dir(&out)
        .map_err(|error| error.to_string())?
        .count()
        != 1
    {
        return Err("solar_task_final_lint_output_not_closed".into());
    }
    let result = files::read(&out.join("task-result.json"), MAX_FINAL_LINT_RESULT_BYTES)
        .map_err(|error| error.to_string())?;
    let result: FinalLintTaskResult = serde_json::from_slice(&result)
        .map_err(|error| format!("solar_task_final_lint_result_invalid: {error}"))?;
    let completion: serde_json::Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("solar_task_final_lint_completion_invalid: {error}"))?;
    let request_sha256 = sha256_hex(&bytes);
    if !preparation::same_family(&result.schema_version, FINAL_LINT_RESULT_SCHEMA)
        || result.request_sha256 != request_sha256
        || result.engine != captured.identity
        || result.completion_build_manifest_sha256 != captured.completion_build_manifest_sha256
        || completion["status"] != "ok"
        || completion["result_file"] != "task-result.json"
        || completion["request_sha256"] != request_sha256
        || completion["schema_version"] != result.schema_version
    {
        return Err("solar_task_final_lint_identity_or_request_mismatch".into());
    }
    Ok(result)
}

#[cfg(all(test, unix))]
#[path = "final_lint_tests.rs"]
mod tests;
