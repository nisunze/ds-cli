use crate::durable_store::{self, Answer, Call, Identity, Principal};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn identity(principal: Principal) -> Identity {
    Identity {
        principal,
        lane: "canary".into(),
        audience: "grid-model-tests".into(),
    }
}
fn alice() -> Identity {
    identity(Principal::Account("alice".into()))
}
fn bob() -> Identity {
    identity(Principal::Account("bob".into()))
}
fn anonymous() -> Identity {
    identity(Principal::Anonymous)
}
fn call(
    connection: &mut Connection,
    identity: &Identity,
    operation: Value,
    bytes: Option<Vec<u8>>,
) -> Result<Answer, String> {
    let call: Call = serde_json::from_value(
        json!({"request": {"store": "grid_model_store", "operation": operation}}),
    )
    .unwrap();
    durable_store::execute(connection, Some(identity), call, bytes)
}
fn read(connection: &mut Connection, identity: &Identity, operation: Value) -> Answer {
    call(connection, identity, operation, None).unwrap()
}
fn model(id: &str, revision: &str, updated: u64) -> Value {
    json!({"model_id": id, "manifest_model_id": "native-feeder", "name": "Feeder A", "revision": revision,
        "updated": updated, "imported_from": "PLS-CADD", "workspace_owner": "temporary"})
}
fn checkpoint(
    connection: &mut Connection,
    identity: &Identity,
    model: Value,
    bytes: &[u8],
) -> Answer {
    call(
        connection,
        identity,
        json!({"action": "checkpoint", "model": model,
        "payload": {"byte_length": bytes.len()}}),
        Some(bytes.to_vec()),
    )
    .unwrap()
}
fn get(connection: &mut Connection, identity: &Identity, id: &str) -> Answer {
    read(
        connection,
        identity,
        json!({"action": "get_model", "model_id": id}),
    )
}
fn binding() -> Value {
    json!({"dsProject": "project-a", "modelId": "project-model", "revisionId": "project-rev-7", "version": 7, "digest": "a".repeat(64)})
}

#[test]
fn fresh_anonymous_store_is_empty_and_a_refused_checkpoint_rolls_back_its_schema() {
    let mut refused = Connection::open_in_memory().unwrap();
    let error = call(
        &mut refused,
        &anonymous(),
        json!({"action": "checkpoint", "model": model("m", "rev:1", 1),
        "payload": {"byte_length": 4}}),
        Some(vec![1]),
    )
    .unwrap_err();
    assert!(error.starts_with("grid_model_store_invalid:"));
    let tables: i64 = refused
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        tables, 0,
        "the transaction must roll back even its schema work"
    );

    let empty = read(&mut refused, &anonymous(), json!({"action": "list_models"}));
    assert_eq!(empty.value["models"], json!([]));
    assert_eq!(empty.generation, Some(0));
    assert!(empty.bytes.is_none());
    assert_eq!(
        get(&mut refused, &anonymous(), "absent").value["model"],
        Value::Null
    );
}

