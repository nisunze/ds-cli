//! The packaging entrypoint answers exactly what installation admits.
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn run(input: &[u8], args: &[&str]) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ds-module-admission"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A refused argument list may exit before reading; a closed pipe is fine.
    let _ = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn request(component: &str, platform: &str, build_info: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ds.module-build-admission-request/v1",
        "component": component,
        "platform": platform,
        "buildInfo": build_info,
    }))
    .unwrap()
}

fn network(target: &str) -> Value {
    json!({"schema":"ds.engine-build/v1","module":"network",
        "engineIdentity":{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
            "sourceSha":"a".repeat(40),"dependencyClosureSha256":"b".repeat(64),"environment":"native",
            "runtime":{"environment":"native","logicalCpus":8,"internalWorkerThreads":1},"jsonObjectOrder":"sorted",
            "capabilities":["lv_processing_v1","lv_publication_v1","lv_analysis_validation_v1","grid_query_v1",
                "grid_inspect_v1","grid_apply_v1","grid_publication_v1","profile_session_v1","report_engineering_v1",
                "report_sizing_v1","report_feeder_selection_v1"]},
        "build":{"target":target,"profile":"release","rustc":"rustc 1.97.0","cargoLockSha256":"c".repeat(64)}})
}

#[test]
fn admitted_network_build_answers_its_release_facts() {
    let (code, stdout, stderr) = run(
        &request("network", "windows", network("x86_64-pc-windows-msvc")),
        &[],
    );
    assert_eq!(code, 0, "{stderr}");
    let answer: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        answer,
        json!({"schema":"ds.module-build-admission/v1","component":"network","platform":"windows",
            "target":"x86_64-pc-windows-msvc","owner":"ds-network","protocol":"ds.network-engine/v1",
            "filename":"ds-network.exe","version":"0.1.0","sourceSha":"a".repeat(40),
            "manifestSha256":"b".repeat(64),"release":format!("ds-network@0.1.0+{}", "a".repeat(40))})
    );
}

#[test]
fn solar_and_reporter_answer_their_module_protocol_and_filename() {
    let source = "d".repeat(40);
    let solar = json!({"schema":"ds.engine-build/v1","name":"ds-solar-engine","version":"0.1.0","source_sha":source,
        "target":"x86_64-unknown-linux-gnu","profile":"release","rustc":"rustc","cargo_lock_sha256":"e".repeat(64),
        "build_manifest_sha256":"f".repeat(64),"schemas":["ds-solar.prepared-calculation-result/v1","ds-solar.result/v1",
            "ds-solar.prepared-city-input/v1","ds-solar.worker-budget/v1"]});
    let (code, stdout, stderr) = run(&request("solar", "linux", solar), &[]);
    assert_eq!(code, 0, "{stderr}");
    let answer: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(answer["filename"], "solar");
    assert_eq!(answer["protocol"], "ds.module/v1");
    let reporter = json!({"schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report",
        "package_version":"0.1.0","source_sha":source,"engine_version":format!("ds-network-reporter@0.1.0+{source}"),
        "features":[],"target":"x86_64-unknown-linux-gnu","profile":"release","rustc":"rustc",
        "cargo_lock_sha256":"e".repeat(64),"build_manifest_sha256":"f".repeat(64),
        "schemas":["ds.engine-build/v1","ds-network-reporter.task-result/v1"]});
    let (code, stdout, stderr) = run(&request("reporter", "linux", reporter), &[]);
    assert_eq!(code, 0, "{stderr}");
    let answer: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(answer["filename"], "reporter");
    assert_eq!(answer["owner"], "ds-network-reporter");
}

