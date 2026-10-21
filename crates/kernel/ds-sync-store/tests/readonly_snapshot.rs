//! Multi-owner retained reads observe one native SQLite snapshot.
use ds_command_kernel::sync_store::{Event, Fence, Scope};
use ds_sync_store::Store;
#[test]
fn readonly_snapshot_does_not_interleave_another_connection_commit() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = dir.path().join("store.sqlite");
    let mut writer = Store::open(&path).unwrap();
    let reader = Store::open_read_only(&path).unwrap().unwrap();
    let fence = Fence {
        account: "user".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install".into(),
    };
    let scope = Scope::Project {
        project: "project".into(),
    };
    reader
        .with_read_snapshot(|reader| {
            assert!(reader.snapshot(&fence, &scope).unwrap().lease.is_none());
            writer
                .apply(
                    &fence,
                    10,
                    Event::LeaseTake {
                        scope: scope.clone(),
                        worker_id: "worker".into(),
                        ttl_ms: 1000,
                    },
                )
                .unwrap();
            assert!(reader.snapshot(&fence, &scope).unwrap().lease.is_none());
        })
        .unwrap();
    assert!(reader.snapshot(&fence, &scope).unwrap().lease.is_some());
    assert!(!reader.has_jobs().unwrap());
}

#[test]
fn project_inventory_includes_retained_reads_and_grants_and_fences_every_axis() {
    use ds_command_kernel::sync_store::GrantRow;
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = dir.path().join("store.sqlite");
    let mut store = Store::open(&path).unwrap();
    let fence = Fence {
        account: "user".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install".into(),
    };
    store
        .apply(
            &fence,
            10,
            Event::HeadsRead {
                scope: Scope::Project {
                    project: "z-read-only".into(),
                },
                heads: vec![],
            },
        )
        .unwrap();
    let grant = GrantRow {
        scope: Scope::Project {
            project: "a-grant-only".into(),
        },
        grant_id: "grant".into(),
        install_id: "install".into(),
        engines: vec![],
        expires_at_ms: 100,
        raw: serde_json::json!({}),
    };
    store
        .apply(&fence, 10, Event::GrantOpened { grant })
        .unwrap();
    for axis in ["account", "deployment", "install"] {
        let mut foreign = fence.clone();
        match axis {
            "account" => foreign.account.push('x'),
            "deployment" => foreign.deployment.push('x'),
            _ => foreign.install_id.push('x'),
        };
        store
            .apply(
                &foreign,
                10,
                Event::HeadsRead {
                    scope: Scope::Project {
                        project: format!("foreign-{axis}"),
                    },
                    heads: vec![],
                },
            )
            .unwrap();
    }
    store
        .apply(
            &fence,
            10,
            Event::HeadsRead {
                scope: Scope::Personal,
                heads: vec![],
            },
        )
        .unwrap();
    drop(store);
    let store = Store::open_read_only(&path).unwrap().unwrap();
    assert_eq!(
        store.projects_of_fence(&fence).unwrap(),
        vec!["a-grant-only", "z-read-only"]
    );
}
