//! Real SQLite acquisition/readback through the same Kernel flow used by
//! native status commands. Scripted responses are fixed; no argument repair,
//! route substitution, retry or remote access occurs in this harness.
use ds_project_data::metadata::{self, transformer_status, Kind, Scope};
use serde_json::{json, Value};
use std::cell::Cell;

fn scope(project: &str) -> Scope {
    Scope {
        principal: "status-owner".into(),
        lane: "canary".into(),
        audience: "status-audience".into(),
        project: project.into(),
    }
}
fn document(name: &str) -> Value {
    json!({"transformers":[{"name":name,"future_optional_metadata":true}]})
}

#[test]
fn first_acquisition_is_once_then_sqlite_readback_needs_no_transport() {
    let root = tempfile::tempdir().unwrap();
    let project = scope("first-project");
    assert!(transformer_status::held(root.path(), &project)
        .unwrap()
        .is_none());
    let attempts = Cell::new(0);
    let expected = document("TX-1");
    let (first, cached) = metadata::acquire(
        root.path(),
        &project,
        Kind::TransformerStatus,
        false,
        || {
            attempts.set(attempts.get() + 1);
            Ok::<_, &'static str>(expected.clone())
        },
    )
    .unwrap();
    assert_eq!(first, expected);
    assert!(!cached);
    // Each API call closes its SQLite connection. This is a new readback;
    // no in-memory observation or held network client is reused.
    let reopened = transformer_status::held(root.path(), &project)
        .unwrap()
        .unwrap();
    assert_eq!(reopened.document, expected);
    let (held, cached) = metadata::acquire(
        root.path(),
        &project,
        Kind::TransformerStatus,
        false,
        || {
            attempts.set(attempts.get() + 1);
            Err::<Value, _>("offline")
        },
    )
    .unwrap();
    assert!(cached);
    assert_eq!(held, expected);
    assert_eq!(attempts.get(), 1);

    let refreshed_document = document("TX-refreshed");
    let (refreshed, cached) =
        metadata::acquire(root.path(), &project, Kind::TransformerStatus, true, || {
            attempts.set(attempts.get() + 1);
            Ok::<_, &'static str>(refreshed_document.clone())
        })
        .unwrap();
    assert!(!cached, "explicit refresh must acquire despite held data");
    assert_eq!(refreshed, refreshed_document);
    assert_eq!(attempts.get(), 2, "one acquisition per explicit refresh");
    let after_refresh = transformer_status::held(root.path(), &project)
        .unwrap()
        .unwrap();
    assert_eq!(after_refresh.document, refreshed_document);
    assert_ne!(after_refresh.sha256, reopened.sha256);
    assert_eq!(after_refresh.revision, reopened.revision + 1);
    assert!(!after_refresh.needs_refresh);
    assert_eq!(after_refresh.last_error, None);
    let observation =
        ds_command_kernel::transformer_status_store::status_observation(&after_refresh, true, None)
            .unwrap();
    assert_eq!(observation.sha256, after_refresh.sha256);
    assert_eq!(observation.observed_at_ms, after_refresh.observed_at_ms);
    assert_eq!(
        observation.source,
        ds_command_kernel::transformer_status_store::StatusObservationSource::Refresh
    );
    let (again, cached) = metadata::acquire(
        root.path(),
        &project,
        Kind::TransformerStatus,
        false,
        || -> Result<Value, &'static str> { panic!("reopened held data must not acquire") },
    )
    .unwrap();
    assert!(cached);
    assert_eq!(again, refreshed_document);
    for crossed in [
        scope("other-project"),
        Scope {
            principal: "other-user".into(),
            ..project.clone()
        },
        Scope {
            lane: "stable".into(),
            ..project.clone()
        },
        Scope {
            audience: "other-credential-audience".into(),
            ..project.clone()
        },
    ] {
        assert!(transformer_status::held(root.path(), &crossed)
            .unwrap()
            .is_none());
        assert_eq!(
            transformer_status::held(root.path(), &project)
                .unwrap()
                .unwrap(),
            after_refresh,
            "a crossed-scope read cannot change the retained observation"
        );
    }
    assert_eq!(attempts.get(), 2);
}

#[test]
fn failed_or_incompatible_acquisition_preserves_the_previous_status() {
    let root = tempfile::tempdir().unwrap();
    let project = scope("project-failure");
    let expected = document("TX-held");
    metadata::observe(root.path(), &project, Kind::TransformerStatus, &expected).unwrap();
    let before = transformer_status::held(root.path(), &project)
        .unwrap()
        .unwrap();
    let attempts = Cell::new(0);
    let unavailable =
        metadata::acquire(root.path(), &project, Kind::TransformerStatus, true, || {
            attempts.set(attempts.get() + 1);
            Err::<Value, _>("transport-unavailable")
        });
    assert!(matches!(
        unavailable,
        Err(metadata::Error::Read("transport-unavailable"))
    ));
    let after = transformer_status::held(root.path(), &project)
        .unwrap()
        .unwrap();
    assert_eq!(after.document, before.document);
    assert_eq!(after.sha256, before.sha256);
    assert_eq!(after.observed_at_ms, before.observed_at_ms);
    assert!(after.last_error.is_some());
    let incompatible =
        metadata::acquire(root.path(), &project, Kind::TransformerStatus, true, || {
            attempts.set(attempts.get() + 1);
            Ok::<_, &'static str>(json!({"transformers":"wrong required field type"}))
        });
    assert!(matches!(incompatible, Err(metadata::Error::Store(_))));
    let retained = transformer_status::held(root.path(), &project)
        .unwrap()
        .unwrap();
    assert_eq!(retained.document, expected);
    assert_eq!(retained.sha256, before.sha256);
    assert_eq!(retained.observed_at_ms, before.observed_at_ms);
    assert_eq!(
        attempts.get(),
        2,
        "one recorded attempt per explicit acquisition"
    );
}
