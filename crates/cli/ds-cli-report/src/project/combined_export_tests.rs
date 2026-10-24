use super::*;

fn parse(tokens: &[&str]) -> Inputs {
    ds_cli_contract::args::parse(
        &COMMAND,
        &tokens
            .iter()
            .map(|token| (*token).to_owned())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

fn row(name: &str, state: &str) -> hold::Row {
    hold::Row {
        name: name.into(),
        kind: "transformer".into(),
        state: state.into(),
        retired: false,
        reason: None,
    }
}

#[test]
fn the_declaration_names_its_project_lane_scope_and_every_refusal_it_raises() {
    assert_eq!(COMMAND.id, "report.project.combined");
    let project = COMMAND.arg("project").unwrap();
    assert!(project.required && project.default.is_none());
    for arg in [
        "transformer",
        "out-dir",
        "dry-run",
        "lane",
        "core-state-dir",
    ] {
        assert!(COMMAND.arg(arg).is_some(), "--{arg}");
    }
    let declared: Vec<&str> = REFUSALS.iter().map(|refusal| refusal.code).collect();
    for code in [
        "combined_report_collision_limit_exceeded",
        "combined_report_rooms_unavailable",
        "combined_report_empty",
        "combined_report_media_scope_unsupported",
        "combined_report_media_grant_unavailable",
        "transformer_not_active",
        "reserved_transformer_identity",
        "export_blocked",
        "engine_refused",
        "report_result_invalid",
        "report_output_exists",
        "report_publish_scope_changed",
        "report_room_not_held",
        "admin_bounds_unavailable",
    ] {
        assert!(declared.contains(&code), "{code} undeclared");
    }
    assert!(COMMAND.summary.len() <= 70, "{}", COMMAND.summary);
    assert!(COMMAND.purpose.contains("never a PDF"));
}

#[test]
fn preflight_refuses_reserved_scope_and_an_existing_package_before_any_state() {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    let out = out.to_str().unwrap();
    assert!(preflight(&parse(&["--project", "p", "--out-dir", out])).is_ok());
    for reserved in ["combined_transformer", "collisions", "all_transformers"] {
        let failure = preflight(&parse(&[
            "--project",
            "p",
            "--out-dir",
            out,
            "--transformer",
            reserved,
        ]))
        .unwrap_err();
        assert_eq!(
            failure.code(),
            "reserved_transformer_identity",
            "{reserved}"
        );
    }
    std::fs::create_dir_all(root.path().join("out").join("combined_transformer")).unwrap();
    assert_eq!(
        preflight(&parse(&["--project", "p", "--out-dir", out]))
            .unwrap_err()
            .code(),
        "report_output_exists"
    );
}

#[test]
fn the_scope_is_every_active_lv_transformer_or_exactly_the_active_names_given() {
    let rows = vec![
        row("tx_b", "active"),
        row("tx_a", "active"),
        row("tx_old", "retired"),
        hold::Row {
            kind: "project_level".into(),
            ..row("mv_data", "active")
        },
    ];
    assert_eq!(
        scope_names(&rows, &TransformerSet::default()).unwrap(),
        ["tx_a", "tx_b"]
    );
    let named = TransformerSet::new(["tx_b".to_string()]).unwrap();
    assert_eq!(scope_names(&rows, &named).unwrap(), ["tx_b"]);
    for inactive in ["tx_old", "tx_missing"] {
        let named = TransformerSet::new([inactive.to_string()]).unwrap();
        assert_eq!(
            scope_names(&rows, &named).unwrap_err().code(),
            "transformer_not_active"
        );
    }
}

#[test]
fn a_blocked_collision_gate_refuses_in_the_owner_words_with_the_policy() {
    let gate = decisions::collision_gate(
        &json!({"project_settings": []}),
        Some(Some(&json!({"layers": {"collisions": 25}}))),
    );
    assert!(!gate.allowed);
    let failure = collision_refusal(&gate);
    assert_eq!(failure.code(), "combined_report_collision_limit_exceeded");
    assert!(
        failure
            .message()
            .starts_with("Too many transformers are overlapping")
    );
    let detail = failure.detail_value().unwrap();
    assert_eq!(detail["collision_rows"], 25);
    assert_eq!(detail["max_collision_rows"], 10);
}

#[test]
fn every_host_failure_code_maps_to_a_declared_refusal() {
    let declared: Vec<&str> = REFUSALS.iter().map(|refusal| refusal.code).collect();
    for code in [
        "report_inputs_invalid",
        "report_staging_failed",
        "report_output_exists",
        "report_result_invalid",
        "report_engine_identity_changed",
        "admin_bounds_unavailable",
        "export_blocked",
        "engine_refused",
        "callee_contract_mismatch",
        "callee_timed_out",
        "reporter_engine_missing",
        "report_engine_unsupported",
    ] {
        let failure = host_failure(HostFailure::new(code, "m"));
        assert_eq!(failure.code(), code);
        assert!(declared.contains(&code), "{code}");
    }
}

#[test]
fn the_render_says_what_was_queued_and_what_was_warned() {
    let rendered = render(&json!({
        "project": "p", "lane": "canary", "publicationQueued": true,
        "scope": {"participating": 2, "transformers": 3},
        "artifacts": [{"filename": "combined_transformer.xlsx", "sizeBytes": 10}],
        "voltageDrop": {"warningCount": 1},
        "warnings": ["1 transformer(s) hold no report feature and contributed nothing: tx_c"],
    }));
    assert!(
        rendered.contains("2 of 3 transformer(s) · queued for Sync"),
        "{rendered}"
    );
    assert!(rendered.contains("combined_transformer.xlsx"), "{rendered}");
    assert!(rendered.contains("voltage-drop"), "{rendered}");
}

fn saved_room(name: &str, version: i64, poles: usize) -> ds_project_data::room_hold::Room {
    let layers = BTreeMap::from([(
        "lv_poles".to_string(),
        json!({
            "type": "FeatureCollection",
            "features": (0..poles).map(|pole| json!({
                "type": "Feature",
                "properties": {"pole_id": format!("{name}-{pole}")},
            })).collect::<Vec<_>>(),
        }),
    )]);
    ds_project_data::room_hold::Room {
        transformer: name.into(),
        version: Some(version),
        content_digest: Some(
            ds_command_kernel::report_export::jcs::layers_content_digest(&layers).unwrap(),
        ),
        layers,
    }
}

/// One combined run's room acquisition, exactly as `run` drives it: one plan
/// over the status listing's heads, then each room through the hold, with
/// every service read counted. Returns the rooms' staged document bytes.
fn combined_rooms(
    hold: Hold,
    names: &[String],
    heads: Vec<Value>,
    saved: &BTreeMap<String, ds_project_data::room_hold::Room>,
    reads: &mut Vec<String>,
) -> (Vec<Vec<u8>>, Value) {
    let mut link = Link::with_schedule(ds_cli_auth::WeakNetwork::new(
        &[std::time::Duration::ZERO; 3],
    ));
    let mut rooms = Rooms::plan(hold, names, hold::Heads::Read(heads)).unwrap();
    let staged = names
        .iter()
        .map(|name| {
            let room = rooms
                .room(name, &mut link, || {
                    reads.push(name.clone());
                    Ok(saved[name].clone())
                })
                .unwrap();
            ds_command_kernel::report_export::transformer_document_bytes(&room.layers).unwrap()
        })
        .collect();
    (staged, rooms.receipt())
}

fn listing(saved: &BTreeMap<String, ds_project_data::room_hold::Room>) -> Vec<Value> {
    saved
        .values()
        .map(|room| json!({"name": room.transformer, "version": room.version}))
        .collect()
}

/// feedback b428693b: a second combined run with nothing changed answers
/// every room from this machine's hold. The status listing is its only
/// freshness evidence; no room is read again and the staged rooms are the
/// same bytes a full read stages.
#[test]
fn an_unchanged_second_run_reads_only_the_listing_and_stages_the_same_rooms() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let names: Vec<String> = ["tx_a", "tx_b", "tx_c"].map(String::from).to_vec();
    let saved: BTreeMap<_, _> = names
        .iter()
        .map(|name| (name.clone(), saved_room(name, 3, 4)))
        .collect();

    let mut cold_reads = Vec::new();
    let (cold, cold_receipt) =
        combined_rooms(hold(), &names, listing(&saved), &saved, &mut cold_reads);
    assert_eq!(cold_reads, names, "a cold run reads every room once");
    assert_eq!(cold_receipt["rooms_fetched"], 3);

    let mut warm_reads = Vec::new();
    let (warm, warm_receipt) =
        combined_rooms(hold(), &names, listing(&saved), &saved, &mut warm_reads);
    assert!(
        warm_reads.is_empty(),
        "unchanged rooms were read: {warm_reads:?}"
    );
    assert_eq!(warm_receipt["rooms_fetched"], 0);
    assert_eq!(warm_receipt["reused"], 3);
    assert_eq!(warm_receipt["heads"], "read");
    assert_eq!(warm, cold, "held rooms stage the bytes a full read stages");

    // The configuration of that unchanged dry run is the retained one too.
    assert_eq!(
        configuration_read(false, false),
        ProjectConfigurationRead::Acquire
    );
}

/// The control: one moved head reads exactly that room again, and the run
/// stages what a full read of the new heads stages.
#[test]
fn one_moved_head_reads_only_that_room_again() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let names: Vec<String> = ["tx_a", "tx_b", "tx_c"].map(String::from).to_vec();
    let mut saved: BTreeMap<_, _> = names
        .iter()
        .map(|name| (name.clone(), saved_room(name, 3, 4)))
        .collect();
    combined_rooms(hold(), &names, listing(&saved), &saved, &mut Vec::new());

    saved.insert("tx_b".into(), saved_room("tx_b", 4, 5));
    let mut reads = Vec::new();
    let (staged, receipt) = combined_rooms(hold(), &names, listing(&saved), &saved, &mut reads);
    assert_eq!(reads, ["tx_b"]);
    assert_eq!(receipt["rooms_fetched"], 1);
    assert_eq!(receipt["reused"], 2);

    let fresh = tempfile::tempdir().unwrap();
    let mut full_reads = Vec::new();
    let (full, _) = combined_rooms(
        Hold::at(fresh.path().to_path_buf(), "uid-1", "project-1"),
        &names,
        listing(&saved),
        &saved,
        &mut full_reads,
    );
    assert_eq!(full_reads.len(), 3);
    assert_eq!(
        staged, full,
        "the mixed run is byte-identical to a full read"
    );
}

