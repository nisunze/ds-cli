//! The shared retention, proven once for every host that binds it.
use super::*;
use crate::PublicationProvider;
use ds_sync_runtime::Producer;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn identity() -> ds_compute_runtime::HostIdentity {
    ds_compute_runtime::HostIdentity {
        owner: "owner-a".into(),
        principal: ds_command_kernel::execution_context::Principal {
            uid: "account-a".into(),
            lane: "canary".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-a".into(),
        },
    }
}

fn retention(database: PathBuf, authorized: Arc<AtomicBool>) -> CommonRetention {
    CommonRetention::new(
        database,
        identity(),
        "project-a",
        Arc::new(move || {
            if authorized.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("authority changed".into())
            }
        }),
    )
}

fn publication(project: &str, build: &str) -> Publication {
    let bytes = b"# sealed Solar draft".to_vec();
    let output = ds_sync_runtime::LocalOutput {
        filename: None,
        paper_size: None,
        presentation: None,
        output_id: "network-draft-en".into(),
        format: "md".into(),
        content_type: "text/markdown".into(),
        sha256: ds_compute_runtime::digest(&bytes),
        size_bytes: bytes.len() as u64,
    };
    close(
        Source {
            project,
            run: "report-run-a",
            operation: "report-city-a",
            variant: "network-draft-en",
            resource: "city-a",
            release: "ds-solar-engine@1.2.3",
            manifest: build,
            input_base: &"b".repeat(64),
        },
        vec![(output, bytes)],
    )
    .unwrap()
}

fn rows_of(database: &std::path::Path) -> Vec<ds_command_kernel::sync_store::ArtifactRow> {
    let store = ds_sync_runtime::open_store(database).unwrap();
    let fence = ds_sync_runtime::solar_producer::fence_of(&identity());
    store
        .lock()
        .unwrap()
        .snapshot(&fence, &rows::store_scope("project-a"))
        .unwrap()
        .artifacts
}

#[test]
fn completion_is_retained_offline_and_replays_after_restart() {
    let dir = crate::test_directory::tempdir().unwrap();
    let database = dir.path().join("store.sqlite");
    let authorized = Arc::new(AtomicBool::new(true));
    let host = retention(database.clone(), authorized.clone());
    assert!(
        host.observe(&publication("project-a", &"a".repeat(64)))
            .unwrap_err()
            .contains("publication_not_sealed")
    );
    let receipt = host
        .retain(publication("project-a", &"a".repeat(64)))
        .unwrap();
    assert_eq!(receipt["state"], "held");
    drop(host);
    let host = retention(database.clone(), authorized);
    assert_eq!(
        receipt,
        host.retain(publication("project-a", &"a".repeat(64)))
            .unwrap()
    );
    assert_eq!(
        receipt,
        host.observe(&publication("project-a", &"a".repeat(64)))
            .unwrap()
    );
    let rows = rows_of(&database);
    assert_eq!(rows.len(), 1);
    let row = rows::local_row_from_store(&rows[0]);
    let store = ds_sync_runtime::open_store(&database).unwrap();
    let fence = ds_sync_runtime::solar_producer::fence_of(&identity());
    assert_eq!(
        ds_sync_runtime::retained_publication::RetainedProducer {
            store: &store,
            fence: &fence,
            project: "project-a",
        }
        .output(&row, "network-draft-en")
        .unwrap(),
        b"# sealed Solar draft"
    );
    let mut declaration = rows::open_declaration("project-a", &row);
    let identity = identity();
    ds_sync_runtime::solar_producer::SolarProducer {
        database: &database,
        identity: &identity,
        project: "project-a".into(),
        failure: None,
    }
    .amend_open_declaration("project-a", &mut declaration)
    .unwrap();
}

#[test]
fn refused_authority_project_or_corrupted_bytes_create_no_publication() {
    let dir = crate::test_directory::tempdir().unwrap();
    let database = dir.path().join("store.sqlite");
    let authorized = Arc::new(AtomicBool::new(false));
    let host = retention(database.clone(), authorized.clone());
    assert!(
        host.retain(publication("project-a", &"a".repeat(64)))
            .is_err()
    );
    assert!(!database.exists(), "refused authority opens no store");
    authorized.store(true, Ordering::SeqCst);
    assert!(
        host.retain(publication("project-b", &"a".repeat(64)))
            .unwrap_err()
            .contains("crosses its captured project")
    );
    assert!(!database.exists(), "another project opens no store");
    let mut invalid = publication("project-a", &"a".repeat(64));
    invalid.outputs[0].bytes[0] = b'!';
    assert!(host.retain(invalid).is_err());
    assert!(rows_of(&database).is_empty());
}

#[test]
fn identical_bytes_from_a_new_build_keep_their_new_provenance() {
    let dir = crate::test_directory::tempdir().unwrap();
    let database = dir.path().join("store.sqlite");
    let host = retention(database.clone(), Arc::new(AtomicBool::new(true)));
    let old = host
        .retain(publication("project-a", &"a".repeat(64)))
        .unwrap();
    let current = host
        .retain(publication("project-a", &"c".repeat(64)))
        .unwrap();
    assert_ne!(old["client_publish_id"], current["client_publish_id"]);
    let rows = rows_of(&database);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].engine_build_manifest_sha256.as_deref(),
        Some("c".repeat(64).as_str())
    );
    let store = ds_sync_runtime::open_store(&database).unwrap();
    let fence = ds_sync_runtime::solar_producer::fence_of(&identity());
    assert!(
        store
            .lock()
            .unwrap()
            .publication_payload(
                &fence,
                "project-a",
                old["client_publish_id"].as_str().unwrap()
            )
            .unwrap()
            .is_none(),
        "the displaced row's bytes are reclaimed"
    );
}

#[test]
fn a_retention_or_observation_wakes_the_host_pass() {
    let dir = crate::test_directory::tempdir().unwrap();
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = woken.clone();
    let host = retention(
        dir.path().join("store.sqlite"),
        Arc::new(AtomicBool::new(true)),
    )
    .with_completion(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    host.retain(publication("project-a", &"a".repeat(64)))
        .unwrap();
    host.observe(&publication("project-a", &"a".repeat(64)))
        .unwrap();
    assert_eq!(woken.load(Ordering::SeqCst), 2);
}
