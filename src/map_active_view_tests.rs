use super::*;

fn command(value: Value) -> Result<Value, String> {
    evaluate(&serde_json::to_vec(&value).unwrap())
}

#[test]
fn accepts_an_exact_camera() {
    let camera = command(json!({
        "schema": REQUEST_SCHEMA, "operation": "camera.set",
        "center": [30.0619, -1.9441], "zoom": 18.5, "pitch": 68.0, "bearing": -32.0
    }))
    .unwrap();
    assert_eq!(camera["operation"], "camera.set");
    assert_eq!(camera["camera"]["center"], json!([30.0619, -1.9441]));
}

#[test]
fn refuses_an_invalid_camera_and_the_retired_renderer() {
    for request in [
        json!({"schema": REQUEST_SCHEMA, "operation": "camera.set", "center": [30.0, -91.0], "zoom": 10.0, "pitch": 40.0, "bearing": 0.0}),
        json!({"schema": REQUEST_SCHEMA, "operation": "renderer.configure", "mode": "2d"}),
    ] {
        assert!(command(request).is_err());
    }
}

#[test]
fn rejects_unknown_fields_and_non_finite_values() {
    assert!(command(json!({
        "schema": REQUEST_SCHEMA, "operation": "camera.set",
        "center": [30.0, -1.0], "zoom": 10.0, "pitch": 40.0, "bearing": 0.0,
        "duration": 500
    }))
    .is_err());
    let raw = br#"{"schema":"ds.map.active-view.request/v1","operation":"camera.set","center":[30,-1],"zoom":1e999,"pitch":40,"bearing":0}"#;
    assert!(evaluate(raw).is_err());
}
