use super::*;

fn wire(operation: Value) -> Value {
    json!({"schema":REQUEST_SCHEMA,"operation":{
        "action":"control","session_id":"held-session","model_id":"held-model",
        "expected_history":{"model_revision":"rev:head","initial_revision":"rev:initial",
            "undo_depth":1,"redo_depth":0,"history_pin":"exact-prefix"},
        "operation":operation
    }})
}

#[test]
fn gesture_controls_keep_closed_station_and_retype_intents() {
    for (operation, mutating) in [
        (
            json!({"action":"preview_structure_station_move","request":{
            "expected_revision":"rev:head","structure_id":"structure-a","station_m":320.},
            "options":{},"held_axis_pin":{}}),
            false,
        ),
        (
            json!({"action":"apply_structure_station_move","request":{
            "expected_revision":"rev:head","structure_id":"structure-a","station_m":320.}}),
            true,
        ),
        (
            json!({"action":"preview_structure_retype","request":{
            "expected_revision":"rev:head","structure_ids":["structure-a"],"structure_type_id":"type-a"}}),
            false,
        ),
        (
            json!({"action":"apply_structure_retype","request":{
            "expected_revision":"rev:head","structure_ids":["structure-a"],"structure_type_id":"type-a"}}),
            true,
        ),
    ] {
        let request =
            Request::parse(&serde_json::to_vec(&wire(operation.clone())).unwrap()).unwrap();
        let Operation::Control {
            operation: admitted,
            ..
        } = request.operation
        else {
            panic!("wrong action")
        };
        assert_eq!(admitted.is_mutating(), mutating);
        assert_eq!(serde_json::to_value(admitted).unwrap(), operation);
    }
    let mut invented_xy = wire(json!({"action":"apply_structure_station_move","request":{
        "expected_revision":"rev:head","structure_id":"structure-a","station_m":320.}}));
    invented_xy["operation"]["operation"]["request"]["x_m"] = json!(12.);
    assert!(Request::parse(&serde_json::to_vec(&invented_xy).unwrap()).is_err());
    let mut invented_operation = invented_xy;
    invented_operation["operation"]["operation"]["action"] = json!("apply_arbitrary_gesture");
    assert!(Request::parse(&serde_json::to_vec(&invented_operation).unwrap()).is_err());
}

#[test]
fn model_library_is_a_fixed_history_fenced_read_with_its_own_capability() {
    let mut input = wire(json!({}));
    let object = input["operation"].as_object_mut().unwrap();
    object.remove("operation");
    object.insert("action".into(), json!("model_library"));
    let request = Request::parse(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(request.operation.action(), "model_library");
    assert!(!request.operation.demands_style());
    let (session, model, history) = operation_owner(&request.operation).unwrap();
    assert_eq!((session, model), ("held-session", "held-model"));
    assert_eq!(history["history_pin"], "exact-prefix");
    assert!(matches!(
        read_capability(&request.operation),
        Some(NetworkCapability::ProfileRetypeV1)
    ));
    assert!(matches!(
        read_projection_operation(&request.operation, "EPSG:3395").unwrap(),
        Some(NativeProfileOperationV1::ModelLibrary {})
    ));
    assert!(
        require_read_capability(
            &[NetworkCapability::ProfileReadProjectionsV1],
            NetworkCapability::ProfileRetypeV1
        )
        .is_err()
    );
    input["operation"]["method"] = json!("arbitrary");
    assert!(Request::parse(&serde_json::to_vec(&input).unwrap()).is_err());
}

#[test]
fn cached_engineering_query_is_closed_and_distinct_from_explicit_analysis() {
    let mut input = wire(json!({}));
    let object = input["operation"].as_object_mut().unwrap();
    object.remove("operation");
    object.insert("action".into(), json!("query_results"));
    object.insert("scope".into(), json!("profile-a"));
    object.insert("expected_engineering_input_root".into(), json!("root-a"));
    object.insert(
        "request".into(),
        json!({"kind":"findings","source":"model_analysis","entity_ids":[]}),
    );
    let request = Request::parse(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(request.operation.action(), "query_results");
    assert!(!request.operation.demands_style());
    assert!(matches!(
        read_capability(&request.operation),
        Some(NetworkCapability::ProfileResultsV1)
    ));
    assert!(matches!(
        read_projection_operation(&request.operation, "EPSG:3395").unwrap(),
        Some(NativeProfileOperationV1::QueryResults { .. })
    ));
    assert!(
        require_read_capability(
            &[NetworkCapability::ProfileViewV1],
            NetworkCapability::ProfileResultsV1
        )
        .is_err()
    );
    input["operation"]["request"]["analyze"] = json!(true);
    assert!(Request::parse(&serde_json::to_vec(&input).unwrap()).is_err());
    input["operation"]["request"]
        .as_object_mut()
        .unwrap()
        .remove("analyze");
    input["operation"]["request"]["source"] = json!("invented");
    assert!(Request::parse(&serde_json::to_vec(&input).unwrap()).is_err());
    input["operation"]["action"] = json!("analyze_results");
    input["operation"]
        .as_object_mut()
        .unwrap()
        .remove("request");
    input["operation"]["options"] = json!({});
    input["operation"]["patch"] = json!({});
    input["operation"]["style_document"] = json!({});
    let request = Request::parse(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(request.operation.action(), "analyze_results");
    assert!(!request.operation.demands_style());
    assert!(matches!(
        request.operation,
        Operation::AnalyzeResults { .. }
    ));
    assert!(
        read_projection_operation(&request.operation, "EPSG:3395")
            .unwrap()
            .is_none()
    );
    input["operation"]["style_document"] = Value::Null;
    let null_style = Request::parse(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert!(null_style.operation.demands_style());
    input["operation"]["save"] = json!(true);
    assert!(Request::parse(&serde_json::to_vec(&input).unwrap()).is_err());
}
