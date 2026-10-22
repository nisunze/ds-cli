//! Retained recovery kept by a Network without authored recovery: the verified
//! move to the current Network, and the explicit discard remedy when that move
//! cannot be exact.
use super::*;
use std::{cell::Cell, fs, path::PathBuf};

const ORIGINAL: &[u8] = b"exact synthetic original";
const CHECKPOINT: &[u8] = br#"{"opaque":"v1 checkpoint"}"#;

fn scope() -> Scope {
    Scope {
        principal: "fixture".into(),
        lane: "canary".into(),
        deployment: "fixture-deployment".into(),
        install: "fixture-install".into(),
        project: "fixture-project".into(),
    }
}
fn history(undo: u64) -> Value {
    json!({"model_revision":format!("revision-{undo}"),"initial_revision":"revision-0",
        "undo_depth":undo,"redo_depth":0,"history_pin":format!("pin-{undo}")})
}
fn source(engine: Value) -> ProfileSourceHeader {
    ProfileSourceHeader {
        scope: source_scope(&scope()),
        schema: "ds.core.profile-source/v1".into(),
        session_id: "0123456789abcdef0123456789abcdef".into(),
        model_id: "model".into(),
        package_sha256: sha(ORIGINAL),
        package_bytes: ORIGINAL.len() as u64,
        initial_history: history(0),
        source_revision: 7,
        coordinate_system: "EPSG:32735".into(),
        engine,
    }
}
/// One retained original with an opaque v1 draft at `history(2)`.
fn stale(database: &Path) -> ProfileSourceHeader {
    let source = source(json!({"fixture":"older Network"}));
    let mut store = Store::open(database).unwrap();
    store
        .retain_profile_source(&source.scope, &source, ORIGINAL, &[], 1)
        .unwrap();
    let header = ds_sync_store::ProfileDraftHeader {
        schema: "ds.core.profile-draft/v1".into(),
        source: source.clone(),
        history: history(2),
        checkpoint_sha256: sha(CHECKPOINT),
        checkpoint_bytes: CHECKPOINT.len() as u64,
    };
    store
        .retain_profile_draft(
            &source.scope,
            &source,
            &source.initial_history,
            &header,
            CHECKPOINT,
            2,
        )
        .unwrap();
    source
}
fn discard(source: &ProfileSourceHeader, model: &str, history: Value) -> Request {
    Request::parse(
        &serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":{
            "action":"discard_recovery","session_id":source.session_id,"model_id":model,
            "expected_history":history}}))
        .unwrap(),
    )
    .unwrap()
}
fn retained(
    database: &Path,
    source: &ProfileSourceHeader,
) -> Option<ds_sync_store::ProfileRecovery> {
    Store::open_read_only(database)
        .unwrap()
        .unwrap()
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
}

#[test]
fn discard_recovery_is_closed_and_never_rehydrates() {
    let base = json!({"action":"discard_recovery","session_id":"captured","model_id":"model",
        "expected_history":history(1)});
    let parse = |operation: &Value| {
        Request::parse(
            &serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":operation})).unwrap(),
        )
    };
    let request = parse(&base).unwrap();
    assert!(operation_owner(&request.operation).is_none());
    assert!(read_capability(&request.operation).is_none());
    let mut extra = base.clone();
    extra["request"] = json!({});
    assert!(parse(&extra).is_err());
    let mut missing = base;
    missing.as_object_mut().unwrap().remove("expected_history");
    assert!(parse(&missing).is_err());
}

#[test]
fn upgrade_refusal_names_one_exact_discard_request() {
    let source = source(json!({"fixture":"older Network"}));
    let error = controls::upgrade::upgrade_required(&source, &history(3), "fixture");
    assert_eq!(error.code(), "core_profile_draft_stale");
    assert!(error.remedy_text().unwrap().contains("discard_recovery"));
    let detail = error.detail_value().unwrap();
    assert_eq!(detail["expected_history"], history(3));
    let request = Request::parse(
        &serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":detail["discard"]}))
            .unwrap(),
    )
    .unwrap();
    match request.operation {
        Operation::DiscardRecovery {
            session_id,
            model_id,
            expected_history,
        } => {
            assert_eq!(session_id, source.session_id);
            assert_eq!(model_id, source.model_id);
            assert_eq!(expected_history, history(3));
        }
        other => panic!("expected the exact discard request, found {other:?}"),
    }
}

