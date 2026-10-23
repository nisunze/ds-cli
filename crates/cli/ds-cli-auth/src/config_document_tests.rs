use super::*;
use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};
use ds_client_core::{ProjectConfigurationChange, TransportResponse};
use ds_command_kernel::{
    durable_store::Request,
    retained_reads::{Kind, Operation},
};
use serde_json::json;
use std::cell::Cell;

fn private_root() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir().expect("private configuration fixture")
}

fn account(uid: &str) -> Identity {
    Identity {
        principal: Principal::Account(uid.into()),
        lane: "canary".into(),
        audience: "fixture-owner-fence".into(),
    }
}
fn config(revision: u64) -> Value {
    json!({"sheets":{"project_settings":[{"parameter":"fixture_revision","value":revision}]},
        "parameters":{},"future_metadata":{"producer":"independently-built"}})
}
fn fresh_fixture(
    body: Value,
) -> (
    FixtureTransport,
    RefCell<ds_client_core::Client<FixtureTransport, crate::test_support::MemoryStore>>,
) {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport
        .lock()
        .configurations
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&body).unwrap(),
        ));
    let client = RefCell::new(signed_in(transport.clone()));
    (transport, client)
}

#[test]
fn refresh_executes_one_closed_get_and_reopened_held_executes_none() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    let (transport, client) = fresh_fixture(config(1));
    let answer = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || {
            client
                .borrow_mut()
                .feeder_configuration(
                    "project-p",
                    Some(&ProjectConfigurationChange::ReadSettings),
                    NOW,
                )
                .map(|answer| answer.document)
                .map_err(crate::map_client)
        },
    )
    .unwrap();
    assert_eq!(
        transport.lock().configuration_requests,
        [("GET".into(), "/config/project-p".into())]
    );
    assert_eq!(serde_json::to_value(&answer.document).unwrap(), config(1));
    assert_eq!(answer.observation.source, ProjectConfigurationRead::Refresh);
    drop(client);
    // New adapter and newly opened native Store simulate an application restart.
    let held = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Held,
        || Ok(account("uid-1")),
        || panic!("held must not call the transport"),
    )
    .unwrap();
    assert_eq!(answer.document, held.document);
    assert_eq!(answer.observation.saved_at_ms, held.observation.saved_at_ms);
    assert_eq!(held.observation.source, ProjectConfigurationRead::Held);
    assert_eq!(transport.lock().configuration_requests.len(), 1);
    assert_eq!(
        read_at(
            &database,
            "project-q",
            ProjectConfigurationRead::Held,
            || Ok(account("uid-1")),
            || panic!("wrong-project held cannot fetch")
        )
        .unwrap_err()
        .code(),
        NOT_HELD.code
    );
    assert_eq!(
        read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Held,
            || Ok(account("other-account")),
            || panic!("another account cannot fetch held")
        )
        .unwrap_err()
        .code(),
        NOT_HELD.code
    );
}

#[test]
fn invalid_or_missing_held_requests_create_nothing_and_send_nothing() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    for (project, mode, expected) in [
        (
            "",
            ProjectConfigurationRead::Refresh,
            "config_input_invalid",
        ),
        ("project-p", ProjectConfigurationRead::Held, NOT_HELD.code),
    ] {
        let error = read_at(
            &database,
            project,
            mode,
            || Ok(account("uid-1")),
            || panic!("invalid or held request cannot fetch"),
        )
        .unwrap_err();
        assert_eq!(error.code(), expected);
        assert!(!database.exists());
    }
}

