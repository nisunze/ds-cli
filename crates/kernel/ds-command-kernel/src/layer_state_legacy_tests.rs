use super::*;
use serde_json::json;

fn request(extra: serde_json::Value) -> Vec<u8> {
    let mut value = json!({"schema": SCHEMA, "op": {"kind": "fold", "values": [true, false]}});
    for (key, item) in extra.as_object().unwrap() {
        value[key] = item.clone();
    }
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn a_pre_3d_removal_desktop_still_sending_its_renderer_mode_is_answered() {
    let plain = evaluate(&request(json!({}))).unwrap();
    for mode in [json!("2d"), json!("3d"), json!({"renderer": "maplibre"})] {
        assert_eq!(evaluate(&request(json!({"mode": mode}))).unwrap(), plain);
    }
}

#[test]
fn any_other_unknown_field_still_refuses() {
    let error = evaluate(&request(json!({"renderer": "2d"}))).unwrap_err();
    assert!(error.contains("unknown field `renderer`"), "{error}");
}
