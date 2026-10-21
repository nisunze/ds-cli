//! Real CLI/MCP dispatch and native task parity; no project authority or network.
#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use ds_solar_contracts::{PREPARED_CALCULATION_TASK_SCHEMA, PreparedCalculationTaskResult};
use serde_json::{Value, json};

fn cli(args: &[&str], engine: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("DS_SOLAR_ENGINE_BIN", engine)
        .env("DS_SRE_REPORTING", "off")
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn task(engine: &Path, request: &Path, out: &Path) -> Value {
    cli(
        &[
            "solar",
            "calculate-prepared",
            "--request",
            request.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--output",
            "json",
        ],
        engine,
    )
}

fn stub(root: &Path, identity: &Value, task_body: &str) -> std::path::PathBuf {
    let path = root.join("engine");
    fs::write(
        &path,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$@" >> "$0.calls"
case "$1" in
build-info) printf '%s\n' '{identity}' ;;
calculate-prepared) {task_body} ;;
*) exit 91 ;;
esac
"#
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn discovery_is_the_same_declared_cli_and_mcp_contract_without_spawning_the_owner() {
    let root = tempfile::tempdir().unwrap();
    let engine = stub(root.path(), &json!({}), "exit 92");
    let descriptor = cli(
        &[
            "capabilities",
            "solar.calculate-prepared",
            "--output",
            "json",
        ],
        &engine,
    );
    let command = &descriptor["data"]["command"];
    assert_eq!(command["authority"], "none");
    assert_eq!(command["effect"], "local_file_write");
    assert_eq!(command["confirmation_required"], false);
    assert_eq!(
        command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| input["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["request", "out", "lane"]
    );
    let lane = &command["inputs"][2];
    assert_eq!(lane["default"], "stable");
    assert_eq!(lane["choices"], json!(["stable", "canary"]));
    let search = cli(
        &[
            "capabilities",
            "--search",
            "calculate-prepared",
            "--output",
            "json",
        ],
        &engine,
    );
    assert!(
        search["data"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "solar.calculate-prepared")
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "serve"])
        .env("DS_SOLAR_ENGINE_BIN", &engine)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{}", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ds_solar","arguments":{"operation":"describe","command":"solar.calculate-prepared"}}})).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|message| message["id"] == 1)
        .unwrap();
    let envelope = &response["result"]["structuredContent"];
    assert_eq!(envelope["data"]["command"], *command, "{response}");
    assert!(
        !engine.with_extension("calls").exists(),
        "discovery must not invoke build-info or task execution"
    );
}

#[test]
fn invalid_addon_lane_refuses_before_any_engine_probe_or_task() {
    let root = tempfile::tempdir().unwrap();
    let engine = stub(root.path(), &json!({}), "exit 92");
    for command in ["engine", "calculate-prepared"] {
        let mut args = vec!["solar", command, "--lane", "local", "--output", "json"];
        if command == "calculate-prepared" {
            args.extend(["--request", "absent.json", "--out", "absent-output"]);
        }
        let refused = cli(&args, &engine);
        assert_eq!(refused["error"]["code"], "invalid_choice", "{refused}");
    }
    assert!(!engine.with_extension("calls").exists());
}

#[test]
fn incompatible_engine_refuses_before_execution_and_native_refusal_stays_typed() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("request with spaces.json");
    let out = root.path().join("fresh out");
    fs::write(&request, "{}").unwrap();
    let identity = json!({"name":"ds-solar-engine","version":"0.1.0","schemas":[]});
    let engine = stub(root.path(), &identity, "exit 92");
    let refused = task(&engine, &request, &out);
    assert_eq!(
        refused["error"]["code"],
        "solar_prepared_schema_unavailable"
    );
    assert_eq!(
        fs::read_to_string(engine.with_extension("calls")).unwrap(),
        "build-info\n"
    );
    assert!(!out.exists());

    fs::remove_file(engine.with_extension("calls")).unwrap();
    let identity = json!({"name":"ds-solar-engine","version":"0.1.0","schemas":[PREPARED_CALCULATION_TASK_SCHEMA]});
    stub(
        root.path(),
        &identity,
        r#"printf '%s\n' '{"status":"error","error":{"code":"invalid_input","message":"sealed prepared input required"}}'; exit 1"#,
    );
    let refused = task(&engine, &request, &out);
    assert_eq!(refused["error"]["code"], "engine_refused");
    assert_eq!(
        refused["error"]["detail"]["native"]["code"],
        "invalid_input"
    );
    assert_eq!(
        fs::read_to_string(engine.with_extension("calls"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "build-info",
            "calculate-prepared",
            "--request",
            request.to_str().unwrap(),
            "--out",
            out.to_str().unwrap()
        ]
    );
    assert!(!out.exists());

    stub(root.path(), &identity, "exit 0");
    let refused = task(&engine, &request, &out);
    assert_eq!(
        refused["error"]["code"], "solar_prepared_result_invalid",
        "a zero exit without a native file result is not completion"
    );
}

