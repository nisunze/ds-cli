use super::*;
use ds_cli_contract::{Format, Output};
use std::cell::RefCell;

#[derive(Default)]
struct Calls {
    stages: Vec<&'static str>,
    operations: Vec<String>,
    preflight_lanes: Vec<String>,
    print_lanes: Vec<String>,
    fail: Option<&'static str>,
    unverified: bool,
    brief: bool,
    a4: Option<&'static str>,
    export_settings: Vec<(Option<String>, Option<String>)>,
}
thread_local! { static CALLS: RefCell<Calls> = RefCell::default(); }

fn stage(name: &'static str) -> Result<(), Failure> {
    CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        calls.stages.push(name);
        if calls.fail == Some(name) {
            Err(Failure::failed("auth_transient", "Injected owner failure.")
                .remedy("Retry the same captured operation."))
        } else {
            Ok(())
        }
    })
}

fn export(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    stage("export")?;
    assert!(inputs.switch("project-config"));
    CALLS.with(|calls| {
        calls.borrow_mut().export_settings.push((
            inputs.value("preset").map(str::to_owned),
            inputs.value("operator").map(str::to_owned),
        ))
    });
    let bytes = ds_network_contracts::lv_processing::encode_native_lv_request_with_config(
            inputs.require("transformer")?,
            &std::collections::BTreeMap::from([(
                "tr".to_owned(),
                json!({"type":"FeatureCollection","features":[]}),
            )]),
            &CALLS.with(|c| {
                let mut document: Value = serde_json::from_str(include_str!(
                    "../../../../../../ds-network-reporter/tests/fixtures/lv-standard/a4-document.json"
                )).unwrap();
                document["project_id"] = json!("explicit-project");
                let mut sheets = std::collections::BTreeMap::from([
                    ("printing_context".to_owned(), json!({"project_id":"explicit-project","project":"Raw sheet caption"})),
                    ("printing_a4".to_owned(), document),
                    ("project_settings".to_owned(), json!([{"parameter":"report_locale","value":"en"}])),
                ]);
                if c.borrow().brief {
                    sheets.get_mut("project_settings").unwrap().as_array_mut().unwrap().push(
                        json!({"parameter":"design_export_format","value":{"schema":"ds.design-output-selection/v1","voltage_drop_report":"brief"}}));
                }
                match c.borrow().a4 {
                    Some("missing") => { sheets.remove("printing_a4"); }
                    Some("crossed") => { sheets.get_mut("printing_a4").unwrap()["project_id"] = json!("other-project"); }
                    _ => {}
                }
                sheets
            }),
        )
        .unwrap();
    write_new(
        Path::new(inputs.require("out")?),
        &bytes,
        &super::super::artifact::PROJECT_REQUEST,
    )?;
    let input: Value = serde_json::from_slice(&bytes).unwrap();
    let raw_config = &input["jobs"][0]["config_dfs"];
    let config_sha = sha256(&serde_json::to_vec(raw_config).unwrap());
    let mut sealed = raw_config.clone();
    sealed["printing_context"]["project"] = json!("Gisagara, CJIC");
    let sheets_json = sealed.to_string();
    let printing_receipt = ds_command_kernel::report_export::InputReceipt {
        schema: 1,
        country: "Rwanda".into(),
        sheets_sha256: sha256(sheets_json.as_bytes()),
        sheets_json,
        reference_semantic_sha256: "a".repeat(64),
        local_print_recipe: None,
    };
    let receipt_bytes = serde_json::to_vec(&printing_receipt).unwrap();
    let receipt_path = Path::new(inputs.require("out")?).with_extension("printing-receipt.json");
    write_new(
        &receipt_path,
        &receipt_bytes,
        &super::super::artifact::PROJECT_REQUEST,
    )?;
    Ok(
        json!({"project":{"ds_project":inputs.require("project")?,"printing_label":"Gisagara, CJIC","printing_receipt":{"path":receipt_path,"sha256":sha256(&receipt_bytes),"bytes":receipt_bytes.len(),"sheets_sha256":printing_receipt.sheets_sha256}},
            "transformer":inputs.require("transformer")?,"lane":inputs.require("lane")?,
            "source":{"state":"fenced","version":7,"content_digest":"a".repeat(64)},
            "project_config":"included","project_config_sha256":config_sha,"request_sha256":sha256(&bytes)}),
    )
}