/// A publication, or an explicit --refresh, reads the configuration from the
/// service; only a run that publishes nothing answers from the retained one.
#[test]
fn only_an_unrefreshed_dry_run_uses_the_retained_configuration() {
    assert_eq!(
        configuration_read(true, false),
        ProjectConfigurationRead::Refresh
    );
    assert_eq!(
        configuration_read(true, true),
        ProjectConfigurationRead::Refresh
    );
    assert_eq!(
        configuration_read(false, true),
        ProjectConfigurationRead::Refresh
    );
    assert_eq!(
        configuration_read(false, false),
        ProjectConfigurationRead::Acquire
    );
    assert!(COMMAND.arg("refresh").is_some());
    let declared: Vec<&str> = REFUSALS.iter().map(|refusal| refusal.code).collect();
    for code in [
        "project_config_context_changed",
        "project_config_store_refused",
        "retained_reads_stale",
        "design_answer_invalid",
    ] {
        assert!(declared.contains(&code), "{code} undeclared");
    }
}

/// The retained document carries every configuration member; a combined run
/// keeps only the receipt and its refusal, exactly as it read them before.
#[test]
fn the_retained_configuration_yields_only_the_receipt_members() {
    let document: ProjectConfigurationDocument = serde_json::from_value(json!({
        "sheets": {"project_settings": [{"parameter": "x", "value": 1}]},
        "groups": {"g": 1},
        "models": [],
        "network_reporter_input_receipt": {"schema": "r", "sheets_sha256": "s"},
        "network_reporter_input_receipt_refusal": "why",
    }))
    .unwrap();
    assert_eq!(
        receipt_members(&document),
        json!({
            "network_reporter_input_receipt": {"schema": "r", "sheets_sha256": "s"},
            "network_reporter_input_receipt_refusal": "why",
        })
    );
    let bare: ProjectConfigurationDocument = serde_json::from_value(json!({"sheets": {}})).unwrap();
    assert_eq!(receipt_members(&bare), json!({}));
}
