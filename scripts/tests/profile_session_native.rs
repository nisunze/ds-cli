#[path = "../../crates/ds-cli-server/src/profile_sessions.rs"]
mod profile_sessions;
use base64::{engine::general_purpose::STANDARD, Engine};
use profile_sessions::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
fn main() {
    let scratch = std::env::args().nth(1).expect("private fixture path");
    let fixture_model = ds_grid_exchange::blank_model::create_blank_model(
        &ds_grid_exchange::blank_model::BlankModelRequest {
            model_id: Some("native-profile-fixture"),
            coordinate_system: Some("EPSG:3395"),
            ..Default::default()
        },
    )
    .unwrap();
    let model_bytes = std::env::args()
        .nth(2)
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or(fixture_model.bytes);
    let package = ds_grid_exchange::package::unpack(&model_bytes).unwrap();
    let expected_model = package.manifest.model.model_id.as_str().to_owned();
    let expected_crs = package.manifest.model.coordinate_system.as_str().to_owned();
    let digest = format!("{:x}", Sha256::digest(&model_bytes));
    let scope = Scope {
        principal: "synthetic-fixture-owner".into(),
        lane: "canary".into(),
        deployment: "https://fixture.invalid".into(),
        install: "synthetic-fixture-install".into(),
        project: "synthetic-profile-project".into(),
    };
    let sessions = Sessions::default();
    let database = std::path::Path::new(&scratch).join("store.sqlite");
    let mut checks = Vec::new();
    let open = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Open {
                    package_base64: STANDARD.encode(&model_bytes),
                    package_sha256: digest.clone(),
                },
            },
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(open["model_id"], expected_model);
    assert_eq!(open["package_sha256"], digest);
    assert_eq!(
        open["engine"]["identity"]["sourceSha"]
            .as_str()
            .expect("qualified native source")
            .len(),
        40
    );
    assert_eq!(
        open["engine"]["identity"]["dependencyClosureSha256"]
            .as_str()
            .expect("qualified native dependency closure")
            .len(),
        64
    );
    assert_eq!(open["engine"]["identity"]["protocolMajor"], 1);
    assert_eq!(open["payload"]["schema"], "ds.network.profile-snapshot/v1");
    let presentation: Value =
        serde_json::from_str(open["payload"]["presentation_json"].as_str().unwrap()).unwrap();
    assert!(presentation.is_object());
    checks.push("exact-package-cold-open-presentation");
    let id = open["session_id"].as_str().unwrap().to_owned();
    let model_id = open["model_id"].as_str().unwrap().to_owned();
    let history = open["history"].clone();
    let snapshot = || Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::Snapshot {
            session_id: id.clone(),
            model_id: model_id.clone(),
            expected_history: history.clone(),
        },
    };
    for field in 0..5 {
        let mut crossed = scope.clone();
        match field {
            0 => crossed.principal = "foreign".into(),
            1 => crossed.lane = "stable".into(),
            2 => crossed.deployment = "https://other.invalid".into(),
            3 => crossed.install = "foreign-install".into(),
            _ => crossed.project = "foreign-project".into(),
        };
        let e = sessions
            .execute(
                crossed,
                std::path::Path::new(&scratch),
                &database,
                snapshot(),
                &|| Ok(()),
            )
            .unwrap_err();
        assert_eq!(e.code(), "core_profile_session_unavailable");
    }
    checks.push("all-five-native-scope-crossings-refused");
    let bad = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Snapshot {
                    session_id: id.clone(),
                    model_id: model_id.clone(),
                    expected_history: json!({}),
                },
            },
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(bad.code(), "core_profile_history_conflict");
    checks.push("stale-history-refused");
    let again = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            snapshot(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(again["history"], history);
    assert_eq!(again["payload"], open["payload"]);
    checks.push("retained-snapshot-identical");
    let view_request = |cached| Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::View {
            session_id: id.clone(),
            model_id: model_id.clone(),
            expected_history: history.clone(),
            options: serde_json::to_value(ds_grid_engine::ProfileAtlasOptions::default()).unwrap(),
            patch: json!({}),
            style_document: serde_json::from_str(include_str!(
                "../../../ds-command-kernel/tests/fixtures/profile-display-style.json"
            ))
            .unwrap(),
            cached,
        },
    };
    let view = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            view_request(false),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(view["history"], history);
    assert_eq!(view["payload"]["ran"], false);
    assert_eq!(view["payload"]["analysis"], Value::Null);
    let scene: Value =
        serde_json::from_str(view["payload"]["scene_json"].as_str().unwrap()).unwrap();
    assert_eq!(scene["model_revision"], history["model_revision"]);
    assert!(view["payload"]["display"].is_object());
    checks.push("native-view-render-display-and-no-analysis");
    let cached = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            view_request(true),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(
        cached["payload"]["scene_json"],
        view["payload"]["scene_json"]
    );
    assert_eq!(cached["history"], history);
    checks.push("cached-native-view-identical");
    let gis = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::GisProjection {
                    session_id: id.clone(),
                    model_id: model_id.clone(),
                    expected_history: history.clone(),
                    label_fields: vec!["number".into(), "station".into()],
                },
            },
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(gis["history"], history);
    assert_eq!(gis["payload"]["type"], "FeatureCollection");
    assert_eq!(gis["payload"]["crs"], expected_crs);
    assert_eq!(gis["payload"]["revision"], history["model_revision"]);
    assert_eq!(gis["engine"], open["engine"]);
    checks.push("retained-gis-projection-exact-history-crs-engine");
    let checkpoint = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Checkpoint {
                    session_id: id.clone(),
                    model_id: model_id.clone(),
                    expected_history: history.clone(),
                },
            },
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(
        checkpoint["payload"]["schema"],
        "ds.network.profile-open/v1"
    );
    checks.push("owner-checkpoint");
    let other = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Open {
                    package_base64: STANDARD.encode(&model_bytes),
                    package_sha256: digest.clone(),
                },
            },
            &|| Ok(()),
        )
        .unwrap();
    sessions.expire_for_test(&id);
    let refused = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Snapshot {
                    session_id: id.clone(),
                    model_id: model_id.clone(),
                    expected_history: json!({}),
                },
            },
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(refused.code(), "core_profile_history_conflict");
    assert_eq!(sessions.held_count_for_test(), 1);
    checks.push("expired-wrong-history-refused-without-launch");
    let attempts = std::sync::atomic::AtomicUsize::new(0);
    let refused = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            snapshot(),
            &|| {
                if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 3 {
                    Err(ds_cli_contract::Failure::unauthorized(
                        "server_owner_changed",
                        "synthetic late owner change",
                    ))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(refused.code(), "server_owner_changed");
    assert_eq!(sessions.held_count_for_test(), 1);
    checks.push("late-owner-change-reaps-candidate-without-adoption");
    let rebound = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            snapshot(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(rebound["session_id"], id);
    assert_eq!(rebound["history"], history);
    assert_eq!(rebound["package_sha256"], digest);
    assert_eq!(rebound["coordinate_system"], expected_crs);
    assert_eq!(rebound["rehydrated"], true);
    checks.push("actual-expired-worker-rehydrated-from-exact-sqlite-original");
    let other_request = |close| Request {
        schema: REQUEST_SCHEMA.into(),
        operation: if close {
            Operation::Close {
                session_id: other["session_id"].as_str().unwrap().into(),
                model_id: other["model_id"].as_str().unwrap().into(),
                expected_history: other["history"].clone(),
            }
        } else {
            Operation::Snapshot {
                session_id: other["session_id"].as_str().unwrap().into(),
                model_id: other["model_id"].as_str().unwrap().into(),
                expected_history: other["history"].clone(),
            }
        },
    };
    let unaffected = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            other_request(false),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(unaffected["rehydrated"], false);
    assert_eq!(unaffected["history"], other["history"]);
    checks.push("independent-active-worker-survives-expiry-and-rebind");
    sessions.expire_for_test(other["session_id"].as_str().unwrap());
    let closed_dormant = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            other_request(true),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(closed_dormant["closed"], true);
    assert_eq!(closed_dormant["rehydrated"], false);
    assert_eq!(sessions.held_count_for_test(), 1);
    checks.push("known-expired-close-removes-source-without-opening-worker");
    let closed = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Close {
                    session_id: id.clone(),
                    model_id: model_id.clone(),
                    expected_history: history.clone(),
                },
            },
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(closed["closed"], true);
    assert_eq!(
        sessions
            .execute(
                scope,
                std::path::Path::new(&scratch),
                &database,
                snapshot(),
                &|| Ok(())
            )
            .unwrap_err()
            .code(),
        "core_profile_session_unavailable"
    );
    checks.push("close-and-reopen-required");
    println!(
        "{}",
        json!({"schema":"ds.profile-owner-native-fixture-proof/v1","checks":checks,"package_sha256":digest,"model_id":model_id,"history":history,"engine_sha256":open["engine"]["executable_sha256"],"engine_source":open["engine"]["identity"]["sourceSha"],"engine_dependency_closure_sha256":open["engine"]["identity"]["dependencyClosureSha256"],"engine_protocol":[open["engine"]["identity"]["protocolMajor"],open["engine"]["identity"]["protocolMinor"]],"idle_control":"private clock-age seam exercises production fifteen-minute retirement; not an elapsed fifteen-minute runtime claim","core_transport_qualified":false,"user_project_effects":false})
    );
}