#[test]
fn the_named_discard_survives_the_core_wire_to_its_caller() {
    let source = source(json!({"fixture":"older Network"}));
    let error = controls::upgrade::upgrade_required(&source, &history(3), "fixture");
    let (status, axum::Json(body)) = crate::host::typed(&error);
    let relayed = crate::typed_refusal(status.as_u16(), &serde_json::to_vec(&body).unwrap());
    assert_eq!(relayed.code(), "core_profile_draft_stale");
    assert_eq!(relayed.class(), error.class());
    assert_eq!(relayed.remedy_text(), error.remedy_text());
    assert_eq!(relayed.detail_value(), error.detail_value());
    let active = Failure::conflict("core_profile_session_active", "fixture");
    let (status, axum::Json(body)) = crate::host::typed(&active);
    let relayed = crate::typed_refusal(status.as_u16(), &serde_json::to_vec(&body).unwrap());
    assert_eq!(relayed.code(), "core_profile_session_active");
}

#[test]
fn same_original_ignores_only_the_engine_pin() {
    let retained = source(json!({"fixture":"older Network"}));
    let mut candidate = retained.clone();
    candidate.engine = json!({"fixture":"current Network"});
    assert!(controls::upgrade::same_original(&candidate, &retained));
    for change in [
        |s: &mut ProfileSourceHeader| s.initial_history = history(1),
        |s: &mut ProfileSourceHeader| s.model_id = "another".into(),
        |s: &mut ProfileSourceHeader| s.source_revision += 1,
        |s: &mut ProfileSourceHeader| s.coordinate_system = "EPSG:4326".into(),
        |s: &mut ProfileSourceHeader| s.session_id = "f".repeat(32),
    ] {
        let mut changed = candidate.clone();
        change(&mut changed);
        assert!(!controls::upgrade::same_original(&changed, &retained));
    }
}

#[test]
fn discard_recovery_removes_only_the_exact_stale_recovery_without_a_process() {
    let root = ds_cli_contract::testing::private_tempdir(".ds-profile-upgrade-");
    let scratch = root.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let database = root.path().join("state.sqlite");
    let source = stale(&database);
    let before = retained(&database, &source).unwrap();
    let sessions = Sessions::default();
    let run = |scope: Scope, request: Request| {
        sessions.execute(scope, &scratch, &database, request, &|| Ok(()))
    };
    for (model, head) in [
        ("another", history(2)),
        ("model", history(0)),
        ("model", history(1)),
    ] {
        let error = run(scope(), discard(&source, model, head)).unwrap_err();
        assert_eq!(error.code(), "core_profile_history_conflict");
    }
    let mut foreign = scope();
    foreign.project = "another-project".into();
    let error = run(foreign, discard(&source, "model", history(2))).unwrap_err();
    assert_eq!(error.code(), "core_profile_session_unavailable");
    // Authority revoked before removal keeps the exact recovery.
    let calls = Cell::new(0);
    let error = sessions
        .execute(
            scope(),
            &scratch,
            &database,
            discard(&source, "model", history(2)),
            &|| {
                calls.set(calls.get() + 1);
                if calls.get() == 3 {
                    Err(Failure::failed("authorization_revoked", "fixture"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "authorization_revoked");
    assert_eq!(retained(&database, &source).unwrap(), before);

    let receipt = run(scope(), discard(&source, "model", history(2))).unwrap();
    assert_eq!(receipt["schema"], RECEIPT_SCHEMA);
    assert_eq!(receipt["session_id"], source.session_id);
    assert_eq!(receipt["model_id"], "model");
    assert_eq!(receipt["history"], history(2));
    assert_eq!(receipt["closed"], true);
    assert_eq!(receipt["discarded"], true);
    assert_eq!(receipt["rehydrated"], false);
    assert!(retained(&database, &source).is_none());
    let error = run(scope(), discard(&source, "model", history(2))).unwrap_err();
    assert_eq!(error.code(), "core_profile_session_unavailable");
    // No Network was captured or launched for any of it.
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    assert_eq!(sessions.held_count_for_test(), 0);
}

fn open(bytes: &[u8], recover: bool) -> Request {
    Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::Open {
            recover,
            package_base64: STANDARD.encode(bytes),
            package_sha256: sha(bytes),
        },
    }
}
fn request(receipt: &Value, action: &str, extra: Value) -> Request {
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
fn moved(receipt: &Value, structure: &Value, label: &str) -> Request {
    let command = json!({"command_kind":"move_structure","id":structure["id"],
        "x_m":structure["x_m"].as_f64().unwrap() + 0.5,"y_m":structure["y_m"],
        "z_m":structure["z_m"],"orientation_rad":structure["orientation_rad"]});
    request(
        receipt,
        "control",
        json!({"operation":{"action":"apply_transaction","commands":[
            {"command_id":label,"command":command}]}}),
    )
}
fn recovery(database: &Path, id: &str) -> ds_sync_store::ProfileRecovery {
    Store::open_read_only(database)
        .unwrap()
        .unwrap()
        .profile_recovery(&source_scope(&scope()), id)
        .unwrap()
        .unwrap()
}
fn kind(draft: &Option<ds_sync_store::ProfileDraft>) -> &'static str {
    match draft {
        None => "no draft",
        Some(ds_sync_store::ProfileDraft::Checkpoint { .. }) => "an opaque v1 checkpoint",
        Some(ds_sync_store::ProfileDraft::Journal(_)) => "authored rows",
    }
}
fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("explicit {name}")))
}