fn compute(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    stage("process")?;
    let input = std::fs::read(inputs.require("input")?).unwrap();
    // Opaque owner bytes: composition must not rewrite or decode them.
    let output = serde_json::to_vec(&json!({"jobs":[{"transformer_name":"T1","ok":true,
            "output":{"gdfs":{"tr":{"type":"FeatureCollection","features":[]}}}}]}))
    .unwrap();
    write_new(
        Path::new(inputs.require("out")?),
        &output,
        &super::super::artifact::RESULT,
    )?;
    Ok(
        json!({"jobs":1,"succeeded":1,"failed":0,"input_sha256":sha256(&input),"result_sha256":sha256(&output)}),
    )
}

fn save(inputs: &Inputs, _: &Context, expected_project: &str) -> Result<Value, Failure> {
    CALLS.with(|calls| {
        calls
            .borrow_mut()
            .operations
            .push(inputs.require("operation-id").unwrap().to_owned())
    });
    stage("save")?;
    let source = project_save::receipt(
        &std::fs::read(inputs.require("source")?).unwrap(),
        project_export::COMMAND.id,
    )
    .unwrap();
    let processed = project_save::receipt(
        &std::fs::read(inputs.require("process-receipt")?).unwrap(),
        process::COMMAND.id,
    )
    .unwrap();
    let input = std::fs::read(inputs.require("input")?).unwrap();
    let output = std::fs::read(inputs.require("result")?).unwrap();
    let (project, version, _) = project_save::verify_receipts(
        &source,
        &processed,
        inputs.require("transformer")?,
        inputs.require("lane")?,
        &input,
        &output,
    )?;
    assert_eq!(project, expected_project);
    let raw: Value = serde_json::from_slice(&input).unwrap();
    assert_eq!(
        raw["jobs"][0]["config_dfs"]["printing_context"]["project"], "Raw sheet caption",
        "project-save must receive original raw settings, not sealed print caption"
    );
    assert!(serde_json::from_slice::<Value>(&output).unwrap()["jobs"][0]["ok"] == true);
    let verified = CALLS.with(|calls| !calls.borrow().unverified);
    Ok(
        json!({"project":project,"lane":inputs.require("lane")?,"result":{"project_id":project,
            "results":[{"transformer_name":inputs.require("transformer")?,"verified":verified,
                "unchanged":false,"version":version+1,"content_digest":layer_digest(),"analysis_sha256":sha256(ANALYSIS)}]}}),
    )
}

