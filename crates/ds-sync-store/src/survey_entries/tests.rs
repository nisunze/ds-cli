use super::*;
use kernel::{Phase, Status};

fn scope() -> SurveyEntryScope {
    SurveyEntryScope {
        account: "owner-a".into(),
        deployment: "stable".into(),
        install_id: "device-a".into(),
    }
}
fn sample(id: &str, project: &str) -> Value {
    json!({"kind":"survey","id":id,"owner_uid":"owner-a", "generation":"11111111-2222-3333-4444-555555555555",
        "project_id":project,"form_id":"form-a","doc_id":format!("doc-{id}"),"operation":"create",
        "payload":{"answers":{"opaque":[1,2,3]},"expected_version":3},"overlay":{"type":"Feature","properties":{"label":"offline"}},
        "status":"pending","attempts":0,"created_at":17,"synced_at":null,"error":null,"idempotency_key":format!("key-{id}")})
}
fn store() -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let held = Store::open(&root.path().join("store.sqlite")).unwrap();
    (root, held)
}
fn first(store: &Store, scope: &SurveyEntryScope) -> Header {
    store.survey_entry_headers(scope, None).unwrap().remove(0)
}
fn schedule(store: &mut Store, scope: &SurveyEntryScope) -> Value {
    store
        .survey_entry_transition(
            scope,
            "project-a",
            json!({"kind":"schedule","current_uid":scope.account,"online":true}),
            100,
        )
        .unwrap()
}

