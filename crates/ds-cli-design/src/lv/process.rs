//! `ds design lv process` — bounded native Fast LV batch compute.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network::network::native_fast_lv::{
    MAX_NATIVE_FAST_LV_INPUT_BYTES, NativeFastLvError, decode_native_fast_lv_request,
    encode_native_fast_lv_result, process_native_fast_lv,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub static COMMAND: Command = Command {
    id: "design.lv.process",
    path: &["design", "lv", "process"],
    contract: 1,
    summary: "Run bounded Fast LV processing from local files on native Rayon.",
    purpose: "Reads one ds.fast-lv.request/v1 file containing 1..=32 independent transformer jobs, validates the closed envelope and a 100,000-feature aggregate bound, and writes one complete ds.fast-lv.result/v1 file. The request carries caller-owned GeoJSON layers, explicit process settings, and explicit network config only: it has no project identity, credentials, browser table addresses, map state, mutable session, or generic engine operation. Independent jobs execute through ds-network's native Rayon batch while input order is preserved. The terminal answer is a bounded digest receipt; full processed layers exist only in --out.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "input",
            "<path>",
            "Closed ds.fast-lv.request/v1 batch document (maximum 64 MiB).",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "Absent path for the complete ds.fast-lv.result/v1 document.",
        )
        .required(),
    ],
    output: "`out`, input/result SHA-256 digests, byte count, engine version, native execution environment, Rayon worker count, job/success/failure counts, and one input-ordered name/status row per job. Processed layers and per-job diagnostics are written only to `out`.",
    examples: &[Example {
        command: "ds design lv process --input ./fast-lv-request.json --out ./fast-lv-result.json --output json",
        note: "Run the closed local batch without a Desktop session or project identity.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "fast_lv_source_not_found",
            when: "--input is absent, not a regular file, or cannot be read",
            remedy: "pass a readable local ds.fast-lv.request/v1 file",
        },
        Refusal {
            code: "fast_lv_input_too_large",
            when: "the request file exceeds 64 MiB",
            remedy: "split the work into smaller closed batches",
        },
        Refusal {
            code: "fast_lv_input_invalid",
            when: "the request is malformed, has unknown fields/settings, or a layer is not a FeatureCollection",
            remedy: "write the exact ds.fast-lv.request/v1 shape shown in the design reference",
        },
        Refusal {
            code: "fast_lv_schema_unsupported",
            when: "the request schema is not ds.fast-lv.request/v1",
            remedy: "migrate the request to the supported v1 schema",
        },
        Refusal {
            code: "fast_lv_bound_refused",
            when: "job, feature, layer, config-sheet, name, or uniqueness bounds are exceeded",
            remedy: "split the batch or shorten the named field reported by the refusal",
        },
        Refusal {
            code: "fast_lv_output_exists",
            when: "--out already exists",
            remedy: "choose a new result path; this command never overwrites",
        },
        Refusal {
            code: "fast_lv_output_write_failed",
            when: "the result cannot be durably written at --out",
            remedy: "choose a writable absent path and retry from the unchanged input",
        },
        Refusal {
            code: "fast_lv_output_too_large",
            when: "the complete result exceeds 256 MiB",
            remedy: "split the request into smaller batches; results are never truncated",
        },
        Refusal {
            code: "fast_lv_result_encoding_failed",
            when: "the owner result cannot be encoded as its v1 document",
            remedy: "keep the input unchanged and report this internal encoding failure",
        },
    ],
    reference: Some("docs/reference/design.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let input_path = PathBuf::from(inputs.require("input")?);
    let output_path = PathBuf::from(inputs.require("out")?);
    if output_path.exists() {
        return Err(Failure::conflict(
            "fast_lv_output_exists",
            "The Fast LV result path already exists.",
        )
        .remedy("Choose a new --out path; existing results are never overwritten."));
    }

    let input = bounded_read(&input_path)?;
    let input_sha256 = sha256(&input);
    let request = decode_native_fast_lv_request(&input).map_err(map_owner_error)?;
    let runtime = ds_network::execution_runtime();
    let result = process_native_fast_lv(request).map_err(map_owner_error)?;
    let jobs = result.jobs.len();
    let succeeded = result.jobs.iter().filter(|job| job.ok).count();
    let failed = jobs - succeeded;
    let statuses = result
        .jobs
        .iter()
        .map(|job| {
            json!({
                "transformer_name": job.transformer_name,
                "ok": job.ok,
            })
        })
        .collect::<Vec<_>>();
    let engine_core_version = result.engine_core_version;
    let output = encode_native_fast_lv_result(&result).map_err(map_owner_error)?;
    let result_sha256 = sha256(&output);
    write_new(&output_path, &output)?;

    Ok(json!({
        "out": output_path,
        "input_sha256": input_sha256,
        "result_sha256": result_sha256,
        "byte_count": output.len(),
        "engine_core_version": engine_core_version,
        "execution_environment": runtime.environment.as_str(),
        "rayon_worker_threads": runtime.internal_worker_threads,
        "jobs": jobs,
        "succeeded": succeeded,
        "failed": failed,
        "results": statuses,
    }))
}