#[test]
fn captured_engine_survives_an_active_pointer_switch() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("first")).unwrap();
    fs::create_dir(root.path().join("second")).unwrap();
    let identity = json!({"name":"ds-solar-engine","version":"0.1.0","schemas":[PREPARED_CALCULATION_TASK_SCHEMA]});
    let first = stub(&root.path().join("first"), &identity, "exit 0");
    let second = stub(
        &root.path().join("second"),
        &json!({"name":"ds-solar-engine","version":"0.1.0","schemas":[]}),
        "exit 92",
    );
    let body = fs::read_to_string(&first).unwrap().replace("build-info) printf", r#"build-info) ln -sf "$(dirname "$0")/../second/engine" "$(dirname "$0")/../current"; printf"#);
    fs::write(&first, body).unwrap();
    let active = root.path().join("current");
    std::os::unix::fs::symlink(&first, &active).unwrap();
    let request = root.path().join("request.json");
    fs::write(&request, "{}").unwrap();
    let out = root.path().join("out");
    let refused = task(&active, &request, &out);
    // The first engine was invoked despite switching its public pointer. It
    // returned no result, which is independently refused as incomplete.
    assert_eq!(refused["error"]["code"], "solar_prepared_result_invalid");
    assert!(
        fs::read_to_string(first.with_extension("calls"))
            .unwrap()
            .contains("calculate-prepared")
    );
    assert!(!second.with_extension("calls").exists());
    assert_eq!(
        task(&active, &request, &out)["error"]["code"],
        "solar_prepared_schema_unavailable"
    );
}