/// Owner failure 2026-10-20: after the authored-recovery fix, Local Dev still
/// refused `Profile recovery checkpoint exceeds the protected 64 MiB bound`,
/// because Open recover:true restored a retained v1 draft through its pinned
/// older Network. Two runs share DS_PROFILE_UPGRADE_ROOT (a private directory):
/// phase `legacy` with an older DS_NETWORK_ENGINE_BIN retains v1 drafts and an
/// original; phase `current` with an authored-recovery Network must move the
/// exact draft to authored rows, re-pin the original, refuse a draft it cannot
/// reproduce with its discard remedy, and keep editing large models.
#[test]
#[ignore = "requires two explicit native Networks, a large real .dsgrid and two runs"]
fn real_v1_drafts_move_to_the_current_authored_network() {
    let bytes = fs::read(env_path("DS_PROFILE_LARGE_PACKAGE")).unwrap();
    assert!(std::env::var_os("DS_NETWORK_ENGINE_BIN").is_some());
    let root = env_path("DS_PROFILE_UPGRADE_ROOT");
    let scratch = root.join("scratch");
    let database = root.join("state.sqlite");
    let sessions = Sessions::default();
    let execute = |request| sessions.execute(scope(), &scratch, &database, request, &|| Ok(()));
    let saved = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap()
    };
    match std::env::var("DS_PROFILE_UPGRADE_PHASE").as_deref() {
        Ok("legacy") => {
            fs::create_dir(&scratch).unwrap();
            // A: three authored moves retained as one opaque v1 checkpoint.
            let opened = execute(open(&bytes, false)).unwrap();
            let snapshot: Value =
                serde_json::from_str(opened["payload"]["snapshot_json"].as_str().unwrap()).unwrap();
            let structures = snapshot["structures"].as_array().unwrap();
            let mut receipt =
                execute(request(&opened, "edit_mode", json!({"enabled":true}))).unwrap();
            for (index, structure) in structures.iter().skip(2).take(3).enumerate() {
                receipt = execute(moved(&receipt, structure, &format!("a-{index}"))).unwrap();
            }
            let id = receipt["session_id"].as_str().unwrap();
            match recovery(&database, id).draft {
                Some(ds_sync_store::ProfileDraft::Checkpoint { header, .. }) => {
                    assert_eq!(header.history, receipt["history"]);
                }
                other => panic!(
                    "an older Network retains a v1 checkpoint, found {}",
                    kind(&other)
                ),
            }
            fs::write(root.join("a.json"), serde_json::to_vec(&receipt).unwrap()).unwrap();
            // B: one move whose retained head no Network can reproduce.
            let other = execute(open(&bytes, false)).unwrap();
            let edit = execute(request(&other, "edit_mode", json!({"enabled":true}))).unwrap();
            let edit = execute(moved(&edit, &structures[5], "b-0")).unwrap();
            let id = edit["session_id"].as_str().unwrap();
            let Some(ds_sync_store::ProfileDraft::Checkpoint {
                header,
                bytes: checkpoint,
            }) = recovery(&database, id).draft
            else {
                panic!("v1 checkpoint expected")
            };
            let mut stale = header.clone();
            stale.history["history_pin"] = json!("unreproducible");
            Store::open(&database)
                .unwrap()
                .retain_profile_draft(
                    &source_scope(&scope()),
                    &header.source,
                    &header.history,
                    &stale,
                    &checkpoint,
                    now_ms(),
                )
                .unwrap();
            let mut edit = edit;
            edit["history"] = stale.history;
            fs::write(root.join("b.json"), serde_json::to_vec(&edit).unwrap()).unwrap();
            // C: an original without edits.
            let original = execute(open(&bytes, false)).unwrap();
            fs::write(root.join("c.json"), serde_json::to_vec(&original).unwrap()).unwrap();
        }
        Ok("current") => {
            let (a, b, c) = (saved("a.json"), saved("b.json"), saved("c.json"));
            // B cannot be reproduced: one typed refusal names its discard, and
            // the stale draft stays untouched until that explicit discard.
            let before = recovery(&database, b["session_id"].as_str().unwrap());
            let error = execute(request(&b, "state", json!({}))).unwrap_err();
            assert_eq!(error.code(), "core_profile_draft_stale", "{error}");
            assert_eq!(
                recovery(&database, b["session_id"].as_str().unwrap()),
                before
            );
            let discard = error.detail_value().unwrap()["discard"].clone();
            let discarded = execute(
                Request::parse(
                    &serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":discard}))
                        .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(discarded["discarded"], true);
            let error = execute(request(&b, "state", json!({}))).unwrap_err();
            assert_eq!(error.code(), "core_profile_session_unavailable");

            // C: the untouched original is re-pinned in place.
            let rebound = execute(request(&c, "snapshot", json!({}))).unwrap();
            assert_eq!(rebound["rehydrated"], true);
            assert_eq!(rebound["history"], c["history"]);
            assert_ne!(rebound["engine"], c["engine"]);
            let source = recovery(&database, c["session_id"].as_str().unwrap()).source;
            assert_eq!(source.engine, rebound["engine"]);
            execute(request(&rebound, "close", json!({}))).unwrap();

            // A: Desktop's open with recovery moves the exact head to rows.
            let started = Instant::now();
            let resumed = execute(open(&bytes, true)).unwrap();
            eprintln!("v1 draft upgrade in {} ms", started.elapsed().as_millis());
            assert_eq!(resumed["recovered_draft"], true);
            assert_eq!(resumed["session_id"], a["session_id"]);
            assert_eq!(resumed["history"], a["history"]);
            assert_ne!(resumed["engine"], a["engine"]);
            let id = a["session_id"].as_str().unwrap();
            let journal = match recovery(&database, id).draft {
                Some(ds_sync_store::ProfileDraft::Journal(journal)) => journal,
                other => panic!("expected authored rows, found {}", kind(&other)),
            };
            assert_eq!(journal.entries.len(), 3);
            assert_eq!(journal.header.history, a["history"]);
            assert_eq!(journal.header.source.engine, resumed["engine"]);
            // Draw the large model, then keep editing through authored rows.
            let style: Value = serde_json::from_str(include_str!(
                "../../../kernel/ds-command-kernel/tests/fixtures/profile-display-style.json"
            ))
            .unwrap();
            let drawn = execute(request(
                &resumed,
                "view",
                json!({"options":{},"patch":{},"style_document":style,"cached":false}),
            ))
            .unwrap();
            let snapshot: Value =
                serde_json::from_str(resumed["payload"]["snapshot_json"].as_str().unwrap())
                    .unwrap();
            let structure = &snapshot["structures"].as_array().unwrap()[9];
            let edited = execute(moved(&drawn, structure, "a-3")).unwrap();
            let journal = match recovery(&database, id).draft {
                Some(ds_sync_store::ProfileDraft::Journal(journal)) => journal,
                other => panic!("expected authored rows, found {}", kind(&other)),
            };
            assert_eq!(journal.entries.len(), 4);
            assert_eq!(journal.header.history, edited["history"]);
            assert!(journal.metadata.len() < 64 * 1024, "no results retained");
            // Restart: the re-pinned rows restore without another upgrade.
            drop(sessions);
            let sessions = Sessions::default();
            let resumed = sessions
                .execute(scope(), &scratch, &database, open(&bytes, true), &|| Ok(()))
                .unwrap();
            assert_eq!(resumed["history"], edited["history"]);
            assert_eq!(resumed["engine"], edited["engine"]);
            sessions
                .execute(
                    scope(),
                    &scratch,
                    &database,
                    request(&resumed, "close", json!({})),
                    &|| Ok(()),
                )
                .unwrap();
        }
        other => panic!("DS_PROFILE_UPGRADE_PHASE must be legacy or current, found {other:?}"),
    }
}

/// Retained drafts on a private COPY of a Core store (never a live store):
/// each named v1 draft must rehydrate through the current authored-recovery
/// Network at its exact head and leave authored rows re-pinned to it.
/// DS_PROFILE_STORE_COPY is the copied store.sqlite; DS_PROFILE_SOURCE_HEADERS
/// lists exact retained source header JSON files separated by ':'.
#[test]
#[ignore = "requires a private copy of a Core store and an authored-recovery Network"]
fn real_store_copy_drafts_move_to_the_current_authored_network() {
    let database = env_path("DS_PROFILE_STORE_COPY");
    let scratch = database.with_file_name("profile-scratch");
    fs::create_dir_all(&scratch).unwrap();
    let headers = std::env::var("DS_PROFILE_SOURCE_HEADERS").expect("explicit source headers");
    for path in headers.split(':') {
        let header: ProfileSourceHeader = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let owner = Scope {
            principal: header.scope.principal.clone(),
            lane: header.scope.lane.clone(),
            deployment: header.scope.deployment.clone(),
            install: header.scope.install.clone(),
            project: header.scope.project.clone(),
        };
        let read = || {
            Store::open_read_only(&database)
                .unwrap()
                .unwrap()
                .profile_recovery(&header.scope, &header.session_id)
                .unwrap()
                .unwrap()
        };
        let before = read();
        let head = before
            .draft
            .as_ref()
            .expect("a retained draft")
            .history()
            .clone();
        let state = json!({"schema":REQUEST_SCHEMA,"operation":{"action":"state",
            "session_id":header.session_id,"model_id":header.model_id,"expected_history":head}});
        let started = Instant::now();
        let receipt = Sessions::default()
            .execute(
                owner,
                &scratch,
                &database,
                Request::parse(&serde_json::to_vec(&state).unwrap()).unwrap(),
                &|| Ok(()),
            )
            .unwrap();
        assert_eq!(receipt["history"], head);
        let after = read();
        let rows = match &after.draft {
            Some(ds_sync_store::ProfileDraft::Journal(journal)) => {
                assert_eq!(journal.header.history, head);
                journal.entries.len()
            }
            other => panic!("expected authored rows, found {}", kind(other)),
        };
        assert!(controls::upgrade::same_original(
            &after.source,
            &before.source
        ));
        assert_ne!(after.source.engine, before.source.engine);
        eprintln!(
            "{} {} undo {}: v1 draft moved to {rows} authored rows in {} ms",
            header.scope.project,
            header.session_id,
            head["undo_depth"],
            started.elapsed().as_millis()
        );
    }
}

fn authored_values(recovery: &ds_sync_store::ProfileRecovery) -> (Value, Vec<Value>) {
    let Some(ds_sync_store::ProfileDraft::Journal(journal)) = &recovery.draft else {
        panic!("expected authored recovery")
    };
    (
        serde_json::from_slice(&journal.metadata).unwrap(),
        journal
            .entries
            .iter()
            .map(|entry| serde_json::from_slice(&entry.bytes).unwrap())
            .collect(),
    )
}

/// Two explicit runs share a private DS_PROFILE_REBIND_ROOT: `retain` with the
/// previous authored Network, `current` with the selected station-capable one.
/// `unavailable` is a separate run with an unavailable selected executable.
/// No old executable is needed for either recovery or refusal; acceptance is
/// the decoded saved structure coordinates, IDs, source and native history.
#[test]
#[ignore = "requires explicit previous/current native Networks and a real .dsgrid"]
fn real_authored_draft_rebinds_selected_network_and_keeps_saved_positions() {
    let bytes = fs::read(env_path("DS_PROFILE_LARGE_PACKAGE")).unwrap();
    let root = env_path("DS_PROFILE_REBIND_ROOT");
    let scratch = root.join("scratch");
    let database = root.join("state.sqlite");
    let sessions = Sessions::default();
    let execute = |request| sessions.execute(scope(), &scratch, &database, request, &|| Ok(()));
    let manifest_path = root.join("authored.json");
    match std::env::var("DS_PROFILE_REBIND_PHASE").as_deref() {
        Ok("retain") => {
            fs::create_dir(&scratch).unwrap();
            let mut cases = Vec::new();
            for index in 0..3 {
                let opened = execute(open(&bytes, false)).unwrap();
                let snapshot: Value =
                    serde_json::from_str(opened["payload"]["snapshot_json"].as_str().unwrap())
                        .unwrap();
                let structures = snapshot["structures"].as_array().unwrap();
                let edit = execute(request(&opened, "edit_mode", json!({"enabled":true}))).unwrap();
                let edited =
                    execute(moved(&edit, &structures[2], &format!("retained-{index}"))).unwrap();
                let snapshot = execute(request(&edited, "snapshot", json!({}))).unwrap();
                let snapshot: Value =
                    serde_json::from_str(snapshot["payload"]["snapshot_json"].as_str().unwrap())
                        .unwrap();
                let id = edited["session_id"].as_str().unwrap();
                let recovered = recovery(&database, id);
                let Some(ds_sync_store::ProfileDraft::Journal(journal)) = &recovered.draft else {
                    panic!("the previous engine must support authored recovery")
                };
                if index > 0 {
                    let mut metadata: Value = serde_json::from_slice(&journal.metadata).unwrap();
                    if index == 1 {
                        metadata["binding_state_sha256"] = json!("f".repeat(64));
                    } else {
                        metadata["entry_count"] = json!("invalid typed count");
                    }
                    let metadata = serde_json::to_vec(&metadata).unwrap();
                    let mut header = journal.header.clone();
                    header.metadata_sha256 = sha(&metadata);
                    header.metadata_bytes = metadata.len() as u64;
                    Store::open(&database)
                        .unwrap()
                        .retain_profile_journal_draft(
                            &recovered.source.scope,
                            &recovered.source,
                            &header.history,
                            Some(&header.chain_sha256),
                            &header,
                            &metadata,
                            header.entry_count,
                            &[],
                            now_ms(),
                        )
                        .unwrap();
                }
                cases.push(json!({"receipt":edited,"snapshot":snapshot}));
            }
            fs::write(manifest_path, serde_json::to_vec(&cases).unwrap()).unwrap();
        }
        phase @ (Ok("current") | Ok("unavailable")) => {
            let cases: Vec<Value> =
                serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
            if phase == Ok("unavailable") {
                let receipt = &cases[0]["receipt"];
                let id = receipt["session_id"].as_str().unwrap();
                let before = recovery(&database, id);
                assert!(execute(request(receipt, "state", json!({}))).is_err());
                let after = recovery(&database, id);
                assert_eq!(after.source, before.source);
                assert_eq!(authored_values(&after), authored_values(&before));
                return;
            }
            for case in &cases[1..] {
                let receipt = &case["receipt"];
                let id = receipt["session_id"].as_str().unwrap();
                let before = recovery(&database, id);
                assert!(execute(request(receipt, "state", json!({}))).is_err());
                let after = recovery(&database, id);
                assert_eq!(after.source, before.source);
                assert_eq!(authored_values(&after), authored_values(&before));
            }
            let saved = &cases[0];
            let receipt = &saved["receipt"];
            let id = receipt["session_id"].as_str().unwrap();
            let before = recovery(&database, id);
            let station_structure = &saved["snapshot"]["structures"][1];
            let target_station = station_structure["station_m"].as_f64().unwrap() + 0.5;
            // This first request must resolve the NEW operation capability on
            // the selected engine, even if the retained engine never had it.
            let edited = execute(request(
                receipt,
                "control",
                json!({"operation":{
                "action":"apply_structure_station_move","request":{
                    "expected_revision":receipt["history"]["model_revision"],
                    "structure_id":station_structure["id"],"station_m":target_station}}}),
            ))
            .unwrap();
            let snapshot = execute(request(&edited, "snapshot", json!({}))).unwrap();
            let snapshot: Value =
                serde_json::from_str(snapshot["payload"]["snapshot_json"].as_str().unwrap())
                    .unwrap();
            assert_eq!(
                snapshot["structures"][2], saved["snapshot"]["structures"][2],
                "the previously saved structure coordinates and ID survive the new engine"
            );
            assert_eq!(snapshot["structures"][1]["id"], station_structure["id"]);
            assert_eq!(snapshot["structures"][1]["station_m"], target_station);
            let after = recovery(&database, id);
            assert!(controls::upgrade::same_original(
                &after.source,
                &before.source
            ));
            let commands = authored_values(&before).1;
            assert_eq!(
                &authored_values(&after).1[..commands.len()],
                commands.as_slice()
            );
            assert_eq!(after.draft.as_ref().unwrap().history(), &edited["history"]);
            drop(sessions);
            let restarted = Sessions::default();
            let resumed = restarted
                .execute(
                    scope(),
                    &scratch,
                    &database,
                    request(&edited, "snapshot", json!({})),
                    &|| Ok(()),
                )
                .unwrap();
            let restored: Value =
                serde_json::from_str(resumed["payload"]["snapshot_json"].as_str().unwrap())
                    .unwrap();
            assert_eq!(resumed["history"], edited["history"]);
            assert_eq!(restored["structures"], snapshot["structures"]);
            eprintln!(
                "selected Network recovered saved structure positions, accepted station {target_station}, and retained its history after restart"
            );
        }
        other => {
            panic!("DS_PROFILE_REBIND_PHASE must be retain/current/unavailable, found {other:?}")
        }
    }
}

#[test]
#[ignore = "requires an explicit native Network without authored recovery and a real .dsgrid"]
fn real_opaque_draft_keeps_working_with_the_selected_legacy_network() {
    let bytes = fs::read(env_path("DS_PROFILE_LARGE_PACKAGE")).unwrap();
    let root = ds_cli_contract::testing::private_tempdir(".ds-profile-opaque-");
    let scratch = root.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let database = root.path().join("state.sqlite");
    let sessions = Sessions::default();
    let execute = |request| sessions.execute(scope(), &scratch, &database, request, &|| Ok(()));
    let opened = execute(open(&bytes, false)).unwrap();
    let snapshot: Value =
        serde_json::from_str(opened["payload"]["snapshot_json"].as_str().unwrap()).unwrap();
    let enabled = execute(request(&opened, "edit_mode", json!({"enabled":true}))).unwrap();
    let edited = execute(moved(
        &enabled,
        &snapshot["structures"][2],
        "legacy-position",
    ))
    .unwrap();
    let saved = execute(request(&edited, "snapshot", json!({}))).unwrap();
    let saved: Value =
        serde_json::from_str(saved["payload"]["snapshot_json"].as_str().unwrap()).unwrap();
    let id = edited["session_id"].as_str().unwrap();
    assert!(matches!(
        recovery(&database, id).draft,
        Some(ds_sync_store::ProfileDraft::Checkpoint { .. })
    ));
    drop(sessions);
    let restarted = Sessions::default();
    let resumed = restarted
        .execute(
            scope(),
            &scratch,
            &database,
            request(&edited, "snapshot", json!({})),
            &|| Ok(()),
        )
        .unwrap();
    let restored: Value =
        serde_json::from_str(resumed["payload"]["snapshot_json"].as_str().unwrap()).unwrap();
    assert_eq!(resumed["history"], edited["history"]);
    assert_eq!(restored["structures"], saved["structures"]);
    assert!(matches!(
        recovery(&database, id).draft,
        Some(ds_sync_store::ProfileDraft::Checkpoint { .. })
    ));
}
