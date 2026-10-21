//! Discover and attempt owner subdivision through the executable. These
//! fixture-only attempts stop at confirmation, before identity or project IO.
use serde_json::Value;

mod common;

#[test]
fn owner_subdivision_is_discoverable_and_captures_retry_inputs() {
    let (search, code) = common::json(&[
        "capabilities",
        "--search",
        "subdivision subordinate",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0);
    assert!(
        search["data"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "pm.task.subdivide")
    );
    for id in ["pm.task.subdivide", "pm.task.progress"] {
        let (descriptor, code) = common::json(&["capabilities", id, "--output", "json"]);
        assert_eq!(code, 0);
        let command = &descriptor["data"]["command"];
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert_eq!(command["confirmation_required"], true);
        for name in ["project", "id", "base-revision"] {
            let arg = command["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|arg| arg["name"] == name)
                .unwrap();
            assert_eq!(arg["required"], true, "{id} {name}");
        }
    }
}

#[test]
fn real_subdivision_and_progress_attempts_refuse_before_any_effect_without_confirmation() {
    for tail in [
        vec![
            "subdivide",
            "--title",
            "Inspect crossing",
            "--request",
            "field@example.com",
        ],
        vec!["progress", "--percent", "50"],
    ] {
        let mut args = vec!["pm", "task"];
        args.extend(tail);
        args.extend([
            "--project",
            "fixture-project",
            "--task",
            "parent",
            "--id",
            "fixture-0001",
            "--base-revision",
            "7",
            "--output",
            "json",
        ]);
        let (answer, code): (Value, i32) = common::json(&args);
        assert_ne!(code, 0);
        assert_eq!(answer["error"]["code"], "confirmation_required", "{answer}");
    }
}