const ANALYSIS: &[u8] = b"{ \"number\": 1.000, \"saved\": true }\n";
fn layer_digest() -> String {
    ds_command_kernel::report_export::jcs::layers_content_digest(&std::collections::BTreeMap::from(
        [(
            "tr".into(),
            json!({"type":"FeatureCollection","features":[]}),
        )],
    ))
    .unwrap()
}
fn preflight(lane: &str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
    stage("preflight")?;
    CALLS.with(|c| c.borrow_mut().preflight_lanes.push(lane.to_owned()));
    Ok(None)
}
fn analysis(
    lane: &str,
    project: &str,
    transformer: &str,
    version: u64,
    digest: &str,
    sha: &str,
) -> Result<Vec<u8>, Failure> {
    stage("analysis")?;
    assert_eq!(
        (lane, project, transformer, version),
        ("canary", "explicit-project", "T1", 8)
    );
    assert_eq!(digest, layer_digest());
    assert_eq!(sha, sha256(ANALYSIS));
    Ok(ANALYSIS.to_vec())
}
fn held_renderer_policy(lane: &str, project: &str) -> Result<Value, Failure> {
    stage("renderer")?;
    assert_eq!(lane, "canary");
    assert_eq!(project, "explicit-project");
    Ok(json!({"held_api_renderer":"same-project"}))
}
fn print(
    inputs: &Inputs,
    _: &Context,
    _: Option<&ds_cli_exec::CapturedExternal>,
) -> Result<Value, Failure> {
    stage("print")?;
    let lane = inputs.require("lane")?;
    CALLS.with(|c| c.borrow_mut().print_lanes.push(lane.to_owned()));
    assert_eq!(lane, "canary");
    assert_eq!(inputs.require("task")?, "voltage-drop");
    let request: Value =
        serde_json::from_slice(&std::fs::read(inputs.require("request")?).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(request["source_document"].as_str().unwrap()).unwrap(),
        ANALYSIS
    );
    assert_eq!(request["source_sha256"], sha256(ANALYSIS));
    assert_eq!(
        request["report_format"],
        CALLS.with(|c| if c.borrow().brief {
            "brief"
        } else {
            "extended"
        })
    );
    assert_eq!(request["project_label"], "Gisagara, CJIC");
    assert_eq!(
        request["network_config"]["printing_a4"]["project_id"],
        "explicit-project"
    );
    assert_eq!(
        request["network_config"]["printing_context"]["project_id"],
        "explicit-project"
    );
    assert_eq!(
        request["renderer_defaults"],
        json!({"held_api_renderer":"same-project"})
    );
    assert!(request.get("printing_identity").is_none());
    let path = request["out_pdf"].as_str().unwrap();
    assert!(!Path::new(path).exists());
    let mut doc = lopdf::Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(lopdf::dictionary! {"Type" => "Page", "Parent" => pages,
    "MediaBox" => vec![0.into(),0.into(),595.into(),842.into()]});
    doc.objects.insert(
        pages,
        lopdf::dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}.into(),
    );
    let catalog = doc.add_object(lopdf::dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", catalog);
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).unwrap();
    std::fs::write(path, &pdf).unwrap();
    Ok(
        json!({"schema":"ds.voltage-drop-pdf.render/v1","transformer":"T1","project_label":"Gisagara, CJIC",
            "out_pdf":path,"source_sha256":sha256(ANALYSIS),"output_sha256":sha256(&pdf),
            "pages":1,"bytes":pdf.len(),"publication":"nothing_published"}),
    )
}

const MOCK: Owners = Owners {
    export,
    process: compute,
    save,
    preflight,
    analysis,
    renderer_policy: held_renderer_policy,
    print,
};

fn context(confirmed: bool) -> Context {
    Context {
        confirmed,
        output: Output::resolve(Format::Json, false, true),
    }
}

fn inputs(directory: &Path, flags: &[&str]) -> Inputs {
    let mut tokens = vec![
        "--project".into(),
        "explicit-project".into(),
        "--transformer".into(),
        "T1".into(),
        "--lane".into(),
        "canary".into(),
        "--out-dir".into(),
        directory.to_str().unwrap().into(),
    ];
    tokens.extend(flags.iter().map(|s| s.to_string()));
    parse(&COMMAND, &tokens).unwrap()
}

fn reset() {
    CALLS.with(|calls| *calls.borrow_mut() = Calls::default());
}

#[test]
fn requested_lane_reaches_print_preflight_and_export() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let result = run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap();
    assert_eq!(result["printed"], true);
    CALLS.with(|c| {
        let calls = c.borrow();
        assert_eq!(calls.preflight_lanes, ["canary"]);
        assert_eq!(calls.print_lanes, ["canary"]);
    });
}

#[test]
fn print_run_refuses_missing_and_crossed_a4_without_fallback_or_recomputation() {
    for variant in ["missing", "crossed"] {
        CALLS.with(|calls| {
            *calls.borrow_mut() = Calls {
                a4: Some(variant),
                ..Calls::default()
            }
        });
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("run");
        let refusal =
            run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap_err();
        assert_eq!(refusal.code(), "fast_lv_run_print_failed");
        assert_eq!(refusal.detail_value().unwrap()["saved"], true);
        assert_eq!(refusal.detail_value().unwrap()["printed"], false);
        let before = std::fs::read(directory.join("request.json")).unwrap();
        let retry = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &MOCK,
        )
        .unwrap_err();
        assert_eq!(retry.code(), "fast_lv_run_print_failed");
        assert_eq!(
            std::fs::read(directory.join("request.json")).unwrap(),
            before
        );
        CALLS.with(|calls| {
            let stages = &calls.borrow().stages;
            assert_eq!(
                stages.iter().filter(|stage| **stage == "process").count(),
                1
            );
            assert_eq!(stages.iter().filter(|stage| **stage == "save").count(), 1);
            assert!(!stages.contains(&"renderer") && !stages.contains(&"print"));
        });
    }
}

