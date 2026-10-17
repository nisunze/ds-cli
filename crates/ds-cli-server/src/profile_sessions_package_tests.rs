use super::*;
use std::{cell::Cell, fs};
fn private_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    root
}
fn scope() -> Scope {
    Scope {
        principal: "fixture".into(),
        lane: "canary".into(),
        deployment: "fixture-deployment".into(),
        install: "fixture-install".into(),
        project: "fixture-project".into(),
    }
}
fn open(bytes: &[u8]) -> Request {
    Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::Open {
            recover: false,
            package_base64: STANDARD.encode(bytes),
            package_sha256: sha(bytes),
        },
    }
}
fn control(receipt: &Value, action: &str) -> Request {
    Request::parse(&serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":{
        "action":action,"session_id":receipt["session_id"],"model_id":receipt["model_id"],"expected_history":receipt["history"]
    }})).unwrap()).unwrap()
}
#[test]
fn only_package_owner_invalid_refusals_preserve_public_request_error() {
    for (code, owner, expected) in [
        (
            "network_engine_refused",
            Some("profile_package_invalid"),
            "core_profile_request_invalid",
        ),
        (
            "network_result_digest_mismatch",
            Some("profile_package_invalid"),
            "core_profile_native_refused",
        ),
        (
            "network_engine_refused",
            Some("other"),
            "core_profile_native_refused",
        ),
        (
            "network_engine_incompatible",
            None,
            "core_profile_native_refused",
        ),
    ] {
        let error = NetworkFailure {
            code,
            message: "fixture".into(),
            owner_error: None,
            owner_code: owner.map(str::to_owned),
            owner_detail: None,
            owner_remedy: None,
            owner_invalid_input: None,
        };
        assert_eq!(package_native(error).code(), expected);
    }
}
#[test]
fn package_digest_refuses_before_engine_capture_or_storage() {
    let root = private_root();
    let sessions = Sessions::default();
    let mut request = open(b"synthetic");
    if let Operation::Open { package_sha256, .. } = &mut request.operation {
        *package_sha256 = "0".repeat(64);
    }
    let error = sessions
        .execute(
            scope(),
            root.path(),
            &root.path().join("state.sqlite"),
            request,
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code(), "core_profile_request_invalid");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(sessions.held.lock().unwrap().is_empty());
}
#[test]
#[ignore = "requires an explicitly selected actual engine and owner-emitted synthetic package"]
fn real_profile_package_open_rebind_and_refusals() {
    let bytes = fs::read(
        std::env::var_os("DS_PROFILE_QUALIFICATION_PACKAGE").expect("explicit package fixture"),
    )
    .unwrap();
    assert!(
        std::env::var_os("DS_NETWORK_ENGINE_BIN").is_some(),
        "explicit real engine"
    );
    let root = private_root();
    let scratch = root.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let database = root.path().join("state.sqlite");
    let sessions = Sessions::default();
    if std::env::var("DS_PROFILE_PACKAGE_CASE").as_deref() == Ok("old") {
        for package in [&bytes[..], b"damaged".as_slice()] {
            let error = sessions
                .execute(scope(), &scratch, &database, open(package), &|| Ok(()))
                .unwrap_err();
            assert_eq!(error.code(), "core_profile_native_refused");
            assert!(error.to_string().contains("profile_package_open_v1"));
            assert!(!database.exists());
            assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
        }
        return;
    }
    let opened = sessions
        .execute(scope(), &scratch, &database, open(&bytes), &|| Ok(()))
        .unwrap();
    assert_eq!(opened["model_id"], "original");
    assert_eq!(opened["source_revision"], 37);
    assert_eq!(opened["coordinate_system"], "EPSG:32735");
    assert_eq!(opened["package_sha256"], sha(&bytes));
    assert_eq!(opened["history"]["undo_depth"], 0);
    let asset_count = opened["payload"]["retained_asset_count"].as_u64().unwrap();
    assert!(asset_count > 0);
    let state = sessions
        .execute(
            scope(),
            &scratch,
            &database,
            control(&opened, "state"),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(state["payload"]["calculation_requests"], 0);
    assert_eq!(state["payload"]["analysis_requests"], 0);
    drop(sessions);
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    let sessions = Sessions::default();
    let resumed = sessions
        .execute(
            scope(),
            &scratch,
            &database,
            control(&opened, "snapshot"),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(resumed["rehydrated"], true);
    for field in [
        "model_id",
        "package_sha256",
        "source_revision",
        "coordinate_system",
        "history",
        "payload",
        "engine",
    ] {
        assert_eq!(resumed[field], opened[field], "restart preserves {field}");
    }
    let wrong = Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::Snapshot {
            session_id: opened["session_id"].as_str().unwrap().into(),
            model_id: "other".into(),
            expected_history: opened["history"].clone(),
        },
    };
    assert!(
        sessions
            .execute(scope(), &scratch, &database, wrong, &|| Ok(()))
            .is_err()
    );
    sessions
        .execute(
            scope(),
            &scratch,
            &database,
            control(&opened, "close"),
            &|| Ok(()),
        )
        .unwrap();
    assert!(sessions.held.lock().unwrap().is_empty());
    let retained = Store::open_read_only(&database)
        .unwrap()
        .unwrap()
        .profile_source(
            &source_scope(&scope()),
            opened["session_id"].as_str().unwrap(),
        )
        .unwrap();
    assert!(retained.is_none());
    let invalid = sessions
        .execute(scope(), &scratch, &database, open(b"damaged"), &|| Ok(()))
        .unwrap_err();
    assert_eq!(invalid.code(), "core_profile_request_invalid");
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    // Revocation after preparation or after the retained worker's first snapshot
    // must abort the process before either live adoption or durable retention.
    for stop in [3, 4] {
        let root = private_root();
        let scratch = root.path().join("scratch");
        fs::create_dir(&scratch).unwrap();
        let database = root.path().join("state.sqlite");
        let count = Cell::new(0);
        let error = sessions
            .execute(scope(), &scratch, &database, open(&bytes), &|| {
                count.set(count.get() + 1);
                if count.get() == stop {
                    Err(Failure::failed("authorization_revoked", "fixture"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(error.code(), "authorization_revoked");
        assert_eq!(count.get(), stop);
        assert!(!database.exists());
        assert!(sessions.held.lock().unwrap().is_empty());
        assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    }
}

fn edit_request(receipt: &Value, action: &str, extra: Value) -> Request {
    let mut operation = json!({"action":action,"session_id":receipt["session_id"],
        "model_id":receipt["model_id"],"expected_history":receipt["history"]});
    operation
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    Request::parse(
        &serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":operation})).unwrap(),
    )
    .unwrap()
}

#[test]
#[ignore = "requires an explicit native edit-controls owner and synthetic package"]
fn real_profile_edits_are_durable_exact_recoverable_and_save_excluded() {
    let bytes =
        fs::read(std::env::var_os("DS_PROFILE_QUALIFICATION_PACKAGE").expect("explicit fixture"))
            .unwrap();
    assert!(std::env::var_os("DS_NETWORK_ENGINE_BIN").is_some());
    let root = private_root();
    let scratch = root.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let database = root.path().join("state.sqlite");
    let sessions = Sessions::default();
    let execute = |sessions: &Sessions, request| {
        sessions.execute(scope(), &scratch, &database, request, &|| Ok(()))
    };
    let opened = execute(&sessions, open(&bytes)).unwrap();
    let id = opened["session_id"].as_str().unwrap().to_owned();
    if std::env::var("DS_PROFILE_EDIT_CASE").as_deref() == Ok("old") {
        let error = execute(
            &sessions,
            edit_request(&opened, "edit_mode", json!({"enabled":true})),
        )
        .unwrap_err();
        assert_eq!(
            error.detail_value().unwrap()["required_capability"],
            "profile_edit_controls_v1"
        );
        assert!(
            Store::open(&database)
                .unwrap()
                .profile_recovery(&source_scope(&scope()), &id)
                .unwrap()
                .unwrap()
                .draft
                .is_none()
        );
        execute(&sessions, control(&opened, "close")).unwrap();
        return;
    }
    let admitted = execute(
        &sessions,
        edit_request(&opened, "edit_mode", json!({"enabled":true})),
    )
    .unwrap();
    assert_eq!(admitted["history"], opened["history"]);
    assert!(
        Store::open(&database)
            .unwrap()
            .profile_recovery(&source_scope(&scope()), &id)
            .unwrap()
            .unwrap()
            .draft
            .is_some()
    );
    let snapshot: Value =
        serde_json::from_str(opened["payload"]["snapshot_json"].as_str().unwrap()).unwrap();
    let structure = snapshot["structures"][0]["id"].clone();
    assert!(structure.is_string());
    let mutation = |receipt: &Value, id: &str| {
        edit_request(
            receipt,
            "control",
            json!({"operation":{"action":"apply_transaction","commands":[{"command_id":id,"command":{"command_kind":"describe_structure","id":structure,"description":id}}]}}),
        )
    };
    let edited = execute(&sessions, mutation(&opened, "edited")).unwrap();
    assert_eq!(edited["history"]["undo_depth"], 1);
    assert_eq!(
        edited["payload"]["presentation"]["history"],
        edited["history"]
    );
    assert_eq!(
        edited["payload"]["presentation"]["coordinate_system"],
        "EPSG:32735"
    );
    assert_eq!(edited["payload"]["presentation"]["model_revision"], 37);
    let retained = Store::open(&database)
        .unwrap()
        .profile_recovery(&source_scope(&scope()), &id)
        .unwrap()
        .unwrap();
    assert_eq!(
        retained.draft.as_ref().unwrap().0.history,
        edited["history"]
    );
    drop(sessions);
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    let sessions = Sessions::default();
    let mut recovery = open(&bytes);
    if let Operation::Open { recover, .. } = &mut recovery.operation {
        *recover = true;
    }
    let selected = std::env::var_os("DS_NETWORK_ENGINE_BIN").unwrap();
    // A restarted authored draft must use its durable immutable pin, even when
    // a different selection is now unavailable. No current-pointer fallback.
    unsafe {
        std::env::set_var(
            "DS_NETWORK_ENGINE_BIN",
            scratch.join("current-selection-unavailable"),
        );
    }
    let resumed = execute(&sessions, recovery);
    unsafe {
        std::env::set_var("DS_NETWORK_ENGINE_BIN", selected);
    }
    let resumed = resumed.unwrap();
    assert_eq!(resumed["recovered_draft"], true);
    assert_eq!(resumed["history"], edited["history"]);
    assert_eq!(resumed["session_id"], id);
    let state = execute(&sessions, control(&resumed, "state")).unwrap();
    assert_eq!(state["payload"]["calculation_requests"], 0);
    let begin = execute(
        &sessions,
        edit_request(
            &resumed,
            "save_control",
            json!({"operation":{"action":"begin","save_id":"one"}}),
        ),
    )
    .unwrap();
    assert_eq!(begin["payload"]["saving"], true);
    sessions.expire_for_test(&id);
    let error = execute(&sessions, mutation(&begin, "blocked")).unwrap_err();
    assert!(error.to_string().contains("save_in_progress"));
    assert_eq!(sessions.held_count_for_test(), 1);
    assert!(execute(&sessions, control(&begin, "close")).is_err());
    let emitted = execute(
        &sessions,
        edit_request(&begin, "package_checkpoint", json!({})),
    )
    .unwrap();
    assert_eq!(emitted["history"], edited["history"]);
    assert_eq!(
        emitted["source_revision"], 37,
        "original source identity remains immutable"
    );
    let package = &emitted["payload"];
    assert_eq!(package["model_revision"], 38);
    assert_eq!(package["coordinate_system"], "EPSG:32735");
    let emitted_bytes = decode_package(
        package["package_base64"].as_str().unwrap(),
        package["package_sha256"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        emitted_bytes.len() as u64,
        package["package_bytes"].as_u64().unwrap()
    );
    let repeated = execute(
        &sessions,
        edit_request(&begin, "package_checkpoint", json!({})),
    )
    .unwrap();
    assert_eq!(repeated["payload"], emitted["payload"]);
    let retained = Store::open(&database)
        .unwrap()
        .profile_recovery(&source_scope(&scope()), &id)
        .unwrap()
        .unwrap();
    let checkpoint: Value = serde_json::from_slice(&retained.draft.unwrap().1).unwrap();
    let lineage: Value =
        serde_json::from_str(checkpoint["package_lineage_json"].as_str().unwrap()).unwrap();
    assert_eq!(lineage["model_revision"], 38);

    let finished = execute(
        &sessions,
        edit_request(
            &begin,
            "save_control",
            json!({"operation":{"action":"finish","save_id":"one"}}),
        ),
    )
    .unwrap();
    let undone = execute(
        &sessions,
        edit_request(&finished, "control", json!({"operation":{"action":"undo"}})),
    )
    .unwrap();
    assert_eq!(undone["history"]["redo_depth"], 1);
    execute(&sessions, control(&undone, "close")).unwrap();
    assert_eq!(sessions.held_count_for_test(), 0);
    let resumed = execute(&sessions, control(&undone, "snapshot")).unwrap();
    assert_eq!(resumed["history"], undone["history"]);
    let state = execute(&sessions, control(&resumed, "state")).unwrap();
    assert_eq!(
        state["payload"]["model_revision"], 38,
        "recovery preserves emitted ordinal"
    );
    let redone = execute(
        &sessions,
        edit_request(&resumed, "control", json!({"operation":{"action":"redo"}})),
    )
    .unwrap();
    assert_eq!(redone["history"], edited["history"]);
    let current = &redone["history"];
    let target = &opened["history"];
    let discarded=execute(&sessions,edit_request(&redone,"discard",json!({"request":{
        "expected_revision":current["model_revision"],"expected_undo_depth":current["undo_depth"],"expected_history_pin":current["history_pin"],
        "target_revision":target["model_revision"],"target_undo_depth":target["undo_depth"],"target_redo_depth":target["redo_depth"],"target_history_pin":target["history_pin"]}}))).unwrap();
    assert_eq!(discarded["history"], opened["history"]);
    // Post-owner authorization failure gives no success and no optimistic head.
    // Its last committed checkpoint remains the only recoverable state.
    let count = Cell::new(0);
    let error = sessions
        .execute(
            scope(),
            &scratch,
            &database,
            mutation(&discarded, "revoked"),
            &|| {
                count.set(count.get() + 1);
                if count.get() == 5 {
                    Err(Failure::failed("authorization_revoked", "fixture"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "authorization_revoked");
    assert_eq!(sessions.held_count_for_test(), 0);
    let recovered = execute(&sessions, control(&discarded, "snapshot")).unwrap();
    assert_eq!(recovered["history"], discarded["history"]);
    execute(&sessions, control(&recovered, "close")).unwrap();
    assert!(
        Store::open(&database)
            .unwrap()
            .profile_recovery(&source_scope(&scope()), &id)
            .unwrap()
            .is_none()
    );
}
