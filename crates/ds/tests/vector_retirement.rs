//! Retired Core Vector routes cannot dispatch while collision counts remain admitted.
use serde_json::Value;
use std::process::Command;

fn call(arguments: &[&str]) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(arguments)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).expect("DS envelope");
    (output.status.success(), value)
}

#[test]
fn retired_vector_commands_are_absent_and_refuse_without_creating_output() {
    let root = std::env::temp_dir().join(format!("ds-vector-retired-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let destination = root.join("must-not-exist.geojson");
    let (success, catalogue) = call(&["capabilities", "data", "--output", "json"]);
    assert!(success);
    assert!(
        catalogue["data"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| !command["id"].as_str().unwrap().starts_with("data.vector."))
    );
    for operation in [
        "buffer",
        "sample",
        "intersect",
        "measure",
        "outliers",
        "random-points-area",
        "collisions",
        "describe",
    ] {
        let id = format!("data.vector.{operation}");
        let (success, descriptor) = call(&["capabilities", &id, "--output", "json"]);
        assert!(!success, "{id}");
        assert_eq!(descriptor["error"]["code"], "unknown_selector", "{id}");
        let (success, refused) = call(&[
            "data",
            "vector",
            operation,
            "--source",
            "missing.geojson",
            "--out",
            destination.to_str().unwrap(),
            "--output",
            "json",
        ]);
        assert!(!success, "{id}");
        assert_eq!(refused["error"]["class"], "invalid_input", "{id}");
        assert_eq!(refused["error"]["retryable"], false, "{id}");
        assert!(!destination.exists(), "retired command wrote an output");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn collision_count_contract_retains_authority_and_rejects_removed_flags() {
    let (success, described) = call(&["capabilities", "design.collisions", "--output", "json"]);
    assert!(success);
    let command = &described["data"]["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    let inputs = command["inputs"].as_array().unwrap();
    assert!(inputs.iter().any(|input| input["name"] == "project"));
    assert!(inputs.iter().any(|input| input["name"] == "lane"));
    assert!(
        inputs
            .iter()
            .all(|input| input["name"] != "regions" && input["name"] != "limit")
    );
    for removed in [vec!["--regions"], vec!["--limit", "10"]] {
        let mut args = vec![
            "design",
            "collisions",
            "--project",
            "fixture-project",
            "--output",
            "json",
        ];
        args.extend(removed);
        let (success, refused) = call(&args);
        assert!(!success);
        assert_eq!(refused["error"]["code"], "unknown_flag");
    }
}