#[test]
fn malformed_native_responses_do_not_replace_held_configuration() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || Ok(config(1)),
    )
    .unwrap();
    for body in [
        json!([]),
        json!({}),
        json!({"sheets":[]}),
        json!({"sheets":null}),
    ] {
        let (transport, client) = fresh_fixture(body);
        let error = read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Refresh,
            || Ok(account("uid-1")),
            || {
                client
                    .borrow_mut()
                    .feeder_configuration(
                        "project-p",
                        Some(&ProjectConfigurationChange::ReadSettings),
                        NOW,
                    )
                    .map(|answer| answer.document)
                    .map_err(crate::map_client)
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "auth_response_unreadable");
        assert_eq!(transport.lock().configuration_requests.len(), 1);
        let held = read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Held,
            || Ok(account("uid-1")),
            || panic!("no hidden refresh"),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(held.document).unwrap(), config(1));
    }
}

#[test]
fn a_refresh_outage_preserves_held_data_but_a_revocation_forgets_it() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || Ok(config(1)),
    )
    .unwrap();
    let error = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || Err(Failure::unavailable("auth_transient", "fixture offline")),
    )
    .unwrap_err();
    assert_eq!(error.code(), "auth_transient");
    assert!(
        read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Held,
            || Ok(account("uid-1")),
            || panic!("no transport")
        )
        .is_ok()
    );
    let error = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || {
            Err(Failure::unauthorized("auth_rejected", "fixture revoked")
                .detail(json!({"http_status":403})))
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "auth_rejected");
    assert_eq!(
        read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Held,
            || Ok(account("uid-1")),
            || panic!("no transport")
        )
        .unwrap_err()
        .code(),
        NOT_HELD.code
    );
}

#[test]
fn an_account_change_during_fetch_is_refused_before_retention() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    let current = RefCell::new(account("account-a"));
    let calls = Cell::new(0);
    let error = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(current.borrow().clone()),
        || {
            calls.set(calls.get() + 1);
            *current.borrow_mut() = account("account-b");
            Ok(config(1))
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), CONTEXT_CHANGED.code);
    assert_eq!(calls.get(), 1);
    for uid in ["account-a", "account-b"] {
        assert_eq!(
            read_at(
                &database,
                "project-p",
                ProjectConfigurationRead::Held,
                || Ok(account(uid)),
                || panic!("held cannot fetch")
            )
            .unwrap_err()
            .code(),
            NOT_HELD.code
        );
    }
}

#[test]
fn a_save_generation_change_refuses_the_older_response() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    let error = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || {
            Store::open(&database)
                .unwrap()
                .durable_store(
                    &account("uid-1"),
                    Call {
                        request: Request::RetainedReads {
                            operation: Operation::Forget {
                                kind: Kind::ProjectConfig,
                                project: Some("project-p".into()),
                                key: None,
                            },
                        },
                        expect_owner: None,
                    },
                    None,
                )
                .unwrap();
            Ok(config(1))
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), STALE.code);
    assert_eq!(
        read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Held,
            || Ok(account("uid-1")),
            || panic!("held cannot fetch")
        )
        .unwrap_err()
        .code(),
        NOT_HELD.code
    );
}

#[test]
fn the_shared_reply_decoder_rejects_incompatible_fields_and_keeps_additions() {
    let document = json!({"project":"project-p","document":config(1),"observation":{"source":"held","saved_at_ms":1},"future_receipt":"kept-compatible"});
    let decoded: ProjectConfigurationReply = serde_json::from_value(document.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded.document).unwrap(), config(1));
    for malformed in [
        {
            let mut value = document.clone();
            value["document"]["sheets"] = json!("incompatible");
            value
        },
        {
            let mut value = document.clone();
            value.as_object_mut().unwrap().remove("project");
            value
        },
        {
            let mut value = document.clone();
            value["observation"]["saved_at_ms"] = json!("yesterday");
            value
        },
    ] {
        assert!(serde_json::from_value::<ProjectConfigurationReply>(malformed).is_err());
    }
    let mut malformed = document;
    malformed["observation"]["source"] = json!("implicit-network-refresh");
    assert!(serde_json::from_value::<ProjectConfigurationReply>(malformed).is_err());
}

