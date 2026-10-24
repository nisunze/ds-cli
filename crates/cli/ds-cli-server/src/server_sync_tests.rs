/// A drain frees a lease only from a provably dead process of this same
/// install (b06fad17): a live holder, another install's worker, this
/// session itself, a malformed id and an unobservable pid all keep it.
#[test]
fn a_lease_is_reclaimed_only_from_a_dead_process_of_this_install() {
    let dead = |_pid: u32| Some(false);
    let live = |_pid: u32| Some(true);
    let unknown = |_pid: u32| None;
    let own = "install-a#cred#100";
    assert!(super::abandoned_holder(
        "install-a",
        own,
        "install-a#cred#3930728",
        dead
    ));
    assert!(!super::abandoned_holder(
        "install-a",
        own,
        "install-a#cred#3930728",
        live
    ));
    assert!(!super::abandoned_holder(
        "install-a",
        own,
        "install-a#cred#3930728",
        unknown
    ));
    assert!(!super::abandoned_holder(
        "install-a",
        own,
        "install-b#cred#3930728",
        dead
    ));
    assert!(!super::abandoned_holder("install-a", own, own, dead));
    assert!(!super::abandoned_holder(
        "install-a",
        own,
        "install-a#3930728",
        dead
    ));
    assert!(!super::abandoned_holder(
        "install-a",
        own,
        "install-a#cred#pid",
        dead
    ));
    assert_eq!(
        super::holder_pid("install-a", "install-a#cred#42"),
        Some(42)
    );
    assert_eq!(super::holder_pid("install-a", "install-a#cred#42#x"), None);
}
use super::*;
use ds_cli_contract::Failure;
use ds_sync_runtime::store::{self, Event};
use serde_json::{Value, json};

fn startup_identity() -> ds_compute_runtime::HostIdentity {
    ds_compute_runtime::HostIdentity {
        owner: "fixture-owner".into(),
        principal: ds_command_kernel::execution_context::Principal {
            uid: "fixture-account".into(),
            lane: "stable".into(),
            deployment: "fixture-deployment".into(),
            install_id: "fixture-install".into(),
        },
    }
}

fn startup_opening<'a>(
    database: &'a Path,
    identity: &'a ds_compute_runtime::HostIdentity,
) -> SyncOpening<'a> {
    SyncOpening {
        database,
        connection_owner: &identity.owner,
        current_owner: &identity.owner,
        identity,
        project: "project-a",
        sync_pass: true,
    }
}

fn seed_startup_scope(store: &mut ds_sync_store::Store, fence: &Fence, project: &str, now: u64) {
    let scope = ds_sync_runtime::rows::store_scope(project);
    let sha = "a".repeat(64);
    let key = format!("publish-{}-{project}", fence.account);
    let row = serde_json::from_value(json!({
        "scope":scope,"engine":"network_reporter","operation":"export-t1","variant":"default",
        "sha256":store::inventory_digest(&[("xlsx".into(),sha.clone())]),
        "size_bytes":4,"produced_at_ms":now-1000,"input_base_fingerprint":"b".repeat(64),
        "engine_release":format!("ds-network-reporter@0.1.0+{}","c".repeat(40)),
        "engine_build_manifest_sha256":"d".repeat(64),"client_publish_id":key,
        "outputs":[{"output_id":"xlsx","sha256":sha,"size_bytes":4}],
        "bytes_locator":"retained-fixture","state":"held","replay_key":key,"updated_at_ms":now-1000
    }))
    .unwrap();
    for event in [
        Event::LocalProduced { row },
        Event::HeadsRead {
            scope: scope.clone(),
            heads: vec![serde_json::from_value(json!({
                "scope":scope,"engine":"network_reporter","operation":"export-other","variant":"default",
                "revision":"old-head","sha256":"e".repeat(64),"published_at_ms":now-1000,"read_at_ms":now
            })).unwrap()],
        },
        Event::GrantOpened { grant: store::GrantRow {
            scope: scope.clone(), grant_id: "fixture-grant".into(), install_id: fence.install_id.clone(),
            engines: vec![], expires_at_ms: now + 300_000, raw: json!({"fixture":"kept unchanged"}),
        }},
        Event::LeaseTake { scope, worker_id: "fixture-worker".into(), ttl_ms: 300_000 },
    ] {
        assert!(store.apply(fence, now, event).unwrap().refusals.is_empty());
    }
}

