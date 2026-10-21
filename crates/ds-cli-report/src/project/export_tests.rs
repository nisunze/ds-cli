#[cfg(unix)]
#[test]
fn captured_cli_report_adapter_keeps_one_program_for_probe_and_typed_render() {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let old = directory.path().join("reporter-old");
    let new = directory.path().join("reporter-new");
    for (path, version) in [(&old, "old"), (&new, "new")] {
        std::fs::write(path, format!("#!/bin/sh\ncase \"$1\" in\nbuild-info) printf '%s\\n' '{{\"fixture\":\"{version}\"}}';;\nrender-print-layout) printf '%s' '{version}' > \"$3\";;\n*) exit 2;;\nesac\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let captured = CapturedCliEngine(DS_REPORT.capture_pinned(&old).unwrap());
    assert_eq!(captured.build_info().unwrap()["fixture"], "old");
    // A later capture sees a different program; the accepted adapter keeps
    // using its own path for every fixed task and its final identity check.
    let later = CapturedCliEngine(DS_REPORT.capture_pinned(&new).unwrap());
    assert_eq!(later.build_info().unwrap()["fixture"], "new");
    let request = directory.path().join("request");
    let result = directory.path().join("result");
    assert!(
        captured
            .render_print_layout(&request, &result)
            .unwrap()
            .succeeded
    );
    assert_eq!(std::fs::read_to_string(request).unwrap(), "old");
    assert_eq!(captured.build_info().unwrap()["fixture"], "old");
}

#[test]
fn saved_analysis_is_retained_for_matching_offline_room_and_revoked_by_service_answer() {
    let scope = ds_command_kernel::report_export::held::Scope {
        project: "project_a".into(),
        lane: "stable".into(),
        principal: "owner_a".into(),
        credential_audience_sha256: "a".repeat(64),
        sheets_sha256: "b".repeat(64),
    };
    let room = ds_project_data::room_hold::Room {
        transformer: "tx_a".into(),
        version: Some(2),
        content_digest: Some("c".repeat(64)),
        layers: BTreeMap::new(),
    };
    let mut inputs = super::super::hold::Inputs {
        project_crs: None,
        printing_style_capture: None,
        transformer_natures: None,
        saved_analyses: BTreeMap::new(),
        rows: Vec::new(),
        configuration: json!({}),
        setups: Vec::new(),
        read_at: "held".into(),
    };
    let document = br#"{"actual_saved_result":true}"#.to_vec();
    let analysis = BatchAnalysis::Ready {
        sha256: ds_command_kernel::report_export::sha256_hex(&document),
        document,
    };
    retain_batch_analysis(&mut inputs, &scope, &room, &analysis).unwrap();
    assert!(matches!(
        held_batch_analysis(&inputs, &scope, &room).unwrap(),
        BatchAnalysis::Ready { .. }
    ));
    let mut changed = room.clone();
    changed.version = Some(3);
    assert!(held_batch_analysis(&inputs, &scope, &changed).is_err());
    let mut foreign = scope.clone();
    foreign.principal = "owner_b".into();
    assert!(held_batch_analysis(&inputs, &foreign, &room).is_err());
    retain_batch_analysis(&mut inputs, &scope, &room, &BatchAnalysis::Stale).unwrap();
    assert!(matches!(
        held_batch_analysis(&inputs, &scope, &room).unwrap(),
        BatchAnalysis::Missing
    ));
}

#[test]
fn explicit_local_proof_capture_keeps_original_server_receipt_and_admits_only_its_draft() {
    use ds_command_kernel::{printing, report_export, style_resolution};
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../ds-command-kernel/tests/fixtures/printing-source-bindings.json"
    ))
    .unwrap();
    let snapshot = style_resolution::Snapshot {
        schema: style_resolution::SCHEMA.into(),
        project_id: "project-a".into(),
        revision_id: "b".repeat(64),
        seed_version: None,
        style_presets: None,
        missing_style_refs: vec![],
        bindings: serde_json::from_value(fixture["bindings"].clone()).unwrap(),
    };
    let mut original = printing::default_layout();
    original.id = "detail".into();
    let sheets = json!({"printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":original}],"project_settings":[{"parameter":"report_locale","value":"en"}]});
    let source = report_export::InputReceipt {
        schema: 1,
        country: "RW".into(),
        reference_semantic_sha256: "a".repeat(64),
        sheets_json: sheets.to_string(),
        sheets_sha256: report_export::sha256_hex(sheets.to_string().as_bytes()),
        local_print_recipe: None,
    };
    let mut draft = original.clone();
    draft.name = "Explicit review draft".into();
    let proof = report_export::proof::with_layouts(&source, &[draft.clone()]).unwrap();
    assert_ne!(proof.sheets_sha256, source.sheets_sha256);
    assert_eq!(
        proof
            .local_print_recipe
            .as_ref()
            .unwrap()
            .source_sheets_sha256,
        source.sheets_sha256
    );
    let capture = printing::style_capture::Capture::new(
        &snapshot,
        "project-a",
        "stable",
        "user-a",
        &"a".repeat(64),
        &source.sheets_sha256,
        &proof.sheets().unwrap(),
        &[draft.clone()],
    )
    .unwrap();
    assert!(capture.for_layout(&draft).is_ok());
    assert!(capture.for_layout(&original).is_err());
    assert_eq!(capture.server_sheets_sha256, source.sheets_sha256);
    assert!(
        ds_report_host::bind_print_styles(
            None,
            &capture,
            "project-a",
            "stable",
            "user-a",
            &"a".repeat(64),
            &proof.sheets().unwrap(),
            &source.sheets_sha256
        )
        .is_ok()
    );
    assert!(
        ds_report_host::bind_print_styles(
            None,
            &capture,
            "project-a",
            "stable",
            "user-a",
            &"a".repeat(64),
            &proof.sheets().unwrap(),
            &proof.sheets_sha256
        )
        .is_err()
    );
    assert_eq!(source.sheets().unwrap(), sheets);
}
#[test]
fn held_style_capture_uses_final_override_precedence_and_selected_layouts_only() {
    let selection = ds_command_kernel::report_formats::normalize_output_selection(
        None,
        Some(vec!["pdf__detail".into()]),
        None,
    )
    .unwrap();
    let mut layout = ds_command_kernel::printing::default_layout();
    layout.id = "detail".into();
    let title = layout
        .elements
        .iter()
        .find(|e| e.kind == ds_command_kernel::printing::ElementKind::Text)
        .unwrap()
        .id
        .clone();
    layout.transformer_overrides =
        serde_json::from_value(json!({"focus":{"elements":{title.clone():{"font_pt":11.0}}}}))
            .unwrap();
    let mut other = layout.clone();
    other.id = "unselected".into();
    let sheets = json!({"printing_setups":[{"id":"detail","layout":layout},{"id":"unselected","layout":other}],"printing_overrides":{"focus":{"detail":{"elements":{title.clone():{"font_pt":13.0}}}}}});
    let rows = vec![
        super::super::hold::Row {
            name: "focus".into(),
            kind: "transformer".into(),
            state: "active".into(),
            retired: false,
            reason: None,
        },
        super::super::hold::Row {
            name: "retired".into(),
            kind: "transformer".into(),
            state: "retired".into(),
            retired: true,
            reason: None,
        },
    ];
    let captured = super::effective_capture_layouts(&sheets, &[], &rows, &selection).unwrap();
    assert_eq!(captured.len(), 2);
    assert!(captured.iter().all(|l| l.id == "detail"));
    let effective = ds_command_kernel::printing::overrides::effective(
        &layout,
        "focus",
        &serde_json::from_value(sheets["printing_overrides"].clone()).unwrap(),
    )
    .unwrap();
    assert!(captured.contains(&effective));
    assert_eq!(
        effective
            .elements
            .iter()
            .find(|e| e.id == title)
            .unwrap()
            .font_pt,
        13.0
    );
    assert!(!captured.iter().any(|l| l.id == "unselected"));
}
#[test]
fn a_pinned_room_must_be_one_exact_saved_revision_of_this_batch() {
    let temp = tempfile::tempdir().unwrap();
    let layers: BTreeMap<String, Value> = BTreeMap::from([(
        "tr".to_owned(),
        json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"kibungo"},"geometry":{"type":"Point","coordinates":[29.7,-2.6]}}]}),
    )]);
    let digest = ds_command_kernel::report_export::jcs::layers_content_digest(&layers).unwrap();
    let write = |name: &str, room: &ds_project_data::room_hold::Room| {
        let path = temp.path().join(name);
        std::fs::write(&path, serde_json::to_vec(room).unwrap()).unwrap();
        path.to_string_lossy().into_owned()
    };
    let room = ds_project_data::room_hold::Room {
        transformer: "kibungo".into(),
        version: Some(2),
        content_digest: Some(digest.clone()),
        layers: layers.clone(),
    };
    let names = vec!["kibungo".to_owned()];
    let good = write("good.json", &room);
    let pins = pinned_rooms(std::slice::from_ref(&good), &names).unwrap();
    assert_eq!(pins["kibungo"].0.version, Some(2));
    assert_eq!(pins["kibungo"].1.len(), 64);
    let mut drifted = room.clone();
    drifted.content_digest = Some("0".repeat(64));
    let drifted = write("drifted.json", &drifted);
    let refused = |paths: &[String]| pinned_rooms(paths, &names).unwrap_err().code().to_owned();
    assert_eq!(refused(&[drifted]), PINNED_ROOM_INVALID.code);
    assert_eq!(
        refused(&[good.clone(), good.clone()]),
        PINNED_ROOM_INVALID.code
    );
    let mut other = room;
    other.transformer = "ruhuha".into();
    let other = write("other.json", &other);
    assert_eq!(refused(&[other]), PINNED_ROOM_INVALID.code);
}

