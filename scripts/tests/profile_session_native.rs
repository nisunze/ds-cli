#[path = "../../crates/cli/ds-cli-server/src/profile_sessions.rs"]
mod profile_sessions;
use base64::{engine::general_purpose::STANDARD, Engine};
use profile_sessions::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
fn staged_files(root: &std::path::Path) -> Vec<(std::path::PathBuf, u64)> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let meta = entry.path().symlink_metadata().unwrap();
        assert!(!meta.file_type().is_symlink());
        if meta.is_dir() {
            result.extend(staged_files(&entry.path()));
        } else {
            result.push((entry.path(), meta.len()));
        }
    }
    result.sort();
    result
}

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
    let property_entity = package
        .snapshot
        .structures
        .first()
        .map(|row| row.id.as_str().to_owned())
        .or_else(|| {
            package
                .snapshot
                .alignments
                .first()
                .map(|row| row.id.as_str().to_owned())
        });
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
    let stored_original = |id: &str| {
        let key = ds_sync_store::ProfileSourceScope {
            principal: scope.principal.clone(),
            lane: scope.lane.clone(),
            deployment: scope.deployment.clone(),
            install: scope.install.clone(),
            project: scope.project.clone(),
        };
        ds_sync_store::Store::open_read_only(&database)
            .unwrap()
            .unwrap()
            .profile_source(&key, id)
            .unwrap()
            .unwrap()
            .0
    };
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
                "../../crates/kernel/ds-command-kernel/tests/fixtures/profile-display-style.json"
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
    let source_before = stored_original(&id);
    let late_calls = std::cell::Cell::new(0);
    let late = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            view_request(true),
            &|| {
                late_calls.set(late_calls.get() + 1);
                if late_calls.get() == 2 {
                    Err(ds_cli_contract::Failure::unauthorized(
                        "server_owner_changed",
                        "synthetic late response owner change",
                    ))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(late.code(), "server_owner_changed");
    assert_eq!(late_calls.get(), 2);
    assert_eq!(stored_original(&id), source_before);
    assert_eq!(sessions.held_count_for_test(), 1);
    checks.push("late-native-view-owner-refusal-adopts-no-derived-receipt-or-original-change");
    let refused_worker = sessions
        .execute(
            scope.clone(),
            std::path::Path::new(&scratch),
            &database,
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Scene {
                    session_id: id.clone(),
                    model_id: model_id.clone(),
                    expected_history: history.clone(),
                    options: serde_json::to_value(ds_grid_engine::ProfileAtlasOptions::default())
                        .unwrap(),
                    label_policy: json!({}),
                    grid_ink: ds_network_contracts::profile::NativeProfileGridInkV1 {
                        color: "#000000".into(),
                        width_px: -1.,
                        dash_px: vec![],
                    },
                    cached: true,
                },
            },
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(refused_worker.code(), "core_profile_native_refused");
    assert_eq!(stored_original(&id), source_before);
    assert_eq!(sessions.held_count_for_test(), 1);
    checks.push("native-worker-scene-refusal-adopts-no-derived-receipt-or-original-change");
    let projection_capable = open["engine"]["identity"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|cap| cap == "profile_read_projections_v1");
    let projection_requests = || {
        vec![
            Operation::FeatureCodes {
                session_id: id.clone(),
                model_id: model_id.clone(),
                expected_history: history.clone(),
            },
            Operation::EngineeringAttributes {
                session_id: id.clone(),
                model_id: model_id.clone(),
                expected_history: history.clone(),
                table: ds_network_contracts::profile::NativeProfileEngineeringTableV1::Structures,
            },
            Operation::EngineeringAttributes {
                session_id: id.clone(),
                model_id: model_id.clone(),
                expected_history: history.clone(),
                table: ds_network_contracts::profile::NativeProfileEngineeringTableV1::Sections,
            },
            Operation::GisProjection {
                session_id: id.clone(),
                model_id: model_id.clone(),
                expected_history: history.clone(),
                label_fields: vec!["number".into(), "station".into()],
            },
        ]
    };
    if projection_capable {
        for operation in projection_requests() {
            let is_gis = matches!(&operation, Operation::GisProjection { .. });
            let is_feature = matches!(&operation, Operation::FeatureCodes { .. });
            let expected_table = match &operation {
                Operation::EngineeringAttributes {
                    table:
                        ds_network_contracts::profile::NativeProfileEngineeringTableV1::Structures,
                    ..
                } => Some("structures"),
                Operation::EngineeringAttributes {
                    table: ds_network_contracts::profile::NativeProfileEngineeringTableV1::Sections,
                    ..
                } => Some("sections"),
                _ => None,
            };
            let result = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    Request {
                        schema: REQUEST_SCHEMA.into(),
                        operation,
                    },
                    &|| Ok(()),
                )
                .unwrap();
            assert_eq!(result["history"], history);
            assert_eq!(result["engine"], open["engine"]);
            assert!(!result["payload"].is_null());
            if is_feature {
                let rows = result["payload"]
                    .as_array()
                    .expect("FeatureCodes owner returns ProjectionRow array");
                for row in rows {
                    assert_eq!(row["model_revision"], history["model_revision"]);
                }
            }
            if let Some(table) = expected_table {
                assert_eq!(result["payload"]["schema_version"], 2);
                assert_eq!(
                    result["payload"]["model_revision"],
                    history["model_revision"]
                );
                assert_eq!(result["payload"]["table"], table);
                assert!(result["payload"]["columns"].is_array());
                assert!(result["payload"]["rows"].is_array());
            }
            if is_gis {
                assert_eq!(result["payload"]["type"], "FeatureCollection");
                assert_eq!(result["payload"]["crs"], expected_crs);
                assert_eq!(result["payload"]["revision"], history["model_revision"]);
            }
        }
        checks
            .push("same-managed-worker-feature-codes-engineering-tables-and-gis-exact-history-crs");
    } else {
        let before = staged_files(std::path::Path::new(&scratch));
        for operation in projection_requests() {
            let error = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    Request {
                        schema: REQUEST_SCHEMA.into(),
                        operation,
                    },
                    &|| Ok(()),
                )
                .unwrap_err();
            assert_eq!(error.code(), "core_profile_native_refused");
            assert_eq!(
                error.detail_value().unwrap()["native_code"],
                "network_engine_incompatible"
            );
        }
        assert_eq!(before, staged_files(std::path::Path::new(&scratch)));
        assert_eq!(sessions.held_count_for_test(), 1);
        checks
            .push("old-managed-engine-projection-capability-refuses-before-staging-or-new-worker");
    }
    let properties_capable = open["engine"]["identity"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|cap| cap == "profile_properties_v1");
    let property_request = |entity: &str| Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::Properties {
            session_id: id.clone(),
            model_id: model_id.clone(),
            expected_history: history.clone(),
            entity_id: entity.into(),
        },
    };
    if properties_capable {
        let entity = property_entity.as_ref().expect("Properties-capable fixture requires an explicitly supplied package with an observed canonical entity; an empty blank is not positive qualification");
        {
            let source_before = stored_original(&id);
            let result = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    property_request(entity),
                    &|| Ok(()),
                )
                .unwrap();
            assert_eq!(result["history"], history);
            assert_eq!(result["engine"], open["engine"]);
            assert_eq!(result["payload"]["schema_version"], 1);
            assert_eq!(result["payload"]["entity_id"], *entity);
            assert_eq!(result["payload"]["revision"], history["model_revision"]);
            assert!(!result["payload"]["groups"].as_array().unwrap().is_empty());
            assert_eq!(stored_original(&id), source_before);
            let attempts = std::sync::atomic::AtomicUsize::new(0);
            let error = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    property_request(entity),
                    &|| {
                        if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                            Err(ds_cli_contract::Failure::unauthorized(
                                "server_owner_changed",
                                "synthetic late Properties owner change",
                            ))
                        } else {
                            Ok(())
                        }
                    },
                )
                .unwrap_err();
            assert_eq!(error.code(), "server_owner_changed");
            assert_eq!(stored_original(&id), source_before);
            checks.push("same-worker-properties-exact-entity-history-and-late-owner-refusal");
        }
        let missing = sessions
            .execute(
                scope.clone(),
                std::path::Path::new(&scratch),
                &database,
                property_request("missing-native-property-entity"),
                &|| Ok(()),
            )
            .unwrap_err();
        assert_eq!(
            missing.detail_value().unwrap()["native_code"],
            "profile_entity_not_found"
        );
        checks.push("properties-missing-entity-refuses-without-empty-sheet");
    } else {
        let files_before = staged_files(std::path::Path::new(&scratch));
        let source_before = stored_original(&id);
        let error = sessions
            .execute(
                scope.clone(),
                std::path::Path::new(&scratch),
                &database,
                property_request("missing-native-property-entity"),
                &|| Ok(()),
            )
            .unwrap_err();
        assert_eq!(
            error.detail_value().unwrap()["required_capability"],
            "profile_properties_v1"
        );
        assert_eq!(files_before, staged_files(std::path::Path::new(&scratch)));
        assert_eq!(stored_original(&id), source_before);
        assert_eq!(sessions.held_count_for_test(), 1);
        checks.push("old-engine-properties-cap-refuses-active-before-staging-or-request");
    }
    let selection_capable = open["engine"]["identity"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "profile_selection_v1");
    // A capability-negative fixture needs valid closed arguments even when its
    // blank original has no rendered structure. Positive selection always uses
    // the real scene below and must produce nonempty owner results.
    let selection_axis = if selection_capable {
        scene["axis_pin"]["digest"]
            .as_str()
            .expect("real selection axis")
    } else {
        "capability-negative-axis"
    };
    let selection_entity = if selection_capable {
        scene["bands"][0]["structures"][0]["entity_id"]
            .as_str()
            .expect("nonempty selection source")
    } else {
        "capability-negative-entity"
    };
    let selection_query = |action: &str| {
        let request = if action == "pick" {
            let (x, y) = if selection_capable {
                (
                    scene["bands"][0]["structures"][0]["ground"]["scene_x"]
                        .as_f64()
                        .expect("real selection x"),
                    scene["bands"][0]["structures"][0]["ground"]["scene_y"]
                        .as_f64()
                        .expect("real selection y"),
                )
            } else {
                (0., 0.)
            };
            json!({"model_revision":history["model_revision"],"axis_pin_digest":selection_axis,"scene_x":x,"scene_y":y,"tolerance_scene_units":3.,"scene_units_per_px":1.,"visibility":{"showGround":true,"showTerrainPoints":true,"showStructures":true,"showWire":true,"showAttachments":true}})
        } else {
            json!({"model_revision":history["model_revision"],"axis_pin_digest":selection_axis,"from_entity_id":selection_entity,"to_entity_id":selection_entity})
        };
        Request::parse(&serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":{"action":action,"session_id":id,"model_id":model_id,"expected_history":history,"request":request}})).unwrap()).unwrap()
    };
    if selection_capable {
        for action in ["pick", "selection_range"] {
            let before = stored_original(&id);
            let result = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    selection_query(action),
                    &|| Ok(()),
                )
                .unwrap();
            assert_eq!(result["history"], history);
            assert_eq!(
                result["payload"]["model_revision"],
                history["model_revision"]
            );
            assert_eq!(
                result["payload"]["axis_pin_digest"],
                scene["axis_pin"]["digest"]
            );
            assert!(!result["payload"][if action == "pick" {
                "candidates"
            } else {
                "entity_ids"
            }]
            .as_array()
            .unwrap()
            .is_empty());
            assert_eq!(stored_original(&id), before);
            let calls = std::cell::Cell::new(0);
            let error = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    selection_query(action),
                    &|| {
                        calls.set(calls.get() + 1);
                        if calls.get() >= 2 {
                            Err(ds_cli_contract::Failure::unavailable(
                                "late_selection_owner_changed",
                                "fixture only",
                            ))
                        } else {
                            Ok(())
                        }
                    },
                )
                .unwrap_err();
            assert_eq!(error.code(), "late_selection_owner_changed");
            assert_eq!(stored_original(&id), before);
        }
        checks.push("same-worker-selection-nonempty-exact-axis-history-late-owner-no-adoption");
    } else {
        for action in ["pick", "selection_range"] {
            let before = stored_original(&id);
            let files_before = staged_files(std::path::Path::new(&scratch));
            let error = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    selection_query(action),
                    &|| Ok(()),
                )
                .unwrap_err();
            assert_eq!(
                error.detail_value().unwrap()["required_capability"],
                "profile_selection_v1"
            );
            assert_eq!(stored_original(&id), before);
            assert_eq!(staged_files(std::path::Path::new(&scratch)), files_before);
        }
        checks.push("old-engine-selection-cap-refuses-active-before-staging-or-request");
    }
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
    if !projection_capable {
        let before = stored_original(&id);
        for operation in projection_requests() {
            let refused = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    Request {
                        schema: REQUEST_SCHEMA.into(),
                        operation,
                    },
                    &|| Ok(()),
                )
                .unwrap_err();
            assert_eq!(refused.code(), "core_profile_native_refused");
            assert_eq!(
                refused.detail_value().unwrap()["required_capability"],
                "profile_read_projections_v1"
            );
            assert_eq!(sessions.held_count_for_test(), 1); // Only independent other worker.
        }
        assert_eq!(stored_original(&id), before);
        checks.push("expired-old-cap-source-refuses-before-candidate-launch-or-source-adoption");
    }
    if !properties_capable {
        let files_before = staged_files(std::path::Path::new(&scratch));
        let source_before = stored_original(&id);
        let error = sessions
            .execute(
                scope.clone(),
                std::path::Path::new(&scratch),
                &database,
                property_request("missing-native-property-entity"),
                &|| Ok(()),
            )
            .unwrap_err();
        assert_eq!(
            error.detail_value().unwrap()["required_capability"],
            "profile_properties_v1"
        );
        assert_eq!(files_before, staged_files(std::path::Path::new(&scratch)));
        assert_eq!(stored_original(&id), source_before);
        assert_eq!(sessions.held_count_for_test(), 1);
        checks.push("expired-old-properties-cap-refuses-before-candidate-launch-or-adoption");
    }
    if !selection_capable {
        for action in ["pick", "selection_range"] {
            let before = stored_original(&id);
            let files_before = staged_files(std::path::Path::new(&scratch));
            let error = sessions
                .execute(
                    scope.clone(),
                    std::path::Path::new(&scratch),
                    &database,
                    selection_query(action),
                    &|| Ok(()),
                )
                .unwrap_err();
            assert_eq!(
                error.detail_value().unwrap()["required_capability"],
                "profile_selection_v1"
            );
            assert_eq!(stored_original(&id), before);
            assert_eq!(staged_files(std::path::Path::new(&scratch)), files_before);
            assert_eq!(sessions.held_count_for_test(), 1);
        }
        checks.push("expired-old-selection-cap-refuses-before-candidate-launch-or-adoption");
    }
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
    let properties_positive_qualified =
        checks.contains(&"same-worker-properties-exact-entity-history-and-late-owner-refusal");
    println!(
        "{}",
        json!({"schema":"ds.profile-owner-native-fixture-proof/v1","checks":checks,"package_sha256":digest,"model_id":model_id,"history":history,"projection_capability_present":projection_capable,"properties_capability_present":properties_capable,"properties_positive_qualified":properties_positive_qualified,"selection_capability_present":selection_capable,"selection_positive_qualified":selection_capable,"engine_executable":open["engine"]["executable"],"engine_sha256":open["engine"]["executable_sha256"],"engine_source":open["engine"]["identity"]["sourceSha"],"engine_dependency_closure_sha256":open["engine"]["identity"]["dependencyClosureSha256"],"engine_protocol":[open["engine"]["identity"]["protocolMajor"],open["engine"]["identity"]["protocolMinor"]],"idle_control":"private clock-age seam exercises production fifteen-minute retirement; not an elapsed fifteen-minute runtime claim","core_transport_qualified":false,"user_project_effects":false})
    );
}
