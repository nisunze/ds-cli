//! The save fence a configuration surface may send is decided here, not by
//! the surface: only a sheet the gateway fences carries the rows it was read as.
use super::*;
use serde_json::json;

#[test]
fn only_a_gateway_fenced_sheet_carries_its_read_rows() {
    let read = json!([{"parameter": "default_process_preset", "value": "sketch"}]);
    assert_eq!(save_fence("project_settings", read.clone()), read);
    for sheet in ["transformer_settings", "lv_poles_rules", "know_columns", ""] {
        assert_eq!(save_fence(sheet, read.clone()), Value::Null, "{sheet}");
    }
    assert_eq!(save_fence("project_settings", Value::Null), Value::Null);
    let door: Value = serde_json::from_str(
        &evaluate(br#"{"op":"save_fence","sheet_key":"project_settings","expected":[{"parameter":"a","value":1}]}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(door["result"], json!([{"parameter": "a", "value": 1}]));
    let door: Value = serde_json::from_str(
        &evaluate(br#"{"op":"save_fence","sheet_key":"transformer_settings","expected":[]}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(door["result"], Value::Null);
}
