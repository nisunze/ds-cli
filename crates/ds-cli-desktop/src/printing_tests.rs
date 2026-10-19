use super::*;
#[test]
fn missing_input_is_a_named_refusal_before_pairing() {
    assert_eq!(
        read_request("/nonexistent/printing-request.json", "remedy")
            .unwrap_err()
            .code(),
        "printing_request_invalid"
    );
}

#[test]
fn read_operations_declare_explicit_project_without_a_switch_operation() {
    assert_eq!(TRANSFORMERS_OP.arguments, ["project", "limit"]);
    assert_eq!(
        PREVIEW_OP.arguments,
        [
            "project",
            "transformer",
            "layout",
            "template",
            "out",
            "retention"
        ]
    );
    assert_eq!(
        EXPORT_OP.arguments,
        [
            "project",
            "transformer",
            "transformers",
            "force",
            "selection",
            "intent",
            "preview_retention"
        ]
    );
}

fn preview_inputs(tokens: &[&str]) -> Result<Inputs, Failure> {
    ds_cli_contract::args::parse(
        &PREVIEW_COMMAND,
        &tokens
            .iter()
            .map(|token| (*token).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn live_preview_refuses_export_options_and_invalid_identity_before_pairing() {
    for extra in [
        "--force",
        "--selection",
        "--intent",
        "--seed",
        "--acquire",
        "--refresh",
    ] {
        assert!(
            preview_inputs(&[
                "--project",
                "huye",
                "--transformer",
                "agasharu",
                "--layout",
                "draft.json",
                extra
            ])
            .is_err()
        );
    }
    let layout = serde_json::to_value(ds_command_kernel::printing::default_layout()).unwrap();
    for name in ["combined_transformer", "AGASHARU", "../agasharu"] {
        let inputs = preview_inputs(&[
            "--project",
            "huye",
            "--transformer",
            name,
            "--layout",
            "draft.json",
        ])
        .unwrap();
        assert_eq!(
            preview_arguments(&inputs, layout.clone())
                .unwrap_err()
                .code(),
            "printing_request_invalid"
        );
    }
    let inputs = preview_inputs(&[
        "--project",
        " huye ",
        "--transformer",
        "agasharu",
        "--layout",
        "draft.json",
    ])
    .unwrap();
    assert_eq!(
        preview_arguments(&inputs, layout).unwrap_err().code(),
        "printing_request_invalid"
    );
}

#[test]
fn live_preview_hands_only_the_validated_draft_and_explicit_identity_to_the_bridge() {
    let inputs = preview_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--layout",
        "draft.json",
    ])
    .unwrap();
    assert!(preview_arguments(&inputs, json!({"schema":"foreign"})).is_err());
    let layout = serde_json::to_value(ds_command_kernel::printing::default_layout()).unwrap();
    assert_eq!(
        preview_arguments(&inputs, layout.clone()).unwrap(),
        json!({"project":"huye","transformer":"agasharu","layout":layout})
    );
}

#[test]
fn live_preview_template_and_delivery_controls_are_closed_before_pairing() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("new.svg");
    let inputs = preview_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--template",
        "location.json",
        "--out",
        output.to_str().unwrap(),
        "--retention",
        "7d",
    ])
    .unwrap();
    assert_eq!(
        preview_template_arguments(&inputs, json!({"kind":"layout","id":"focus-a0"})).unwrap(),
        json!({"project":"huye","transformer":"agasharu","template":{"kind":"layout","id":"focus-a0"},"out":output,"retention":"7d"})
    );
    for location in [
        json!({"kind":"layout","id":"../other"}),
        json!({"kind":"layout","id":"a0","extra":true}),
        json!({"kind":"governed_html","id":"a4"}),
    ] {
        assert!(preview_template_arguments(&inputs, location).is_err());
    }
    std::fs::write(&output, b"keep").unwrap();
    assert!(preview_template_arguments(&inputs, json!({"kind":"layout","id":"focus-a0"})).is_err());
    assert_eq!(std::fs::read(output).unwrap(), b"keep");
    for args in [
        vec!["--project", "huye", "--transformer", "agasharu"],
        vec![
            "--project",
            "huye",
            "--transformer",
            "agasharu",
            "--layout",
            "draft.json",
            "--template",
            "location.json",
        ],
    ] {
        let inputs = preview_inputs(&args).unwrap();
        assert!(
            preview(
                &inputs,
                &Context {
                    confirmed: false,
                    output: ds_cli_contract::Output::resolve(
                        ds_cli_contract::Format::Json,
                        false,
                        true
                    )
                }
            )
            .is_err()
        );
    }
}