#[test]
fn command_defaults_ignore_a_saved_print_selection_but_an_explicit_set_is_used() {
    let inputs = ds_cli_contract::args::parse(
        &super::COMMAND,
        &[
            "--project".into(),
            "project-a".into(),
            "--out-dir".into(),
            "out".into(),
            "--dry-run".into(),
        ],
    )
    .unwrap();
    let receipt = |sheets: serde_json::Value| ds_command_kernel::report_export::InputReceipt {
        local_print_recipe: None,
        schema: 1,
        country: "RW".into(),
        sheets_json: sheets.to_string(),
        sheets_sha256: ds_command_kernel::report_export::sha256_hex(sheets.to_string().as_bytes()),
        reference_semantic_sha256: "a".repeat(64),
    };
    let run = |source: &ds_command_kernel::report_export::InputReceipt| {
        let selection = super::run_output_selection(&inputs).unwrap();
        let selection = super::run_outputs(&inputs, source, selection).unwrap();
        super::reuse::policy_outputs(source, Some(&selection)).unwrap()
    };
    // The saved print is never implicit, and a project selection without
    // the saved voltage-drop analysis does not export or require it.
    let source = receipt(
        serde_json::json!({"project_settings":[{"parameter":"design_export_format","value":["pdf__absent_external_setup","gpkg"]}]}),
    );
    assert_eq!(run(&source), ["shp", "kmz", "xlsx"]);
    assert!(!ds_command_kernel::report_formats::requires_voltage_drop_analysis(&run(&source)));
    assert!(super::reuse::policy_outputs(&source, None).is_err());
    // A project whose own selection exports the analysis gets it.
    let exported = receipt(
        serde_json::json!({"project_settings":[{"parameter":"design_export_format","value":{"schema":"ds.design-output-selection/v1","geospatial":["kmz","shp"],"tabular":["xlsx","voltage_drop"]}}]}),
    );
    assert_eq!(run(&exported), ["shp", "kmz", "xlsx", "voltage_drop"]);
    // A project that stores no selection keeps the documented defaults.
    assert_eq!(
        run(&receipt(serde_json::json!({}))),
        ["shp", "kmz", "xlsx", "voltage_drop"]
    );
    // An unreadable stored selection is refused by name, never guessed.
    let unreadable = receipt(
        serde_json::json!({"project_settings":[{"parameter":"design_export_format","value":{"schema":"ds.design-output-selection/v1","tabular":"xlsx"}}]}),
    );
    let selection = super::run_output_selection(&inputs).unwrap();
    assert_eq!(
        super::run_outputs(&inputs, &unreadable, selection)
            .unwrap_err()
            .code(),
        INPUTS_INVALID.code
    );

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("selection.json");
    std::fs::write(
        &path,
        br#"{"schema":"ds.design-output-selection/v1","tabular":["xlsx"]}"#,
    )
    .unwrap();
    let inputs = ds_cli_contract::args::parse(
        &super::COMMAND,
        &[
            "--project".into(),
            "project-a".into(),
            "--out-dir".into(),
            "out".into(),
            "--selection".into(),
            path.display().to_string(),
            "--dry-run".into(),
        ],
    )
    .unwrap();
    let selection = super::run_output_selection(&inputs).unwrap();
    let selected = super::reuse::policy_outputs(&source, Some(&selection)).unwrap();
    assert_eq!(selected, ["xlsx"]);
    // This is the same demand gate used before acquiring saved analysis
    // and its transformer-nature projection for the batch.
    assert!(!ds_command_kernel::report_formats::requires_voltage_drop_analysis(&selected));
    assert!(super::preflight(&inputs).is_ok());
    std::fs::write(&path, b"{\"output_root\":\"C:\\\\External\"}").unwrap();
    assert!(super::preflight(&inputs).is_err());
}