#[test]
fn incompatible_builds_refuse_with_the_installed_reason() {
    let mut missing = network("x86_64-unknown-linux-gnu");
    missing["engineIdentity"]["capabilities"] =
        json!(["lv_processing_v1", "grid_query_v1", "grid_inspect_v1"]);
    let (code, stdout, stderr) = run(&request("network", "linux", missing), &[]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(
        stderr.starts_with("module_engine_incompatible: required capability is absent"),
        "{stderr}"
    );
    let (code, _, stderr) = run(
        &request("network", "linux", network("x86_64-pc-windows-msvc")),
        &[],
    );
    assert_eq!(code, 1);
    assert!(stderr.contains("protocol or target"), "{stderr}");
}

#[test]
fn requests_are_closed_bounded_and_argument_free() {
    let mut extra: Value = serde_json::from_slice(&request(
        "network",
        "linux",
        network("x86_64-unknown-linux-gnu"),
    ))
    .unwrap();
    extra["unexpected"] = json!(true);
    for input in [
        serde_json::to_vec(&extra).unwrap(),
        request("core", "linux", json!({})),
        b"{torn".to_vec(),
        vec![b' '; 64 * 1024 + 1],
    ] {
        let (code, stdout, stderr) = run(&input, &[]);
        assert_eq!(code, 1, "{stderr}");
        assert!(stdout.is_empty());
        assert!(
            stderr.starts_with("module_admission_request_invalid:"),
            "{stderr}"
        );
    }
    let mut schema: Value = serde_json::from_slice(&request(
        "network",
        "linux",
        network("x86_64-unknown-linux-gnu"),
    ))
    .unwrap();
    schema["schema"] = json!("ds.module-build-admission-request/v2");
    assert_eq!(run(&serde_json::to_vec(&schema).unwrap(), &[]).0, 1);
    let (code, _, stderr) = run(
        &request("network", "macos", network("x86_64-unknown-linux-gnu")),
        &[],
    );
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("module_platform_unsupported:"),
        "{stderr}"
    );
    let (code, stdout, _) = run(
        &request("network", "linux", network("x86_64-unknown-linux-gnu")),
        &["--allow"],
    );
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
}

#[test]
fn browser_engine_and_ui_units_answer_their_unit_facts() {
    let network = json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
        "sourceSha":"a".repeat(40),"dependencyClosureSha256":"b".repeat(64),"environment":"wasm",
        "runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},"jsonObjectOrder":"sorted",
        "capabilities":["lv_processing_v1"]});
    let (code, stdout, stderr) = run(&request("network", "browser", network.clone()), &[]);
    assert_eq!(code, 0, "{stderr}");
    let answer: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(answer["kind"], "engine");
    assert_eq!(answer["protocol"], "ds.network-wasm/v1");
    assert_eq!(
        answer["files"],
        json!([
            "ds_network_wasm.js",
            "ds_network_wasm.d.ts",
            "ds_network_wasm_bg.wasm",
            "ds_network_wasm_bg.wasm.d.ts"
        ])
    );
    assert_eq!(
        answer["release"],
        format!("ds-network@0.1.0+{}", "a".repeat(40))
    );
    let identity: Value = serde_json::from_str(include_str!(
        "../../ds-command-kernel/src/module_release/fixtures/interactions-ui-engine-build.json"
    ))
    .unwrap();
    let (code, stdout, stderr) = run(&request("interactions", "browser", identity.clone()), &[]);
    assert_eq!(code, 0, "{stderr}");
    let answer: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(answer["kind"], "ui");
    assert_eq!(answer["owner"], "ds-interactions");
    assert_eq!(answer["buildManifest"], identity);
    assert_eq!(answer["manifestSha256"].as_str().unwrap().len(), 64);
    let mut native = network;
    native["environment"] = json!("native");
    let (code, _, stderr) = run(&request("network", "browser", native), &[]);
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("module_engine_incompatible:"),
        "{stderr}"
    );
    // Solar is a browser engine unit admitted by its prepared city calculation.
    let solar = json!({"schema":"ds.solar-engine/v1","protocolMajor":1,"environment":"wasm","coreVersion":"0.1.0",
        "sourceSha":"c".repeat(40),"dependencyClosureSha256":"d".repeat(64),"capabilities":["calculate_prepared_city_v1"]});
    let (code, stdout, stderr) = run(&request("solar", "browser", solar.clone()), &[]);
    assert_eq!(code, 0, "{stderr}");
    let answer: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(answer["kind"], "engine");
    assert_eq!(answer["owner"], "ds-solar");
    assert_eq!(answer["protocol"], "ds.solar-wasm/v1");
    assert_eq!(answer["files"][0], "ds_solar_wasm.js");
    assert_eq!(
        answer["release"],
        format!("ds-solar@0.1.0+{}", "c".repeat(40))
    );
    let (code, _, stderr) = run(&request("solar", "browser", json!({})), &[]);
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("module_engine_incompatible:"),
        "{stderr}"
    );
    let mut uncapable = solar;
    // A unit without the prepared city entry the Kernel calls refuses.
    uncapable["capabilities"] = json!(["calculate_prepared_v1"]);
    let (code, _, stderr) = run(&request("solar", "browser", uncapable), &[]);
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("module_engine_incompatible:"),
        "{stderr}"
    );
    let (code, _, _) = run(&request("command-kernel", "browser", identity), &[]);
    assert_eq!(
        code, 1,
        "an interactions identity cannot admit the Kernel unit"
    );
}