#[test]
fn failed_sync_open_retains_its_cause_across_restart_without_touching_work() {
    let dir = ds_cli_contract::testing::private_tempdir("core-test-");
    let database = dir.path().join("sync.sqlite");
    let identity = startup_identity();
    let fence = fence_of(&identity);
    let other = fence_for("other-account", "other-deployment", "other-install");
    let scope = ds_sync_runtime::rows::store_scope("project-a");
    let now = ds_cli_contract::util::now_ms();
    let mut store = ds_sync_store::Store::open(&database).unwrap();
    for (f, project) in [
        (&fence, "project-a"),
        (&fence, "project-b"),
        (&other, "project-a"),
    ] {
        seed_startup_scope(&mut store, f, project, now);
    }
    let before = [&fence, &other].map(|f| {
        ["project-a", "project-b"].map(|project| {
            let scope = ds_sync_runtime::rows::store_scope(project);
            (
                serde_json::to_value(store.snapshot(f, &scope).unwrap()).unwrap(),
                store.heads_read_at_ms(f, &scope).unwrap(),
            )
        })
    });
    drop(store);
    let error = refresh_for_sync_pass::<()>(
        startup_opening(&database, &identity),
        || {
            Err(Failure::unavailable(
                "device_auth_transient",
                "private diagnostic must not be retained",
            )
            .detail(json!({"transport":{"cause":"deadline_exceeded"}})))
        },
        || Ok(()),
    )
    .unwrap_err();
    assert_eq!(error, "private diagnostic must not be retained");
    let mut store = ds_sync_store::Store::open(&database).unwrap();
    for (i, f) in [&fence, &other].iter().enumerate() {
        for (j, project) in ["project-a", "project-b"].iter().enumerate() {
            let scope = ds_sync_runtime::rows::store_scope(project);
            assert_eq!(
                serde_json::to_value(store.snapshot(f, &scope).unwrap()).unwrap(),
                before[i][j].0
            );
            assert_eq!(store.heads_read_at_ms(f, &scope).unwrap(), before[i][j].1);
            let receipts = store.receipts(f, &scope, 50).unwrap();
            assert_eq!(receipts.len(), usize::from(i == 0 && j == 0));
        }
    }
    let receipts = store.receipts(&fence, &scope, 50).unwrap();
    let receipt = &receipts[0];
    assert_eq!(
        (receipt.action.as_str(), receipt.outcome.as_str()),
        ("pass", "failed")
    );
    assert_eq!(receipt.failure_code.as_deref(), Some("deadline_exceeded"));
    assert!(receipt.identity.is_none() && receipt.client_publish_id.is_none());
    assert!(
        !serde_json::to_string(receipt)
            .unwrap()
            .contains("private diagnostic")
    );
    let projection = ds_sync_runtime::host::store_read_projection(
        Some(&store),
        &fence,
        "project-a",
        vec![],
        now + 1,
    )
    .unwrap();
    assert_eq!(
        projection["receipts"][0]["failure_code"],
        "deadline_exceeded"
    );
    let status = store
        .recovery(
            &fence,
            &scope,
            now + 1,
            store::recovery::Command::Status { limit: 50 },
        )
        .unwrap();
    let row: Value = serde_json::to_value(
        status
            .rows
            .iter()
            .find(|r| r.id == "publish-fixture-account-project-a")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(row["state"], "held");
    assert_eq!(row["failure_code"], "deadline_exceeded");
    assert_eq!(row["next_retry"]["when"], "next_pass");
}

#[test]
fn ordinary_open_success_and_invalid_identity_record_no_failed_pass() {
    let dir = ds_cli_contract::testing::private_tempdir("core-test-");
    let identity = startup_identity();
    for (name, connection_owner, current_owner, project, sync_pass, succeeds, called) in [
        (
            "ordinary",
            "fixture-owner",
            "fixture-owner",
            "project-a",
            false,
            false,
            true,
        ),
        (
            "success",
            "fixture-owner",
            "fixture-owner",
            "project-a",
            true,
            true,
            true,
        ),
        (
            "connection",
            "other-owner",
            "fixture-owner",
            "project-a",
            true,
            false,
            false,
        ),
        (
            "credential",
            "fixture-owner",
            "other-owner",
            "project-a",
            true,
            false,
            false,
        ),
        (
            "project",
            "fixture-owner",
            "fixture-owner",
            "../project-a",
            true,
            false,
            false,
        ),
    ] {
        let database = dir.path().join(format!("{name}.sqlite"));
        let observed = std::cell::Cell::new(false);
        let result = refresh_for_sync_pass(
            SyncOpening {
                connection_owner,
                current_owner,
                project,
                sync_pass,
                ..startup_opening(&database, &identity)
            },
            || {
                observed.set(true);
                if succeeds {
                    Ok(())
                } else {
                    Err(Failure::unavailable(
                        "device_auth_transient",
                        "fixture outage",
                    ))
                }
            },
            || Ok(()),
        );
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(observed.get(), called);
        assert!(!database.exists(), "{name} must not initialize a store");
    }
}

#[test]
fn failed_open_without_transport_detail_retains_the_original_typed_code() {
    let dir = ds_cli_contract::testing::private_tempdir("core-test-");
    let database = dir.path().join("sync.sqlite");
    let identity = startup_identity();
    assert!(
        refresh_for_sync_pass::<()>(
            startup_opening(&database, &identity),
            || Err(Failure::conflict(
                "headless_signed_out",
                "fixture signed out"
            )),
            || Ok(())
        )
        .is_err()
    );
    let store = ds_sync_store::Store::open(&database).unwrap();
    let rows = store
        .receipts(
            &fence_of(&identity),
            &ds_sync_runtime::rows::store_scope("project-a"),
            50,
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].failure_code.as_deref(), Some("headless_signed_out"));
}

