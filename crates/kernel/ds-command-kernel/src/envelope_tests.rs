use super::*;
use serde_json::json;

#[test]
fn every_class_has_one_token_and_the_token_names_it_back() {
    let mut tokens = std::collections::BTreeSet::new();
    for class in Class::ALL {
        let token = class.token();
        assert!(tokens.insert(token), "{token} names two classes");
        assert_eq!(Class::from_token(token), Some(class));
        assert_eq!(serde_json::to_value(class).expect("encodes"), json!(token));
    }
    assert_eq!(
        tokens.into_iter().collect::<Vec<_>>(),
        [
            "conflict",
            "failed",
            "internal",
            "invalid_input",
            "unauthorized",
            "unavailable",
        ]
    );
    assert_eq!(Class::from_token("ok"), None);
    assert_eq!(Class::from_token("InvalidInput"), None);
}

#[test]
fn only_unavailable_and_conflict_are_retryable() {
    let retryable: Vec<_> = Class::ALL
        .into_iter()
        .filter(|class| class.retryable())
        .collect();
    assert_eq!(retryable, [Class::Unavailable, Class::Conflict]);
}

#[test]
fn a_success_envelope_carries_exactly_five_members_in_order() {
    let envelope = success("report.project.export", 1, json!({"results": []}));
    assert_eq!(
        serde_json::to_string(&envelope).expect("encodes"),
        r#"{"v":1,"command":"report.project.export","contract":1,"status":"ok","data":{"results":[]}}"#
    );
}

#[test]
fn an_error_envelope_omits_what_the_refusal_does_not_carry() {
    let bare = Refusal::new(Class::Internal, "kernel_defect", "the kernel panicked");
    assert_eq!(
        serde_json::to_string(&error("ds", 0, &bare)).expect("encodes"),
        r#"{"v":1,"command":"ds","contract":0,"status":"error","error":{"class":"internal","code":"kernel_defect","message":"the kernel panicked","retryable":false}}"#
    );
    let full = Refusal::new(
        Class::Unavailable,
        "reporter_engine_missing",
        "the ds-report engine is not installed",
    )
    .remedy("install the native profile")
    .next("ds doctor")
    .next("ds report project export --transformer T --preview-layout draft.json")
    .detail(json!({"engine": "ds-report"}));
    assert_eq!(
        serde_json::to_string(&error("report.project.export", 1, &full)).expect("encodes"),
        r#"{"v":1,"command":"report.project.export","contract":1,"status":"error","error":{"class":"unavailable","code":"reporter_engine_missing","message":"the ds-report engine is not installed","retryable":true,"remedy":"install the native profile","next":["ds doctor","ds report project export --transformer T --preview-layout draft.json"],"detail":{"engine":"ds-report"}}}"#
    );
}

#[test]
fn a_refusal_reads_back_from_its_own_json_and_refuses_a_stranger_member() {
    let refusal: Refusal = serde_json::from_value(json!({
        "class": "conflict",
        "code": "desktop_project_not_open",
        "message": "the desktop has another project open",
        "next": ["ds project select P"]
    }))
    .expect("decodes");
    assert_eq!(refusal.class, Class::Conflict);
    assert_eq!(refusal.remedy, None);
    assert_eq!(refusal.next, ["ds project select P"]);
    assert_eq!(refusal.detail, None);
    assert!(serde_json::from_value::<Refusal>(json!({
        "class": "failed",
        "code": "x",
        "message": "y",
        "retryable": true
    }))
    .is_err());
}
