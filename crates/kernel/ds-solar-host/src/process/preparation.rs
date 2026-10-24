//! Bounded cancellable transport for the fixed Solar preparation task.
use super::*;
use ds_solar_contracts::preparation_task::*;
use ds_solar_contracts::reference_task::{
    MAX_REFERENCE_OPEN_ARTIFACT_BYTES, REFERENCE_OPEN_ARTIFACT_NAMES, ReferenceOpenArtifact,
};
use ds_solar_contracts::{BundleBytes, SolarError, SolarResult};

pub(crate) fn descriptors(bundle: &BundleBytes) -> SolarResult<Vec<ReferenceOpenArtifact>> {
    raw_files(bundle)
        .into_iter()
        .map(|(name, bytes)| {
            if bytes.is_empty() || bytes.len() > MAX_REFERENCE_OPEN_ARTIFACT_BYTES {
                return Err(SolarError::reference_unit_invalid(
                    "reference candidate exceeds its artifact bound",
                ));
            }
            Ok(ReferenceOpenArtifact {
                name: name.into(),
                bytes: bytes.len(),
                sha256: ds_solar_contracts::digest::hex_sha256(bytes),
            })
        })
        .collect()
}

fn raw_files(bundle: &BundleBytes) -> [(&str, &[u8]); 4] {
    [
        (REFERENCE_OPEN_ARTIFACT_NAMES[0], &bundle.weather_json),
        (
            REFERENCE_OPEN_ARTIFACT_NAMES[1],
            &bundle.reference_unit_json,
        ),
        (
            REFERENCE_OPEN_ARTIFACT_NAMES[2],
            &bundle.reference_unit_parquet,
        ),
        (REFERENCE_OPEN_ARTIFACT_NAMES[3], &bundle.manifest_json),
    ]
}

pub(crate) fn invoke(
    engine: &Path,
    scratch: &Path,
    operation: ReferencePreparationOperation,
    bundle: Option<&BundleBytes>,
    cancel: &CancelToken,
) -> Result<ReferencePreparationOutput, ProcessFailure> {
    invoke_inner(engine, scratch, operation, bundle, cancel).map_err(ProcessFailure::from)
}

fn invoke_inner(
    engine: &Path,
    scratch: &Path,
    operation: ReferencePreparationOperation,
    bundle: Option<&BundleBytes>,
    cancel: &CancelToken,
) -> Result<ReferencePreparationOutput, String> {
    let captured = PreparedEngine::capture_transport(engine, scratch, 1, cancel)?;
    validate_completion_stamp(
        &captured.identity,
        &captured.completion_build_manifest_sha256,
    )?;
    let request = ReferencePreparationRequest {
        schema_version: REFERENCE_PREPARATION_REQUEST_SCHEMA.into(),
        operation,
    };
    let bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_REFERENCE_PREPARATION_REQUEST_BYTES {
        return Err("solar_preparation_request_too_large".into());
    }
    if let Some(bundle) = bundle {
        descriptors(bundle).map_err(|error| error.to_string())?;
        for (name, body) in raw_files(bundle) {
            files::write(
                &captured.directory.path().join("reference").join(name),
                body,
            )
            .map_err(|error| error.to_string())?;
        }
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
        Task::Preparation {
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
        return Err("solar_task_preparation_output_not_closed".into());
    }
    let result = files::read(
        &out.join("task-result.json"),
        MAX_REFERENCE_PREPARATION_RESULT_BYTES,
    )
    .map_err(|error| error.to_string())?;
    let result: ReferencePreparationTaskResult = serde_json::from_slice(&result)
        .map_err(|error| format!("solar_task_preparation_result_invalid: {error}"))?;
    let completion: serde_json::Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("solar_task_preparation_completion_invalid: {error}"))?;
    let request_sha256 = sha256_hex(&bytes);
    if !same_family(&result.schema_version, REFERENCE_PREPARATION_RESULT_SCHEMA)
        || result.request_sha256 != request_sha256
        || result.engine != captured.identity
        || result.completion_build_manifest_sha256 != captured.completion_build_manifest_sha256
        || completion["status"] != "ok"
        || completion["result_file"] != "task-result.json"
        || completion["request_sha256"] != request_sha256
        || completion["schema_version"] != result.schema_version
    {
        return Err("solar_task_preparation_identity_or_request_mismatch".into());
    }
    match (&request.operation, &result.output) {
        (
            ReferencePreparationOperation::Plan { city, .. },
            ReferencePreparationOutput::Plan { city: planned, .. },
        ) => {
            if city.identity != planned.identity
                || city.revision != planned.revision
                || city.run_options != planned.run_options
            {
                return Err("solar_task_preparation_city_mismatch".into());
            }
            planned.validate().map_err(|error| error.to_string())?;
        }
        (
            ReferencePreparationOperation::Prepare {
                city, prepared_at, ..
            },
            ReferencePreparationOutput::Prepared { input },
        ) => {
            if city.identity != input.city.identity
                || city.revision != input.city.revision
                || city.run_options != input.city.run_options
                || input.prepared_at != *prepared_at
            {
                return Err("solar_task_preparation_city_mismatch".into());
            }
            input.validate().map_err(|error| error.to_string())?;
        }
        _ => return Err("solar_task_preparation_operation_mismatch".into()),
    }
    check_cancel(cancel)?;
    Ok(result.output)
}

pub(super) fn validate_completion_stamp(
    identity: &EngineIdentity,
    stamp: &str,
) -> Result<(), String> {
    if stamp.len() != 64
        || !stamp
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || (!identity.schema.is_empty() && stamp != identity.build_manifest_sha256)
    {
        return Err("solar_engine_identity_completion_stamp_invalid".into());
    }
    Ok(())
}

pub(super) fn same_family(actual: &str, expected: &str) -> bool {
    let Some((family, version)) = actual.rsplit_once("/v") else {
        return false;
    };
    expected
        .rsplit_once("/v")
        .is_some_and(|(expected, _)| family == expected)
        && !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