#[test]
fn identity_rotation_during_refresh_cannot_create_a_startup_receipt() {
    let dir = ds_cli_contract::testing::private_tempdir("core-test-");
    let expected = startup_identity();
    for field in [
        "account",
        "owner",
        "lane",
        "deployment",
        "install",
        "credential",
        "signed-out",
    ] {
        let database = dir.path().join(format!("{field}.sqlite"));
        let attempted = std::cell::Cell::new(false);
        let mut current = expected.clone();
        let mut binding = "original";
        match field {
            "account" => current.principal.uid = "another-account".into(),
            "owner" => current.owner = "another-owner".into(),
            "lane" => current.principal.lane = "canary".into(),
            "deployment" => current.principal.deployment = "another-deployment".into(),
            "install" => current.principal.install_id = "another-install".into(),
            "credential" => binding = "replacement",
            _ => {}
        }
        assert!(
            refresh_for_sync_pass::<()>(
                startup_opening(&database, &expected),
                || {
                    attempted.set(true);
                    Err(Failure::unavailable(
                        "device_auth_transient",
                        "fixture outage",
                    ))
                },
                || {
                    assert!(
                        attempted.get(),
                        "identity is rechecked after the upstream attempt"
                    );
                    if field == "signed-out" {
                        Err("fixture signed out".into())
                    } else {
                        require_opening_identity(&expected, &current, "original", binding)
                    }
                }
            )
            .is_err()
        );
        assert!(
            !database.exists(),
            "{field} rotation must not initialize a store"
        );
    }
}

#[test]
fn sync_accepts_matching_owner_digests_and_compares_user_ids_separately() {
    let owner = digest(b"uid/lane/audience");
    assert_ne!(owner, "uid-a");
    assert!(require_sync_identity(&owner, &owner, &owner, "uid-a", "uid-a").is_ok());
    assert!(require_sync_identity("previous-owner", &owner, &owner, "uid-a", "uid-a").is_err());
    assert!(require_sync_identity(&owner, &owner, "other-principal", "uid-a", "uid-a").is_err());
    assert!(require_sync_identity(&owner, &owner, &owner, "uid-b", "uid-a").is_err());
}

#[test]
fn fence_carries_native_principal_deployment_and_registered_install() {
    let first = fence_for(
        "uid-a",
        "https://gateway.example.com",
        "550e8400-e29b-41d4-a716-446655440000",
    );
    let same = fence_for(
        "uid-a",
        "https://gateway.example.com",
        "550e8400-e29b-41d4-a716-446655440000",
    );
    let different_install = fence_for(
        "uid-a",
        "https://gateway.example.com",
        "550e8400-e29b-41d4-a716-446655440001",
    );
    assert_eq!(first.account, "uid-a");
    assert_eq!(first.deployment, "https://gateway.example.com");
    assert_eq!(first.install_id, same.install_id);
    assert_ne!(first.install_id, different_install.install_id);
}

#[test]
fn a_produced_project_must_match_the_project_its_session_was_opened_for() {
    assert!(require_session_project("project-a", "project-a").is_ok());
    assert!(require_session_project("project-b", "project-a").is_err());
}
