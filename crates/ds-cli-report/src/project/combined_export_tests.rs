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
