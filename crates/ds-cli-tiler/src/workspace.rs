//! `ds tiler workspace` — build one sealed workspace with the native tiler.
//!
//! The engine contract is intentionally narrower than a general process
//! runner: exactly `ds-vector-tiler workspace-tile <workspace-root>`, with a
//! single absolute root that the caller supplied. No source, artifact,
//! tippecanoe, PMTiles, credential, URL, or Cloud Run argument crosses this
//! boundary. The fixed `ds` process contract binds its one pinned Tippecanoe
//! addition through the child environment; the PMTiles writer is linked Rust
//! code inside the owner, not a user-controlled tool or CLI flag.

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DS_VECTOR_TILER, WORKSPACE_TIMEOUT, tiler_availability};

const RESULT_SCHEMA: &str = "ds-vector-tiler.workspace-tile-result/v2";
const INPUT_SCHEMA: &str = "ds-vector-tiler.workspace-tile/v2";
const OPERATION: &str = "workspace-tile";
const ARTIFACT_ROOT: &str = "artifacts/tiles";
const PMTILES_CONTENT_TYPE: &str = "application/vnd.pmtiles";

pub static COMMAND: Command = Command {
    id: "tiler.workspace",
    path: &["tiler", "workspace"],
    contract: 1,
    summary: "Build a local PMTiles artifact from one sealed workspace.",
    purpose: "\
Runs the native `ds-vector-tiler workspace-tile` contract over exactly one \
absolute workspace root. The engine reads its sealed `snapshot/tiles.json`, \
hash-pins inputs and its Tippecanoe addition, uses its linked Rust PMTiles \
writer, and writes no-clobber \
local artifacts below that workspace. This command never starts an HTTP \
service, calls Cloud Run, uploads bytes, or accepts a source/output/tool/URL \
argument. It rejects an engine response unless it attests local-only execution \
and returns only bounded workspace-relative artifact selectors.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "workspace",
        "<dir>",
        "Absolute sealed tiler workspace containing snapshot/tiles.json.",
    )
    .required()],
    output: "\
The verified local execution receipt: status, engine/tool identity, feature/tile \
counts, and workspace-relative result/artifact \
selectors. No absolute output path, source bytes, credential, URL, or remote \
location is returned.",
    examples: &[Example {
        command: "ds tiler workspace --workspace /data/network-tile-run --output json",
        note: "Builds the workspace's no-clobber PMTiles result locally.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "tiler_engine_missing",
            when: "`ds-vector-tiler` is not installed or DS_VECTOR_TILER_BIN is invalid",
            remedy: "install the desktop tiler binary, or set DS_VECTOR_TILER_BIN to a built executable",
        },
        Refusal {
            code: "tiler_addition_missing",
            when: "the required pinned Tippecanoe desktop addition is absent or not absolute",
            remedy: "install tippecanoe beside ds-vector-tiler, or set DS_VECTOR_TILER_TIPPECANOE_BIN to its exact absolute path",
        },
        Refusal {
            code: "workspace_not_absolute",
            when: "--workspace is relative",
            remedy: "pass the canonical absolute root of a sealed tiler workspace",
        },
        Refusal {
            code: "workspace_not_found",
            when: "--workspace cannot be resolved on the local filesystem",
            remedy: "check the workspace root and its permissions",
        },
        Refusal {
            code: "workspace_not_directory",
            when: "--workspace resolves to a file instead of a directory",
            remedy: "pass the directory that contains snapshot/tiles.json",
        },
        Refusal {
            code: "engine_refused",
            when: "the local tiler rejected its manifest, pins, additions or output collision",
            remedy: "read detail.engine, correct the sealed workspace or desktop additions, then retry",
        },
        Refusal {
            code: "tiler_contract_mismatch",
            when: "the engine succeeded but did not attest the required local result contract",
            remedy: "update ds and ds-vector-tiler together; do not substitute an HTTP or Cloud Run response",
        },
        Refusal {
            code: "callee_timed_out",
            when: "the local tiler exceeded the four-hour execution bound",
            remedy: "reduce the sealed snapshot or investigate the local native additions before retrying",
        },
    ],
    reference: Some("docs/reference/tiler.md"),
    availability,
};

