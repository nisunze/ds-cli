//! The headless command is usable without pairing and its input is closed.
mod common;
use serde_json::{Value, json};
#[test]
fn camera_runs_without_desktop_and_refuses_unknown_fields() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("camera.json");
    std::fs::write(&path, r#"{"operation":"fit"}"#).unwrap();
    let args = [
        "map",
        "canvas",
        "camera",
        "--request",
        path.to_str().unwrap(),
        "--output",
        "json",
    ];
    let run = common::invoke(&args);
    assert_eq!(run.code, 0, "{}", run.stdout);
    let result: Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(result["data"], json!({"zoom":1.,"pan_x":0.,"pan_y":0.}));
    std::fs::write(&path, r#"{"operation":"fit","unknown":true}"#).unwrap();
    let bad = common::invoke(&args);
    assert_ne!(bad.code, 0);
    assert!(
        bad.stdout.contains("canvas_request_invalid"),
        "{}",
        bad.stdout
    );
}