fn assert_configuration(reply: &ProjectConfigurationReply, project: &str, revision: u64) {
    let expected: ProjectConfigurationDocument = serde_json::from_value(config(revision)).unwrap();
    assert_eq!(reply.project, project);
    assert_eq!(reply.document, expected);
}

fn overlapping_refreshes(outer_project: &str, inner_project: &str, inner_uid: &str) {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    let outer_calls = Cell::new(0);
    let inner_calls = Cell::new(0);
    let inner = RefCell::new(None);
    let inner_revision = if inner_project == outer_project && inner_uid == "uid-1" {
        1
    } else {
        2
    };

    // The outer NativeEdge has read its fence and opened a real Store before
    // its fetch completes. A separate NativeEdge commits the overlapping read
    // at precisely that point; no scheduler or timing assumption is involved.
    let outer = read_at(
        &database,
        outer_project,
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || {
            outer_calls.set(outer_calls.get() + 1);
            let answer = read_at(
                &database,
                inner_project,
                ProjectConfigurationRead::Refresh,
                || Ok(account(inner_uid)),
                || {
                    inner_calls.set(inner_calls.get() + 1);
                    Ok(config(inner_revision))
                },
            )
            .expect("the inner refresh succeeds");
            *inner.borrow_mut() = Some(answer);
            Ok(config(1))
        },
    );
    assert_eq!(outer_calls.get(), 1);
    assert_eq!(inner_calls.get(), 1);
    let outer = outer.expect("another refresh must not invalidate this response");
    let inner = inner.into_inner().expect("the inner refresh returned");
    assert_configuration(&outer, outer_project, 1);
    assert_configuration(&inner, inner_project, inner_revision);
    assert_eq!(outer.observation.source, ProjectConfigurationRead::Refresh);
    assert_eq!(inner.observation.source, ProjectConfigurationRead::Refresh);

    // Each call reopens SQLite after both NativeEdges have been dropped.
    for (project, uid, revision) in [
        (outer_project, "uid-1", 1),
        (inner_project, inner_uid, inner_revision),
    ] {
        let held = read_at(
            &database,
            project,
            ProjectConfigurationRead::Held,
            || Ok(account(uid)),
            || panic!("reopened held reads must not fetch"),
        )
        .unwrap();
        assert_configuration(&held, project, revision);
        assert_eq!(held.observation.source, ProjectConfigurationRead::Held);
    }
    assert_eq!((outer_calls.get(), inner_calls.get()), (1, 1));
}

#[test]
fn overlapping_same_project_refreshes_succeed_and_reopen_held_configuration() {
    overlapping_refreshes("project-p", "project-p", "uid-1");
}

#[test]
fn overlapping_different_project_refreshes_keep_each_held_configuration() {
    overlapping_refreshes("project-p", "project-q", "uid-1");
}

#[test]
fn overlapping_different_account_refreshes_keep_each_held_configuration() {
    overlapping_refreshes("project-p", "project-p", "uid-2");
}