#[test]
fn project_batch_stages_exact_saved_analysis_only_for_matching_room_pins() {
    let room = ds_project_data::room_hold::Room {
        transformer: "fill_in_kanto2".into(),
        version: Some(3),
        content_digest: Some("a".repeat(64)),
        layers: BTreeMap::new(),
    };
    let document = b"{ \"schema\": \"ds.lv-voltage-drop.analysis/v1\", \"summary\": {} }\n";
    let sha = ds_command_kernel::report_export::sha256_hex(document);
    let metadata = json!({"state":"ready", "analysis_sha256":sha});
    let admitted = fenced_batch_analysis(
        &room,
        &room.transformer,
        Some(3),
        room.content_digest.as_deref(),
        Some(&metadata),
        Some(document),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let request = directory.path().join("request.json");
    std::fs::write(
        &request,
        serde_json::to_vec(
            &json!({"transformer":room.transformer,"formats":["xlsx","voltage_drop"]}),
        )
        .unwrap(),
    )
    .unwrap();
    struct CapturingEngine;
    impl ReportEngine for CapturingEngine {
        fn build_info(&self) -> Result<Value, HostFailure> {
            Ok(json!({}))
        }
        fn export_transformer_report(
            &self,
            request: &Path,
            _: &Path,
        ) -> Result<EngineExit, HostFailure> {
            let request: Value = serde_json::from_slice(&std::fs::read(request).unwrap()).unwrap();
            assert!(
                request["voltage_drop_result"].is_object(),
                "batch adapter dropped the saved analysis: {request}"
            );
            Ok(EngineExit {
                succeeded: true,
                summary: String::new(),
            })
        }
    }
    let analyses = std::sync::Mutex::new(BTreeMap::from([(room.transformer.clone(), admitted)]));
    let engine = SavedAnalysisEngine {
        analyses: &analyses,
        delegate: &CapturingEngine,
        report_format: "brief",
    };
    engine
        .export_transformer_report(&request, &directory.path().join("result.json"))
        .unwrap();
    assert!(
        analyses.lock().unwrap().is_empty(),
        "analysis is consumed once under this batch's transformer scope"
    );
    let staged: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(staged["voltage_drop_result"]["path"].as_str().unwrap()).unwrap(),
        document
    );
    assert_eq!(staged["voltage_drop_result"]["sha256"], sha);
    assert_eq!(staged["voltage_drop_result"]["report_format"], "brief");
    assert_eq!(staged["formats"], json!(["xlsx", "voltage_drop"]));

    // Moved saved heads and different layer pins are never substituted for a held room.
    for (version, digest) in [
        (Some(4), room.content_digest.as_deref()),
        (Some(3), Some("other")),
    ] {
        let stale = fenced_batch_analysis(
            &room,
            &room.transformer,
            version,
            digest,
            Some(&metadata),
            Some(document),
        )
        .unwrap();
        assert!(matches!(stale, BatchAnalysis::Stale));
    }
    assert!(matches!(
        fenced_batch_analysis(
            &room,
            &room.transformer,
            Some(3),
            room.content_digest.as_deref(),
            None,
            None
        )
        .unwrap(),
        BatchAnalysis::Missing
    ));
    let stale = json!({"state":"stale"});
    assert!(matches!(
        fenced_batch_analysis(
            &room,
            &room.transformer,
            Some(3),
            room.content_digest.as_deref(),
            Some(&stale),
            None
        )
        .unwrap(),
        BatchAnalysis::Stale
    ));
    let wrong = json!({"state":"ready","analysis_sha256":"b".repeat(64)});
    assert!(
        fenced_batch_analysis(
            &room,
            &room.transformer,
            Some(3),
            room.content_digest.as_deref(),
            Some(&wrong),
            Some(document)
        )
        .is_err()
    );
    for (analysis, code) in [
        (BatchAnalysis::Missing, "missing"),
        (BatchAnalysis::Stale, "stale"),
    ] {
        std::fs::write(&request, b"{}").unwrap();
        stage_batch_analysis(&request, &analysis, "brief").unwrap();
        let staged: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
        assert_eq!(staged["voltage_drop_refusal"], code);
        assert!(staged.get("voltage_drop_result").is_none());
    }
}

