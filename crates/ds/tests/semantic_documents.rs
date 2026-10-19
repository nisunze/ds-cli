//! Actual CLI descriptors and no-effect refusal boundaries for pinned document derivatives.
mod common;
#[test]
fn semantic_lifecycle_describes_fixed_native_effects_and_refuses_before_source_use() {
    for (name, effect) in [
        ("export", "local_file_write"),
        ("lookup", "read_only"),
        ("read", "local_file_write"),
        ("import", "global_write"),
    ] {
        let id = format!("assets.semantic.{name}");
        let (value, code) = common::json(&["capabilities", &id, "--output", "json"]);
        assert_eq!(code, 0);
        let command = &value["data"]["command"];
        assert_eq!(command["id"], id);
        assert_eq!(command["effect"], effect);
        assert_eq!(command["authority"], "headless_project");
        if name == "export" {
            let description = command.to_string();
            assert!(description.contains("DOCX") && description.contains("XLSX"));
            assert!(description.contains("externally"));
        }
        assert!(
            command["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg["name"] == "manifest")
        );
    }
    let (value, code) = common::json(&[
        "assets",
        "semantic",
        "lookup",
        "--project",
        "project",
        "--manifest",
        "relative.json",
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    let (descriptor, status) =
        common::json(&["capabilities", "assets.semantic.lookup", "--output", "json"]);
    assert_eq!(status, 0);
    if descriptor["data"]["command"]["availability"] == "unavailable" {
        // This source binary has no matching packaged native profile. Prove
        // that guard explicitly; malformed-input execution is owner-unit proof.
        assert_eq!(
            descriptor["data"]["command"]["unavailable"]["code"],
            "native_profile_not_configured"
        );
        assert_eq!(value["error"]["code"], "native_profile_not_configured");
    } else {
        assert_eq!(value["error"]["code"], "asset_semantic_invalid");
    }
    let (value, code) = common::json(&[
        "assets",
        "semantic",
        "import",
        "--project",
        "project",
        "--manifest",
        "relative.json",
        "--markdown",
        "relative.md",
        "--output",
        "json",
    ]);
    assert_ne!(code, 0);
    assert_eq!(value["error"]["code"], "confirmation_required");
}