fn unrelated_forget_during_refresh(other_project: &str, other_uid: &str) {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    read_at(
        &database,
        other_project,
        ProjectConfigurationRead::Refresh,
        || Ok(account(other_uid)),
        || Ok(config(2)),
    )
    .unwrap();
    let calls = Cell::new(0);
    let refreshed = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || {
            calls.set(calls.get() + 1);
            Store::open(&database)
                .unwrap()
                .durable_store(
                    &account(other_uid),
                    Call {
                        request: Request::RetainedReads {
                            operation: Operation::Forget {
                                kind: Kind::ProjectConfig,
                                project: Some(other_project.into()),
                                key: None,
                            },
                        },
                        expect_owner: None,
                    },
                    None,
                )
                .unwrap();
            Ok(config(1))
        },
    );
    assert_eq!(calls.get(), 1);
    let refreshed = refreshed.expect("an unrelated forget must not invalidate this response");
    assert_configuration(&refreshed, "project-p", 1);
    let held = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Held,
        || Ok(account("uid-1")),
        || panic!("held must not fetch"),
    )
    .unwrap();
    assert_configuration(&held, "project-p", 1);
    assert_eq!(
        read_at(
            &database,
            other_project,
            ProjectConfigurationRead::Held,
            || Ok(account(other_uid)),
            || panic!("the forgotten scope must not fetch"),
        )
        .unwrap_err()
        .code(),
        NOT_HELD.code
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn forgetting_an_unrelated_project_during_fetch_preserves_this_configuration() {
    unrelated_forget_during_refresh("project-q", "uid-1");
}

#[test]
fn forgetting_an_unrelated_account_during_fetch_preserves_this_configuration() {
    unrelated_forget_during_refresh("project-p", "uid-2");
}

#[test]
fn acquire_reads_held_sqlite_offline_and_fetches_only_when_nothing_is_held() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    let fetches = std::cell::Cell::new(0);
    let fetch = |revision: u64| -> Result<Value, Failure> {
        fetches.set(fetches.get() + 1);
        Ok(config(revision))
    };
    let first = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Acquire,
        || Ok(account("uid-1")),
        || fetch(1),
    )
    .unwrap();
    assert_eq!(first.observation.source, ProjectConfigurationRead::Refresh);
    assert_eq!(fetches.get(), 1, "nothing held: one explicit acquisition");
    // Offline: a fetch would fail, and none is attempted while a document is held.
    let held = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Acquire,
        || Ok(account("uid-1")),
        || Err(Failure::failed("auth_transient", "offline")),
    )
    .unwrap();
    assert_eq!(held.observation.source, ProjectConfigurationRead::Held);
    assert_eq!(held.document, first.document);
    assert_eq!(fetches.get(), 1);
    // Another account holds nothing here and acquires its own document once.
    let other = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Acquire,
        || Ok(account("uid-2")),
        || fetch(2),
    )
    .unwrap();
    assert_eq!(other.observation.source, ProjectConfigurationRead::Refresh);
    assert_eq!(fetches.get(), 2);
    // Nothing held and offline: the transport refusal, never a fabricated document.
    let refused = read_at(
        &database,
        "project-q",
        ProjectConfigurationRead::Acquire,
        || Ok(account("uid-1")),
        || Err(Failure::failed("auth_transient", "offline")),
    )
    .unwrap_err();
    assert_eq!(refused.code(), "auth_transient");
}

#[test]
fn a_native_configuration_change_forgets_the_held_document_once() {
    let root = private_root();
    let database = root.path().join("store.sqlite");
    read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || Ok(config(1)),
    )
    .unwrap();
    read_at(
        &database,
        "project-q",
        ProjectConfigurationRead::Refresh,
        || Ok(account("uid-1")),
        || Ok(config(9)),
    )
    .unwrap();
    forget_at(&database, "project-p", &account("uid-1")).unwrap();
    assert_eq!(
        read_at(
            &database,
            "project-p",
            ProjectConfigurationRead::Held,
            || Ok(account("uid-1")),
            || panic!("held cannot fetch")
        )
        .unwrap_err()
        .code(),
        NOT_HELD.code
    );
    // The next held-first read acquires the saved document; another project is kept.
    let fresh = read_at(
        &database,
        "project-p",
        ProjectConfigurationRead::Acquire,
        || Ok(account("uid-1")),
        || Ok(config(2)),
    )
    .unwrap();
    assert_eq!(fresh.observation.source, ProjectConfigurationRead::Refresh);
    assert_eq!(serde_json::to_value(&fresh.document).unwrap(), config(2));
    let kept = read_at(
        &database,
        "project-q",
        ProjectConfigurationRead::Held,
        || Ok(account("uid-1")),
        || panic!("held cannot fetch"),
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&kept.document).unwrap(), config(9));
}