#[test]
fn legacy_first_save_receipt_can_print_without_rewrite_or_resave() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
    let path = directory.join("save.json");
    let mut envelope: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    envelope.as_object_mut().unwrap().remove("run_operation_id");
    let first = serde_json::to_vec(&envelope).unwrap();
    std::fs::write(&path, &first).unwrap();
    let printed = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    assert_eq!(printed["printed"], true);
    assert_eq!(std::fs::read(&path).unwrap(), first);
    assert_eq!(
        CALLS.with(|c| c.borrow().stages.clone()),
        [
            "export",
            "process",
            "save",
            "preflight",
            "analysis",
            "renderer",
            "print"
        ]
    );
}

#[test]
fn source_brief_selection_is_passed_explicitly_to_saved_analysis_presenter() {
    reset();
    CALLS.with(|c| c.borrow_mut().brief = true);
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let printed = run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap();
    assert_eq!(printed["printed"], true);
}

#[test]
fn retry_preflight_keeps_captured_successful_save_evidence_without_effects() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
    fn missing(_: &str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
        Err(Failure::unavailable("reporter_browser_missing", "gone")
            .remedy("configure chromium for reporter"))
    }
    let owners = Owners {
        preflight: missing,
        ..MOCK
    };
    let failure = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &owners,
    )
    .unwrap_err();
    assert_eq!(failure.code(), "reporter_browser_missing");
    assert_eq!(failure.detail_value().unwrap()["saved"], true);
    assert_eq!(failure.detail_value().unwrap()["printed"], false);
    assert_eq!(
        CALLS.with(|c| c.borrow().stages.clone()),
        ["export", "process", "save"]
    );
}

#[test]
fn print_preflight_refuses_before_directory_or_project_effects() {
    for code in [
        "reporter_engine_missing",
        "unknown_task",
        "reporter_browser_missing",
    ] {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        fn missing_reporter(_: &str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
            Err(Failure::unavailable("reporter_engine_missing", "missing")
                .remedy("install reporter"))
        }
        fn missing_task(_: &str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
            Err(Failure::unavailable("unknown_task", "missing").remedy("update reporter"))
        }
        fn missing_browser(_: &str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
            Err(Failure::unavailable("reporter_browser_missing", "missing")
                .remedy("configure chromium for reporter"))
        }
        let owners = Owners {
            preflight: match code {
                "reporter_engine_missing" => missing_reporter,
                "unknown_task" => missing_task,
                _ => missing_browser,
            },
            ..MOCK
        };
        assert_eq!(
            run_with(
                &inputs(&directory, &["--print-a4"]),
                &context(true),
                &owners
            )
            .unwrap_err()
            .code(),
            code
        );
        assert!(!directory.exists());
        assert!(CALLS.with(|c| c.borrow().stages.is_empty()));
    }
}

#[test]
fn saved_but_print_failed_resumes_without_save_export_compute_or_overwrite() {
    reset();
    CALLS.with(|c| c.borrow_mut().fail = Some("print"));
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let error = run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap_err();
    assert_eq!(error.code(), "fast_lv_run_print_failed");
    assert_eq!(error.detail_value().unwrap()["saved"], true);
    assert_eq!(error.detail_value().unwrap()["printed"], false);
    let first = std::fs::read(directory.join("save.json")).unwrap();
    assert_eq!(
        error.detail_value().unwrap()["save"],
        read_receipt(&directory.join("save.json"), &project_save::COMMAND).unwrap()
    );
    assert_eq!(
        std::fs::read(directory.join("print-1/saved-analysis.json")).unwrap(),
        ANALYSIS
    );
    // Simulate an owner that wrote partial output before failing. Retry
    // must choose a fresh attempt, never remove the failed output.
    std::fs::write(
        directory.join("print-1/voltage-drop-a4.pdf"),
        b"partial pdf",
    )
    .unwrap();
    CALLS.with(|c| c.borrow_mut().fail = None);
    let result = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    assert_eq!(result["saved"], true);
    assert_eq!(result["printed"], true);
    assert_eq!(std::fs::read(directory.join("save.json")).unwrap(), first);
    assert_eq!(
        std::fs::read(directory.join("print-1/voltage-drop-a4.pdf")).unwrap(),
        b"partial pdf"
    );
    let pdf = std::fs::read(directory.join("print-2/voltage-drop-a4.pdf")).unwrap();
    let receipt = std::fs::read(directory.join("print.json")).unwrap();
    let resumed = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    assert_eq!(resumed["print"], result["print"]);
    assert_eq!(
        std::fs::read(directory.join("print.json")).unwrap(),
        receipt
    );
    assert_eq!(
        std::fs::read(directory.join("print-2/voltage-drop-a4.pdf")).unwrap(),
        pdf
    );
    assert_eq!(
        CALLS.with(|c| c.borrow().stages.clone()),
        [
            "preflight",
            "export",
            "process",
            "save",
            "analysis",
            "renderer",
            "print",
            "preflight",
            "analysis",
            "renderer",
            "print",
            "preflight",
            "analysis"
        ]
    );
}

