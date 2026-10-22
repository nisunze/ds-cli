use super::*;
use serde_json::json;

#[test]
fn real_serializers_roundtrip_success_and_complete_refusal_without_rewriting() {
    let success = success_envelope("design.lv.process", 4, json!({"jobs": 2}));
    let mut value = serde_json::to_value(success).unwrap();
    value["files"] = json!({"out":{"jobs":[]}});
    value["v"] = json!(9);
    value["contract"] = json!(27);
    value["additive"] = json!({"future":true});
    let decoded: CommandEnvelope<Value> = serde_json::from_value(value).unwrap();
    match decoded {
        CommandEnvelope::Ok {
            v,
            contract,
            data,
            files,
            ..
        } => {
            assert_eq!((v, contract), (9, 27));
            assert_eq!(data["jobs"], 2);
            assert_eq!(files["out"]["jobs"], json!([]));
        }
        _ => panic!("success changed status"),
    }
    let failure = Failure::conflict("source_changed", "Use the captured source.")
        .remedy("Read the intended source.")
        .next("ds design inspect --project A")
        .next("ds design lv process --input request.json --out another.json")
        .detail(json!({"project":"A","expected":7}))
        .not_retryable();
    let value = serde_json::to_value(error_envelope("design.lv.process", 4, &failure)).unwrap();
    let CommandEnvelope::<Value>::Error { error, .. } = serde_json::from_value(value).unwrap()
    else {
        panic!()
    };
    error.validate().unwrap();
    assert_eq!(error, CommandRefusal::from_failure(&failure));
    let roundtrip = error.into_failure().unwrap();
    assert_eq!(roundtrip.class(), failure.class());
    assert!(!roundtrip.retryable());
    assert_eq!(roundtrip.next_commands(), failure.next_commands());
    assert_eq!(roundtrip.detail_value(), failure.detail_value());
}

#[test]
fn explicit_null_detail_and_compatible_additions_are_preserved() {
    let failure = Failure::invalid("input_invalid", "Use a valid input.").detail(Value::Null);
    let mut value = serde_json::to_value(error_envelope("design.lv.process", 4, &failure)).unwrap();
    value["error"]["future"] = json!({"advice":1});
    let CommandEnvelope::<Value>::Error { error, .. } = serde_json::from_value(value).unwrap()
    else {
        panic!()
    };
    assert_eq!(error.detail, Some(Value::Null));
    assert_eq!(serde_json::to_value(error).unwrap()["detail"], Value::Null);
}

#[test]
fn incompatible_envelopes_and_malformed_owner_refusals_fail_clearly() {
    let base = serde_json::to_value(success_envelope("design.lv.process", 4, json!({}))).unwrap();
    for (field, bad) in [
        ("status", json!("queued")),
        ("v", json!("1")),
        ("contract", json!(null)),
        ("files", json!([])),
    ] {
        let mut value = base.clone();
        value[field] = bad;
        assert!(
            serde_json::from_value::<CommandEnvelope<Value>>(value).is_err(),
            "{field}"
        );
    }
    let mut value = base;
    value.as_object_mut().unwrap().remove("data");
    assert!(serde_json::from_value::<CommandEnvelope<Value>>(value).is_err());
    let failure = Failure::failed("job_failed", "The job failed.");
    let base = serde_json::to_value(error_envelope("design.lv.process", 4, &failure)).unwrap();
    for (field, bad) in [
        ("class", json!("future_class")),
        ("code", json!("")),
        ("message", json!("")),
        ("retryable", json!(null)),
        ("next", json!([1])),
    ] {
        let mut value = base.clone();
        value["error"][field] = bad;
        match serde_json::from_value::<CommandEnvelope<Value>>(value) {
            Err(_) => {}
            Ok(CommandEnvelope::Error { error, .. }) => {
                assert!(error.validate().is_err(), "{field}")
            }
            _ => panic!("refusal became success"),
        }
    }
}
