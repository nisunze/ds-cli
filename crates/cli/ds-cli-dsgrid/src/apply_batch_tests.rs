use super::*;

fn row(id: &str) -> NativeGridBatchCommandV1 {
    NativeGridBatchCommandV1 {
        command_id: id.into(),
        command: json!({"command_kind": "describe_structure", "id": "structure-1", "description": "reviewed"}),
        review_ref: Some(format!("comment:{id}")),
    }
}

#[test]
fn cherry_pick_preserves_batch_order_and_refuses_unknown_ids() {
    let make = || vec![row("first"), row("second"), row("third")];
    let selected = select_commands(make(), &["third".into(), "first".into()]).unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|row| row.command_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "third"]
    );
    assert!(select_commands(make(), &["missing".into()]).is_err());
    assert!(select_commands(make(), &["first".into(), "first".into()]).is_err());
}

#[test]
fn guard_bytes_are_pinned_and_name_a_full_source_digest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guard.json");
    let guard = json!({
        "source_sha256": format!("sha256:{}", "a".repeat(64)),
        "alignment_ids": ["alignment-1"],
        "allowed_command_kinds": ["retype_structure"]
    });
    let bytes = serde_json::to_vec(&guard).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let (pinned, digest, document) = read_guard(path.to_str().unwrap()).unwrap();
    assert_eq!(pinned, bytes);
    assert_eq!(document, guard);
    assert_eq!(digest, sha256(&bytes));
    std::fs::write(&path, br#"{"source_sha256":"sha256:abc"}"#).unwrap();
    assert_eq!(
        read_guard(path.to_str().unwrap()).unwrap_err().code(),
        "correction_guard_invalid"
    );
    std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
    assert_eq!(
        read_guard(path.to_str().unwrap()).unwrap_err().code(),
        "correction_guard_invalid"
    );
}

/// The owner admits the guard document this host forwards unchanged:
/// the same typed scope the linked command parsed, unknown fields refused.
#[test]
fn forwarded_guard_documents_keep_the_owner_scope_shape() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Owner {
        source_sha256: String,
        #[serde(default)]
        review_state: Option<String>,
        #[serde(default)]
        decision_ref: Option<String>,
        #[serde(default)]
        preservation_source_ref: Option<String>,
        #[serde(flatten)]
        scope: ds_grid_engine::correction_scope::CorrectionScope,
    }
    let raw = json!({
        "source_sha256": format!("sha256:{}", "a".repeat(64)),
        "alignment_ids": ["alignment-1"],
        "allowed_command_kinds": ["retype_structure"]
    });
    let owner: Owner = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(owner.scope.alignment_ids.len(), 1);
    let mut extra = raw;
    extra["allow_unscoped"] = json!(true);
    assert!(serde_json::from_value::<Owner>(extra).is_err());
}
