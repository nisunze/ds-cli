//! Local adapter controls; retained native acceptance requires an explicit test binary.
use super::*;
use ds_cli_contract::args::parse;
use ds_solar_contracts::{PreparedCalculationRequest, PreparedSolarCityInput};
use std::fs;
#[path = "../../../kernel/ds-compute-runtime/tests/common/mod.rs"]
mod prepared_fixture;

fn prepared(root: &Path) -> PathBuf {
    let dir = root.join("prepared");
    fs::create_dir(&dir).unwrap();
    let request: PreparedCalculationRequest =
        serde_json::from_slice(&prepared_fixture::prepared_solar_request()).unwrap();
    fs::write(
        dir.join("bere.prepared.json"),
        serde_json::to_vec_pretty(&request.prepared).unwrap(),
    )
    .unwrap();
    fs::write(dir.join("bere.claim.json"), b"ignored local claim fixture").unwrap();
    dir
}
fn inputs(prepared: &Path, out: &Path) -> Inputs {
    parse(
        &COMMAND,
        &[
            "--prepared".into(),
            prepared.to_str().unwrap().into(),
            "--out".into(),
            out.to_str().unwrap().into(),
            "--lane".into(),
            "canary".into(),
            "--run-id".into(),
            "managed-local-fixture".into(),
        ],
    )
    .unwrap()
}
#[test]
fn contract_three_is_discoverable_and_preserves_defaults_and_four_hour_deadline() {
    assert_eq!(COMMAND.contract, 3);
    let success = serde_json::to_value(ds_cli_contract::outcome::success_envelope(
        COMMAND.id,
        COMMAND.contract,
        json!({"local":true}),
    ))
    .unwrap();
    let failure = Failure::invalid("prepared_batch_invalid", "boundary control");
    let refusal = serde_json::to_value(ds_cli_contract::outcome::error_envelope(
        COMMAND.id,
        COMMAND.contract,
        &failure,
    ))
    .unwrap();
    assert_eq!(success["contract"], 3);
    assert_eq!(refusal["contract"], 3);
    assert_eq!(refusal["error"]["code"], "prepared_batch_invalid");

    assert!(matches!(availability(), Availability::Available));
    assert_eq!(RUN_TIMEOUT, Duration::from_secs(4 * 60 * 60));
    assert_eq!(RUN_TIMEOUT, ds_solar_host::process::MAX_BATCH_TIMEOUT);
    let parsed = parse(
        &COMMAND,
        &["--prepared".into(), "p".into(), "--out".into(), "o".into()],
    )
    .unwrap();
    assert_eq!(parsed.value("concurrency"), Some("10"));
    assert!(!parsed.switch("charts"));
    assert_eq!(parsed.value("lane").unwrap_or("stable"), "stable");
}
#[test]
fn all_selected_preflight_and_expired_deadline_refuse_before_engine_capture() {
    let root = tempfile::tempdir().unwrap();
    let dir = prepared(root.path());
    let out = root.path().join("out");
    let args = inputs(&dir, &out);
    assert_eq!(
        run_with_capture(
            &args,
            || panic!("expired deadline captured an engine"),
            Duration::ZERO
        )
        .unwrap_err()
        .code(),
        "callee_timed_out"
    );
    let path = dir.join("bere.prepared.json");
    let mut input: PreparedSolarCityInput =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    input.input_digest = "sha256:tampered".into();
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(
        run_with_capture(
            &args,
            || panic!("invalid prepared input captured an engine"),
            RUN_TIMEOUT
        )
        .unwrap_err()
        .code(),
        "engine_refused"
    );
    assert!(!out.exists());
    assert_eq!(
        fs::read(dir.join("bere.claim.json")).unwrap(),
        b"ignored local claim fixture"
    );
}
#[cfg(unix)]
#[test]
fn absent_and_corrupt_managed_selection_never_use_legacy_or_path_binaries() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "run::adapter_tests::isolated_no_fallback",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DS_SOLAR_RUN_TEST_ROOT", root.path())
        .env("PATH", root.path())
        .env("DS_SOLAR_BIN", root.path().join("ds-solar"))
        .env_remove("DS_SOLAR_ENGINE_BIN")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(unix)]
