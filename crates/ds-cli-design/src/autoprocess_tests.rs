use super::*;

#[test]
fn mode_is_a_headless_kernel_section_and_renders() {
    let answer = section(
        "mode",
        json!({
            "process_active": true,
            "auto_process_enabled": true,
        }),
    )
    .unwrap();
    assert_eq!(answer["mode"], "auto");
    assert_eq!(answer["reason_key"], "autoprocess_mode_enabled");
    assert!(render(&json!({"mode": answer})).contains("mode               auto"));
}

#[test]
fn differential_scope_reads_layers_and_rejects_host_authored_mapping() {
    let request: ds_network_contracts::differential::DifferentialPlanRequest = serde_json::from_value( json!({
            "gdfs": {
                "lv_lines":{"features":[{"id":"f1","properties":{"path_id":"path1"}},{"id":"f2","properties":{"path_id":"path2"}}]},
                "lv_poles":{"features":[{"id":"p1","properties":{"path_id":"path1"}}]}
            },
            "differential":{"selected_feeders":[],"auto_process":{
                "differential_enabled":true,"is_mv_session":false,"accumulator_bound":true,"force_full":false,
                "changed_features":[{"layer_name":"lv_poles","feature_id":"p1"}]
            }},
            "customer_source_addresses":[]
        })).unwrap();
    assert_eq!(
        request.differential.auto_process.unwrap().changed_features[0].feature_id,
        "p1"
    );
    assert_eq!(section("differential_scope",json!({"differential_enabled":true,"change_count":1,"blocking_diagnostics":false,"mapping":{"unmapped":false,"feeder_ids":["f1"]}})).unwrap_err().code(),"autoprocess_request_invalid");
}

#[test]
fn mode_has_no_lane_member_to_switch_on() {
    // There is one way to process a transformer: the only question left
    // is whether it runs by itself. A document written against the old
    // lane switch is refused by name, never answered as if it were valid.
    let manual = section(
        "mode",
        json!({ "process_active": false, "auto_process_enabled": true }),
    )
    .unwrap();
    assert_eq!(manual["mode"], "manual");
    assert_eq!(manual["reason_key"], "autoprocess_mode_manual");
    let stale = section(
        "mode",
        json!({
            "is_fast_lane": true,
            "fast_process_active": true,
            "auto_process_enabled": true,
        }),
    );
    assert_eq!(stale.unwrap_err().code(), "autoprocess_request_invalid");
}