#[test]
fn admission_is_atomic_and_duplicate_create_is_scoped() {
    let (_root, mut store) = store();
    let scope = scope();
    let row = sample("one", "project-a");
    store
        .survey_entry_admit_batch(&scope, vec![row.clone()], 20)
        .unwrap();
    let mut duplicate = sample("two", "project-a");
    duplicate["doc_id"] = row["doc_id"].clone();
    assert!(store
        .survey_entry_admit_batch(&scope, vec![sample("three", "project-a"), duplicate], 20)
        .is_err());
    assert!(store.survey_entry_row(&scope, "three").unwrap().is_none());
    assert_eq!(store.survey_entry_headers(&scope, None).unwrap().len(), 1);
    let mut other_project = sample("two", "project-b");
    other_project["doc_id"] = row["doc_id"].clone();
    store
        .survey_entry_admit_batch(&scope, vec![other_project], 20)
        .unwrap();
    let mut other_scope = scope.clone();
    other_scope.account = "owner-b".into();
    let mut other_row = row.clone();
    other_row["owner_uid"] = json!("owner-b");
    store
        .survey_entry_admit_batch(&other_scope, vec![other_row], 20)
        .unwrap();
    assert_eq!(
        store
            .survey_entry_headers(&scope, Some("project-a"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .survey_entry_headers(&other_scope, None)
            .unwrap()
            .len(),
        1
    );
    let mut wrong_install = scope.clone();
    wrong_install.install_id = "other".into();
    assert!(store
        .survey_entry_row(&wrong_install, "one")
        .unwrap()
        .is_none());
    assert!(store
        .survey_entry_headers(&wrong_install, None)
        .unwrap()
        .is_empty());
    let mut wrong_deployment = scope.clone();
    wrong_deployment.deployment = "canary".into();
    assert!(store
        .survey_entry_row(&wrong_deployment, "one")
        .unwrap()
        .is_none());
}

#[test]
fn admission_refuses_ownerless_foreign_invalid_and_cross_project_batches() {
    let (_root, mut store) = store();
    let scope = scope();
    for key in ["owner_uid", "generation"] {
        let mut row = sample("one", "project-a");
        row.as_object_mut().unwrap().remove(key);
        assert!(store
            .survey_entry_admit_batch(&scope, vec![row.clone()], 20)
            .is_err());
    }
    let mut foreign = sample("one", "project-a");
    foreign["owner_uid"] = json!("owner-b");
    assert!(store
        .survey_entry_admit_batch(&scope, vec![sample("two", "project-a"), foreign], 20)
        .is_err());
    assert!(store.survey_entry_headers(&scope, None).unwrap().is_empty());
    assert!(store
        .survey_entry_admit_batch(
            &scope,
            vec![sample("one", "project-a"), sample("two", "project-b")],
            20
        )
        .is_err());
    for invalid in ["", " padded", "with\ncontrol"] {
        let mut invalid_scope = scope.clone();
        invalid_scope.install_id = invalid.into();
        assert!(store.survey_entry_headers(&invalid_scope, None).is_err());
    }
    let mut row = sample("one", "project-a");
    row["generation"] = json!("not-a-uuid");
    assert!(store
        .survey_entry_admit_batch(&scope, vec![row], 20)
        .is_err());
    assert!(!has_table(&store.connection).unwrap());
}

#[test]
fn admitted_row_and_replay_metadata_survive_restart() {
    let (root, mut store) = store();
    let scope = scope();
    let mut row = sample("one", "project-a");
    row["synced_at"] = json!(777);
    row["payload"]["opaque_float"] = json!(1.25);
    store
        .survey_entry_admit_batch(&scope, vec![row.clone()], 20)
        .unwrap();
    let before = first(&store, &scope);
    let mut advanced = before.clone();
    advanced.status = Status::Syncing;
    advanced.lease = 42;
    advanced.attempts = 4;
    advanced.subeffect_phase = Some(Phase::RefreshAuth);
    advanced.error = Some("retained error".into());
    advanced.next_attempt_at = 12345;
    advanced.failure_code = Some("prior-code".into());
    assert!(store
        .survey_entry_cas_apply(&scope, &Fence::from(&before), &advanced, None)
        .unwrap());
    let held = store.survey_entry_row(&scope, "one").unwrap().unwrap();
    assert_eq!(held["payload"], row["payload"]);
    assert_eq!(held["overlay"], row["overlay"]);
    assert_eq!(held["synced_at"], 777);
    assert_eq!(held["created_at"], row["created_at"]);
    assert_eq!(held["idempotency_key"], row["idempotency_key"]);
    drop(store);
    let mut store = Store::open(&root.path().join("store.sqlite")).unwrap();
    assert_eq!(store.survey_entry_row(&scope, "one").unwrap(), Some(held));
    assert!(header_equal(&first(&store, &scope), &advanced).unwrap());
    assert_eq!(
        crate::schema_version(&store.connection).unwrap(),
        Some(crate::SCHEMA_VERSION)
    );
    store
        .survey_entry_transition(
            &scope,
            "project-a",
            json!({"kind":"recover","current_uid":scope.account}),
            100,
        )
        .unwrap();
    assert_ne!(first(&store, &scope).status, Status::Syncing);
}

#[test]
fn header_projection_defaults_only_and_does_not_load_payload() {
    let (_root, mut store) = store();
    let scope = scope();
    let mut row = sample("one", "project-a");
    row["lease"] = Value::Null;
    row["next_attempt_at"] = Value::Null;
    row["conflict_rebased"] = Value::Null;
    row["legacy_confirmed"] = Value::Null;
    store
        .survey_entry_admit_batch(&scope, vec![row.clone()], 20)
        .unwrap();
    let h = first(&store, &scope);
    assert_eq!(h.lease, 0);
    assert_eq!(h.next_attempt_at, 0);
    assert!(!h.conflict_rebased);
    let raw = serde_json::to_value(&h).unwrap();
    assert!(raw.get("payload").is_none());
    assert!(raw.get("overlay").is_none());
    let held = store.survey_entry_row(&scope, "one").unwrap().unwrap();
    assert_eq!(held["payload"], row["payload"]);
    assert_eq!(held["overlay"], row["overlay"]);
    assert_eq!(held["generation"], row["generation"]);
    assert_eq!(held["synced_at"], row["synced_at"]);
    // Header-only reads still work even when a payload cannot be read/parsed.
    store
        .connection
        .execute("UPDATE survey_entry_outbox SET row='broken payload'", [])
        .unwrap();
    assert_eq!(store.survey_entry_headers(&scope, None).unwrap().len(), 1);
    assert!(store.survey_entry_row(&scope, "one").is_err());
}

#[test]
fn exact_cas_fence_immutable_fields_and_payload_patch() {
    let (_root, mut store) = store();
    let scope = scope();
    store
        .survey_entry_admit_batch(&scope, vec![sample("one", "project-a")], 20)
        .unwrap();
    let h = first(&store, &scope);
    let fence = Fence::from(&h);
    for field in [
        "project_id",
        "form_id",
        "doc_id",
        "operation",
        "idempotency_key",
        "created_at",
        "id",
    ] {
        let mut value = serde_json::to_value(&h).unwrap();
        value[field] = match field {
            "operation" => json!("delete"),
            "created_at" => json!(18),
            _ => json!("different"),
        };
        let changed: Header = serde_json::from_value(value).unwrap();
        assert!(
            !store
                .survey_entry_cas_apply(&scope, &fence, &changed, None)
                .unwrap(),
            "{field}"
        );
    }
    for field in [
        "owner_uid",
        "generation",
        "lease",
        "status",
        "subeffect_phase",
    ] {
        let mut value = serde_json::to_value(&fence).unwrap();
        value[field] = match field {
            "lease" => json!(1),
            "status" => json!("syncing"),
            "subeffect_phase" => json!("dispatch"),
            _ => json!("different"),
        };
        let stale: Fence = serde_json::from_value(value).unwrap();
        assert!(
            !store
                .survey_entry_cas_apply(&scope, &stale, &h, None)
                .unwrap(),
            "{field}"
        );
        assert!(
            !store.survey_entry_cas_remove(&scope, &stale).unwrap(),
            "{field}"
        );
    }
    let before = store.survey_entry_row(&scope, "one").unwrap().unwrap();
    let mut next = h.clone();
    next.lease = 1;
    next.status = Status::Syncing;
    next.subeffect_phase = Some(Phase::Dispatch);
    assert!(store
        .survey_entry_cas_apply(
            &scope,
            &fence,
            &next,
            Some(&PayloadPatch {
                expected_version: 9
            })
        )
        .unwrap());
    assert!(!store.survey_entry_cas_remove(&scope, &fence).unwrap());
    let after = store.survey_entry_row(&scope, "one").unwrap().unwrap();
    assert_eq!(after["overlay"], before["overlay"]);
    assert_eq!(after["synced_at"], before["synced_at"]);
    assert_eq!(after["payload"]["answers"], before["payload"]["answers"]);
    assert_eq!(after["payload"]["expected_version"], 9);
    let mut foreign_scope = scope.clone();
    foreign_scope.account = "owner-b".into();
    assert!(!store
        .survey_entry_cas_remove(&foreign_scope, &Fence::from(&next))
        .unwrap());
    assert!(store
        .survey_entry_cas_remove(&scope, &Fence::from(&next))
        .unwrap());
    assert!(!store
        .survey_entry_cas_apply(&scope, &Fence::from(&next), &next, None)
        .unwrap());
    assert!(!store
        .survey_entry_cas_remove(&scope, &Fence::from(&next))
        .unwrap());
}

#[test]
fn sqlite_errors_roll_back_admission_and_transition() {
    let (_root, mut store) = store();
    let scope = scope();
    store
        .survey_entry_admit_batch(&scope, vec![sample("one", "project-a")], 20)
        .unwrap();
    store.connection.execute_batch("CREATE TRIGGER refuse_insert BEFORE INSERT ON survey_entry_outbox WHEN NEW.id='three' BEGIN SELECT RAISE(ABORT,'disk-like write failure'); END;").unwrap();
    assert!(store
        .survey_entry_admit_batch(
            &scope,
            vec![sample("two", "project-a"), sample("three", "project-a")],
            20
        )
        .is_err());
    assert!(store.survey_entry_row(&scope, "two").unwrap().is_none());
    store
        .survey_entry_admit_batch(&scope, vec![sample("two", "project-a")], 20)
        .unwrap();
    store.connection.execute_batch("CREATE TRIGGER refuse_update BEFORE UPDATE ON survey_entry_outbox WHEN NEW.id='two' BEGIN SELECT RAISE(ABORT,'write failure'); END;").unwrap();
    assert!(store
        .survey_entry_transition(
            &scope,
            "project-a",
            json!({"kind":"schedule","current_uid":scope.account,"online":true}),
            100
        )
        .is_err());
    assert!(store
        .survey_entry_headers(&scope, None)
        .unwrap()
        .iter()
        .all(|h| h.status == Status::Pending && h.lease == 0));
}

#[test]
fn two_connections_schedule_one_lease_and_late_result_cannot_settle_successor() {
    let (root, mut store) = store();
    let scope = scope();
    store
        .survey_entry_admit_batch(
            &scope,
            vec![sample("one", "project-a"), sample("other", "project-b")],
            20,
        )
        .unwrap_err();
    store
        .survey_entry_admit_batch(&scope, vec![sample("one", "project-a")], 20)
        .unwrap();
    store
        .survey_entry_admit_batch(&scope, vec![sample("other", "project-b")], 20)
        .unwrap();
    let path = root.path().join("store.sqlite");
    let mut second = Store::open(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let ready = barrier.clone();
    let other_scope = scope.clone();
    let worker = std::thread::spawn(move || {
        ready.wait();
        schedule(&mut second, &other_scope)
    });
    barrier.wait();
    let left = schedule(&mut store, &scope);
    let right = worker.join().unwrap();
    let dispatches = left["actions"]
        .as_array()
        .unwrap()
        .iter()
        .chain(right["actions"].as_array().unwrap())
        .filter(|a| a["kind"] == "dispatch")
        .count();
    assert_eq!(dispatches, 1);
    let old = store
        .survey_entry_headers(&scope, Some("project-a"))
        .unwrap()
        .remove(0);
    assert_eq!(old.lease, 1);
    assert_eq!(old.status, Status::Syncing);
    let unrelated = store
        .survey_entry_headers(&scope, Some("project-b"))
        .unwrap()
        .remove(0);
    assert_eq!(unrelated.lease, 0);
    assert_eq!(unrelated.status, Status::Pending);
    store
        .survey_entry_transition(
            &scope,
            "project-a",
            json!({"kind":"recover","current_uid":scope.account}),
            101,
        )
        .unwrap();
    store
        .survey_entry_transition(
            &scope,
            "project-a",
            json!({"kind":"schedule","current_uid":scope.account,"online":true}),
            102,
        )
        .unwrap();
    let successor = store
        .survey_entry_headers(&scope, Some("project-a"))
        .unwrap()
        .remove(0);
    assert!(successor.lease > old.lease);
    let reply=store.survey_entry_transition(&scope,"project-a",json!({"kind":"result","current_uid":scope.account,"id":old.id,"generation":old.generation,"lease":old.lease,"outcome":{"kind":"success"}}),102).unwrap();
    assert!(!reply["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["kind"] == "remove"));
    assert!(store.survey_entry_row(&scope, "one").unwrap().is_some());
    assert_eq!(
        store
            .survey_entry_headers(&scope, Some("project-a"))
            .unwrap()[0]
            .lease,
        successor.lease
    );
}

#[test]
fn transition_refuses_wrong_authority_unknown_event_and_admission_without_writes() {
    let (_root, mut store) = store();
    let scope = scope();
    for event in [
        json!({"kind":"unknown"}),
        json!({"kind":"admit_batch"}),
        json!({"kind":"admit"}),
        json!({"kind":"schedule","current_uid":"owner-b","online":true}),
        json!({"kind":"summary","current_uid":"owner-a","project_id":"project-b","online":true}),
    ] {
        assert!(store
            .survey_entry_transition(&scope, "project-a", event, 100)
            .is_err());
    }
    assert!(!has_table(&store.connection).unwrap());
    assert!(store.survey_entry_headers(&scope, None).unwrap().is_empty());
}

#[test]
fn readonly_reads_create_no_survey_table_and_writes_propagate_errors() {
    let (root, store) = store();
    let scope = scope();
    assert!(store.survey_entry_headers(&scope, None).unwrap().is_empty());
    drop(store);
    let mut readonly = Store::open_read_only(&root.path().join("store.sqlite"))
        .unwrap()
        .unwrap();
    assert!(readonly.survey_entry_row(&scope, "one").unwrap().is_none());
    assert!(!has_table(&readonly.connection).unwrap());
    assert!(readonly
        .survey_entry_admit_batch(&scope, vec![sample("one", "project-a")], 20)
        .is_err());
    assert!(!has_table(&readonly.connection).unwrap());
}

#[test]
fn bounded_inputs_and_oversized_stored_rows_refuse_explicitly() {
    let (_root, mut store) = store();
    let scope = scope();
    assert!(store.survey_entry_admit_batch(&scope, vec![], 20).is_err());
    assert!(store
        .survey_entry_admit_batch(&scope, vec![sample("one", "project-a"); 501], 20)
        .is_err());
    let mut oversized = sample("huge", "project-a");
    oversized["payload"]["large"] = json!("x".repeat(MAX_SURVEY_ENTRY_BYTES));
    assert!(store
        .survey_entry_admit_batch(&scope, vec![oversized], 20)
        .is_err());
    store
        .survey_entry_admit_batch(&scope, vec![sample("one", "project-a")], 20)
        .unwrap();
    store
        .connection
        .execute(
            "UPDATE survey_entry_outbox SET row=printf('%.*c',8388609,'x')",
            [],
        )
        .unwrap();
    assert!(store.survey_entry_row(&scope, "one").is_err());
    assert_eq!(store.survey_entry_headers(&scope, None).unwrap().len(), 1);
    store
        .connection
        .execute(
            "UPDATE survey_entry_outbox SET header=printf('%.*c',65537,'x')",
            [],
        )
        .unwrap();
    assert!(store.survey_entry_headers(&scope, None).is_err());
}

#[test]
fn capacity_admission_rolls_back_and_headers_never_silently_truncate() {
    let (_root, mut store) = store();
    let scope = scope();
    let mut seed = sample("seed", "project-a");
    seed["operation"] = json!("update");
    store
        .survey_entry_admit_batch(&scope, vec![seed.clone()], 20)
        .unwrap();
    store
        .connection
        .execute("DELETE FROM survey_entry_outbox", [])
        .unwrap();
    let header = serde_json::to_string(&project_header(&seed).unwrap()).unwrap();
    let raw = serde_json::to_string(&seed).unwrap();
    store.connection.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO survey_entry_outbox SELECT ?1,?2,?3,'row-'||x,'project-a','form-a','doc-seed','update',json_set(?4,'$.id','row-'||x),json_set(?5,'$.id','row-'||x) FROM n",params![scope.account,scope.deployment,scope.install_id,header,raw]).unwrap();
    assert!(store
        .survey_entry_admit_batch(&scope, vec![sample("overflow", "project-a")], 20)
        .is_err());
    assert!(store
        .survey_entry_row(&scope, "overflow")
        .unwrap()
        .is_none());
    assert_eq!(
        store.survey_entry_headers(&scope, None).unwrap().len(),
        10000
    );
    store.connection.execute("INSERT INTO survey_entry_outbox SELECT account,deployment,install_id,'overflow',project,form,doc,operation,header,row FROM survey_entry_outbox LIMIT 1",[]).unwrap();
    assert!(store.survey_entry_headers(&scope, None).is_err());
}

fn discard_fixture(store: &mut Store, scope: &SurveyEntryScope) {
    store
        .survey_entry_admit_batch(
            scope,
            vec![
                sample("one", "project-a"),
                sample("two", "project-a"),
                sample("pending", "project-a"),
            ],
            20,
        )
        .unwrap();
    store
        .survey_entry_admit_batch(scope, vec![sample("other", "project-b")], 20)
        .unwrap();
    for id in ["one", "two", "other"] {
        let header = store
            .survey_entry_headers(scope, None)
            .unwrap()
            .into_iter()
            .find(|h| h.id == id)
            .unwrap();
        let mut terminal = header.clone();
        terminal.status = if id == "two" {
            Status::Conflict
        } else {
            Status::Failed
        };
        assert!(store
            .survey_entry_cas_apply(scope, &Fence::from(&header), &terminal, None)
            .unwrap());
    }
}
fn discard_expected(prepared: &[SurveyEntryDiscardPrepared]) -> Vec<SurveyEntryDiscardExpected> {
    prepared
        .iter()
        .map(|p| SurveyEntryDiscardExpected {
            fence: p.fence.clone(),
            row_sha256: p.row_sha256.clone(),
        })
        .collect()
}
fn audit_candidates(store: &Store, scope: &SurveyEntryScope) -> Vec<SurveyEntryDiscardPrepared> {
    prepare_discard(
        Some(store),
        scope,
        "project-a",
        &["one".into(), "two".into()],
        100,
    )
    .unwrap()
}

#[test]
fn discard_preparation_keeps_full_rows_and_exact_kernel_eligibility_until_commit() {
    let (_root, mut store) = store();
    let scope = scope();
    discard_fixture(&mut store, &scope);
    let before = store.survey_entry_headers(&scope, None).unwrap();
    let prepared = prepare_discard(
        Some(&store),
        &scope,
        "project-a",
        &[
            "one".into(),
            "two".into(),
            "pending".into(),
            "other".into(),
            "missing".into(),
        ],
        100,
    )
    .unwrap();
    assert_eq!(
        prepared
            .iter()
            .map(|p| p.fence.id.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(store.survey_entry_headers(&scope, None).unwrap()).unwrap()
    );
    for row in &prepared {
        assert_eq!(
            row.row,
            store
                .survey_entry_row(&scope, &row.fence.id)
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            row.row_sha256,
            sha256_hex(&serde_json::to_vec(&row.row).unwrap())
        );
        assert!(row.row["payload"].is_object());
        assert!(row.row["overlay"].is_object());
    }
    assert_eq!(
        store
            .survey_entry_commit_discard(&scope, "project-a", &discard_expected(&prepared), 101)
            .unwrap(),
        vec!["one", "two"]
    );
    assert!(store.survey_entry_row(&scope, "one").unwrap().is_none());
    assert!(store.survey_entry_row(&scope, "two").unwrap().is_none());
    assert!(store.survey_entry_row(&scope, "pending").unwrap().is_some());
    assert!(store.survey_entry_row(&scope, "other").unwrap().is_some());
}

#[test]
fn audited_discard_refuses_concurrent_row_header_lease_generation_project_or_presence_changes_atomically(
) {
    for change in [
        "payload",
        "overlay",
        "attempts",
        "lease",
        "generation",
        "project",
        "missing",
    ] {
        let (root, mut store) = store();
        let scope = scope();
        discard_fixture(&mut store, &scope);
        let expected = discard_expected(&audit_candidates(&store, &scope));
        let mut writer = Store::open(&root.path().join("store.sqlite")).unwrap();
        let mut row = writer.survey_entry_row(&scope, "two").unwrap().unwrap();
        let header = project_header(&row).unwrap();
        match change {
            "payload" | "overlay" => {
                row[change]["changed_after_prepare"] = json!(true);
                writer
                    .connection
                    .execute(
                        "UPDATE survey_entry_outbox SET row=?1 WHERE id='two'",
                        params![serde_json::to_string(&row).unwrap()],
                    )
                    .unwrap();
            }
            "project" => {
                row["project_id"] = json!("project-b");
                let header = project_header(&row).unwrap();
                writer.connection.execute("UPDATE survey_entry_outbox SET project='project-b',header=?1,row=?2 WHERE id='two'",
                    params![serde_json::to_string(&header).unwrap(),serde_json::to_string(&row).unwrap()]).unwrap();
            }
            "missing" => {
                assert!(writer
                    .survey_entry_cas_remove(&scope, &Fence::from(&header))
                    .unwrap());
            }
            _ => {
                let mut next = header.clone();
                match change {
                    "attempts" => next.attempts += 1,
                    "lease" => next.lease += 1,
                    "generation" => {
                        next.generation = Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into())
                    }
                    _ => unreachable!(),
                }
                assert!(writer
                    .survey_entry_cas_apply(&scope, &Fence::from(&header), &next, None)
                    .unwrap());
            }
        }
        let before_one = store.survey_entry_row(&scope, "one").unwrap();
        let before_two = store.survey_entry_row(&scope, "two").unwrap();
        assert!(
            store
                .survey_entry_commit_discard(&scope, "project-a", &expected, 101)
                .is_err(),
            "{change}"
        );
        assert_eq!(
            store.survey_entry_row(&scope, "one").unwrap(),
            before_one,
            "{change}"
        );
        assert_eq!(
            store.survey_entry_row(&scope, "two").unwrap(),
            before_two,
            "{change}"
        );
    }
}

#[test]
fn audited_discard_refuses_foreign_scope_project_and_nonterminal_rows_without_partial_removal() {
    let (_root, mut store) = store();
    let scope = scope();
    discard_fixture(&mut store, &scope);
    let expected = discard_expected(&audit_candidates(&store, &scope));
    for field in ["account", "deployment", "install_id"] {
        let mut foreign = scope.clone();
        match field {
            "account" => foreign.account = "foreign".into(),
            "deployment" => foreign.deployment = "foreign".into(),
            _ => foreign.install_id = "foreign".into(),
        }
        assert!(store
            .survey_entry_commit_discard(&foreign, "project-a", &expected, 101)
            .is_err());
    }
    assert!(store
        .survey_entry_commit_discard(&scope, "project-b", &expected, 101)
        .is_err());
    let pending = store.survey_entry_row(&scope, "pending").unwrap().unwrap();
    let mut mixed = vec![
        expected[0].clone(),
        SurveyEntryDiscardExpected {
            fence: Fence::from(&project_header(&pending).unwrap()),
            row_sha256: sha256_hex(&serde_json::to_vec(&pending).unwrap()),
        },
    ];
    assert!(store
        .survey_entry_commit_discard(&scope, "project-a", &mixed, 101)
        .is_err());
    mixed[1] = expected[1].clone();
    mixed[1].fence.owner_uid = Some("foreign".into());
    assert!(store
        .survey_entry_commit_discard(&scope, "project-a", &mixed, 101)
        .is_err());
    assert_eq!(store.survey_entry_headers(&scope, None).unwrap().len(), 4);
}

#[test]
fn discard_batches_are_closed_bounded_unique_and_rollback_sqlite_removal_failures() {
    let (_root, mut store) = store();
    let scope = scope();
    discard_fixture(&mut store, &scope);
    for ids in [
        vec!["one".into(), "one".into()],
        vec!["".into()],
        vec![" padded".into()],
        vec!["control\n".into()],
        (0..501).map(|n| format!("row-{n}")).collect(),
    ] {
        assert!(prepare_discard(Some(&store), &scope, "project-a", &ids, 100).is_err());
    }
    let expected = discard_expected(&audit_candidates(&store, &scope));
    for invalid in [vec![expected[0].clone(); 2], vec![expected[0].clone(); 501]] {
        assert!(store
            .survey_entry_commit_discard(&scope, "project-a", &invalid, 101)
            .is_err());
    }
    for digest in ["bad".into(), "A".repeat(64), "0".repeat(64)] {
        let mut invalid = expected.clone();
        invalid[1].row_sha256 = digest;
        assert!(store
            .survey_entry_commit_discard(&scope, "project-a", &invalid, 101)
            .is_err());
    }
    let mut value = serde_json::to_value(&expected[0]).unwrap();
    value["row"] = json!({});
    assert!(serde_json::from_value::<SurveyEntryDiscardExpected>(value).is_err());
    assert!(store
        .survey_entry_transition(
            &scope,
            "project-a",
            json!({"kind":"discard","current_uid":scope.account,"ids":["one"]}),
            100
        )
        .is_err());
    store.connection.execute_batch("CREATE TRIGGER refuse_discard BEFORE DELETE ON survey_entry_outbox WHEN OLD.id='two' BEGIN SELECT RAISE(ABORT,'discard write failure'); END;").unwrap();
    assert!(store
        .survey_entry_commit_discard(&scope, "project-a", &expected, 101)
        .is_err());
    assert_eq!(store.survey_entry_headers(&scope, None).unwrap().len(), 4);
}

#[test]
fn empty_discard_and_missing_table_preparation_create_nothing() {
    let scope = scope();
    assert!(
        prepare_discard(None, &scope, "project-a", &["missing".into()], 100)
            .unwrap()
            .is_empty()
    );
    assert!(prepare_discard(None, &scope, "project-a", &[], 100)
        .unwrap()
        .is_empty());
    assert!(prepare_discard(None, &scope, "project-a", &["".into()], 100).is_err());
    assert!(prepare_discard(None, &scope, "project-a", &[], u64::MAX).is_err());
    let (_root, mut store) = store();
    assert!(
        prepare_discard(Some(&store), &scope, "project-a", &["missing".into()], 100)
            .unwrap()
            .is_empty()
    );
    assert!(store
        .survey_entry_commit_discard(&scope, "project-a", &[], 100)
        .unwrap()
        .is_empty());
    assert!(!has_table(&store.connection).unwrap());
}

#[test]
fn audited_discard_bounds_complete_row_bytes_without_partial_preparation_or_deletion() {
    let (_root, mut store) = store();
    let scope = scope();
    let mut ids = Vec::new();
    let mut expected = Vec::new();
    for index in 0..5 {
        let id = format!("large-{index}");
        let mut row = sample(&id, "project-a");
        row["payload"]["retained"] = json!("x".repeat(7 * 1024 * 1024));
        store
            .survey_entry_admit_batch(&scope, vec![row], 20)
            .unwrap();
        let header = store
            .survey_entry_headers(&scope, Some("project-a"))
            .unwrap()
            .into_iter()
            .find(|h| h.id == id)
            .unwrap();
        let mut terminal = header.clone();
        terminal.status = Status::Failed;
        assert!(store
            .survey_entry_cas_apply(&scope, &Fence::from(&header), &terminal, None)
            .unwrap());
        expected.extend(discard_expected(
            &prepare_discard(
                Some(&store),
                &scope,
                "project-a",
                std::slice::from_ref(&id),
                100,
            )
            .unwrap(),
        ));
        ids.push(id);
    }
    assert!(prepare_discard(Some(&store), &scope, "project-a", &ids, 100).is_err());
    assert!(store
        .survey_entry_commit_discard(&scope, "project-a", &expected, 101)
        .is_err());
    assert_eq!(
        store
            .survey_entry_headers(&scope, Some("project-a"))
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn native_create_preparation_and_receipt_are_read_only_and_exactly_fenced() {
    let (_root, mut store) = store();
    let scope = scope();
    let authority = ds_command_kernel::survey_entry_create::direct::Authority {
        firebase_project: "test-project".into(),
        uid: scope.account.clone(),
        email: "test@example.invalid".into(),
        lane: "canary".into(),
    };
    let mut row = sample("create-one", "project-a");
    row["form_id"] = json!("field_notes");
    row["created_at"] = json!(1792314166506u64);
    row["payload"] = json!({"project_id":"project-a","form_id":"field_notes",
        "doc_id":"doc-create-one","operation":"create","idempotency_key":"key-create-one",
        "data":{"note":"SQLite offline acceptance"},
        "metadata":{"created_at":"2026-10-18T09:02:46.506Z","origin":"web"}});
    store
        .survey_entry_admit_batch(&scope, vec![row], 1792314166506)
        .unwrap();
    let fence = Fence::from(&first(&store, &scope));
    let before = store.survey_entry_row(&scope, &fence.id).unwrap();
    let prepared =
        create::prepare_direct(Some(&store), &scope, "project-a", &fence, &authority).unwrap();
    assert_eq!(
        prepared["plan"]["payload"],
        before.as_ref().unwrap()["payload"]
    );
    let expected: create::Expected = serde_json::from_value(prepared["expected"].clone()).unwrap();
    let response = json!({"commitTime":"2026-10-18T09:02:47Z","writeResults":[{"updateTime":"2026-10-18T09:02:47Z","transformResults":[{"timestampValue":"2026-10-18T09:02:47Z"}]}]});
    let verified = create::verify_direct(
        Some(&store),
        &scope,
        "project-a",
        &expected,
        &authority,
        200,
        &response,
    )
    .unwrap();
    assert_eq!(verified["receipt"]["doc_id"], "doc-create-one");
    assert_eq!(store.survey_entry_row(&scope, &fence.id).unwrap(), before);
    assert!(create::verify_direct(
        Some(&store),
        &scope,
        "project-a",
        &expected,
        &authority,
        409,
        &response
    )
    .is_err());
    let mut crossed = response.clone();
    crossed["writeResults"][0]["transformResults"] = json!([]);
    assert!(create::verify_direct(
        Some(&store),
        &scope,
        "project-a",
        &expected,
        &authority,
        200,
        &crossed
    )
    .is_err());
    let mut crossed_authority = authority.clone();
    crossed_authority.firebase_project = "other-project".into();
    assert!(create::verify_direct(
        Some(&store),
        &scope,
        "project-a",
        &expected,
        &crossed_authority,
        200,
        &response
    )
    .is_err());
    let mut crossed_digest = expected.clone();
    crossed_digest.direct_sha256 = Some("0".repeat(64));
    assert!(create::verify_direct(
        Some(&store),
        &scope,
        "project-a",
        &crossed_digest,
        &authority,
        200,
        &response
    )
    .is_err());
    let mut stale = expected.clone();
    stale.row_sha256 = "0".repeat(64);
    assert!(create::verify_direct(
        Some(&store),
        &scope,
        "project-a",
        &stale,
        &authority,
        200,
        &response
    )
    .is_err());
    let mut crossed_fence = fence.clone();
    crossed_fence.lease += 1;
    assert!(create::prepare_direct(
        Some(&store),
        &scope,
        "project-a",
        &crossed_fence,
        &authority
    )
    .is_err());
    crossed_fence = fence.clone();
    crossed_fence.owner_uid = Some("other-owner".into());
    assert!(create::prepare_direct(
        Some(&store),
        &scope,
        "project-a",
        &crossed_fence,
        &authority
    )
    .is_err());
    assert!(create::prepare_direct(Some(&store), &scope, "project-b", &fence, &authority).is_err());
    assert!(create::prepare_direct(None, &scope, "project-a", &fence, &authority).is_err());
    assert_eq!(store.survey_entry_row(&scope, &fence.id).unwrap(), before);
}
