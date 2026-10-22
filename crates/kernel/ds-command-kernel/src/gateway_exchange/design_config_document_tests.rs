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

fn complete_reply() -> ProjectConfigurationReply {
    serde_json::from_value(
        json!({"project":"project-p", "document":{"sheets":{},"future_document":true},
        "observation":{"source":"refresh","saved_at_ms":123}}),
    )
    .unwrap()
}

fn file_envelope() -> Value {
    let answer = complete_reply();
    let receipt = answer
        .file_receipt(
            "project-p",
            ProjectConfigurationRead::Refresh,
            "/private/config.json".into(),
        )
        .unwrap();
    // Exercise the producer's actual typed constructor and JSON serialization.
    let receipt = serde_json::from_slice::<Value>(&serde_json::to_vec(&receipt).unwrap()).unwrap();
    let document =
        serde_json::from_slice::<Value>(&serde_json::to_vec(&answer.document).unwrap()).unwrap();
    json!({"v":1,"command":"design.config.read","contract":1,"status":"ok","data":receipt,"files":{"out":document}})
}

#[test]
fn typed_file_constructor_json_and_owner_decoder_round_trip_across_additive_producers() {
    let mut envelope = file_envelope();
    envelope["v"] = json!(2);
    envelope["contract"] = json!(27);
    envelope["future_envelope"] = json!({"producer":"independent"});
    envelope["data"]["future_receipt"] = json!(true);
    envelope["data"]["observation"]["future_observation"] = json!(true);
    assert_eq!(
        admit_project_configuration_file_reply(
            "project-p",
            ProjectConfigurationRead::Refresh,
            envelope
        )
        .unwrap(),
        Some(complete_reply())
    );
}

#[test]
fn file_and_inline_admission_refuse_crossed_context_and_unsafe_observations() {
    for (field, value) in [
        ("project", json!("project-q")),
        ("source", json!("held")),
        ("saved_at_ms", json!(-1)),
        ("saved_at_ms", json!("123")),
        ("saved_at_ms", json!(1.5)),
        ("saved_at_ms", json!(9_007_199_254_740_992u64)),
    ] {
        let mut envelope = file_envelope();
        if field == "project" {
            envelope["data"][field] = value;
        } else {
            envelope["data"]["observation"][field] = value;
        }
        assert!(
            matches!(admit_project_configuration_file_reply("project-p", ProjectConfigurationRead::Refresh, envelope),
            Err(ProjectConfigurationFileFailure::Receipt(Refusal {code, ..})) if code == PROJECT_CONFIG_RECEIPT_INVALID)
        );
    }
    let mut inline = serde_json::to_value(complete_reply()).unwrap();
    inline["observation"]["saved_at_ms"] = json!(9_007_199_254_740_992u64);
    assert_eq!(
        admit_project_configuration_reply("project-p", ProjectConfigurationRead::Refresh, inline)
            .unwrap_err()
            .code,
        PROJECT_CONFIG_RECEIPT_INVALID
    );
    let mut unsafe_reply = complete_reply();
    unsafe_reply.observation.saved_at_ms = 9_007_199_254_740_992;
    assert_eq!(
        unsafe_reply
            .file_receipt("project-p", ProjectConfigurationRead::Refresh, "out".into())
            .unwrap_err()
            .code,
        PROJECT_CONFIG_RECEIPT_INVALID
    );
}

#[test]
fn malformed_file_envelopes_and_documents_keep_their_own_refusals() {
    for (field, value) in [
        ("v", json!("2")),
        ("contract", json!("2")),
        ("command", json!("other.command")),
        ("status", json!("pending")),
        ("data", json!({})),
        ("files", json!({})),
    ] {
        let mut envelope = file_envelope();
        envelope[field] = value;
        assert!(
            matches!(admit_project_configuration_file_reply("project-p", ProjectConfigurationRead::Refresh, envelope),
            Err(ProjectConfigurationFileFailure::Receipt(Refusal {code, ..})) if code == PROJECT_CONFIG_RECEIPT_INVALID)
        );
    }
    for document in [Value::Null, json!([]), json!({}), json!({"sheets":[]})] {
        let mut envelope = file_envelope();
        envelope["files"]["out"] = document;
        assert!(
            matches!(admit_project_configuration_file_reply("project-p", ProjectConfigurationRead::Refresh, envelope),
            Err(ProjectConfigurationFileFailure::Receipt(Refusal {code, ..})) if code == ANSWER_INVALID)
        );
    }
    assert!(admit_project_configuration_file_reply(
        "",
        ProjectConfigurationRead::Refresh,
        file_envelope()
    )
    .is_err());
    assert!(serde_json::from_value::<ProjectConfigurationRead>(json!("implicit")).is_err());
}

#[test]
fn real_command_refusals_survive_and_only_explicit_held_absence_is_null() {
    let error = json!({"class":"unavailable","code":"core_incompatible","message":"Missing document capability.",
        "retryable":false,"remedy":"Update Core independently.","future_error":{"producer":2}});
    let mut envelope =
        json!({"v":2,"command":"design.config.read","contract":99,"status":"error","error":error});
    assert_eq!(
        admit_project_configuration_file_reply(
            "project-p",
            ProjectConfigurationRead::Refresh,
            envelope.clone()
        ),
        Err(ProjectConfigurationFileFailure::Command(error))
    );
    envelope["error"]["code"] = json!("project_config_not_held");
    assert_eq!(
        admit_project_configuration_file_reply(
            "project-p",
            ProjectConfigurationRead::Held,
            envelope.clone()
        )
        .unwrap(),
        None
    );
    assert!(matches!(
        admit_project_configuration_file_reply(
            "project-p",
            ProjectConfigurationRead::Refresh,
            envelope.clone()
        ),
        Err(ProjectConfigurationFileFailure::Command(_))
    ));
    envelope["error"]["code"] = json!(7);
    assert!(matches!(
        admit_project_configuration_file_reply(
            "project-p",
            ProjectConfigurationRead::Held,
            envelope
        ),
        Err(ProjectConfigurationFileFailure::Receipt(_))
    ));
}