#[test]
fn live_preview_cannot_admit_inline_or_crossed_native_file_receipts() {
    let inputs = preview_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--layout",
        "draft.json",
    ])
    .unwrap();
    let artifact = json!({"schema":"ds.print-preview-artifact/v1","path":"/native/preview.svg","sha256":"a".repeat(64),"size_bytes":9*1024*1024,
            "content_type":"image/svg+xml","authority_sha256":"b".repeat(64),"result_sha256":"c".repeat(64),"created_at_unix_ms":100,"expires_at_unix_ms":86_400_100,"retention_ms":86_400_000});
    let result = json!({"project":"huye","preview":{"layout_sha256":"d".repeat(64),"artifact_schema":"ds.print-preview-artifact/v1"},"results":[{"transformer":"agasharu","page":artifact,"sha256":"a".repeat(64),"size_bytes":9*1024*1024}]});
    assert_eq!(
        admit_preview_result(&inputs, result.clone()).unwrap(),
        result
    );
    for field in [
        "path",
        "sha256",
        "size_bytes",
        "expires_at_unix_ms",
        "authority_sha256",
    ] {
        let mut bad = result.clone();
        bad["results"][0]["page"][field] = json!("other");
        assert!(admit_preview_result(&inputs, bad).is_err());
    }
    let mut bad = result;
    bad["results"][0]["page"]["svg"] = json!("<svg/>");
    assert!(admit_preview_result(&inputs, bad).is_err());
}

#[test]
fn live_preview_binds_selected_missing_geography_to_the_native_read_only_plan() {
    let inputs = preview_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--layout",
        "draft.json",
    ])
    .unwrap();
    let mut layout = ds_command_kernel::printing::default_layout();
    for context in ds_command_kernel::printing::context::derived_layers() {
        layout = ds_command_kernel::printing::context::toggle(layout, context, true, None).unwrap();
    }
    let layout = serde_json::to_value(layout).unwrap();
    let arguments = preview_arguments(&inputs, layout.clone()).unwrap();
    assert_eq!(arguments.as_object().unwrap().len(), 3);
    assert_eq!(PREVIEW_OP.operation, "printing.preview");
    for online in [false, true] {
        let request = serde_json::to_vec(&json!({"op":"preview_context_preparation","layout":arguments["layout"],"online":online})).unwrap();
        let native: Value =
            serde_json::from_str(&ds_command_kernel::printing::evaluate(&request).unwrap())
                .unwrap();
        assert_eq!(native["acquire_missing"], false);
        assert!(
            native["contexts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["source"]["kind"] == "google_open_buildings")
        );
        assert!(
            native["contexts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["source"]["kind"] == "elevation_contours")
        );
    }
    assert_eq!(arguments["layout"], layout);
}

#[test]
fn request_writes_require_the_exact_project_locally() {
    let prepare = json!({"layout": {}, "expectedRevision": "", "selection": {}});
    assert!(
        require_request_project(&prepare, "printing.prepare")
            .expect_err("prepare project is required")
            .message()
            .contains("explicit project")
    );
    assert_eq!(
        require_request_project(&json!({"project":"huye"}), "printing.prepare").unwrap(),
        "huye"
    );
}

fn export_inputs(tokens: &[&str]) -> Result<Inputs, Failure> {
    ds_cli_contract::args::parse(
        &EXPORT_COMMAND,
        &tokens
            .iter()
            .map(|token| (*token).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn ordinary_export_omits_intent_and_preserves_the_default_request() {
    let inputs = export_inputs(&["--project", "huye", "--transformer", "agasharu"]).unwrap();
    // No selection is invented here: the Desktop's kernel policy runs the
    // held export's SHP/KMZ/XLSX defaults, never a voltage-drop demand.
    assert_eq!(
        export_arguments(&inputs).unwrap(),
        json!({
            "project": "huye",
            "transformer": "agasharu",
            "force": false
        })
    );
}

#[test]
fn preview_export_forwards_only_the_explicit_desktop_intent() {
    let ordinary = export_inputs(&["--project", "huye", "--transformer", "agasharu"]).unwrap();
    let preview = export_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--intent",
        "preview",
    ])
    .unwrap();
    let mut expected = export_arguments(&ordinary).unwrap();
    expected["intent"] = json!("preview");
    let arguments = export_arguments(&preview).unwrap();
    assert_eq!(arguments, expected);
    assert!(arguments.get("localReview").is_none());
}
#[test]
fn a4_preview_retention_is_declared_and_main_export_refuses_before_pairing() {
    let preview = export_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--intent",
        "preview",
        "--preview-retention",
        "7d",
    ])
    .unwrap();
    assert_eq!(
        export_arguments(&preview).unwrap()["preview_retention"],
        "7d"
    );
    let main = export_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "agasharu",
        "--preview-retention",
        "7d",
    ])
    .unwrap();
    assert_eq!(
        export_arguments(&main).unwrap_err().code(),
        "printing_request_invalid"
    );
    assert!(
        export_inputs(&[
            "--project",
            "huye",
            "--transformer",
            "agasharu",
            "--intent",
            "preview",
            "--preview-retention",
            "30d"
        ])
        .is_err()
    );
}

#[test]
fn invalid_or_combined_preview_is_refused_before_pairing() {
    assert_eq!(
        export_inputs(&[
            "--project",
            "huye",
            "--transformer",
            "agasharu",
            "--intent",
            "main",
        ])
        .unwrap_err()
        .code(),
        "invalid_choice"
    );
    let combined = export_inputs(&[
        "--project",
        "huye",
        "--transformer",
        "combined_transformer",
        "--intent",
        "preview",
    ])
    .unwrap();
    assert_eq!(
        export_arguments(&combined).unwrap_err().code(),
        "printing_request_invalid"
    );
    let ordinary =
        export_inputs(&["--project", "huye", "--transformer", "combined_transformer"]).unwrap();
    assert_eq!(
        export_arguments(&ordinary).unwrap(),
        json!({"project":"huye","transformer":"combined_transformer","force":false})
    );
}
