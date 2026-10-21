use ds_profile_view::panel_model::*;
#[test]
fn numeric_control_preserves_native_request_and_replaces_only_named_field() {
    let control:NumericControl=serde_json::from_value(serde_json::json!({"minimum":0,"maximum":1000,"value_path":["terrain","lowest_wire_drop_m"],"request":{"terrain":{"corridor_half_width_m":15,"gap_tolerance_m":100,"ground_clearance_offset_m":6,"lowest_wire_drop_m":0}}})).unwrap();
    let value = serde_json::to_value(control.patch(7.).unwrap()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"terrain":{"corridor_half_width_m":15.,"gap_tolerance_m":100.,"ground_clearance_offset_m":6.,"lowest_wire_drop_m":7.}})
    );
    assert!(control.patch(f64::NAN).is_err());
    assert!(control.patch(1001.).is_err());
}
#[test]
fn style_payload_rejects_unknown_keys() {
    assert!(serde_json::from_str::<DisplayPatch>(r#"{"anything":"else"}"#).is_err());
    assert!(serde_json::from_str::<DisplayPatch>(r#"{"styles":{"wire":{"extra":true}}}"#).is_err());
}
#[test]
fn delete_payload_uses_existing_native_command_shape() {
    let command = HeadCommand {
        command_id: "cmd".into(),
        command: ModelCommand::DeleteProfileEntities {
            ids: vec!["pole".into()],
        },
    };
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({"command_id":"cmd","command":{"command_kind":"delete_profile_entities","ids":["pole"]}})
    );
}
#[test]
fn nullable_sag_facts_remain_null() {
    let command = ModelCommand::SetTensionSectionsSagMode {
        ids: vec!["section".into()],
        sag: SagMode::FromCriteria,
        sag_condition: None,
        sag_weather_state_id: None,
        criterion_set_id: "criterion".into(),
        ruling_span_m: None,
    };
    let value = serde_json::to_value(command).unwrap();
    assert_eq!(value["sag_condition"], serde_json::Value::Null);
    assert_eq!(value["sag_weather_state_id"], serde_json::Value::Null);
    assert_eq!(value["sag"]["mode"], "from_criteria");
}
#[test]
fn legacy_generic_mode_field_deserializes_without_inference() {
    let panel: PanelModel =
        serde_json::from_value(serde_json::json!({"genericMode":true})).unwrap();
    assert!(panel.generic);
}
