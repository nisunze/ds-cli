use super::*;

#[test]
fn managed_output_commands_share_the_native_gate() {
    let expected = ds_cli_auth::native_availability as fn() -> ds_cli_contract::spec::Availability;
    for command in [
        &status::COMMAND,
        &preflight::COMMAND,
        &plan::COMMAND,
        &generate::COMMAND,
    ] {
        assert!(std::ptr::fn_addr_eq(command.availability, expected));
    }
}

#[test]
fn plan_decision_matches_the_pipeline_staleness_order() {
    assert_eq!(
        plan_decision(TileOperationStatus::InProgress, None, Some(true), true),
        (false, "Tile generation is already in progress.")
    );
    assert!(plan_decision(TileOperationStatus::Completed, Some(1), None, true).0);
    assert!(plan_decision(TileOperationStatus::NotStarted, None, None, false).0);
    assert!(plan_decision(TileOperationStatus::Completed, Some(1), Some(true), false).0);
    assert!(plan_decision(TileOperationStatus::Stale, Some(1), None, false).0);
    assert!(!plan_decision(TileOperationStatus::Completed, Some(1), Some(false), false).0);
}

#[test]
fn plan_never_dispatches_and_generate_reaches_no_service() {
    let plan = include_str!("plan.rs");
    assert!(plan.contains("ds_cli_auth::tile_status"));
    assert!(plan.contains("ds_cli_auth::tile_preflight"));
    assert!(!plan.contains("ds_cli_auth::tile_generate"));

    let generate = include_str!("generate.rs");
    assert!(!generate.contains("ds_cli_auth::tile_"));
    assert!(generate.contains("ds core tile submit"));
}

#[test]
fn status_and_preflight_lines_read_the_wire_shapes() {
    let line = status_line(&json!({"status": "completed", "total_features": 12, "dirty": true}));
    assert_eq!(
        line,
        "completed · 12 features · sources changed since this output"
    );
    let line = preflight_line(
        &json!({"status": "ready", "will_export_layers": 3, "expected_layer_count": 4, "empty_layers": ["spans"]}),
    );
    assert_eq!(line, "ready · 3/4 layers · empty: spans");
    assert_eq!(preflight_line(&Value::Null), "preflight not run");
}