#[test]
fn moved_head_refuses_print_from_first_save_without_recompute_or_resave() {
    fn moved(
        _: &str,
        project: &str,
        transformer: &str,
        version: u64,
        _: &str,
        _: &str,
    ) -> Result<Vec<u8>, Failure> {
        stage("analysis")?;
        assert_eq!(
            (project, transformer, version),
            ("explicit-project", "T1", 8)
        );
        Err(Failure::conflict("auth_response_unreadable", "moved head")
            .remedy("review working head")
            .detail(json!({"http_status":409,"service_code":"analysis_stale"})))
    }
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
    let first = std::fs::read(directory.join("save.json")).unwrap();
    let owners = Owners {
        analysis: moved,
        ..MOCK
    };
    let failure = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &owners,
    )
    .unwrap_err();
    assert_eq!(failure.detail_value().unwrap()["saved"], true);
    assert_eq!(
        failure.detail_value().unwrap()["print_error"]["detail"]["http_status"],
        409
    );
    assert_eq!(std::fs::read(directory.join("save.json")).unwrap(), first);
    assert_eq!(
        CALLS.with(|c| c.borrow().stages.clone()),
        ["export", "process", "save", "preflight", "analysis"]
    );
    assert!(!directory.join("print-1").exists());
}

#[test]
fn mismatched_saved_layers_and_wrong_raw_analysis_never_reach_reporter() {
    fn wrong(_: &str, _: &str, _: &str, _: u64, _: &str, _: &str) -> Result<Vec<u8>, Failure> {
        stage("analysis")?;
        Ok(b"{}".to_vec())
    }
    for wrong_bytes in [true, false] {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        if !wrong_bytes {
            let path = directory.join("save.json");
            let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            saved["data"]["result"]["results"][0]["content_digest"] = json!("a".repeat(64));
            std::fs::write(path, saved.to_string()).unwrap();
        }
        fn same(_: &str, _: &str, _: &str, _: u64, _: &str, _: &str) -> Result<Vec<u8>, Failure> {
            stage("analysis")?;
            Ok(ANALYSIS.to_vec())
        }
        let owners = Owners {
            analysis: if wrong_bytes { wrong } else { same },
            ..MOCK
        };
        assert_eq!(
            run_with(
                &inputs(&directory, &["--resume", "--print-a4"]),
                &context(true),
                &owners
            )
            .unwrap_err()
            .code(),
            "fast_lv_run_print_failed"
        );
        assert!(!CALLS.with(|c| c.borrow().stages.contains(&"print")));
    }
}

#[test]
fn composes_exact_owner_artifacts_and_retries_without_export_or_compute() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let result = run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
    assert_eq!(result["saved"], true);
    assert_eq!(result["printed"], false);
    assert_eq!(
        result["save"]["result"]["results"][0]["analysis_sha256"],
        sha256(ANALYSIS)
    );
    assert_eq!(
        CALLS.with(|c| c.borrow().stages.clone()),
        ["export", "process", "save"]
    );
    let first_save = std::fs::read(directory.join("save.json")).unwrap();
    let resumed = run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK).unwrap();
    assert_eq!(resumed["operation_id"], result["operation_id"]);
    assert_eq!(
        CALLS.with(|c| c.borrow().stages.clone()),
        ["export", "process", "save", "save"]
    );
    CALLS.with(|c| {
        let calls = c.borrow();
        assert!(calls.preflight_lanes.is_empty());
        assert!(calls.print_lanes.is_empty());
    });
    assert_eq!(
        std::fs::read(directory.join("save.json")).unwrap(),
        first_save
    );
    assert!(!directory.join(".run-lock").exists());
}

