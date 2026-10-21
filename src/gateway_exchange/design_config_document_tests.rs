use super::*;

#[test]
fn project_config_exchange_uses_the_shared_complete_document_contract() {
    let value = json!({"sheets":{"project_settings":[]},"parameters":{},"future_metadata":{"producer":"other-build"}});
    let call = DesignCall::ProjectConfig {
        project: "project-p".into(),
    };
    let plan = call.plan().unwrap();
    assert_eq!(plan.request.method, Method::Get);
    assert_eq!(plan.request.path, "/config/project-p");
    let Admitted::Done(answer) = call.admit(200, value.clone()).unwrap() else {
        panic!("one complete response expected")
    };
    assert_eq!(answer, value);
    let typed: ProjectConfigurationDocument = serde_json::from_value(answer).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), value);
}

#[test]
fn missing_and_incompatible_config_sheets_are_refused_by_both_decoders() {
    for value in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"sheets":null}),
        json!({"sheets":[]}),
        json!({"sheets":"old-shape"}),
    ] {
        assert!(serde_json::from_value::<ProjectConfigurationDocument>(value.clone()).is_err());
        let call = DesignCall::ProjectConfig {
            project: "project-p".into(),
        };
        assert_eq!(call.admit(200, value).unwrap_err().code, ANSWER_INVALID);
    }
}

#[test]
fn configuration_read_modes_and_receipts_match_the_typed_wire() {
    assert_eq!(ProjectConfigurationRead::NAMES, &["held", "refresh"]);
    for name in ProjectConfigurationRead::NAMES {
        let mode: ProjectConfigurationRead = serde_json::from_value(json!(name)).unwrap();
        assert_eq!(serde_json::to_value(mode).unwrap(), json!(name));
    }
    let reply = json!({"project":"project-p","document":{"sheets":{},"unknown_addition":true},
        "observation":{"source":"held","saved_at_ms":123},"future_receipt":1});
    let typed: ProjectConfigurationReply = serde_json::from_value(reply).unwrap();
    assert_eq!(typed.project, "project-p");
    assert_eq!(typed.document.additional["unknown_addition"], true);
    assert!(serde_json::from_value::<ProjectConfigurationReply>(
        json!({"project":"project-p", "document":{"sheets":{}}})
    )
    .is_err());
}