fn availability() -> Availability {
    tiler_availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let root = canonical_workspace(inputs.require("workspace")?)?;

    // The only external invocation this domain can make. `OPERATION` is a
    // source constant, and the one typed argument is an already canonical
    // workspace root; no caller-supplied argv can reach the native process.
    let args = [root.into_os_string()];
    let completed = DS_VECTOR_TILER.call(OPERATION, &args, WORKSPACE_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_VECTOR_TILER.failure_from(&completed, OPERATION));
    }
    if completed.truncated {
        return Err(contract_failure(
            "stdout",
            "a complete bounded workspace-tile result document",
        ));
    }
    let result: Value = serde_json::from_str(&completed.stdout).map_err(|_| {
        contract_failure(
            "stdout",
            "one ds-vector-tiler.workspace-tile-result/v2 JSON document",
        )
    })?;
    validate_result(result)
}

fn canonical_workspace(raw: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(Failure::invalid(
            "workspace_not_absolute",
            "--workspace must be an absolute sealed workspace root",
        )
        .remedy("pass the canonical absolute root of a sealed tiler workspace"));
    }
    let path = path.canonicalize().map_err(|_| {
        Failure::invalid(
            "workspace_not_found",
            format!("`{}` cannot be resolved", path.display()),
        )
        .remedy("check the workspace root and its permissions")
    })?;
    if !path.is_dir() {
        return Err(Failure::invalid(
            "workspace_not_directory",
            format!("`{}` is not a directory", path.display()),
        )
        .remedy("pass the directory that contains snapshot/tiles.json"));
    }
    Ok(path)
}

/// Check the engine's result at the process boundary before it becomes `ds`
/// output. This is intentionally strict: a remote-looking response is not a
/// degraded local result and must never become one by interpretation.
fn validate_result(result: Value) -> Result<Value, Failure> {
    let _object = result
        .as_object()
        .ok_or_else(|| contract_failure("document", "a JSON object"))?;
    expect_string(&result, "/schema", RESULT_SCHEMA)?;
    expect_string(&result, "/input_schema", INPUT_SCHEMA)?;
    expect_string(&result, "/operation", OPERATION)?;
    expect_string(&result, "/execution/location", "local")?;
    expect_bool(&result, "/execution/remote_execution", false)?;
    expect_bool(&result, "/execution/cloud_run", false)?;
    expect_bool(&result, "/execution/upload", false)?;
    expect_string(&result, "/execution/input_manifest", "snapshot/tiles.json")?;
    expect_string(&result, "/execution/artifact_root", ARTIFACT_ROOT)?;
    expect_string(&result, "/publication_state", "local_preview_only")?;
    expect_bool(&result, "/shared", false)?;
    expect_bool(&result, "/serving_pointer_registered", false)?;
    expect_string(&result, "/engine/name", "ds-vector-tiler")?;

    let engine_release = string_at(&result, "/engine/release")?;
    if engine_release.trim().is_empty() {
        return Err(contract_failure("/engine/release", "a non-empty release"));
    }

    let output_name = string_at(&result, "/output_name")?;
    if !is_slug(output_name) {
        return Err(contract_failure(
            "/output_name",
            "a canonical local artifact slug",
        ));
    }
    let result_manifest = string_at(&result, "/result_manifest")?;
    let expected_result_manifest = format!("{ARTIFACT_ROOT}/{output_name}.result.json");
    if result_manifest != expected_result_manifest {
        return Err(contract_failure(
            "/result_manifest",
            "the result selector derived from output_name",
        ));
    }

    let tools = validate_tools(&result)?;
    let pmtiles_writer = validate_pmtiles_writer(&result)?;
    let status = string_at(&result, "/status")?;
    let artifacts = array_at(&result, "/artifacts")?;
    let normalized_artifacts = match status {
        "success" => {
            if artifacts.len() != 1 {
                return Err(contract_failure(
                    "/artifacts",
                    "exactly one PMTiles artifact for a successful workspace run",
                ));
            }
            let artifact = validate_artifact(&artifacts[0], output_name)?;
            if result.get("artifact") != Some(&artifacts[0]) {
                return Err(contract_failure(
                    "/artifact",
                    "the compatibility artifact selector matching artifacts[0]",
                ));
            }
            vec![artifact]
        }
        "empty" => {
            if !artifacts.is_empty() {
                return Err(contract_failure(
                    "/artifacts",
                    "an empty artifact inventory for an empty workspace run",
                ));
            }
            if result
                .get("artifact")
                .is_some_and(|artifact| !artifact.is_null())
            {
                return Err(contract_failure(
                    "/artifact",
                    "no singular artifact for an empty workspace run",
                ));
            }
            Vec::new()
        }
        _ => {
            return Err(contract_failure(
                "/status",
                "success or empty after a zero-exit workspace-tile call",
            ));
        }
    };

    let total_features = u64_at(&result, "/total_features")?;
    let tile_count = u64_at(&result, "/tile_count")?;
    Ok(json!({
        "schema": RESULT_SCHEMA,
        "status": status,
        "execution": {
            "location": "local",
            "remote_execution": false,
            "cloud_run": false,
            "upload": false,
        },
        "engine": { "name": "ds-vector-tiler", "release": engine_release },
        "output_name": output_name,
        "result_manifest": result_manifest,
        "total_features": total_features,
        "tile_count": tile_count,
        "tools": tools,
        "pmtiles_writer": pmtiles_writer,
        "artifacts": normalized_artifacts,
    }))
}