#[test]
fn native_saved_analysis_refusals_remain_independent_formats_without_retry() {
    for (code, missing) in [
        ("TRANSFORMER_ANALYSIS_MISSING", true),
        ("TRANSFORMER_ANALYSIS_STALE", false),
        // What ds-client-core now carries (be552f2b).
        ("transformer_analysis_missing", true),
        ("transformer_analysis_stale", false),
    ] {
        let failure = Failure::failed("auth_response_unreadable", "fenced read refused")
            .detail(json!({"http_status":409,"service_code":code}));
        let refusal = batch_analysis_refusal(&failure).unwrap();
        assert_eq!(matches!(refusal, BatchAnalysis::Missing), missing);
    }
    for (status, code) in [
        (403, "TRANSFORMER_ANALYSIS_STALE"),
        (409, "TRANSFORMER_ANALYSIS_INVALID"),
        (503, "TRANSFORMER_ANALYSIS_MISSING"),
    ] {
        let failure = Failure::failed("auth_response_unreadable", "read refused")
            .detail(json!({"http_status":status,"service_code":code}));
        assert!(batch_analysis_refusal(&failure).is_none());
    }
}

#[test]
fn transformer_nature_projection_is_required_only_for_voltage_drop_output() {
    use ds_command_kernel::report_formats::requires_voltage_drop_analysis as requires;
    assert!(!requires(&["shp".to_string(), "xlsx".to_string()]));
    assert!(requires(&["xlsx".to_string(), "voltage_drop".to_string()]));
    assert!(requires(&["voltage_drop_pdf".to_string()]));
}

#[test]
fn governed_transformer_nature_resolves_held_and_fetched_transformers() {
    let document = serde_json::json!({
        "schema_version": "ds-report.design-tags/v3",
        "project_id": "project-a",
        "selected_definition_ids": ["transformer_nature"],
        "groups": [{
            "definition_id": "transformer_nature",
            "state": "active",
            "cardinality": "single",
            "values": [
                {"value": "new_transformer"},
                {"value": "fill_in"},
                {"value": "upgrade"}
            ]
        }],
        "assignments": [
            {
                "object": {"kind": "lv_transformer", "id": "held_fill_in"},
                "definition_id": "transformer_nature",
                "values": ["fill_in"]
            },
            {
                "object": {"kind": "lv_transformer", "id": "fetched_upgrade"},
                "definition_id": "transformer_nature",
                "values": ["upgrade"]
            }
        ]
    });
    let names = vec!["held_fill_in".to_string(), "fetched_upgrade".to_string()];
    let natures =
        transformer_natures_from_projection(&document.to_string(), "project-a", &names).unwrap();
    assert_eq!(natures["held_fill_in"], "fill_in");
    assert_eq!(natures["fetched_upgrade"], "upgrade");
}

#[test]
fn unassigned_transformers_do_not_inherit_a_name_based_or_stale_nature() {
    let document = serde_json::json!({
        "schema_version": "ds-report.design-tags/v3",
        "project_id": "project-a",
        "selected_definition_ids": ["transformer_nature"],
        "groups": [{
            "definition_id": "transformer_nature",
            "state": "active",
            "cardinality": "single",
            "values": [{"value": "fill_in"}, {"value": "upgrade"}]
        }],
        "assignments": []
    });
    let names = vec!["fill_in_unassigned".to_string()];
    let natures =
        transformer_natures_from_projection(&document.to_string(), "project-a", &names).unwrap();
    assert!(natures.is_empty());
}