#[test]
fn failed_stages_never_reach_later_owners_or_create_a_saved_receipt() {
    for (failure, expected) in [
        ("export", vec!["export"]),
        ("process", vec!["export", "process"]),
        ("save", vec!["export", "process", "save"]),
    ] {
        reset();
        CALLS.with(|calls| calls.borrow_mut().fail = Some(failure));
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        assert_eq!(
            run_with(&inputs(&directory, &[]), &context(true), &MOCK)
                .unwrap_err()
                .code(),
            "auth_transient"
        );
        assert_eq!(CALLS.with(|c| c.borrow().stages.clone()), expected);
        assert!(!directory.join("save.json").exists());
        if failure == "save" {
            CALLS.with(|calls| calls.borrow_mut().fail = None);
            let result =
                run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK).unwrap();
            assert_eq!(result["saved"], true);
            CALLS.with(|calls| {
                let calls = calls.borrow();
                assert_eq!(calls.operations.len(), 2);
                assert_eq!(calls.operations[0], calls.operations[1]);
                assert_eq!(calls.stages, ["export", "process", "save", "save"]);
            });
        }
    }
}

#[test]
fn unverified_save_is_a_failure() {
    reset();
    CALLS.with(|calls| calls.borrow_mut().unverified = true);
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("unverified");
    assert_eq!(
        run_with(&inputs(&directory, &[]), &context(true), &MOCK)
            .unwrap_err()
            .code(),
        "fast_lv_run_save_unverified"
    );
    assert!(!directory.join("save.json").exists());
}

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn failed_native_job_never_reaches_save() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("native-failure");
    // Real owner request encoder and processor: empty source is not a
    // successful job, even though process writes a complete batch result.
    let native = Owners {
        export,
        process: process::run_receipt,
        save,
        ..MOCK
    };
    assert_eq!(
        run_with(&inputs(&directory, &[]), &context(true), &native)
            .unwrap_err()
            .code(),
        "fast_lv_run_process_failed"
    );
    assert_eq!(CALLS.with(|c| c.borrow().stages.clone()), ["export"]);
    assert!(directory.join("result.json").exists());
    assert!(directory.join("process.json").exists());
    assert!(!directory.join("save.json").exists());
}

#[test]
fn print_confirmation_existing_directory_and_lease_refuse_before_owners() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    assert_eq!(
        run_with(&inputs(&directory, &[]), &context(false), &MOCK)
            .unwrap_err()
            .code(),
        "confirmation_required"
    );
    assert!(!directory.exists());
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        run_with(&inputs(&directory, &[]), &context(true), &MOCK)
            .unwrap_err()
            .code(),
        "fast_lv_run_output_exists"
    );
    std::fs::write(directory.join(".run-lock"), b"another invocation").unwrap();
    assert_eq!(
        run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK)
            .unwrap_err()
            .code(),
        "fast_lv_run_busy"
    );
    assert_eq!(
        std::fs::read(directory.join(".run-lock")).unwrap(),
        b"another invocation"
    );
    assert!(CALLS.with(|c| c.borrow().stages.is_empty()));
}

#[test]
fn resume_refuses_modified_bytes_and_cross_project_context() {
    for change in ["input", "result", "project", "lane", "transformer"] {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        let before = CALLS.with(|c| c.borrow().stages.len());
        if matches!(change, "input" | "result") {
            let file = if change == "input" {
                "request.json"
            } else {
                "result.json"
            };
            std::fs::write(directory.join(file), b"modified").unwrap();
        } else {
            let path = directory.join("source.json");
            let mut receipt: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if change == "project" {
                receipt["data"]["project"]["ds_project"] = json!("other");
            } else {
                receipt["data"][change] = json!("other");
            }
            std::fs::write(path, receipt.to_string()).unwrap();
        }
        assert!(run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK).is_err());
        assert_eq!(CALLS.with(|c| c.borrow().stages.len()), before);
    }
}

