use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
fn required(key: &str) -> PathBuf {
    PathBuf::from(env::var_os(key).unwrap_or_else(|| panic!("{key} must name an acceptance input")))
}
fn run_isolated(engine: &Path, request: &Path, golden: Option<&Path>, expected: Option<&str>) {
    let mut c = Command::new(env::current_exe().unwrap());
    c.args([
        "--exact",
        "isolated_native_client_case",
        "--ignored",
        "--nocapture",
    ])
    .env("DS_NETWORK_ENGINE_BIN", engine)
    .env("DS_NETWORK_TEST_REQUEST", request)
    .env_remove("DS_NETWORK_TEST_GOLDEN")
    .env_remove("DS_NETWORK_TEST_EXPECTED_CODE");
    if let Some(path) = golden {
        c.env("DS_NETWORK_TEST_GOLDEN", path);
    }
    if let Some(code) = expected {
        c.env("DS_NETWORK_TEST_EXPECTED_CODE", code);
    }
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "isolated client failed:\n{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
#[ignore = "requires explicit built owner and immutable pre-extraction golden directory"]
fn acceptance_real_owner_preserves_preextraction_byte_goldens() {
    let engine = required("DS_NETWORK_REAL_ENGINE_TEST");
    let fixtures = required("DS_NETWORK_BOUNDARY_FIXTURES");
    for name in ["mixed", "approved"] {
        run_isolated(
            &engine,
            &fixtures.join(format!("{name}.request.json")),
            Some(&fixtures.join(format!("{name}.result.json"))),
            None,
        );
    }
}
#[test]
#[ignore = "requires explicit native receipt fixture and immutable request fixtures"]
fn acceptance_receiver_refuses_incompatible_incomplete_and_corrupt_owners() {
    let fixture = required("DS_NETWORK_TEST_OWNER_FIXTURE");
    let request = required("DS_NETWORK_BOUNDARY_FIXTURES").join("mixed.request.json");
    let temp = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        ("incompatible", "network_engine_incompatible"),
        ("malformed", "network_receipt_invalid"),
        ("no-terminal", "network_receipt_invalid"),
        ("partial", "network_result_invalid"),
        ("wrong-order", "network_result_invalid"),
        ("wrong-digest", "network_result_digest_mismatch"),
        ("ignored-cancel", "callee_cancelled"),
    ] {
        let name = if cfg!(windows) {
            format!("{mode}.exe")
        } else {
            mode.into()
        };
        let exe = temp.path().join(name);
        fs::copy(&fixture, &exe).unwrap();
        run_isolated(&exe, &request, None, Some(expected));
    }
    run_isolated(
        &temp.path().join(if cfg!(windows) {
            "missing.exe"
        } else {
            "missing"
        }),
        &request,
        None,
        Some("network_engine_missing"),
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires explicit adversarial executable and frozen request fixtures"]
fn acceptance_captured_selection_survives_active_pointer_update() {
    let mut command = Command::new(env::current_exe().unwrap());
    let result = command
        .args([
            "--exact",
            "isolated_captured_selection_case",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(unix)]
#[test]
#[ignore = "internal subprocess isolates active-pointer environment"]
fn isolated_captured_selection_case() {
    use ds_network_client::{CapturedNetworkClient, NetworkEnginePin};
    use ds_network_contracts::engine::JsonObjectOrder;
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let alpha = directory.path().join("captured-alpha");
    let beta = directory.path().join("incompatible");
    let active = directory.path().join("active");
    let counter = directory.path().join("probes");
    fs::copy(required("DS_NETWORK_TEST_OWNER_FIXTURE"), &alpha).unwrap();
    fs::copy(required("DS_NETWORK_TEST_OWNER_FIXTURE"), &beta).unwrap();
    symlink(&alpha, &active).unwrap();
    env::set_var("DS_NETWORK_ENGINE_BIN", &active);
    fs::write(&counter, "").unwrap();
    let request =
        fs::read(required("DS_NETWORK_BOUNDARY_FIXTURES").join("mixed.request.json")).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let pin = captured.pin();
    assert_eq!(captured.executable(), fs::canonicalize(&alpha).unwrap());
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
    fs::remove_file(&active).unwrap();
    symlink(&beta, &active).unwrap();
    let first = captured
        .process_lv_with_order(
            &request,
            directory.path(),
            &|| false,
            JsonObjectOrder::Sorted,
        )
        .unwrap();
    let second = captured
        .process_lv_with_order(
            &request,
            directory.path(),
            &|| false,
            JsonObjectOrder::Sorted,
        )
        .unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(
        fs::read_to_string(&counter).unwrap().lines().count(),
        1,
        "jobs must not reprobe or recapture active selection"
    );
    let persisted: NetworkEnginePin =
        serde_json::from_slice(&serde_json::to_vec(&pin).unwrap()).unwrap();
    let reopened = CapturedNetworkClient::from_pin(&persisted, &|| false).unwrap();
    assert_eq!(reopened.executable(), fs::canonicalize(&alpha).unwrap());
    assert_eq!(
        reopened
            .process_lv_with_order(
                &request,
                directory.path(),
                &|| false,
                JsonObjectOrder::Sorted
            )
            .unwrap()
            .bytes,
        first.bytes
    );
    assert_eq!(
        fs::read_to_string(&counter).unwrap().lines().count(),
        2,
        "resume admits retained artifact once"
    );
    let mut corrupt = persisted.clone();
    corrupt.executable_sha256 = "0".repeat(64);
    match CapturedNetworkClient::from_pin(&corrupt, &|| false) {
        Err(error) => assert_eq!(error.code(), "network_engine_changed"),
        Ok(_) => panic!("corrupt retained artifact pin was admitted"),
    }
    assert_eq!(
        fs::read_to_string(&counter).unwrap().lines().count(),
        2,
        "corruption must refuse before probing"
    );
    match CapturedNetworkClient::capture(1, &|| false) {
        Err(error) => assert_eq!(error.code(), "network_engine_incompatible"),
        Ok(_) => panic!("new incompatible active unit was admitted"),
    }
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 3);
}
#[test]
#[ignore = "internal helper invoked only in an isolated acceptance subprocess"]
fn isolated_native_client_case() {
    let request = fs::read(required("DS_NETWORK_TEST_REQUEST")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let ignored_cancel = required("DS_NETWORK_ENGINE_BIN")
        .file_stem()
        .is_some_and(|name| name == "ignored-cancel");
    let cancellation = std::cell::Cell::new(false);
    let cancel = || {
        if ignored_cancel && !cancellation.get() {
            let result_exists = fs::read_dir(scratch.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|run| run.path().join("output/result.json").is_file());
            cancellation.set(result_exists);
        }
        cancellation.get()
    };
    let result = ds_network_client::process_lv(&request, 1, scratch.path(), &cancel);
    if let Ok(expected) = env::var("DS_NETWORK_TEST_EXPECTED_CODE") {
        let failure = result.expect_err("adversarial owner must refuse");
        assert_eq!(failure.code(), expected);
        return;
    }
    let outcome = result.expect("real owner must complete");
    let golden = fs::read(required("DS_NETWORK_TEST_GOLDEN")).unwrap();
    assert_eq!(
        outcome.bytes, golden,
        "process handoff changed original result bytes"
    );
    assert!(!outcome.cancelled);
    assert_eq!(outcome.identity.schema, "ds.network-engine/v1");
    assert_eq!(outcome.identity.protocol_major, 1);
    assert_eq!(outcome.runtime.internal_worker_threads, 1);
    let exe = fs::canonicalize(required("DS_NETWORK_ENGINE_BIN")).unwrap();
    assert_eq!(
        outcome.executable_sha256,
        format!("{:x}", Sha256::digest(fs::read(exe).unwrap()))
    );
    assert_eq!(
        outcome.result.jobs.len(),
        ds_network_contracts::lv_processing::decode_native_lv_request(&request)
            .unwrap()
            .jobs
            .len()
    );
}
