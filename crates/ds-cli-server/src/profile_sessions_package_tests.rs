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
