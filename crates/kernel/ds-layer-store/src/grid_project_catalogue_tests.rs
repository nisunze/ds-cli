use super::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
/// Privacy-checked stores refuse world-writable ancestors such as /tmp.
fn private_root() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .prefix("grid-catalogue-")
        .tempdir_in(dirs::home_dir().unwrap())
        .unwrap()
}
fn scope(project: &str) -> Scope {
    Scope {
        principal: "owner".into(),
        project: project.into(),
        lane: "local".into(),
        audience_sha256: "a".repeat(64),
    }
}
fn answer(project: &str) -> Value {
    json!({"project":project,"models":[],"more":false})
}
fn head(model: &str, digest: char) -> Value {
    json!({"model_id":model,"display_name":model,"model_kind":"mv_line","lifecycle":"promoted","state":"active",
            "head_revision_id":"rev","head_version":1,"head_model_digest":digest.to_string().repeat(64),"updated_at":"2026-10-18T07:07:51Z"})
}
fn demand() -> Demand {
    Demand {
        online: true,
        acquire: true,
        refresh: false,
    }
}
fn offline() -> Demand {
    Demand {
        online: false,
        acquire: false,
        refresh: false,
    }
}
fn refresh() -> Demand {
    Demand {
        online: true,
        acquire: true,
        refresh: true,
    }
}
#[test]
fn real_adapter_coalesces_warm_offline_and_unused_invalidation_without_remote_reads() {
    let root = private_root();
    let requests = Arc::new(AtomicUsize::new(0));
    let room = scope("template");
    std::thread::scope(|threads| {
        for _ in 0..6 {
            let requests = requests.clone();
            let room = room.clone();
            let path = root.path();
            threads.spawn(move || {
                read(
                    path,
                    &room,
                    demand(),
                    || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        Ok::<_, &str>(answer("template"))
                    },
                    |_| false,
                )
                .unwrap()
            });
        }
    });
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    invalidate(root.path(), &room).unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    for online in [false, false] {
        read(
            root.path(),
            &room,
            Demand {
                online,
                acquire: false,
                refresh: false,
            },
            || panic!("offline performs no transport"),
            |_: &&str| false,
        )
        .unwrap();
    }
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    read(
        root.path(),
        &room,
        demand(),
        || {
            requests.fetch_add(1, Ordering::SeqCst);
            Ok::<_, &str>(answer("template"))
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert!(matches!(
        read(
            root.path(),
            &scope("foreign"),
            offline(),
            || panic!("missing offline project does not acquire"),
            |_: &&str| false
        ),
        Err(Failure::AcquisitionRequired)
    ));
}
#[test]
fn incomplete_or_foreign_selected_sources_refuse_before_cache_or_transport_access() {
    let root = private_root();
    let scope = scope("template");
    let source = mv_models::SourceBinding {
        project_id: "foreign".into(),
        model_id: "head".into(),
        revision_id: "rev".into(),
        content_sha256: "a".repeat(64),
        version: 1,
        worker_id: "local".into(),
        worker_model_id: "native".into(),
        worker_revision: "rev:1".into(),
        history_pin: "pin".into(),
        coordinate_system: "EPSG:32736".into(),
        read_only: false,
    };
    let calls = AtomicUsize::new(0);
    let context = json!({"account":"owner","project":"template","generation":1});
    let inventory = ds_command_kernel::grid_held_models::admit(
        json!({"captured_context":context,"context":context,"total":0,"rows":[],"mutated":false}),
    )
    .unwrap();
    let mut unbound = source.clone();
    unbound.project_id = "template".into();
    for (inventory, source) in [(Some(inventory), source), (None, unbound)] {
        assert!(matches!(
            consume(
                root.path(),
                &scope,
                demand(),
                inventory,
                Some(source),
                || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, &str>(answer("template"))
                },
                |_| false
            ),
            Err(Failure::Admission(mv_models::STALE))
        ));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!root.path().join("grid-project-catalogue").exists());
}
#[cfg(unix)]
#[test]
fn foreign_scope_links_are_not_followed_or_written() {
    let root = private_root();
    let foreign = tempfile::tempdir().unwrap();
    let scope = scope("template");
    let owned = directory(root.path(), &scope).unwrap();
    fs::create_dir_all(owned.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(foreign.path(), &owned).unwrap();
    assert!(matches!(
        read(
            root.path(),
            &scope,
            demand(),
            || panic!("foreign path does not acquire"),
            |_: &&str| false
        ),
        Err(Failure::Storage(_))
    ));
    assert_eq!(fs::read_dir(foreign.path()).unwrap().count(), 0);
}
#[test]
fn revoked_or_crossed_scope_never_reuses_a_previous_success() {
    let root = private_root();
    let room = scope("template");
    read(
        root.path(),
        &room,
        demand(),
        || Ok::<_, &str>(answer("template")),
        |_| false,
    )
    .unwrap();
    assert!(matches!(
        read(
            root.path(),
            &room,
            refresh(),
            || Err("unauthorized"),
            |e| *e == "unauthorized"
        ),
        Err(Failure::Remote(_))
    ));
    assert!(matches!(
        read(
            root.path(),
            &room,
            offline(),
            || panic!("revoked does not acquire"),
            |_: &&str| false
        ),
        Err(Failure::AccessRevoked)
    ));
    for changed in 0..3 {
        let mut other = room.clone();
        match changed {
            0 => other.principal = "peer".into(),
            1 => other.lane = "other".into(),
            _ => other.audience_sha256 = "b".repeat(64),
        };
        assert!(matches!(
            read(
                root.path(),
                &other,
                offline(),
                || panic!("scope does not acquire"),
                |_: &&str| false
            ),
            Err(Failure::AcquisitionRequired)
        ));
    }
}
#[test]
fn a_refresh_persists_first_arrivals_that_offline_reads_paint_at_once() {
    let root = private_root();
    let room = scope("template");
    let unread = catalog(
        root.path(),
        &room,
        offline(),
        || panic!("held reads never acquire"),
        |_: &&str| false,
    )
    .unwrap();
    assert_eq!(unread.catalog["observation"]["state"], "unread");
    assert!(
        !root.path().join("grid-project-catalogue").exists(),
        "a held read creates nothing"
    );
    let three = json!({"project":"template","models":[head("m-a",'a'),head("m-b",'b'),head("m-c",'c')],"more":false});
    let first = catalog(
        root.path(),
        &room,
        refresh(),
        || Ok::<_, &str>(three),
        |_| false,
    )
    .unwrap();
    assert_eq!(first.catalog["observation"]["source"], "refresh");
    assert_eq!(
        first.catalog["changes"]["added"].as_array().unwrap().len(),
        3
    );
    let six = json!({"project":"template","models":[head("m-a",'a'),head("m-b",'b'),head("m-c",'c'),head("m-d",'d'),head("m-e",'e'),head("m-f",'f')],"more":false});
    let second = catalog(
        root.path(),
        &room,
        refresh(),
        || Ok::<_, &str>(six.clone()),
        |_| false,
    )
    .unwrap();
    assert_eq!(
        second.catalog["changes"]["added"],
        json!(["m-d", "m-e", "m-f"])
    );
    let held = catalog(
        root.path(),
        &room,
        offline(),
        || panic!("offline performs no transport"),
        |_: &&str| false,
    )
    .unwrap();
    assert_eq!(held.catalog["observation"]["state"], "ready");
    assert_eq!(held.catalog["mv_models"].as_array().unwrap().len(), 6);
    // Equal source basis: no row moves, the newcomers keep their first-seen time.
    let again = catalog(
        root.path(),
        &room,
        refresh(),
        || Ok::<_, &str>(six),
        |_| false,
    )
    .unwrap();
    assert_eq!(again.catalog["changes"]["fresh"], true);
    assert_eq!(
        again.catalog["mv_models"][5]["first_seen_at_ms"],
        second.catalog["mv_models"][5]["first_seen_at_ms"]
    );
    // A failed refresh keeps and labels the six rows.
    let failed = catalog(root.path(), &room, refresh(), || Err("offline"), |_| false).unwrap();
    assert!(failed.refresh_failure.is_some());
    assert_eq!(failed.catalog["observation"]["state"], "held");
    assert_eq!(failed.catalog["mv_models"].as_array().unwrap().len(), 6);
    // The exact verified version of one held head is retained without IO.
    let version = json!({"model_id":"m-a","revision_id":"rev","version":1,"model":{"digest":"a".repeat(64),"byte_length":7}});
    let retained = retain_version(root.path(), &room, "m-a", version).unwrap();
    assert_eq!(retained["mv_models"][0]["retained_current"], true);
    assert!(matches!(
        retain_version(
            root.path(),
            &room,
            "m-b",
            json!({"model_id":"m-b","revision_id":"old","version":1,"model":{"digest":"b".repeat(64),"byte_length":7}})
        ),
        Err(Failure::Admission(kernel::VERSION_STALE))
    ));
    // Printing's consuming read sees the same six heads.
    let picker = consume(
        root.path(),
        &room,
        offline(),
        None,
        None,
        || panic!("offline"),
        |_: &&str| false,
    )
    .unwrap();
    assert_eq!(picker["models"].as_array().unwrap().len(), 6);
}