/// 09b78d74: a proof's refs the receipt never sealed are named all at
/// once, with the source each was looked for in, instead of failing in
/// the engine one ref at a time.
#[test]
fn unsealed_proof_styles_are_named_with_their_sources() {
    let mut layout = ds_command_kernel::printing::default_layout();
    layout.style_refs = [
        ("roads", "gt/roads_print"),
        ("cells", "gt/cell_boundaries_print"),
        ("kivu", "gt/kivu_lake_print"),
        ("again", "gt/cell_boundaries_print"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let sheets = serde_json::json!({"printing_styles": {"gt/roads_print": {"type": "line"}}});
    let unsealed = super::unsealed_print_style_refs(&sheets, std::slice::from_ref(&layout));
    assert_eq!(unsealed, ["gt/cell_boundaries_print", "gt/kivu_lake_print"]);
    assert!(super::unsealed_print_style_refs(
        &serde_json::json!({"printing_styles": {"gt/roads_print": {}, "gt/cell_boundaries_print": {}, "gt/kivu_lake_print": {}}}),
        std::slice::from_ref(&layout)
    )
    .is_empty());
    let held = "gt/cell_boundaries_print".to_string();
    let absent = "gt/kivu_lake_print".to_string();
    let refused = super::unsealed_print_styles_refusal(&[&held], &[&absent], "proof");
    assert_eq!(refused.code(), "report_inputs_invalid");
    let text = format!(
        "{} {}",
        refused.message(),
        refused.remedy_text().unwrap_or("")
    );
    for needle in [
        "SELECTED printing setups",
        "ds style list --query _print",
        "gt/cell_boundaries_print",
        "published nowhere",
        "gt/kivu_lake_print",
        "ds report project outputs set",
    ] {
        assert!(text.contains(needle), "{text}");
    }
}
/// The batch screen an operator actually reads. A run that delivered four
/// artifacts and lost its A0 must not print as plain `ok`: on Gisagara
/// that would be 86 `ok` rows over 59 missing sheets, the same work lost
/// — now silently rather than loudly.
#[test]
fn a_partial_run_is_visible_on_the_screen_with_the_member_that_lost_it() {
    let data = serde_json::json!({
        "project": {"project_name": "Gisagara", "ds_project": "gisagara"},
        "lane": "stable",
        "batch": {
            "status": "completed", "completed": 2, "failed": 0,
            "partial_formats": 1, "concurrency": 4, "receipt": "/out/report-batch.json",
        },
        "engine": {"engine_version": "ds-network-reporter@0.1.0", "publication_state": "pending"},
        "results": [
            {"transformer": "upgrade_ruturo", "status": "ok",
             "receipt": "upgrade_ruturo/report-run.json", "artifacts": 4,
             "failed_formats": [{
                 "output_id": "pdf__a0-landscape-gisagara-cjic",
                 "format": "pdf__a0-landscape-gisagara-cjic",
                 "code": "format_build_failed",
                 "message": "table lv_schedule has 212 rows; needs 3 panels",
                 "remedy": "raise `panels` on `lv_schedule` in this printing setup",
                 "layout": {"element_id": "lv_schedule", "knob": "panels",
                            "detail": "212 rows need 3 panels; the layout permits 1"}
             }]},
            {"transformer": "tx_b", "status": "ok", "receipt": "tx_b/report-run.json",
             "artifacts": 5,
             "survey_layers_omitted": {"code": "print_survey_layers_omitted", "forms": ["field_notes", "transformer_survey"]},
             "survey_layers": [{"form": "customer_survey", "layer": "survey_customer_survey", "features": 3}]},
        ],
    });
    let screen = super::render(&data);
    assert!(screen.contains("1 partial"), "{screen}");
    assert!(screen.contains("partial upgrade_ruturo"), "{screen}");
    assert!(screen.contains("4 artifact(s), 1 lost"), "{screen}");
    assert!(
        screen.contains("pdf__a0-landscape-gisagara-cjic"),
        "{screen}"
    );
    assert!(screen.contains("panels on lv_schedule"), "{screen}");
    assert!(screen.contains("the layout permits 1"), "{screen}");
    assert!(screen.contains("fix: raise `panels`"), "{screen}");
    // A run that lost nothing still reads exactly as it did.
    assert!(screen.contains("  ok     tx_b"), "{screen}");
    assert!(
        screen.contains("survey omitted: field_notes, transformer_survey"),
        "{screen}"
    );
    assert!(
        screen.contains("survey appended: customer_survey (3)"),
        "{screen}"
    );
}

/// The naming is bounded, and the truncation says how much it did not
/// print and where the rest is.
#[test]
fn a_batch_that_lost_many_formats_bounds_what_it_prints() {
    let row = |name: &str| {
        serde_json::json!({
            "transformer": name, "status": "ok", "receipt": format!("{name}/report-run.json"),
            "artifacts": 4,
            "failed_formats": [{"output_id": "pdf__a0", "format": "pdf__a0",
                "code": "format_build_failed", "message": "m", "remedy": "r"}],
        })
    };
    let names: Vec<_> = (0..14).map(|i| format!("tx_{i:02}")).collect();
    let data = serde_json::json!({
        "project": {"project_name": "Gisagara", "ds_project": "gisagara"},
        "lane": "stable",
        "batch": {"status": "completed", "completed": 14, "failed": 0,
                  "partial_formats": 14, "concurrency": 4, "receipt": "/out/report-batch.json"},
        "engine": {"engine_version": "e", "publication_state": "pending"},
        "results": names.iter().map(|n| row(n)).collect::<Vec<_>>(),
    });
    let screen = super::render(&data);
    assert_eq!(screen.matches("           lost ").count(), 10, "{screen}");
    assert!(screen.contains("4 further lost format(s)"), "{screen}");
    // Every transformer is still named, bounded or not.
    assert!(
        names.iter().all(|name| screen.contains(name.as_str())),
        "{screen}"
    );
}

#[test]
fn current_selection_resolves_only_its_named_layouts_once() {
    let selection = ds_command_kernel::report_formats::normalize_output_selection(
        None,
        Some(vec![
            "png__detail".into(),
            "jpeg__detail".into(),
            "pdf__detail".into(),
        ]),
        None,
    )
    .unwrap();
    assert_eq!(
        super::ds_cli_report_named_setups(&selection).unwrap(),
        vec!["detail"]
    );
    let inputs = ds_cli_contract::args::parse(
        &super::COMMAND,
        &[
            "--project".into(),
            "project-a".into(),
            "--out-dir".into(),
            "out".into(),
            "--dry-run".into(),
        ],
    )
    .unwrap();
    assert!(
        super::ds_cli_report_named_setups(&super::run_output_selection(&inputs).unwrap())
            .unwrap()
            .is_empty()
    );
}

/// Acceptance A: a publishing run whose link latched unreachable says,
/// in words, that it published nothing and why — never a queued row.
#[test]
fn a_print_held_because_unreachable_is_never_published_as_current() {
    let receipt = super::withheld_offline("auth_transient: the service answered 503");
    assert_eq!(receipt["stage"], "nothing_published");
    assert_eq!(receipt["published_nothing"], true);
    assert_eq!(receipt["reason"], "service_unreachable");
    assert_ne!(
        receipt["stage"],
        ds_command_kernel::report::PublicationStage::Queued.as_str()
    );
    assert!(receipt["unreachable"].as_str().unwrap().contains("503"));
}

/// The field schedule with its pauses removed: the same attempts, no wait.
const IMMEDIATE: ds_cli_auth::WeakNetwork =
    ds_cli_auth::WeakNetwork::new(&[std::time::Duration::ZERO; 3]);

#[test]
fn a_blink_is_retried_but_a_wrong_request_is_not() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let value = super::with_weak_network(&IMMEDIATE, || {
        calls.set(calls.get() + 1);
        if calls.get() < 3 {
            Err(ds_cli_contract::Failure::unavailable(
                "auth_transient",
                "blink",
            ))
        } else {
            Ok("room")
        }
    })
    .unwrap();
    assert_eq!((value, calls.get()), ("room", 3));
    let calls = Cell::new(0);
    let refused = super::with_weak_network(&IMMEDIATE, || {
        calls.set(calls.get() + 1);
        Err::<(), _>(ds_cli_contract::Failure::invalid(
            "report_inputs_invalid",
            "wrong",
        ))
    })
    .unwrap_err();
    assert_eq!((refused.code(), calls.get()), ("report_inputs_invalid", 1));
    // Four blinks in a row is an outage: the last refusal comes back.
    let calls = Cell::new(0);
    let outage = super::with_weak_network(&IMMEDIATE, || {
        calls.set(calls.get() + 1);
        Err::<(), _>(ds_cli_contract::Failure::unavailable(
            "auth_transient",
            "blink",
        ))
    })
    .unwrap_err();
    assert_eq!((outage.code(), calls.get()), ("auth_transient", 4));
}

use super::*;

#[test]
fn batch_inputs_cannot_cross_principal_lane_audience_or_project() {
    use ds_cli_auth::ProviderIdentity;
    let identity = ProviderIdentity::new("stable", &"a".repeat(64), "user-a").unwrap();
    assert!(require_same_context(&identity, "project-a", &identity, "project-a").is_ok());
    for other in [
        ProviderIdentity::new("stable", &"a".repeat(64), "user-b").unwrap(),
        ProviderIdentity::new("canary", &"a".repeat(64), "user-a").unwrap(),
        ProviderIdentity::new("stable", &"b".repeat(64), "user-a").unwrap(),
    ] {
        assert_eq!(
            require_same_context(&identity, "project-a", &other, "project-a")
                .unwrap_err()
                .code,
            INPUTS_INVALID.code
        );
    }
    assert!(require_same_context(&identity, "project-a", &identity, "project-b").is_err());
}

/// Every code the host crate can answer with is a literal arm here and a
/// documented refusal, so a caller can plan for it from `--help`.
#[test]
fn every_host_code_is_mapped_and_documented() {
    for code in [
        ds_report_host::CODE_INPUTS_INVALID,
        ds_report_host::CODE_STAGING_FAILED,
        ds_report_host::CODE_ENGINE_REFUSED,
        ds_report_host::CODE_EXPORT_BLOCKED,
        ds_report_host::CODE_RESULT_INVALID,
        ds_report_host::CODE_OUTPUT_EXISTS,
        ds_report_host::CODE_ENGINE_IDENTITY_CHANGED,
        ds_report_host::CODE_ENGINE_IDENTITY_INVALID,
        ds_report_host::CODE_ADMIN_BOUNDS_UNAVAILABLE,
        DS_REPORT.missing_code,
        "callee_timed_out",
    ] {
        let failure = host_failure(HostFailure::new(code, "why").with_detail(json!({"k": 1})));
        assert_eq!(failure.code(), code, "{code} must survive the mapping");
        assert!(failure.remedy_text().is_some(), "{code} needs a remedy");
        assert_eq!(failure.detail_value().unwrap()["k"], 1);
        assert!(
            REFUSALS.iter().any(|refusal| refusal.code == code),
            "{code} is not documented"
        );
    }
    // An unknown host code never becomes an undocumented one.
    assert_eq!(
        host_failure(HostFailure::new("something_new", "why")).code(),
        "report_result_invalid"
    );
    assert!(
        REFUSALS
            .iter()
            .any(|refusal| refusal.code == NOT_ACTIVE.code)
    );
}

/// Every failure the holdings crate can answer with for one transformer
/// becomes a batch row under a code this command documents.
#[test]
fn every_context_failure_is_a_documented_row_code() {
    use ds_project_data::Failure as Cause;
    for cause in [
        Cause::NotHeld("x".into()),
        Cause::AcquisitionFailed("x".into()),
        Cause::ProviderUnavailable("x".into()),
        Cause::BundleUnavailable("x".into()),
        Cause::Unsupported("x".into()),
        Cause::TooLarge("x".into()),
        Cause::CatalogInvalid("x".into()),
        Cause::Store("x".into()),
    ] {
        let failure = context_failure(cause);
        assert!(
            REFUSALS.iter().any(|refusal| refusal.code == failure.code),
            "{} is not documented",
            failure.code
        );
        let mapped = host_failure(failure.clone());
        assert_eq!(
            mapped.code(),
            failure.code,
            "{} must survive the mapping",
            failure.code
        );
        assert!(mapped.remedy_text().is_some());
    }
}

/// The preview classifies as the desktop door does: every code in the
/// host crate's class table lands in the same class through
/// `preview_failure`, and where this command's own `host_failure` table
/// knows the code the two tables agree — one classification, two doors.
#[test]
fn preview_refusals_classify_as_the_kernel_class_table_does() {
    for (code, class) in ds_report_host::CLASS_TABLE {
        let refusal = PreviewRefusal::Host(HostFailure::new(code, "why"));
        assert_eq!(refusal.class(), *class);
        let failure = preview_failure(refusal.clone());
        assert_eq!(failure.code(), *code, "{code} must survive the mapping");
        assert_eq!(
            failure.class().token(),
            class.token(),
            "{code} classifies differently from the kernel"
        );
        assert_eq!(failure.class().retryable(), class.retryable());
        let own = host_failure(HostFailure::new(code, "why"));
        if own.code() == *code {
            assert_eq!(
                own.class().token(),
                class.token(),
                "{code}: host_failure and CLASS_TABLE disagree"
            );
        }
        if REFUSALS.iter().any(|documented| documented.code == *code) {
            assert!(failure.remedy_text().is_some(), "{code} needs a remedy");
        }
    }
}

/// Every preview refusal this host can reach is documented. The kernel's
/// room and capture admissions are not reachable here: the CLI admits
/// the service's room itself and hands no captures, so those codes stay
/// the desktop door's.
#[test]
fn every_reachable_preview_code_is_documented() {
    const DESKTOP_ONLY: &[&str] = &[
        "print_preview_room_unavailable",
        "print_preview_room_incomplete",
        "print_context_capture_invalid",
        "print_context_survey_empty",
        "print_context_capture_changed",
        "print_context_foreign_project",
        "print_context_capture_incomplete",
        "print_context_not_held_source",
        "print_context_omitted",
    ];
    for (code, _) in ds_report_host::CLASS_TABLE {
        if DESKTOP_ONLY.contains(code) {
            continue;
        }
        assert!(
            REFUSALS.iter().any(|documented| documented.code == *code),
            "{code} is not documented"
        );
    }
    // A kernel refusal keeps its message key and params for a localised
    // surface, and a host failure keeps its own detail.
    let kernel = preview_failure(PreviewRefusal::Kernel(
        ds_command_kernel::printing::preview::Refusal {
            code: "print_context_survey_empty".into(),
            message_key: "printing_context_survey_empty".into(),
            params: BTreeMap::from([("name".to_string(), "poles".to_string())]),
        },
    ));
    assert_eq!(kernel.code(), "print_context_survey_empty");
    assert_eq!(kernel.class(), ExitClass::InvalidInput);
    assert_eq!(
        kernel.detail_value().unwrap()["message_key"],
        "printing_context_survey_empty"
    );
    assert_eq!(kernel.detail_value().unwrap()["params"]["name"], "poles");
    let host = preview_failure(PreviewRefusal::Host(
        HostFailure::new("export_blocked", "blocked").with_detail(json!({"blockers": ["x"]})),
    ));
    assert_eq!(host.class(), ExitClass::Failed);
    assert_eq!(host.remedy_text(), Some(EXPORT_BLOCKED.remedy));
    assert_eq!(host.detail_value().unwrap()["blockers"][0], "x");
}

#[test]
fn the_engine_summary_is_bounded_and_prefers_stderr() {
    assert_eq!(bounded_summary("bad\u{7} thing\n", "ignored"), "bad thing");
    assert_eq!(bounded_summary("   ", "stdout words"), "stdout words");
    assert_eq!(bounded_summary(&"x".repeat(5_000), "").len(), 2_000);
}

#[test]
fn the_descriptor_stays_headless_and_bounded() {
    assert_eq!(COMMAND.authority, Authority::HeadlessProject);
    assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
    assert!(COMMAND.summary.len() <= 70);
    assert!(
        COMMAND
            .arg("project")
            .is_some_and(|arg| arg.required && arg.default.is_none())
    );
    // Acceptance B: there is no `--publish`. An export publishes, and the
    // delivery publishes nothing only in a dry run. Preview modes have
    // their own retained-file result. If this switch
    // ever comes back, every run that forgets it strands its artifacts
    // again.
    assert!(COMMAND.args.iter().all(|arg| arg.name != "publish"));
    assert!(COMMAND.args.iter().any(|arg| arg.name == "dry-run"));
    assert!(COMMAND.args.iter().any(|arg| arg.name == "seed"));
    assert!(
        !COMMAND
            .purpose
            .contains("External print context is omitted")
    );
    assert!(COMMAND.args.iter().any(|arg| arg.name == "core-state-dir"));
    assert!(
        COMMAND
            .purpose
            .contains("Prints regenerate; data may reuse")
    );
}

#[test]
fn release_reports_seal_only_verified_outputs_for_the_server_queue() {
    let root = tempfile::tempdir().unwrap();
    let report_dir = root.path().join("report");
    std::fs::create_dir(&report_dir).unwrap();
    let artifact = report_dir.join("report.xlsx");
    std::fs::write(&artifact, b"x").unwrap();
    let mut run = ds_report_host::RunOutcome {
        transformer: "tx-a".into(),
        client_run_id: "report-00000000000000000000000000000000".into(),
        artifact_dir: report_dir,
        receipt_path: root.path().join("report-run.json"),
        receipt: json!({
            "project_id": "project-a",
            "transformer": "tx-a",
            "transformer_revision": 3,
            "input_base_fingerprint": "a".repeat(64),
            "room_content_sha256": "b".repeat(64),
        }),
        artifacts: vec![ds_command_kernel::report_export::VerifiedArtifact {
            output_id: "xlsx".into(),
            format: "xlsx".into(),
            filename: "report.xlsx".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            size_bytes: 1,
            sha256: "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881".into(),
            paper_size: None,
            presentation: None,
        }],
        // A partial run: the workbook completed, the A0 sheet did not.
        // `--publish` seals what completed and queues nothing for what
        // did not — which is the whole reason the completed work is
        // allowed to leave the machine.
        failed: vec![ds_command_kernel::report_export::FailedFormat {
            output_id: "pdf__a0-landscape-gisagara-cjic".into(),
            format: "pdf__a0-landscape-gisagara-cjic".into(),
            code: "format_build_failed".into(),
            message: "table lv_schedule has 212 rows; needs 3 panels at the authored type size (maximum 1)".into(),
            remedy: "raise `panels` on `lv_schedule`".into(),
            layout: None,
        }],
        warnings: vec![],
        survey_layers_omitted: None,
        survey_layers: Vec::new(),
        engine: ds_command_kernel::report_export::EngineIdentity {
            engine_version: format!("ds-network-reporter@0.1.0+{}", "c".repeat(40)),
            build_manifest_sha256: "d".repeat(64),
            package_version: "0.1.0".into(),
            source_sha: "c".repeat(40),
            profile: "release".into(),
        },
    };
    let fence = ds_command_kernel::sync_store::Fence {
        account: "owner-a".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install-1".into(),
    };
    let queue_at = |name: &str| {
        PublicationQueue::at(
            root.path().join(name).join("report-artifacts"),
            ds_sync_runtime::open_store(&root.path().join(name).join("store.sqlite")).unwrap(),
            fence.clone(),
        )
    };
    let queue = queue_at("queue");
    let SealedRun::Queued(sealed) =
        seal_run_for_server(&run, "owner-a", "project-a", &queue, &|| Ok(())).unwrap()
    else {
        panic!("a first seal is queued")
    };
    assert_eq!(sealed.receipt.project_id, "project-a");
    assert_eq!(sealed.receipt.transformer, "tx-a");
    assert_eq!(
        sealed.row.state,
        ds_command_kernel::sync_store::ArtifactState::Held
    );
    // The store is the queue: the row is there, and the bytes it names
    // are the committed batch.
    let status = queue
        .store
        .lock()
        .unwrap()
        .queue(&fence, Some("project-a"), ds_sync_runtime::now_ms())
        .unwrap();
    assert_eq!(status.queued_batches, 1);
    assert_eq!(status.projects[0].transformers, vec!["tx-a".to_string()]);
    let batch = ds_report_artifacts::committed_batch(&queue.root, &sealed.receipt.batch_id)
        .unwrap()
        .expect("the batch the row names is committed");
    assert!(ds_report_artifacts::open_batch_output(&queue.root, &batch, "xlsx").is_ok());
    // Only what completed is queued; the format that failed is not
    // invented into the publication.
    assert!(
        ds_report_artifacts::open_batch_output(
            &queue.root,
            &batch,
            "pdf__a0-landscape-gisagara-cjic"
        )
        .is_err()
    );
    // The same run sealed again is the same row, and no second batch.
    let SealedRun::AlreadyRecorded { replay_key, .. } =
        seal_run_for_server(&run, "owner-a", "project-a", &queue, &|| Ok(())).unwrap()
    else {
        panic!("the same bytes are already recorded")
    };
    assert_eq!(replay_key, sealed.receipt.client_publish_id);
    assert_eq!(
        ds_report_artifacts::list_project_publication_batches_from_root(&queue.root, "project-a")
            .unwrap()
            .len(),
        1
    );

    let rollback_queue = queue_at("rollback");
    let error = seal_run_for_server(&run, "owner-a", "project-a", &rollback_queue, &|| {
        Err(Failure::conflict(
            PUBLISH_SCOPE_CHANGED.code,
            "scope changed",
        ))
    })
    .unwrap_err();
    assert_eq!(error.code(), PUBLISH_SCOPE_CHANGED.code);
    assert!(
        ds_report_artifacts::list_project_publication_batches_from_root(
            &rollback_queue.root,
            "project-a",
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        rollback_queue
            .store
            .lock()
            .unwrap()
            .queue(&fence, None, ds_sync_runtime::now_ms())
            .unwrap()
            .queued_batches,
        0,
        "a refused seal records no row"
    );

    std::fs::write(root.path().join("report/report.xlsx"), b"corrupt").unwrap();
    let corrupt_queue = queue_at("corrupt");
    assert_eq!(
        seal_run_for_server(&run, "owner-a", "project-a", &corrupt_queue, &|| Ok(()))
            .unwrap_err()
            .code(),
        PUBLISH_ROOT.code,
    );
    assert!(
        ds_report_artifacts::list_project_publication_batches_from_root(
            &corrupt_queue.root,
            "project-a",
        )
        .unwrap()
        .is_empty()
    );

    run.engine.engine_version = ds_command_kernel::report::DEVELOPMENT_ENGINE.into();
    assert_eq!(
        seal_run_for_server(&run, "owner-a", "project-a", &queue_at("local"), &|| Ok(()))
            .unwrap_err()
            .code(),
        PUBLISH_LOCAL_ONLY.code,
    );
}

#[test]
fn server_queue_root_is_the_exact_custom_server_state_child() {
    assert_eq!(
        server_report_artifacts_root("stable", Some(Path::new("/var/lib/ds"))).unwrap(),
        PathBuf::from("/var/lib/ds/report-artifacts"),
    );
    assert_eq!(
        server_report_artifacts_root("stable", Some(Path::new("relative")))
            .unwrap_err()
            .code(),
        PUBLISH_ROOT.code,
    );
}

fn transformer_natures_from_projection(
    document: &str,
    project: &str,
    transformers: &[String],
) -> Result<BTreeMap<String, String>, Failure> {
    ds_command_kernel::report_export::held::transformer_natures_from_projection(
        document,
        project,
        transformers,
    )
    .map_err(|e| super::invalid_transformer_nature_projection(&e))
}

#[test]
fn an_output_directory_holding_a_report_refuses_before_any_work() {
    let dir = tempfile::tempdir().unwrap();
    let names = vec!["agahehe".to_string()];
    assert!(
        existing_output(dir.path(), &names).is_ok(),
        "an empty directory is fresh"
    );
    std::fs::create_dir(dir.path().join("agahehe")).unwrap();
    assert_eq!(
        existing_output(dir.path(), &names).unwrap_err().code(),
        OUTPUT_EXISTS.code
    );
    let other = tempfile::tempdir().unwrap();
    std::fs::write(
        other
            .path()
            .join(ds_command_kernel::report_export::BATCH_RECEIPT_FILE),
        b"{}",
    )
    .unwrap();
    assert_eq!(
        existing_output(other.path(), &[]).unwrap_err().code(),
        OUTPUT_EXISTS.code
    );
}
