use ds_profile_view::payload::evaluate;
use serde_json::{json, Value};
#[test]
fn station_request_keeps_native_identity_and_units() {
    let result:Value=serde_json::from_str(&evaluate(&json!({"kind":"station","expected_revision":"r","structure_id":"p","station_m":123.45}).to_string()).unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"expected_revision":"r","structure_id":"p","station_m":123.45})
    );
}
fn ground() -> Value {
    json!({"kind":"ground","command_id":"nonce","expected_revision":"r","entity_id":"p","elevation_m":123.45,"sheet":{"revision":"r","kind":"terrain_point","command_schema_version":2,"groups":[{"fields":[{"id":"z_m","can_edit":true,"editor":{"command_kind":"edit_profile_properties"}}]}]}})
}
#[test]
fn ground_descriptor_controls_native_envelope() {
    let result: Value = serde_json::from_str(&evaluate(&ground().to_string()).unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"command_id":"nonce","expected_revision":"r","command_schema_version":2,"command":{"command_kind":"edit_profile_properties","edits":[{"entity_id":"p","field_id":"z_m","value":123.45}]}})
    );
}
#[test]
fn stale_or_noneditable_ground_descriptor_refuses() {
    for case in 0..4 {
        let mut value = ground();
        match case {
            0 => value["sheet"]["revision"] = json!("other"),
            1 => value["sheet"]["kind"] = json!("structure"),
            2 => value["sheet"]["groups"][0]["fields"][0]["can_edit"] = json!(false),
            _ => {
                value["sheet"]["groups"][0]["fields"][0]["editor"]["command_kind"] = json!("other")
            }
        };
        assert!(evaluate(&value.to_string())
            .unwrap_err()
            .starts_with("profile_ground_not_editable"));
    }
}
#[test]
fn section_creation_keeps_native_first_support_pins() {
    let value = json!({"kind":"create","pointer":{"model_revision":"r","axis_pin_digest":"axis","scene_x":10,"scene_y":20,"tolerance_scene_units":12},"id":"new","mode":"tension_section","choice":"template","first_support_id":"p","first_alignment_id":"a"});
    let result: Value = serde_json::from_str(&evaluate(&value.to_string()).unwrap()).unwrap();
    assert_eq!(
        result["operation"],
        json!({"mode":"tension_section","id":"new","template_section_id":"template","first_support_id":"p","first_alignment_id":"a"})
    );
    assert_eq!(result["axis_pin_digest"], "axis");
}
#[test]
fn lasso_builds_native_profile_polygon() {
    let result:Value=serde_json::from_str(&evaluate(&json!({"kind":"lasso","model_id":"m","expected_revision":"r","polygon":[{"scene_x":1,"scene_y":2},{"scene_x":3,"scene_y":4},{"scene_x":5,"scene_y":6}],"mode":"add"}).to_string()).unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"model_id":"m","expected_revision":"r","space":"profile","polygon":[[1.,2.],[3.,4.],[5.,6.]],"mode":"add"})
    );
}
