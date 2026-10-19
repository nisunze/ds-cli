use super::*;

#[test]
fn mv_picker_consumes_the_declared_held_inventory_envelope_and_refuses_other_answers() {
    let context = json!({"account":"owner","project":"project","generation":1});
    let inventory =
        json!({"captured_context":context,"context":context,"total":0,"rows":[],"mutated":false});
    let envelope =
        json!({"v":1,"command":"map.profile.view","status":"ok","data":{"held_models":inventory}});
    assert_eq!(held_inventory(envelope.clone()).unwrap().total, 0);
    assert_eq!(held_inventory(inventory).unwrap().total, 0);
    for (field, value) in [
        ("command", json!("another.command")),
        ("status", json!("error")),
    ] {
        let mut other = envelope.clone();
        other[field] = value;
        assert!(held_inventory(other).is_err());
    }
}

#[test]
fn mv_picker_refuses_mixed_modes_before_authentication_and_bounds_input_files() {
    let context = Context {
        confirmed: false,
        output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
    };
    for flags in [
        vec!["--mv-picker", "--cursor", "old"],
        vec!["--refresh"],
        vec!["--source-binding", "missing.json"],
    ] {
        let mut tokens: Vec<String> = vec![
            "--project".into(),
            "project".into(),
            "--lane".into(),
            "stable".into(),
        ];
        tokens.extend(flags.into_iter().map(str::to_owned));
        let inputs = ds_cli_contract::args::parse(&LIST, &tokens).unwrap();
        assert_eq!(
            list(&inputs, &context).unwrap_err().code(),
            "grid_project_output_invalid"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bounded.json");
    std::fs::write(&path, b"{\"a\":1}").unwrap();
    assert_eq!(owned_json(path.to_str().unwrap(), 7).unwrap()["a"], 1);
    assert!(owned_json(path.to_str().unwrap(), 6).is_err());
    let source_path = dir.path().join("source.json");
    std::fs::write(&source_path,serde_json::to_vec(&json!({"project_id":"project","model_id":"head","revision_id":"rev","content_sha256":"a".repeat(64),"version":1,"worker_id":"local","worker_model_id":"native","worker_revision":"rev:1","history_pin":"pin","coordinate_system":"EPSG:32736","read_only":false})).unwrap()).unwrap();
    let inputs = ds_cli_contract::args::parse(
        &LIST,
        &[
            "--project".into(),
            "project".into(),
            "--lane".into(),
            "stable".into(),
            "--mv-picker".into(),
            "--source-binding".into(),
            source_path.to_str().unwrap().into(),
        ],
    )
    .unwrap();
    assert_eq!(
        list(&inputs, &context).unwrap_err().code(),
        "grid_project_output_invalid"
    );
    #[cfg(unix)]
    {
        let link = dir.path().join("alias.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(owned_json(link.to_str().unwrap(), 100).is_err());
    }
}

const HEAD_DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn backup() -> Value {
    json!({
        "bucket": "ds-grid-model-backups",
        "object": format!("project-grid-models/v1/project-1/model-1/revision-1/{HEAD_DIGEST}.dsgrid"),
        "generation": 1_758_870_000_123_456_i64,
        "digest": HEAD_DIGEST,
        "byte_length": 4096
    })
}

/// The Server's `deletion` answer, as ds-brain writes it.
fn deletion(backup: Value) -> Value {
    json!({
        "project_id": "project-1",
        "model_id": "model-1",
        "head_revision_id": "revision-1",
        "head_model_digest": HEAD_DIGEST,
        "deleted_at": "2026-09-26T07:00:00Z",
        "revisions_retained": true,
        "model_object_retained": true,
        "backup": backup
    })
}

#[test]
fn retire_refuses_when_no_verified_backup_of_the_head_is_confirmed() {
    // The Server takes and verifies the backup before it retires the
    // head; when it cannot, it answers 5xx and the head stays active.
    // That reached the caller as an authentication hiccup.
    for cause in [
        Failure::unavailable(
            "auth_transient",
            "the native authentication service is temporarily unavailable",
        ),
        Failure::unavailable(
            "auth_response_unreadable",
            "the authentication or project response did not match its closed contract",
        ),
    ] {
        let refused = retired(Err(cause), HEAD_DIGEST).expect_err("no backup, no receipt");
        assert_eq!(refused.code(), RETIRE_BACKUP.code);
        assert_eq!(refused.remedy_text(), Some(RETIRE_BACKUP.remedy));
    }
    // An answer without the backup, or with another head's backup, is
    // never accepted as a retirement.
    let mut other = backup();
    other["digest"] = json!("2".repeat(64));
    let mut unwritten = backup();
    unwritten["generation"] = json!(0);
    let mut empty = backup();
    empty["byte_length"] = json!(0);
    for backup in [Value::Null, other, unwritten, empty] {
        let refused = retired(Ok(deletion(backup)), HEAD_DIGEST).expect_err("unverified backup");
        assert_eq!(refused.code(), RETIRE_BACKUP.code);
    }
    // The Server's own named refusals keep their names.
    let fenced = retired(
        Err(Failure::invalid("grid_request_refused", "moved head")),
        HEAD_DIGEST,
    )
    .expect_err("refused");
    assert_eq!(fenced.code(), "grid_request_refused");
    let declared = RETIRE
        .refusals
        .iter()
        .find(|r| r.code == RETIRE_BACKUP.code)
        .expect("retire declares the backup refusal");
    assert!(declared.when.contains("backup") && declared.remedy.len() > 10);
}

#[test]
fn retire_envelope_names_the_verified_backup_identity() {
    let envelope = retired(Ok(deletion(backup())), HEAD_DIGEST).expect("verified backup accepted");
    assert_eq!(envelope["backup"], backup());
    assert_eq!(envelope["head_model_digest"], HEAD_DIGEST);
    assert_eq!(envelope["revisions_retained"], true);
    for field in ["backup", "bucket", "object", "generation", "byte_length"] {
        assert!(
            RETIRE.output.contains(field),
            "retire's output contract must name `{field}`"
        );
    }
}
