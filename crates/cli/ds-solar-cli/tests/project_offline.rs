use ds_solar_reference_host::{FixtureReferenceProvider, ReferencePreparer};
#[path = "../src/fixtures.rs"]
#[allow(dead_code)]
mod fixtures;

use ds_solar_contracts::ReferencePin;
use ds_solar_host::cache::DirectoryReferenceCache;
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn invoke(root: &Path, index: usize, request: Value) -> Value {
    let input = root.join(format!("request-{index}.json"));
    let result = root.join(format!("result-{index}.json"));
    ds_solar_project::files::json(&input, &request).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ds-solar"))
        .arg("project")
        .arg("--request")
        .arg(input)
        .arg("--result")
        .arg(&result)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&std::fs::read(result).unwrap()).unwrap()
}

#[test]
fn actual_owner_binary_creates_city_and_draft_offline() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let workspace = root.join("project");
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures");
    let fixture = fixtures::FixtureSet::new(&fixture_root);
    let city = fixture
        .city_input("aderm_bere", "project-1", "eds_project/project-1/eds_solar")
        .unwrap();
    let provider = FixtureReferenceProvider::new(
        vec![fixture.reference_bundle("aderm_bere").unwrap()],
        ReferencePin::current(),
    )
    .unwrap();
    let cache = DirectoryReferenceCache::new(root.join("cache"));
    ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(city, "2026-09-05T00:00:00Z")
        .unwrap();
    invoke(
        root,
        1,
        json!({"operation":"initialize","workspace":workspace,"identity":{"project_id":"project-1","root":"eds_project/project-1/eds_solar"}}),
    );
    let seeded = invoke(
        root,
        2,
        json!({"operation":"city_write","workspace":workspace,"city":"aderm_bere","snapshot":fixture_root.join("city/aderm_bere.snapshot.json")}),
    );
    assert_eq!(seeded["city_id"], "aderm_bere");
    assert_eq!(seeded["created"], true);
    assert_eq!(seeded["editable"], true);
    assert_eq!(seeded["publication"], "pending");
    let result = invoke(
        root,
        3,
        json!({"operation":"run","workspace":workspace,"cache":root.join("cache"),"request":{"run_id":"offline","cities":["aderm_bere"],"concurrency":1,"charts":true,"drafts":["apd","network","plant","financial"]}}),
    );
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["publication"], "local_only");
    let files = result["artifacts"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|a| a["name"].as_str().unwrap().ends_with("apd-draft-fr.md"))
    );
    assert!(files.iter().any(|a| a["media_type"] == "image/png"));
    let again = invoke(
        root,
        4,
        json!({"operation":"result","workspace":workspace,"run_id":"offline"}),
    );
    assert_eq!(again, result);
    assert_eq!(
        invoke(root, 5, json!({"operation":"status","workspace":workspace}))["pending_uploads"],
        1
    );
}