#[test]
fn operation_key_ignores_paths_and_changes_with_every_captured_fence() {
    let source = json!({"project":{"ds_project":"p"},"lane":"canary","transformer":"T1",
            "source":{"version":1,"content_digest":"digest"},"out":"path1"});
    let processed = json!({"input_sha256":"i","result_sha256":"r","out":"path1"});
    let original = operation_id(&source, &processed);
    let mut other = processed.clone();
    other["out"] = json!("path2");
    assert_eq!(operation_id(&source, &other), original);
    for field in ["input_sha256", "result_sha256"] {
        let mut other = processed.clone();
        other[field] = json!("changed");
        assert_ne!(operation_id(&source, &other), original);
    }
    for pointer in [
        "/project/ds_project",
        "/lane",
        "/transformer",
        "/source/version",
        "/source/content_digest",
    ] {
        let mut other = source.clone();
        *other.pointer_mut(pointer).unwrap() = json!("changed");
        assert_ne!(operation_id(&other, &processed), original);
    }
}

#[test]
fn save_success_requires_identity_and_both_verified_digests() {
    let saved = json!({"project":"p","lane":"canary","result":{"project_id":"p","results":[{
        "transformer_name":"T1","verified":true,"version":8,
        "content_digest":"a".repeat(64),"analysis_sha256":"b".repeat(64)
    }]}});
    require_verified_save(&saved, "p", "T1", "canary").unwrap();
    for pointer in [
        "/project",
        "/lane",
        "/result/project_id",
        "/result/results/0/transformer_name",
        "/result/results/0/verified",
        "/result/results/0/version",
        "/result/results/0/content_digest",
        "/result/results/0/analysis_sha256",
    ] {
        let mut changed = saved.clone();
        *changed.pointer_mut(pointer).unwrap() = Value::Null;
        assert_eq!(
            require_verified_save(&changed, "p", "T1", "canary")
                .unwrap_err()
                .code(),
            "fast_lv_run_save_unverified"
        );
    }
    let mut changed = saved.clone();
    changed["result"]["results"][0]["error_code"] = json!("TRANSFORMER_SAVE_READBACK_UNVERIFIED");
    assert!(require_verified_save(&changed, "p", "T1", "canary").is_err());
    changed["result"]["results"] = json!([]);
    assert!(require_verified_save(&changed, "p", "T1", "canary").is_err());
}

