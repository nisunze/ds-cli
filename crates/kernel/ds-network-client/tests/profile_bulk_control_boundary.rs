//! Captured-process qualification of bulk controls and previous-engine compatibility.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::profile::*;
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf, process::Command};
fn path(key: &str) -> PathBuf {
    PathBuf::from(env::var_os(key).expect(key))
}
#[test]
#[ignore = "requires exact new/previous native engines and emitted owning input fixtures"]
fn acceptance_profile_bulk_control_staging_and_previous_engine_compatibility() {
    for (case, engine) in [
        ("current", "DS_PROFILE_REAL_ENGINE"),
        ("old", "DS_PROFILE_OLD_ENGINE"),
    ] {
        let result = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_bulk_control",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_ENGINE_BIN", path(engine))
            .env("DS_PROFILE_BULK_CASE", case)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
#[test]
#[ignore = "isolates captured engine selection in an acceptance subprocess"]
fn isolated_bulk_control() {
    let scratch = tempfile::tempdir().unwrap();
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let open = fs::read(path("DS_PROFILE_OPEN_FIXTURE")).unwrap();
    let mut operation: Value =
        serde_json::from_slice(&fs::read(path("DS_PROFILE_CONTROL_INPUT")).unwrap()).unwrap();
    assert!(serde_json::to_vec(&operation).unwrap().len() > MAX_PROFILE_CONTROL_BYTES);
    let mut session = client
        .open_profile_session(&open, scratch.path(), &|| false)
        .unwrap();
    let history = session.history().clone();
    let run = fs::read_dir(scratch.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let clean = || assert_eq!(fs::read_dir(&run).unwrap().count(), 2);
    let native = |operation| NativeProfileOperationV1::Control { operation };
    // Previous engines keep the existing successful small-control wire shape.
    let small =
        json!({"action":"preview_transaction", "commands":[operation["commands"][0].clone()]});
    let result = session.request(native(small.clone()), &|| false).unwrap();
    let preview: Value = serde_json::from_slice(&result.bytes).unwrap();
    assert_eq!(preview["applied"], false);
    assert_eq!(session.history(), &history);
    clean();
    let result = session.request(native(operation.clone()), &|| false);
    if env::var("DS_PROFILE_BULK_CASE").unwrap() == "old" {
        let error = result.unwrap_err();
        assert_eq!(error.code(), "network_engine_incompatible");
        assert_eq!(error.message, "Update Network to edit this terrain batch.");
        assert_eq!(error.owner_remedy.as_deref(), Some(error.message.as_str()));
        assert_eq!(
            error.owner_detail.as_deref(),
            Some(&json!({
                "component":"network", "required_capability":"profile_bulk_controls_v1",
            }))
        );
        assert!(!session.is_closed());
        assert_eq!(session.history(), &history);
        clean();
        for operation in [
            json!({"action":"preview_structure_station_move","request":{
                "expected_revision":history["model_revision"],"structure_id":"one","station_m":12.},
                "options":{},"held_axis_pin":{}}),
            json!({"action":"apply_structure_retype","request":{
                "expected_revision":history["model_revision"],"structure_ids":["one"],"structure_type_id":"new"}}),
        ] {
            let preflight = session
                .check_control_transport(&native(operation.clone()), Some(&history))
                .unwrap_err();
            assert_eq!(preflight.code(), "network_engine_incompatible");
            let error = session.request(native(operation), &|| false).unwrap_err();
            assert_eq!(error.code(), "network_engine_incompatible");
            assert!(!session.is_closed());
            assert_eq!(session.history(), &history);
            clean();
        }
        let error = session
            .request(NativeProfileOperationV1::ModelLibrary {}, &|| false)
            .unwrap_err();
        assert_eq!(error.code(), "network_engine_incompatible");
        assert!(!session.is_closed());
        assert_eq!(session.history(), &history);
        clean();
        session.request(native(small), &|| false).unwrap();
        session.close(&|| false).unwrap();
        return;
    }
    let preview: Value = serde_json::from_slice(&result.unwrap().bytes).unwrap();
    assert_eq!(preview["applied"], false);
    assert_eq!(session.history(), &history);
    clean();
    let oversized = json!({"action":"preview_transaction", "commands":[{
        "command_id":"over-bound", "command":{"opaque":"x".repeat(MAX_PROFILE_CONTROL_INPUT_BYTES)}
    }]});
    let error = session.request(native(oversized), &|| false).unwrap_err();
    assert_eq!(error.code(), "profile_control_input_invalid");
    assert!(!session.is_closed());
    assert_eq!(session.history(), &history);
    clean();
    // A typed malformed owner payload refuses without changing the retained head.
    let malformed = json!({"action":"apply_transaction", "commands":[{
        "command_id":"malformed", "command":{"opaque":"x".repeat(70*1024)}
    }]});
    assert_eq!(
        session
            .request(native(malformed), &|| false)
            .unwrap_err()
            .code(),
        "network_engine_refused"
    );
    assert_eq!(session.history(), &history);
    assert!(!session.is_closed());
    clean();
    operation["action"] = json!("apply_transaction");
    let applied: Value =
        serde_json::from_slice(&session.request(native(operation), &|| false).unwrap().bytes)
            .unwrap();
    assert_eq!(applied["applied"], true);
    assert_ne!(session.history(), &history);
    clean();
    let retained = session.history().clone();
    let checkpoint = session.close(&|| false).unwrap();
    let mut resumed = client
        .open_profile_session(&checkpoint.bytes, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(resumed.history(), &retained);
    resumed.close(&|| false).unwrap();
}