pub fn render(value: &Value) -> String {
    let jobs = value["jobs"].as_u64().unwrap_or(0);
    let succeeded = value["succeeded"].as_u64().unwrap_or(0);
    let failed = value["failed"].as_u64().unwrap_or(0);
    let mut text = format!(
        "Fast LV processed {jobs} transformer(s): {succeeded} succeeded, {failed} failed.\nResult: {}\nSHA-256: {}",
        value["out"].as_str().unwrap_or(""),
        value["result_sha256"].as_str().unwrap_or(""),
    );
    if failed > 0 {
        text.push_str("\nInspect the result document for per-transformer diagnostics.");
    }
    text
}

fn bounded_read(path: &Path) -> Result<Vec<u8>, Failure> {
    let mut file = File::open(path).map_err(|error| {
        Failure::invalid(
            "fast_lv_source_not_found",
            format!("Could not open the Fast LV request: {error}"),
        )
        .remedy("Pass a readable local ds.fast-lv.request/v1 file.")
    })?;
    let metadata = file.metadata().map_err(|error| {
        Failure::invalid(
            "fast_lv_source_not_found",
            format!("Could not inspect the Fast LV request: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(Failure::invalid(
            "fast_lv_source_not_found",
            "The Fast LV request is not a regular file.",
        )
        .remedy("Pass a readable local ds.fast-lv.request/v1 file."));
    }
    if metadata.len() > MAX_NATIVE_FAST_LV_INPUT_BYTES as u64 {
        return Err(map_owner_error(NativeFastLvError::InputTooLarge {
            actual: metadata.len().try_into().unwrap_or(usize::MAX),
            maximum: MAX_NATIVE_FAST_LV_INPUT_BYTES,
        }));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take((MAX_NATIVE_FAST_LV_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Failure::invalid(
                "fast_lv_source_not_found",
                format!("Could not read the Fast LV request: {error}"),
            )
        })?;
    if bytes.len() > MAX_NATIVE_FAST_LV_INPUT_BYTES {
        return Err(map_owner_error(NativeFastLvError::InputTooLarge {
            actual: bytes.len(),
            maximum: MAX_NATIVE_FAST_LV_INPUT_BYTES,
        }));
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        Failure::failed(
            "fast_lv_output_write_failed",
            "The Fast LV result path has no file name.",
        )
    })?;
    let stage = parent.join(format!(
        ".{}.ds-fast-lv-{}-{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::hard_link(&stage, path)?;
        // The hard link is already the complete immutable result. A failed
        // stage cleanup is not permission to report failure after publishing
        // it (and a retry would honestly find `--out` occupied).
        let _ = std::fs::remove_file(&stage);
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&stage);
        if path.exists() {
            return Err(Failure::conflict(
                "fast_lv_output_exists",
                "The Fast LV result path already exists.",
            )
            .remedy("Choose a new --out path; existing results are never overwritten."));
        }
        return Err(Failure::failed(
            "fast_lv_output_write_failed",
            format!("Could not write the Fast LV result: {error}"),
        )
        .remedy("Choose a writable absent path and retry from the unchanged input."));
    }
    Ok(())
}

fn map_owner_error(error: NativeFastLvError) -> Failure {
    let code = error.code();
    match code {
        "fast_lv_input_too_large"
        | "fast_lv_input_invalid"
        | "fast_lv_schema_unsupported"
        | "fast_lv_bound_refused" => Failure::invalid(code, error.to_string()),
        "fast_lv_output_too_large" => Failure::failed(code, error.to_string())
            .remedy("Split the request into smaller batches; results are never truncated."),
        "fast_lv_result_encoding_failed" => Failure::internal(code, error.to_string()),
        _ => Failure::internal("fast_lv_result_encoding_failed", error.to_string()),
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_is_offline_local_file_compute() {
        assert_eq!(COMMAND.authority, Authority::None);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.path, ["design", "lv", "process"]);
    }

    #[test]
    fn renderer_never_projects_full_layers_or_errors() {
        let text = render(&json!({
            "jobs": 2,
            "succeeded": 1,
            "failed": 1,
            "out": "result.json",
            "result_sha256": "abc",
            "results": [{ "transformer_name": "T1", "ok": false }],
        }));
        assert!(text.contains("1 succeeded, 1 failed"));
        assert!(!text.contains("T1"));
        assert!(!text.contains("gdfs"));
    }
}