#[test]
fn sqlite_reopen_retains_exact_bytes_metadata_and_principal_isolation() {
    let directory = std::env::current_dir()
        .unwrap()
        .join(".verification")
        .join("grid-model-store-tests");
    std::fs::create_dir_all(&directory).unwrap();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = directory.join(format!("{}-{unique}.sqlite", std::process::id()));
    let expected;
    {
        let mut connection = Connection::open(&path).unwrap();
        expected = checkpoint(
            &mut connection,
            &alice(),
            model("same-id", "rev:alice", 3),
            &[0, 255, 1, 4],
        )
        .value["model"]
            .clone();
        checkpoint(
            &mut connection,
            &bob(),
            model("same-id", "rev:bob", 4),
            &[8, 9],
        );
        checkpoint(
            &mut connection,
            &anonymous(),
            model("same-id", "rev:anonymous", 5),
            &[7],
        );
    }
    {
        let mut connection = Connection::open(&path).unwrap();
        let retained = get(&mut connection, &alice(), "same-id");
        assert_eq!(retained.value["model"], expected);
        assert_eq!(retained.bytes, Some(vec![0, 255, 1, 4]));
        assert_eq!(expected["size_bytes"], 4);
        assert_eq!(
            expected["content_digest"],
            durable_store::sha256(&[0, 255, 1, 4])
        );
        assert_eq!(
            get(&mut connection, &bob(), "same-id").bytes,
            Some(vec![8, 9])
        );
        assert_eq!(
            get(&mut connection, &anonymous(), "same-id").bytes,
            Some(vec![7])
        );
        let listed = read(&mut connection, &alice(), json!({"action": "list_models"}));
        assert_eq!(listed.value["models"], json!([expected]));
        assert!(
            listed.bytes.is_none(),
            "listing metadata must not acquire model bytes"
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn metadata_edits_preserve_bytes_and_detached_provenance_survives_a_checkpoint() {
    let mut connection = Connection::open_in_memory().unwrap();
    let original = checkpoint(
        &mut connection,
        &alice(),
        model("m", "rev:dirty", 1),
        &[3, 2, 1],
    );
    read(
        &mut connection,
        &alice(),
        json!({"action": "rename_model", "model_id": "m", "name": "  West feeder  "}),
    );
    read(
        &mut connection,
        &alice(),
        json!({"action": "bind_model", "model_id": "m", "binding": binding()}),
    );
    let mismatch = read(
        &mut connection,
        &alice(),
        json!({"action": "assign_mv_workspace", "model_id": "m", "project": "project-b", "project_model_id": "project-model"}),
    );
    assert_eq!(mismatch.value["assigned"], false);
    let assigned = read(
        &mut connection,
        &alice(),
        json!({"action": "assign_mv_workspace", "model_id": "m", "project": "project-a", "project_model_id": "project-model"}),
    );
    assert_eq!(assigned.value["assigned"], true);
    read(
        &mut connection,
        &alice(),
        json!({"action": "detach_models", "project": "project-a", "project_model_id": "project-model"}),
    );
    let detached = get(&mut connection, &alice(), "m");
    assert_eq!(detached.bytes, Some(vec![3, 2, 1]));
    let record = &detached.value["model"];
    assert_eq!(record["name"], "West feeder");
    assert_eq!(record["revision"], "rev:dirty");
    assert_eq!(record["updated"], 1);
    assert_eq!(record["workspace_owner"], "mv-design");
    assert!(record.get("project_binding").is_none());
    assert_eq!(record["retired_project_origin"], binding());
    assert_eq!(
        record["content_digest"],
        original.value["model"]["content_digest"]
    );
    checkpoint(
        &mut connection,
        &alice(),
        model("m", "rev:dirtier", 2),
        &[4, 5],
    );
    let latest = get(&mut connection, &alice(), "m");
    assert_eq!(latest.bytes, Some(vec![4, 5]));
    assert_eq!(latest.value["model"]["retired_project_origin"], binding());
    assert!(latest.value["model"].get("project_binding").is_none());
}

#[test]
fn removing_a_workspace_is_atomic_with_bytes_results_groups_and_sessions() {
    let mut connection = Connection::open_in_memory().unwrap();
    checkpoint(&mut connection, &alice(), model("m", "rev:1", 1), &[1, 2]);
    checkpoint(&mut connection, &alice(), model("other", "rev:2", 2), &[3]);
    read(
        &mut connection,
        &alice(),
        json!({"action": "put_result", "result": {"result_id": "r", "model_id": "m",
        "pinned_content_root": "rev:1", "created": 1, "artifact": {"result_id": "r"}}}),
    );
    let group = read(&mut connection, &alice(), json!({"action": "begin_group", "name": "Import",
        "source": {"filename": "m.dsgrid", "sha256": format!("sha256:{}", "a".repeat(64)), "byte_size": 2}, "created_from": "drop", "now": 1})).value["group"]["group_id"].clone();
    read(
        &mut connection,
        &alice(),
        json!({"action": "settle_group", "group_id": group, "status": {"kind": "ready"},
        "model": {"session_model_id": "m", "active_revision": "rev:1"}}),
    );
    read(
        &mut connection,
        &alice(),
        json!({"action": "put_session", "project": "project-a", "session": {
        "selected_model_id": "m", "selected_revision_id": "rev:1", "selected_provenance": {
            "imported_from": null, "template_origin": null, "example_origin": null, "version_origin": null,
            "read_only": false, "temp_group_id": null}, "view_state": {"layer_visibility": {}, "grouping_mode": "layer"},
        "last_selected": 1, "open_models": []}}),
    );
    connection.execute_batch("CREATE TRIGGER refuse_result_removal BEFORE DELETE ON grid_model_results BEGIN SELECT RAISE(ABORT,'retained work'); END;").unwrap();
    assert!(call(
        &mut connection,
        &alice(),
        json!({"action": "remove_model", "model_id": "m"}),
        None
    )
    .is_err());
    assert_eq!(
        get(&mut connection, &alice(), "m").bytes,
        Some(vec![1, 2]),
        "a later refusal restores earlier byte deletion"
    );
    assert_eq!(
        read(
            &mut connection,
            &alice(),
            json!({"action": "list_results", "model_id": "m"})
        )
        .value["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_ne!(
        read(
            &mut connection,
            &alice(),
            json!({"action": "get_session", "project": "project-a"})
        )
        .value["session"],
        Value::Null
    );
    connection
        .execute_batch("DROP TRIGGER refuse_result_removal")
        .unwrap();
    let removed = read(
        &mut connection,
        &alice(),
        json!({"action": "remove_model", "model_id": "m"}),
    );
    assert_eq!(removed.value["removed_group_ids"], json!([group]));
    assert_eq!(removed.value["removed_result_count"], 1);
    assert_eq!(
        get(&mut connection, &alice(), "m").value["model"],
        Value::Null
    );
    assert_eq!(
        read(
            &mut connection,
            &alice(),
            json!({"action": "get_group", "group_id": group})
        )
        .value["group"],
        Value::Null
    );
    assert_eq!(
        read(
            &mut connection,
            &alice(),
            json!({"action": "get_session", "project": "project-a"})
        )
        .value["session"],
        Value::Null
    );
    assert_eq!(get(&mut connection, &alice(), "other").bytes, Some(vec![3]));
}

#[test]
fn multipart_checkpoint_is_visible_only_when_complete_and_reads_fence_replacement() {
    let mut connection = Connection::open_in_memory().unwrap();
    checkpoint(
        &mut connection,
        &anonymous(),
        model("m", "rev:old", 1),
        &[1],
    );
    let part = vec![9; crate::byte_parts::PART_BYTES as usize];
    let mut operation = json!({"action": "checkpoint", "model": model("m", "rev:new", 2),
        "payload": {"byte_length": part.len() + 3, "part": 0}});
    let staged = call(
        &mut connection,
        &anonymous(),
        operation.clone(),
        Some(part.clone()),
    )
    .unwrap();
    assert_eq!(get(&mut connection, &anonymous(), "m").bytes, Some(vec![1]));
    operation["payload"]["part"] = json!(1);
    operation["payload"]["staging"] = staged.value["staged"]["staging"].clone();
    call(
        &mut connection,
        &anonymous(),
        operation,
        Some(vec![2, 3, 4]),
    )
    .unwrap();
    let first = get(&mut connection, &anonymous(), "m");
    assert_eq!(first.bytes, Some(part));
    assert_eq!(first.value["parts"], 2);
    let version = first.value["version"].clone();
    let last = read(
        &mut connection,
        &anonymous(),
        json!({"action": "get_model", "model_id": "m", "part": 1, "expect_version": version}),
    );
    assert_eq!(last.bytes, Some(vec![2, 3, 4]));
    checkpoint(
        &mut connection,
        &anonymous(),
        model("m", "rev:replaced", 3),
        &[5],
    );
    let refusal = call(
        &mut connection,
        &anonymous(),
        json!({"action": "get_model", "model_id": "m", "part": 1, "expect_version": version}),
        None,
    )
    .unwrap_err();
    assert!(refusal.contains("changed during its read"));
    assert_eq!(get(&mut connection, &anonymous(), "m").bytes, Some(vec![5]));
}

#[test]
fn corrupt_or_missing_authored_bytes_refuse_without_discarding_work() {
    let mut connection = Connection::open_in_memory().unwrap();
    checkpoint(&mut connection, &alice(), model("m", "rev:1", 1), &[1, 2]);
    connection
        .execute(
            "UPDATE grid_models_parts SET bytes=?1",
            params![vec![8u8, 8]],
        )
        .unwrap();
    let error = call(
        &mut connection,
        &alice(),
        json!({"action": "get_model", "model_id": "m"}),
        None,
    )
    .unwrap_err();
    assert!(error.contains("missing or corrupt"));
    assert_eq!(
        read(&mut connection, &alice(), json!({"action": "list_models"})).value["models"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let parts: i64 = connection
        .query_row("SELECT COUNT(*) FROM grid_models_parts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        parts, 1,
        "corrupt authored bytes must remain available for recovery"
    );
    connection
        .execute("DELETE FROM grid_models_blobs", [])
        .unwrap();
    let missing = call(
        &mut connection,
        &alice(),
        json!({"action": "get_model", "model_id": "m"}),
        None,
    )
    .unwrap_err();
    assert!(missing.contains("bytes are missing"));
    assert_eq!(
        read(&mut connection, &alice(), json!({"action": "list_models"})).value["models"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