#[test]
fn replacing_a_receipt_between_checks_cannot_redirect_the_save_project() {
    fn replaced(inputs: &Inputs, context: &Context, project: &str) -> Result<Value, Failure> {
        let path = inputs.require("source")?;
        let mut receipt: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        receipt["data"]["project"]["ds_project"] = json!("other-project");
        std::fs::write(path, receipt.to_string()).unwrap();
        project_save::run_for_project(inputs, context, project)
    }
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let owners = Owners {
        export,
        process: compute,
        save: replaced,
        ..MOCK
    };
    let refusal = run_with(&inputs(&directory, &[]), &context(true), &owners).unwrap_err();
    assert_eq!(refusal.code(), "fast_lv_save_input_invalid");
    assert!(refusal.message().contains("another explicit project"));
    assert!(!directory.join("save.json").exists());
}
#[test]
fn obsolete_id_caption_uses_a_fresh_attempt_and_correct_receipt_is_idempotent() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let first = run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap();
    let old_pdf = PathBuf::from(first["print"]["out_pdf"].as_str().unwrap());
    let old_pdf_bytes = std::fs::read(&old_pdf).unwrap();
    let path = directory.join("print.json");
    let mut old: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    old["data"]["project_label"] = json!("explicit-project");
    let old_bytes = serde_json::to_vec(&old).unwrap();
    std::fs::write(&path, &old_bytes).unwrap();
    std::fs::write(old_pdf.parent().unwrap().join("report.json"), &old_bytes).unwrap();
    let corrected = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    assert_eq!(corrected["print"]["project_label"], "Gisagara, CJIC");
    let new_pdf = Path::new(corrected["print"]["out_pdf"].as_str().unwrap());
    assert_ne!(new_pdf, old_pdf);
    assert_eq!(std::fs::read(&old_pdf).unwrap(), old_pdf_bytes);
    assert_eq!(
        std::fs::read(new_pdf.parent().unwrap().join("previous-print.json")).unwrap(),
        old_bytes
    );
    let selected = std::fs::read(&path).unwrap();
    let counts = CALLS.with(|c| c.borrow().stages.clone());
    let repeated = run_with(
        &inputs(&directory, &["--resume", "--print-a4"]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    assert_eq!(repeated["print"], corrected["print"]);
    assert_eq!(std::fs::read(&path).unwrap(), selected);
    CALLS.with(|c| {
        let stages = &c.borrow().stages;
        for stage in ["export", "process", "save", "renderer", "print"] {
            assert_eq!(
                stages.iter().filter(|v| **v == stage).count(),
                counts.iter().filter(|v| **v == stage).count()
            );
        }
        assert_eq!(stages.iter().filter(|v| **v == "analysis").count(), 3);
    });
}
#[test]
fn unsealed_or_crossed_caption_refuses_print_without_an_extra_analysis_read() {
    for change in 0..8 {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        let path = directory.join("source.json");
        let mut source: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        match change {
            0 => source["data"]["project"]["printing_label"] = Value::Null,
            1 => source["data"]["project"]["printing_label"] = json!("Other caption"),
            2 => source["data"]["project"]["printing_label"] = json!("explicit-project"),
            3 => {
                std::fs::remove_file(directory.join("request.printing-receipt.json")).unwrap();
            }
            4 => {
                let receipt = directory.join("request.printing-receipt.json");
                let mut bytes = std::fs::read(&receipt).unwrap();
                bytes.push(b' ');
                std::fs::write(receipt, bytes).unwrap();
            }
            5 => {
                source["data"]["project"]["printing_receipt"]["path"] =
                    json!(temp.path().join("foreign-receipt.json"))
            }
            6 => source["data"]["project_config_sha256"] = json!("c".repeat(64)),
            _ => {
                let path = directory.join("request.printing-receipt.json");
                let mut r: ds_command_kernel::report_export::InputReceipt =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                let mut sheets: Value = serde_json::from_str(&r.sheets_json).unwrap();
                sheets["printing_context"]["project_id"] = json!("other-project");
                r.sheets_json = sheets.to_string();
                r.sheets_sha256 = sha256(r.sheets_json.as_bytes());
                let bytes = serde_json::to_vec(&r).unwrap();
                std::fs::write(path, &bytes).unwrap();
                source["data"]["project"]["printing_receipt"]["sha256"] = json!(sha256(&bytes));
                source["data"]["project"]["printing_receipt"]["bytes"] = json!(bytes.len());
                source["data"]["project"]["printing_receipt"]["sheets_sha256"] =
                    json!(r.sheets_sha256);
            }
        }
        std::fs::write(&path, serde_json::to_vec(&source).unwrap()).unwrap();
        let failed = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &MOCK,
        )
        .unwrap_err();
        assert_eq!(failed.code(), "fast_lv_run_print_failed");
        assert_eq!(failed.detail_value().unwrap()["saved"], true);
        CALLS.with(|c| {
            let stages = &c.borrow().stages;
            assert!(
                !stages.contains(&"analysis")
                    && !stages.contains(&"renderer")
                    && !stages.contains(&"print")
            );
            assert_eq!(stages.iter().filter(|s| **s == "save").count(), 1);
        });
    }
}

#[test]
fn a_preset_and_operator_toggles_reach_the_export_and_a_resume_reuses_the_capture() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("run");
    let operator = temp.path().join("assign.json");
    let operator = operator.to_str().unwrap();
    run_with(
        &inputs(&directory, &["--preset", "sketch", "--operator", operator]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    run_with(
        &inputs(&directory, &["--resume", "--preset", "sketch"]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    CALLS.with(|c| {
        assert_eq!(
            c.borrow().export_settings,
            vec![(Some("sketch".to_owned()), Some(operator.to_owned()))],
            "the capture is exported once with the preset and resumed without re-export"
        );
    });
    assert!(
        COMMAND
            .refusals
            .iter()
            .any(|refusal| refusal.code == "process_settings_invalid")
    );
}

#[test]
fn without_a_preset_the_export_keeps_owner_defaults() {
    reset();
    let temp = tempfile::tempdir().unwrap();
    run_with(
        &inputs(&temp.path().join("run"), &[]),
        &context(true),
        &MOCK,
    )
    .unwrap();
    CALLS.with(|c| assert_eq!(c.borrow().export_settings, vec![(None, None)]));
}