fn validate_tools(result: &Value) -> Result<Value, Failure> {
    let version = string_at(result, "/tools/tippecanoe/version")?;
    if version.trim().is_empty() {
        return Err(contract_failure(
            "/tools/tippecanoe/version",
            "a non-empty pinned addition version",
        ));
    }
    let sha256 = string_at(result, "/tools/tippecanoe/sha256")?;
    if !is_sha256(sha256) {
        return Err(contract_failure(
            "/tools/tippecanoe/sha256",
            "sha256:<64 lowercase hexadecimal characters>",
        ));
    }
    Ok(json!({
        "tippecanoe": { "version": version, "sha256": sha256 },
    }))
}

/// `pmtiles` is linked Rust implementation, rather than an executable
/// addition. Accept only the precise v2 provenance shape so a returned
/// receipt cannot quietly re-introduce a Go/sidecar tool into local execution.
fn validate_pmtiles_writer(result: &Value) -> Result<Value, Failure> {
    expect_string(result, "/pmtiles_writer/implementation", "linked-rust")?;
    expect_string(result, "/pmtiles_writer/crate", "pmtiles")?;
    let version = string_at(result, "/pmtiles_writer/version")?;
    if version.trim().is_empty() {
        return Err(contract_failure(
            "/pmtiles_writer/version",
            "a non-empty linked Rust crate version",
        ));
    }
    Ok(json!({
        "implementation": "linked-rust",
        "crate": "pmtiles",
        "version": version,
    }))
}

fn validate_artifact(artifact: &Value, output_name: &str) -> Result<Value, Failure> {
    let _object = artifact
        .as_object()
        .ok_or_else(|| contract_failure("/artifacts/0", "an artifact object"))?;
    let logical_name = artifact
        .get("logical_name")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_failure("/artifacts/0/logical_name", "a PMTiles logical name"))?;
    let expected_logical_name = format!("{output_name}.pmtiles");
    if logical_name != expected_logical_name {
        return Err(contract_failure(
            "/artifacts/0/logical_name",
            "the PMTiles logical name derived from output_name",
        ));
    }
    let path = artifact
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_failure("/artifacts/0/path", "a workspace-relative selector"))?;
    let expected_path = format!("{ARTIFACT_ROOT}/{logical_name}");
    if path != expected_path || !is_logical_selector(path) {
        return Err(contract_failure(
            "/artifacts/0/path",
            "the bounded PMTiles selector below artifacts/tiles",
        ));
    }
    let sha256 = artifact
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_failure("/artifacts/0/sha256", "a pinned SHA-256"))?;
    if !is_sha256(sha256) {
        return Err(contract_failure(
            "/artifacts/0/sha256",
            "sha256:<64 lowercase hexadecimal characters>",
        ));
    }
    let size_bytes = artifact
        .get("size_bytes")
        .and_then(Value::as_u64)
        .filter(|size| *size > 0)
        .ok_or_else(|| contract_failure("/artifacts/0/size_bytes", "a positive byte size"))?;
    let content_type = artifact
        .get("content_type")
        .and_then(Value::as_str)
        .ok_or_else(|| contract_failure("/artifacts/0/content_type", PMTILES_CONTENT_TYPE))?;
    if content_type != PMTILES_CONTENT_TYPE {
        return Err(contract_failure(
            "/artifacts/0/content_type",
            PMTILES_CONTENT_TYPE,
        ));
    }
    Ok(json!({
        "logical_name": logical_name,
        "path": path,
        "sha256": sha256,
        "size_bytes": size_bytes,
        "content_type": content_type,
    }))
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Failure> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| contract_failure(pointer, "a string"))
}

fn u64_at(value: &Value, pointer: &str) -> Result<u64, Failure> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .ok_or_else(|| contract_failure(pointer, "a non-negative whole number"))
}

