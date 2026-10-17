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
