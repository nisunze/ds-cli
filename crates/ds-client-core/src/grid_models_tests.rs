use super::*;
#[test]
fn every_action_this_owner_sends_is_a_declared_gateway_action() {
    let operation = crate::gateway::operation("domains.grid.models.action").unwrap();
    for action in ACTIONS {
        assert!(operation.allows_action(action), "{action}");
    }
}
#[test]
fn refusals_keep_the_route_sentence_and_name_a_server_that_predates_an_action() {
    let body = |message: &str| {
        serde_json::to_vec(
            &json!({"success":false,"error":{"code":"VALIDATION_FAILED","message":message}}),
        )
        .unwrap()
    };
    let unknown = TransportResponse::new(400, body("Unknown DS Grid model action"));
    let refused = refusal(&unknown, "get_export_artifact", false);
    assert_eq!(
        refused.service_refusal().unwrap().code(),
        Some(SERVER_ACTION_UNSUPPORTED)
    );
    // The same sentence for an action every Server serves is plain validation.
    let refused = refusal(&unknown, "list_exports", false);
    assert_eq!(
        refused.service_refusal().unwrap().code(),
        Some("grid_request_refused")
    );
    // An explicit bump sent to a Server whose decoder predates the field.
    let undecodable = TransportResponse::new(400, body("Invalid DS Grid model request"));
    let refused = refusal(&undecodable, "create_version", true);
    assert_eq!(
        refused.service_refusal().unwrap().code(),
        Some(SERVER_ACTION_UNSUPPORTED)
    );
    let advance = TransportResponse::new(
        400,
        body("invalid project grid model: manifest_model_revision must advance beyond 4"),
    );
    let refused = refusal(&advance, "create_version", false);
    assert!(refused
        .service_refusal()
        .unwrap()
        .message()
        .unwrap()
        .contains("must advance"));
    let missing = refusal(&TransportResponse::new(404, b"{}"), "get_model", false);
    assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
    assert_eq!(
        missing.service_refusal().unwrap().code(),
        Some("grid_model_not_found")
    );
    let reused = refusal(&TransportResponse::new(409, b"{}"), "publish_export", false);
    assert_eq!(
        reused.service_refusal().unwrap().code(),
        Some("grid_export_conflict")
    );
}
#[test]
fn protected_combined_refusal_preserves_every_owner_and_code() {
    let response = TransportResponse::new(409, serde_json::to_vec(&json!({
            "error": {"code": "composite_model_protected", "message": "edit north, south, additional"}
        })).unwrap());
    for action in [
        "create_version",
        "update_model",
        "set_approval",
        "bump_version",
        "delete_model",
        "restore_model",
    ] {
        let error = refusal(&response, action, false);
        let refusal = error.service_refusal().unwrap();
        assert_eq!(refusal.code(), Some("composite_model_protected"));
        assert_eq!(refusal.message(), Some("edit north, south, additional"));
    }
    // An unrecognized server code never acquires this policy by its message.
    let ordinary = TransportResponse::new(
        409,
        serde_json::to_vec(&json!({"error":{"code":"CONFLICT","message":"edit north, south"}}))
            .unwrap(),
    );
    assert_eq!(
        refusal(&ordinary, "create_version", false)
            .service_refusal()
            .unwrap()
            .code(),
        Some("grid_publication_conflict")
    );
}

#[test]
fn export_ids_follow_the_catalog_grammar() {
    let publish = |export: &str| Command::PublishExport {
        model: "model-1".into(),
        revision: "rev-1".into(),
        export: export.into(),
        output: "pls-delivered-workspace".into(),
        format: "pls_cadd_bak".into(),
        bytes: b"bak".to_vec(),
    };
    assert!(publish("submission-2").validate("project").is_ok());
    assert!(publish("Submission 2").validate("project").is_err());
    assert!(Command::DownloadExport {
        model: "model-1".into(),
        revision: "rev-1".into(),
        export: "submission-2".into(),
        output: "../x".into(),
    }
    .validate("project")
    .is_err());
}
#[test]
fn storage_locator_cannot_redirect_credentials_or_substitute_objects() {
    let object = "ds-grid/objects/sha256/ab/model.dsgrid";
    assert!(locator(
        &format!("https://storage.googleapis.com/bucket/{object}?X-Goog-Signature=issued"),
        object,
        12
    )
    .is_ok());
    for url in [
        "http://storage.googleapis.com/bucket/x",
        "https://evil.example/x",
        "https://storage.googleapis.com/bucket/wrong",
        "https://user@storage.googleapis.com/bucket/x",
    ] {
        assert!(locator(url, object, 12).is_err());
    }
    assert!(Command::List {
        limit: 101,
        cursor: None,
        include_deleted: true,
    }
    .validate("project")
    .is_err());
    assert!(Command::Download {
        model: "../x".into(),
        revision: "r".into()
    }
    .validate("project")
    .is_err());
    assert!(Command::Delete {
        model: "model-1".into(),
        expected_revision: "revision-1".into(),
        expected_digest: "a".repeat(64),
        reason: "Superseded by the reviewed model".into(),
    }
    .validate("project")
    .is_ok());
    assert!(Command::Restore {
        model: "model-1".into(),
        expected_revision: "revision-1".into(),
        expected_digest: "a".repeat(64),
        reason: "Needed again".into(),
    }
    .validate("project")
    .is_ok());
    assert!(Command::ListVersions {
        model: "model-1".into(),
        limit: 50,
        cursor: None,
    }
    .validate("project")
    .is_ok());
    assert!(Command::Delete {
        model: "model-1".into(),
        expected_revision: "revision-1".into(),
        expected_digest: "a".repeat(64),
        reason: " ".into(),
    }
    .validate("project")
    .is_err());
}