fn array_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, Failure> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| contract_failure(pointer, "an array"))
}

fn expect_string(value: &Value, pointer: &str, expected: &str) -> Result<(), Failure> {
    if string_at(value, pointer)? != expected {
        return Err(contract_failure(pointer, expected));
    }
    Ok(())
}

fn expect_bool(value: &Value, pointer: &str, expected: bool) -> Result<(), Failure> {
    if value.pointer(pointer).and_then(Value::as_bool) != Some(expected) {
        return Err(contract_failure(
            pointer,
            if expected { "true" } else { "false" },
        ));
    }
    Ok(())
}

fn contract_failure(field: &str, expected: &str) -> Failure {
    Failure::failed(
        "tiler_contract_mismatch",
        format!("ds-vector-tiler returned an invalid `{field}` field"),
    )
    .remedy(
        "update ds and ds-vector-tiler together; do not substitute an HTTP or Cloud Run response",
    )
    .detail(json!({ "field": field, "expected": expected }))
}

fn is_slug(value: &str) -> bool {
    value.len() >= 2
        && value.len() <= 200
        && !value.starts_with(['_', '-'])
        && !value.ends_with(['_', '-'])
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn is_sha256(value: &str) -> bool {
    let Some(raw) = value.strip_prefix("sha256:") else {
        return false;
    };
    raw.len() == 64
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_logical_selector(value: &str) -> bool {
    !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains('\\')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}\nresult    {}\nfeatures  {}\ntiles     {}\n",
        data["status"].as_str().unwrap_or("?"),
        data["result_manifest"].as_str().unwrap_or(""),
        data["total_features"],
        data["tile_count"],
    );
    for artifact in data["artifacts"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "artifact  {}  {}\n",
            artifact["logical_name"].as_str().unwrap_or(""),
            artifact["sha256"].as_str().unwrap_or(""),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result() -> Value {
        json!({
            "schema": RESULT_SCHEMA,
            "input_schema": INPUT_SCHEMA,
            "operation": OPERATION,
            "status": "success",
            "execution": {
                "location": "local",
                "remote_execution": false,
                "cloud_run": false,
                "upload": false,
                "input_manifest": "snapshot/tiles.json",
                "artifact_root": ARTIFACT_ROOT,
            },
            "publication_state": "local_preview_only",
            "shared": false,
            "serving_pointer_registered": false,
            "engine": { "name": "ds-vector-tiler", "release": "0.2.0" },
            "output_name": "network",
            "result_manifest": "artifacts/tiles/network.result.json",
            "tools": {
                "tippecanoe": { "version": "2.0", "sha256": format!("sha256:{}", "a".repeat(64)) },
            },
            "pmtiles_writer": { "implementation": "linked-rust", "crate": "pmtiles", "version": "0.24.0" },
            "total_features": 2,
            "tile_count": 3,
            "artifacts": [{
                "logical_name": "network.pmtiles",
                "path": "artifacts/tiles/network.pmtiles",
                "sha256": format!("sha256:{}", "c".repeat(64)),
                "size_bytes": 12,
                "content_type": PMTILES_CONTENT_TYPE,
            }],
        })
    }

    #[test]
    fn keeps_only_local_logical_artifact_receipts() {
        let mut input = result();
        input["artifact"] = input["artifacts"][0].clone();
        let output = validate_result(input).unwrap();
        assert_eq!(output["status"], "success");
        assert_eq!(output["execution"]["cloud_run"], false);
        assert_eq!(
            output["artifacts"][0]["path"],
            "artifacts/tiles/network.pmtiles"
        );
        assert!(output.get("workspace").is_none());
    }

    #[test]
    fn refuses_a_result_that_claims_cloud_execution() {
        let mut input = result();
        input["artifact"] = input["artifacts"][0].clone();
        input["execution"]["cloud_run"] = Value::Bool(true);
        let failure = validate_result(input).unwrap_err();
        assert_eq!(failure.code(), "tiler_contract_mismatch");
    }

    #[test]
    fn refuses_nonlogical_artifact_selectors() {
        let mut input = result();
        input["artifact"] = input["artifacts"][0].clone();
        input["artifacts"][0]["path"] = Value::String("/tmp/network.pmtiles".to_string());
        input["artifact"] = input["artifacts"][0].clone();
        let failure = validate_result(input).unwrap_err();
        assert_eq!(failure.code(), "tiler_contract_mismatch");
    }
}
