//! Explicit executable acceptance; subprocesses isolate addon selection.
use ds_design_workspace::Workspace;
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf, process::Command};

fn required(key: &str) -> PathBuf {
    env::var_os(key).map(PathBuf::from).expect(key)
}
fn document(name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":"ds.design.snapshot/v1","transformer":name,"crs":"EPSG:4326",
        "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},
        "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]}},
        "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true})).unwrap()
}

#[test]
#[ignore = "requires explicit DS_NETWORK_WORKSPACE_TEST_ENGINE"]
fn acceptance_retained_workspace_engine_and_missing_addon_preserve_work() {
    for case in [
        "active-update",
        "corrupt-pin",
        "missing-addon",
        "historical-pin",
    ] {
        let result = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_native_workspace_case",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_WORKSPACE_TEST_CASE", case)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{case}:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
#[ignore = "internal environment-isolated executable acceptance helper"]
fn isolated_native_workspace_case() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    Workspace::init(&root, "explicit-project").unwrap();
    let mut workspace = Workspace::open(&root).unwrap();
    let first = workspace
        .write(&document("T1"), None, "create-one")
        .unwrap();
    workspace
        .write(&document("T2"), None, "create-two")
        .unwrap();
    let before = workspace.status().unwrap();
    let case = env::var("DS_NETWORK_WORKSPACE_TEST_CASE").unwrap();
    if case == "missing-addon" {
        env::set_var("DS_NETWORK_ENGINE_BIN", temp.path().join("absent"));
        let error = workspace
            .prepare_run("unavailable", &["T1".into()])
            .unwrap_err();
        assert!(error.to_string().contains("network_engine_missing"));
        assert_eq!(workspace.status().unwrap(), before);
        assert!(workspace.run_status("unavailable").is_err());
        assert_eq!(
            workspace.read("T1", None).unwrap().0,
            first["revision"].as_str().unwrap()
        );
        return;
    }
    let retained = temp.path().join(if cfg!(windows) {
        "retained.exe"
    } else {
        "retained"
    });
    fs::copy(required("DS_NETWORK_WORKSPACE_TEST_ENGINE"), &retained).unwrap();
    env::set_var("DS_NETWORK_ENGINE_BIN", &retained);
    workspace
        .prepare_run("captured", &["T2".into(), "T1".into()])
        .unwrap();
    let captured_status = workspace.status().unwrap();
    let database = rusqlite::Connection::open(root.join("design.sqlite")).unwrap();
    if case == "corrupt-pin" || case == "historical-pin" {
        if case == "corrupt-pin" {
            fs::write(&retained, "different retained artifact").unwrap();
        } else {
            database
                .execute(
                    "UPDATE runs SET engine='0.1.0:historical-host-digest' WHERE id='captured'",
                    [],
                )
                .unwrap();
        }
        let error = workspace.process("captured", 1).unwrap_err().to_string();
        assert!(
            error.contains(if case == "corrupt-pin" {
                "network_engine_changed"
            } else {
                "network_engine_pin_upgrade_required"
            }),
            "{error}"
        );
        let status = workspace.run_status("captured").unwrap();
        assert!(status["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["state"] == "pending" && j["result_sha256"].is_null()));
        assert_eq!(workspace.status().unwrap(), captured_status);
        return;
    }
    // Changing active selection must not redirect a captured run or metadata retry.
    env::set_var(
        "DS_NETWORK_ENGINE_BIN",
        temp.path().join("new-active-unavailable"),
    );
    assert_eq!(
        workspace
            .prepare_run("captured", &["T2".into(), "T1".into()])
            .unwrap()["resumed"],
        true
    );
    assert!(workspace
        .prepare_run("new-run", &["T1".into()])
        .unwrap_err()
        .to_string()
        .contains("network_engine_missing"));
    let run = workspace
        .process(
            "captured",
            2.min(std::thread::available_parallelism().unwrap().get()),
        )
        .unwrap();
    assert_eq!((run.completed, run.failed, run.pending), (2, 0, 0));
    let pin: String = database
        .query_row("SELECT engine FROM runs WHERE id='captured'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let pin: Value = serde_json::from_str(&pin).unwrap();
    assert_eq!(pin["executable"], retained.to_str().unwrap());
    let bytes: Vec<u8> = database
        .query_row(
            "SELECT result FROM jobs WHERE run='captured' AND transformer='T1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["worker_executable_sha256"], pin["executable_sha256"]);
    assert_eq!(result["engine_version"], pin["identity"]["coreVersion"]);
    // Completed run and saved reports do not need another active engine.
    fs::remove_file(retained).unwrap();
    assert_eq!(workspace.process("captured", 1).unwrap().completed, 2);
    workspace
        .report_inputs("captured", "T1", &temp.path().join("report"))
        .unwrap();
    assert_eq!(
        workspace.read("T1", None).unwrap().0,
        first["revision"].as_str().unwrap()
    );
}