#[test]
fn actual_native_prepared_task_matches_cli_and_mcp_with_zero_provider_calls() {
    // Release qualification builds both Solar executables and sets these; a
    // machine without them reports a named skip instead of proving nothing.
    let (Some(engine), Some(preparation_tool)) = (
        std::env::var_os("DS_SOLAR_ENGINE_BIN").map(std::path::PathBuf::from),
        std::env::var_os("DS_SOLAR_BIN"),
    ) else {
        eprintln!(
            "SKIPPED: native calculate-prepared parity needs DS_SOLAR_ENGINE_BIN (the \
             calculate-prepared engine) and DS_SOLAR_BIN (the fixture preparation tool)"
        );
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../ds-solar/fixtures");
    let prepared = root.path().join("prepared");
    let preparation = Command::new(preparation_tool)
        .args(["prepare", "--fixtures"])
        .arg(fixtures)
        .args(["--city", "aderm_bere", "--out"])
        .arg(&prepared)
        .arg("--cache")
        .arg(root.path().join("cache"))
        .output()
        .unwrap();
    assert!(
        preparation.status.success(),
        "{}",
        String::from_utf8_lossy(&preparation.stderr)
    );
    let input: Value =
        serde_json::from_slice(&fs::read(prepared.join("aderm_bere.prepared.json")).unwrap())
            .unwrap();
    let request = root.path().join("request.json");
    fs::write(
        &request,
        serde_json::to_vec(
            &json!({"prepared":input,"render_charts":false,"run_id":"cli-native-parity"}),
        )
        .unwrap(),
    )
    .unwrap();
    let direct_out = root.path().join("direct");
    let direct = Command::new(&engine)
        .args(["calculate-prepared", "--request"])
        .arg(&request)
        .arg("--out")
        .arg(&direct_out)
        .output()
        .unwrap();
    assert!(
        direct.status.success(),
        "{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    let direct: PreparedCalculationTaskResult =
        serde_json::from_slice(&fs::read(direct_out.join("task-result.json")).unwrap()).unwrap();
    let out = root.path().join("cli");
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["solar", "calculate-prepared", "--request"])
        .arg(&request)
        .arg("--out")
        .arg(&out)
        .args(["--output", "json"])
        .env("DS_SOLAR_ENGINE_BIN", &engine)
        .env("HTTP_PROXY", &proxy_url)
        .env("HTTPS_PROXY", &proxy_url)
        .env("ALL_PROXY", &proxy_url)
        .env("NO_PROXY", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.len() < 8192);
    assert_eq!(
        proxy.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let data = &result["data"];
    assert_eq!(data["result_digest"], direct.result.result_digest);
    assert_eq!(data["request_sha256"], direct.request_sha256);
    assert_eq!(
        data["engine"],
        serde_json::to_value(&direct.engine).unwrap()
    );
    assert_eq!(data["artifact_count"], direct.outputs.len());
    assert_eq!(data["network"], false);
    assert_eq!(
        data["result_file"],
        out.join("task-result.json").to_str().unwrap()
    );
    if direct.engine.source_sha.is_empty() {
        assert!(
            data["engine"].get("source_sha").is_none(),
            "development provenance must not be invented"
        );
    }
    let receipt: PreparedCalculationTaskResult =
        serde_json::from_slice(&fs::read(out.join("task-result.json")).unwrap()).unwrap();
    for artifact in &receipt.outputs {
        let bytes = fs::read(out.join("artifacts").join(&artifact.name)).unwrap();
        assert_eq!(bytes.len() as u64, artifact.byte_count);
        assert_eq!(
            ds_solar_contracts::digest::sha256_hex(&bytes),
            artifact.content_digest
        );
    }
    let mcp_out = root.path().join("mcp");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "serve"])
        .env("DS_SOLAR_ENGINE_BIN", &engine)
        .env("HTTP_PROXY", &proxy_url)
        .env("HTTPS_PROXY", &proxy_url)
        .env("ALL_PROXY", &proxy_url)
        .env("NO_PROXY", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{}", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ds_solar","arguments":{"operation":"invoke","command":"solar.calculate-prepared","arguments":{"request":request,"out":mcp_out}}}})).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|message| message["id"] == 1)
        .unwrap();
    let envelope = &response["result"]["structuredContent"];
    assert_eq!(envelope["status"], "ok", "{response}");
    assert_eq!(envelope["data"]["result_digest"], data["result_digest"]);
    assert_eq!(envelope["data"]["request_sha256"], data["request_sha256"]);
    assert_eq!(envelope["data"]["engine"], data["engine"]);
    assert_eq!(
        proxy.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let fake_root = root.path().join("fake");
    fs::create_dir(&fake_root).unwrap();
    let identity = serde_json::to_value(&direct.engine).unwrap();
    let fake = stub(
        &fake_root,
        &identity,
        r#"mkdir "$5"; cp "$0.receipt" "$5/task-result.json""#,
    );
    let mut wrong: Value =
        serde_json::from_slice(&fs::read(out.join("task-result.json")).unwrap()).unwrap();
    wrong["engine"]["version"] = json!("incompatible");
    fs::write(
        fake.with_extension("receipt"),
        serde_json::to_vec(&wrong).unwrap(),
    )
    .unwrap();
    assert_eq!(
        task(&fake, &request, &root.path().join("wrong-build"))["error"]["code"],
        "solar_prepared_result_invalid"
    );
    let mut huge_identity = identity;
    huge_identity["version"] = json!("x".repeat(16 * 1024));
    wrong["engine"] = huge_identity.clone();
    stub(
        &fake_root,
        &huge_identity,
        r#"mkdir "$5"; cp "$0.receipt" "$5/task-result.json""#,
    );
    fs::write(
        fake.with_extension("receipt"),
        serde_json::to_vec(&wrong).unwrap(),
    )
    .unwrap();
    assert_eq!(
        task(&fake, &request, &root.path().join("oversized-completion"))["error"]["code"],
        "solar_prepared_result_invalid"
    );
    let before = fs::read(&request).unwrap();
    let again = task(&engine, &request, &out);
    assert_eq!(again["error"]["code"], "engine_refused");
    assert_eq!(fs::read(&request).unwrap(), before);
}
