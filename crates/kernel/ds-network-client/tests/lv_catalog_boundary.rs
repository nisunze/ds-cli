//! Core admits an LV process request against the captured engine's own LV
//! processor catalogue. An engine that publishes none is refused before any
//! LV work is staged, and the real engine's answer is the offer.
#![cfg(unix)]
use ds_network_contracts::engine::JsonObjectOrder;
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

fn isolated(case: &str, engine: &Path) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            case,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request(settings: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ds.lv.process.request/v1",
        "jobs": [{
            "transformer_name": "T1",
            "gdfs": {"tr": {"type": "FeatureCollection", "features": [{
                "type": "Feature",
                "id": "T1-tr",
                "geometry": {"type": "Point", "coordinates": [30.0, -2.0]},
                "properties": {"name": "T1", "names": "T1"}
            }]}},
            "settings": settings,
        }],
    }))
    .unwrap()
}

/// A released engine from before the catalogue task: it identifies itself
/// and records any task it is asked to run.
const OLDER: &str = r#"{"schema":"ds.engine-build/v1","module":"network","engineIdentity":{"schema":"ds.network-engine/v1","jsonObjectOrder":"insertion","protocolMajor":1,"protocolMinor":0,"coreVersion":"older-engine","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":1,"internalWorkerThreads":1},"capabilities":["lv_processing_v1","grid_file_operations_v1"]}}"#;

#[test]
fn an_engine_without_the_catalogue_task_is_refused_before_any_lv_work() {
    let root = tempfile::tempdir().unwrap();
    let engine = root.path().join("ds-network");
    let tasks = root.path().join("tasks");
    fs::write(
        &engine,
        format!(
            "#!/bin/sh\nif [ \"$1\" = build-info ]; then printf '%s\\n' '{OLDER}'; exit 0; fi\necho \"$1\" >> '{}'\nexit 1\n",
            tasks.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&engine, fs::Permissions::from_mode(0o700)).unwrap();
    isolated("isolated_older_engine", &engine);
    assert!(
        !tasks.exists(),
        "the older engine ran a task: {:?}",
        fs::read_to_string(&tasks)
    );
}

#[test]
#[ignore = "isolated subprocess with an explicit engine override"]
fn isolated_older_engine() {
    let scratch = tempfile::tempdir().unwrap();
    let error = ds_network_client::process_lv_with_order(
        &request(json!({})),
        1,
        scratch.path(),
        &|| false,
        JsonObjectOrder::Insertion,
    )
    .unwrap_err();
    assert_eq!(error.code(), "network_engine_incompatible");
    assert!(
        error.to_string().contains("lv_processor_catalog_v1"),
        "{error}"
    );
    assert_eq!(
        fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "nothing staged"
    );
}

/// The engine the lane names for tests (`DS_NETWORK_ENGINE_BIN`).
fn actual_engine() -> PathBuf {
    PathBuf::from(std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit actual engine"))
}

#[test]
fn the_actual_engine_answers_the_catalogue_it_offers() {
    let client = ds_network_client::CapturedNetworkClient::capture(1, &|| false).unwrap();
    assert_eq!(
        client.executable(),
        fs::canonicalize(actual_engine()).unwrap()
    );
    let scratch = tempfile::tempdir().unwrap();
    let catalogue = client
        .lv_processor_catalog(scratch.path(), &|| false)
        .unwrap();
    let shipped: Value = serde_json::from_str(
        &fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../../ds-network/crates/ds-network-contracts/src/processor_presets.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(catalogue.catalog(), &shipped);
    assert!(catalogue.offer().offers_preset("sketch"));
    // Read once per capture: the second answer is the same held value.
    let again = client
        .lv_processor_catalog(scratch.path(), &|| false)
        .unwrap();
    assert!(std::ptr::eq(catalogue, again));
}

#[test]
fn a_setting_the_actual_engine_does_not_offer_is_refused_before_staging() {
    let scratch = tempfile::tempdir().unwrap();
    let error = ds_network_client::process_lv_with_order(
        &request(json!({"teleport_poles": true})),
        1,
        scratch.path(),
        &|| false,
        JsonObjectOrder::Insertion,
    )
    .unwrap_err();
    assert_eq!(error.code(), "lv_process_setting_not_offered");
    assert!(
        error.to_string().contains("`teleport_poles`") && error.to_string().contains("`T1`"),
        "{error}"
    );
    // An offered key of another kind is refused the same way, by name.
    let error = ds_network_client::process_lv_with_order(
        &request(json!({"renumber_lines": 1})),
        1,
        scratch.path(),
        &|| false,
        JsonObjectOrder::Insertion,
    )
    .unwrap_err();
    assert_eq!(error.code(), "lv_process_setting_not_offered");
    assert!(error.to_string().contains("only as a boolean"), "{error}");
    assert_eq!(
        fs::read_dir(scratch.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("ds-network-run-"))
            .count(),
        0,
        "no LV run was staged"
    );
}
