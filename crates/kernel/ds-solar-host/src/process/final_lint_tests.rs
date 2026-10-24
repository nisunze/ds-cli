use super::*;
use ds_cli_exec::solar::Solar;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

const DRAFT: &str = "# Report\n\n| Years | 25 |\n";

fn fixture(root: &Path) -> (Solar, Value, Value) {
    let mut identity = EngineIdentity::current();
    identity.schema.clear();
    identity.schemas = vec![FINAL_LINT_REQUEST_SCHEMA.into()];
    let stamp = "a".repeat(64);
    let probe = ds_solar_contracts::engine_probe::EngineBuildProbe {
        identity: identity.clone(),
        completion_build_manifest_sha256: stamp.clone(),
    };
    let request = FinalLintRequest {
        schema_version: FINAL_LINT_REQUEST_SCHEMA.into(),
        draft: DRAFT.into(),
        final_markdown: DRAFT.into(),
    };
    let digest = sha256_hex(&serde_json::to_vec(&request).unwrap());
    let reply = json!(FinalLintTaskResult {
        schema_version: FINAL_LINT_RESULT_SCHEMA.into(),
        engine: identity,
        completion_build_manifest_sha256: stamp,
        request_sha256: digest.clone(),
        report: ds_solar_contracts::LintReport::default(),
        rendered: "Solar owns the presentation".into(),
    });
    let completion = json!({"status":"ok","result_file":"task-result.json",
        "request_sha256":digest,"schema_version":FINAL_LINT_RESULT_SCHEMA});
    fs::write(root.join("probe"), serde_json::to_vec(&probe).unwrap()).unwrap();
    fs::write(root.join("reply"), serde_json::to_vec(&reply).unwrap()).unwrap();
    fs::write(
        root.join("completion"),
        serde_json::to_vec(&completion).unwrap(),
    )
    .unwrap();
    let executable = root.join("solar");
    fs::write(&executable, "#!/bin/sh\nroot=$(dirname \"$0\")\ncase \"$1\" in\nbuild-info) cat \"$root/probe\";;\nlint-final) touch \"$root/started\"; mkdir \"$5\"; cp \"$root/reply\" \"$5/task-result.json\"; cat \"$root/completion\";;\n*) exit 90;;\nesac\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    (
        Solar::capture_pinned(&executable).unwrap(),
        reply,
        completion,
    )
}

#[test]
fn typed_lint_accepts_additive_future_reply_and_keeps_the_addons_rendering() {
    let root = tempfile::tempdir().unwrap();
    let (engine, mut reply, mut completion) = fixture(root.path());
    reply["schema_version"] = json!("ds-solar.final-lint-result/v2");
    reply["new_member"] = json!({"future":true});
    completion["schema_version"] = reply["schema_version"].clone();
    fs::write(
        root.path().join("reply"),
        serde_json::to_vec(&reply).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("completion"),
        serde_json::to_vec(&completion).unwrap(),
    )
    .unwrap();
    let result = invoke(&engine, root.path(), DRAFT, DRAFT, &CancelToken::new()).unwrap();
    assert!(result.report.errors.is_empty());
    assert_eq!(result.rendered, "Solar owns the presentation");
}

#[test]
fn a_lint_reply_must_belong_to_this_request_and_this_captured_producer() {
    for pointer in [
        "/request_sha256",
        "/engine/version",
        "/completion_build_manifest_sha256",
        "/schema_version",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (engine, mut reply, _) = fixture(root.path());
        *reply.pointer_mut(pointer).unwrap() = json!("wrong");
        fs::write(
            root.path().join("reply"),
            serde_json::to_vec(&reply).unwrap(),
        )
        .unwrap();
        let error = invoke(&engine, root.path(), DRAFT, DRAFT, &CancelToken::new()).unwrap_err();
        assert!(
            error.to_string().contains("identity_or_request_mismatch"),
            "{pointer}: {error}"
        );
    }
}

#[test]
fn an_old_addon_refuses_before_lint_process_and_cancellation_precedes_discovery() {
    let root = tempfile::tempdir().unwrap();
    let (engine, _, _) = fixture(root.path());
    let mut probe: Value =
        serde_json::from_slice(&fs::read(root.path().join("probe")).unwrap()).unwrap();
    probe["schemas"] = json!([]);
    fs::write(
        root.path().join("probe"),
        serde_json::to_vec(&probe).unwrap(),
    )
    .unwrap();
    let error = invoke(&engine, root.path(), DRAFT, DRAFT, &CancelToken::new()).unwrap_err();
    assert_eq!(error.code(), Some("solar_engine_update_required"));
    assert!(!root.path().join("started").exists());
    fs::remove_file(root.path().join("solar")).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(
        invoke(&engine, root.path(), DRAFT, DRAFT, &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn the_real_addon_lints_through_the_host_without_a_linked_algorithm() {
    let root = tempfile::tempdir().unwrap();
    let engine = Solar::capture_pinned(super::super::tests::engine_under_test()).unwrap();
    let valid = invoke(&engine, root.path(), DRAFT, DRAFT, &CancelToken::new()).unwrap();
    assert!(valid.report.errors.is_empty());
    let changed = invoke(
        &engine,
        root.path(),
        DRAFT,
        "# Report\n\n| Years | 26 |\n",
        &CancelToken::new(),
    )
    .unwrap();
    assert!(
        changed
            .report
            .errors
            .iter()
            .any(|finding| finding.code == "immutable-content-changed")
    );
}