#[test]
#[ignore = "isolated environment control"]
fn isolated_no_fallback() {
    use std::os::unix::fs::PermissionsExt;
    let root = PathBuf::from(std::env::var_os("DS_SOLAR_RUN_TEST_ROOT").unwrap());
    for name in ["ds-solar", "solar"] {
        let path = root.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\necho called > '{}'\nexit 99\n",
                root.join("called").display()
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let dir = prepared(&root);
    let out = root.join("out");
    let args = inputs(&dir, &out);
    let base = root.join("addons");
    assert_eq!(
        run_with_capture(
            &args,
            || crate::capture_engine_at(&args, &base),
            RUN_TIMEOUT
        )
        .unwrap_err()
        .code(),
        "solar_engine_missing"
    );
    fs::create_dir_all(base.join("solar/canary")).unwrap();
    fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
    assert_eq!(
        run_with_capture(
            &args,
            || crate::capture_engine_at(&args, &base),
            RUN_TIMEOUT
        )
        .unwrap_err()
        .code(),
        "module_receipt_invalid"
    );
    assert!(!root.join("called").exists());
    assert!(!out.exists());
}
#[cfg(unix)]
#[test]
#[ignore = "requires explicitly qualified retained Solar executable"]
fn managed_batch_runs_exact_offline_artifacts_through_the_retained_capture() {
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    let binary = PathBuf::from(std::env::var_os("DS_MODULE_TEST_SOLAR_BIN").unwrap());
    let identity = crate::Solar::capture_pinned(&binary)
        .unwrap()
        .build_info()
        .unwrap()
        .clone();
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("addons");
    let first = crate::engine_selection_tests::select(&base, "canary", &binary, &identity, b"");
    let dir = prepared(root.path());
    let out = root.path().join("out");
    let args = inputs(&dir, &out);
    let captured = crate::capture_engine_at(&args, &base).unwrap();
    let second = crate::engine_selection_tests::select(
        &base,
        "canary",
        &binary,
        &identity,
        b"\nlocal-test-trailer\n",
    );
    assert_ne!(first, second);
    fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
    let receipt = run_with_capture(&args, || Ok(captured), RUN_TIMEOUT).unwrap();
    let expected: ds_solar_contracts::EngineIdentity = serde_json::from_value(identity).unwrap();
    assert_eq!(receipt["engine"], serde_json::to_value(expected).unwrap());
    assert_eq!(receipt["engine_path"], first.to_str().unwrap());
    assert_eq!(receipt["lane"], "canary");
    assert_eq!(receipt["network"], false);
    assert_eq!(receipt["concurrency"], "10");
    assert_eq!(receipt["charts"], false);
    let batch: ds_solar_contracts::SolarArtifactBatch =
        serde_json::from_slice(&fs::read(out.join("batch.json")).unwrap()).unwrap();
    ds_solar_project::files::verify(&out, &batch).unwrap();
    assert_eq!(receipt["batch_digest"], batch.batch_digest);
    assert_eq!(receipt["artifact_count"], batch.artifacts.len());
    assert!(
        batch
            .artifacts
            .iter()
            .any(|artifact| artifact.name.ends_with("apd-draft-fr.md")),
        "default documents must remain enabled"
    );
    let events: Vec<ds_solar_contracts::ProgressEvent> =
        serde_json::from_slice(&fs::read(out.join("progress.json")).unwrap()).unwrap();
    assert!(!events.is_empty());
    assert_eq!(
        fs::read(dir.join("bere.claim.json")).unwrap(),
        b"ignored local claim fixture"
    );
    assert_eq!(
        crate::capture_engine_at(&args, &base).err().unwrap().code(),
        "module_receipt_invalid"
    );
    // A retained receipt refuses changed executable bytes before any task starts.
    let current = crate::engine_selection_tests::select(
        &base,
        "canary",
        &binary,
        &serde_json::to_value(&receipt["engine"]).unwrap(),
        b"",
    );
    let held = crate::capture_engine_at(&args, &base).unwrap();
    fs::write(&current, b"changed selected executable").unwrap();
    let next = inputs(&dir, &root.path().join("tampered-out"));
    assert_eq!(
        run_with_capture(&next, || Ok(held), RUN_TIMEOUT)
            .unwrap_err()
            .code(),
        "module_receipt_invalid"
    );
    assert!(!root.path().join("tampered-out").exists());
}

#[test]
fn fixed_task_identity_and_prepared_file_limits_refuse_before_capture() {
    let root = tempfile::tempdir().unwrap();
    let dir = prepared(root.path());
    let out = root.path().join("out");
    for (length, expected) in [(256, "capture_control"), (257, "prepared_batch_invalid")] {
        let args = parse(
            &COMMAND,
            &[
                "--prepared".into(),
                dir.to_str().unwrap().into(),
                "--out".into(),
                out.to_str().unwrap().into(),
                "--run-id".into(),
                "r".repeat(length),
            ],
        )
        .unwrap();
        let called = Cell::new(false);
        let failure = run_with_capture(
            &args,
            || {
                called.set(true);
                Err(Failure::invalid(
                    "capture_control",
                    "valid input reached capture",
                ))
            },
            RUN_TIMEOUT,
        )
        .unwrap_err();
        assert_eq!(failure.code(), expected);
        assert_eq!(called.get(), length == 256);
    }
    let path = dir.join("bere.prepared.json");
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(ds_solar_project::preparation::MAX_PREPARED_INPUT_BYTES + 1)
        .unwrap();
    let args = inputs(&dir, &out);
    let failure = run_with_capture(
        &args,
        || panic!("oversized file captured engine"),
        RUN_TIMEOUT,
    )
    .unwrap_err();
    assert_eq!(failure.code(), "engine_refused");
    assert!(
        failure.message().contains("exceed") || failure.message().contains("large"),
        "{}",
        failure.message()
    );
    assert!(!out.exists());
}
