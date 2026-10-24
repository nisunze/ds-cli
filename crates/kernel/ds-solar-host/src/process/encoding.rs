//! Encoding is a Solar task. Core verifies its exact bytes and passes the DTO on.
use super::*;
use crate::output::GeneratedDocument;
use base64::Engine;
use ds_cli_exec::solar::{Solar, Task as SolarTask};
use ds_solar_contracts::output_task::*;

pub(super) fn encode(
    engine: &Path,
    scratch: &Path,
    artifacts: &[GeneratedDocument],
    cancel: &CancelToken,
) -> Result<Vec<OutputFile>, String> {
    check_cancel(cancel)?;
    let selected =
        Solar::capture_pinned(engine).map_err(|error| format!("{}: {error}", error.code()))?;
    selected.with_task_executable(SolarTask::EncodeOutputs, |path| encode_inner(path, scratch, artifacts, cancel))
        .map_err(|error| if matches!(error.code(), "unknown_task" | "solar_engine_update_required") {
            "solar_engine_update_required: install a Solar addon for the captured lane that declares output encoding".into()
        } else { format!("{}: {error}", error.code()) })?
}

fn encode_inner(
    engine: &Path,
    scratch: &Path,
    artifacts: &[GeneratedDocument],
    cancel: &CancelToken,
) -> Result<Vec<OutputFile>, String> {
    let captured = PreparedEngine::capture_transport(engine, scratch, 1, cancel)?;
    super::preparation::validate_completion_stamp(
        &captured.identity,
        &captured.completion_build_manifest_sha256,
    )?;
    let request = OutputEncodingRequest {
        schema_version: OUTPUT_ENCODING_REQUEST_SCHEMA.into(),
        outputs: artifacts.iter().map(|file| file.entry.clone()).collect(),
    };
    let bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > OUTPUT_ENCODING_MAX_REQUEST_BYTES {
        return Err("solar_task_encoding_request_too_large".into());
    }
    for artifact in artifacts {
        check_cancel(cancel)?;
        files::write(
            &captured
                .directory
                .path()
                .join("artifacts")
                .join(&artifact.entry.name),
            &artifact.bytes,
        )
        .map_err(|error| error.to_string())?;
    }
    let input = captured.directory.path().join("request.json");
    let out = captured.directory.path().join("out");
    files::write(&input, &bytes).map_err(|error| error.to_string())?;
    // The caller's calculation lease already owns the worker budget; encoding
    // is sequential on that same lease and starts no parallel computation.
    verify_executable(&captured.executable, &captured.stamp)?;
    let stdout = run_task(
        &captured.executable,
        captured.directory.path(),
        1,
        Task::Encoding {
            request: &input,
            out: &out,
        },
        cancel,
        Duration::from_secs(60),
    );
    verify_executable(&captured.executable, &captured.stamp)?;
    let stdout = stdout?;
    check_cancel(cancel)?;
    ordinary_directory(&out)?;
    if fs::read_dir(&out)
        .map_err(|error| error.to_string())?
        .count()
        != 1
    {
        return Err("solar_task_encoding_output_not_closed".into());
    }
    let result = files::read(
        &out.join("task-result.json"),
        OUTPUT_ENCODING_MAX_RESULT_BYTES,
    )
    .map_err(|error| error.to_string())?;
    let result: OutputEncodingTaskResult = serde_json::from_slice(&result)
        .map_err(|error| format!("solar_task_encoding_result_invalid: {error}"))?;
    let completion: serde_json::Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("solar_task_encoding_completion_invalid: {error}"))?;
    let request_sha256 = sha256_hex(&bytes);
    if !same_family(&result.schema_version, OUTPUT_ENCODING_RESULT_SCHEMA)
        || result.engine != captured.identity
        || result.request_sha256 != request_sha256
        || result.completion_build_manifest_sha256 != captured.completion_build_manifest_sha256
        || completion["status"] != "ok"
        || completion["result_file"] != "task-result.json"
        || completion["request_sha256"] != request_sha256
        || completion["schema_version"] != result.schema_version
        || result.outputs.len() != artifacts.len()
    {
        return Err("solar_task_encoding_identity_or_request_mismatch".into());
    }
    for (encoded, artifact) in result.outputs.iter().zip(artifacts) {
        check_cancel(cancel)?;
        if encoded.entry != artifact.entry
            || encoded.body_base64.len() > MAX_OUTPUT_FILE_BYTES.div_ceil(3) * 4
            || base64::engine::general_purpose::STANDARD
                .decode(&encoded.body_base64)
                .map_err(|_| "solar_task_encoding_invalid_base64")?
                != artifact.bytes
        {
            return Err("solar_task_encoding_bytes_mismatch".into());
        }
    }
    Ok(result.outputs)
}

fn same_family(actual: &str, expected: &str) -> bool {
    let Some((family, version)) = actual.rsplit_once("/v") else {
        return false;
    };
    expected
        .rsplit_once("/v")
        .is_some_and(|(expected, _)| family == expected)
        && !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_digit())
}
