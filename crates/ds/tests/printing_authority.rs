//! An agent reaches complete print controls from the map authority without source reads.
mod common;
#[test]
fn map_print_discovery_is_compact_and_exposes_the_real_kernel_schemas() {
    let (index, code) = common::json(&["map", "print", "schema", "--output", "json"]);
    assert_eq!(code, 0);
    assert!(index.to_string().len() < 2000);
    assert_eq!(
        index["data"]["sections"],
        serde_json::json!(["request", "choices", "layout", "edit", "outputs"])
    );
    // The choices section is the kernel's advertised DPI/paper shortlist and
    // defaults, not a JSON schema: it is what a picker renders.
    let (choices, code) = common::json(&[
        "map",
        "print",
        "schema",
        "--section",
        "choices",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0);
    assert_eq!(choices["data"]["dpi"]["default"], 300);
    assert_eq!(
        choices["data"]["paper"]["choices"],
        serde_json::json!(["A3", "A0"])
    );
    for section in ["request", "layout", "edit", "outputs"] {
        let (schema, code) = common::json(&[
            "map",
            "print",
            "schema",
            "--section",
            section,
            "--output",
            "json",
        ]);
        assert_eq!(code, 0);
        assert!(schema["data"]["$schema"].is_string());
        if section == "request" {
            for field in ["page_mode", "paper", "dpi", "layout", "codes"] {
                assert!(schema["data"]["properties"][field].is_object(), "{field}");
            }
        }
        if section == "layout" {
            for field in [
                "styles",
                "style_overrides",
                "context_layers",
                "elements",
                "layer_order",
            ] {
                assert!(schema["data"]["properties"][field].is_object(), "{field}");
            }
            let text = schema.to_string();
            for field in [
                "dash_mm",
                "width_mm",
                "label_pt",
                "halo_mm",
                "heading_mode",
                "value_key",
                "max_categories",
                "overflow",
                "indent_mm",
            ] {
                assert!(text.contains(field), "{field}");
            }
        }
    }
}

#[test]
fn report_print_transient_furniture_refusal_explains_governed_adoption_before_rendering() {
    let (schema, code) = common::json(&[
        "report",
        "plan-profile-config",
        "schema",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0);
    let (contract, code) = common::json(&[
        "capabilities",
        "report.plan-profile-config",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0);
    assert!(
        contract["data"]["command"]["refusals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "mv_print_legacy_request_refused")
    );
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.json");
    let destination = root.path().join("delivery");
    for (field, value) in [
        (
            "settings",
            serde_json::json!({"project_title":"UNAPPROVED TITLE"}),
        ),
        ("logo_files", serde_json::json!([])),
        (
            "variants",
            serde_json::json!([{"name":"booklet","ink_mode":"monochrome"}]),
        ),
        ("schema", serde_json::json!("ds.grid-plan-profile-print/v1")),
    ] {
        let mut example = schema["data"]["example"].clone();
        example[field] = value;
        let bytes = serde_json::to_vec(&example).unwrap();
        std::fs::write(&config, &bytes).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "report",
                "plan-profile-config",
                "--project",
                "gisagara",
                "--config",
            ])
            .arg(&config)
            .arg("--out")
            .arg(&destination)
            .args(["--output", "json"])
            .env("DS_CONFIG_HOME", root.path())
            // Satisfy executable discovery; reaching this engine would fail,
            // rather than accepting or rendering transient furniture.
            .env("DS_REPORT_BIN", "/bin/false")
            .output()
            .unwrap();
        let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!output.status.success(), "{field}: {answer}");
        assert_eq!(
            answer["error"]["code"], "mv_print_legacy_request_refused",
            "{field}: {answer}"
        );
        let remedy = answer["error"]["remedy"].as_str().unwrap();
        assert!(remedy.contains("mv-setup set"), "{field}: {answer}");
        assert!(
            remedy.contains("plan-profile-config schema"),
            "{field}: {answer}"
        );
        assert!(!destination.exists());
        assert_eq!(std::fs::read(&config).unwrap(), bytes);
    }
}

#[test]
fn governed_print_documents_require_explicit_project_before_authentication() {
    for id in ["report.standard.list", "report.standard.get"] {
        let (contract, code) = common::json(&["capabilities", id, "--output", "json"]);
        assert_eq!(code, 0);
        let command = &contract["data"]["command"];
        assert_eq!(command["authority"], "headless_project");
        assert!(
            command["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg["name"] == "project" && arg["required"] == true)
        );
    }
    for args in [
        vec!["report", "standard", "list", "--output", "json"],
        vec![
            "report",
            "standard",
            "get",
            "--kind",
            "a4",
            "--id",
            "voltage-drop-a4-v1",
            "--output",
            "json",
        ],
    ] {
        let (refused, code) = common::json(&args);
        assert_ne!(code, 0);
        let text = refused.to_string();
        assert!(text.contains("project"), "{refused}");
        assert!(
            !text.contains("headless_signed_out"),
            "missing project reached identity restore: {refused}"
        );
    }
}

#[test]
fn project_layout_copy_uses_project_runtime_and_requires_its_destination() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("copy.json");
    std::fs::write(&request, serde_json::json!({
        "action":"copy",
        "source":{"scope":"project","project":"template_project","id":"template_a3","revision":"a".repeat(64)},
        "destination":{"scope":"project","id":"owned_a3","expected_revision":""}
    }).to_string()).unwrap();
    let bundle = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "report",
            "layout",
            "copy",
            "--request",
            request.to_str().unwrap(),
            "--yes",
            "--output",
            "json",
        ])
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", bundle)
        .env("DS_CONFIG_HOME", root.path())
        .output()
        .unwrap();
    let refused: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!output.status.success());
    assert_eq!(refused["error"]["code"], "project_required", "{refused}");
}
