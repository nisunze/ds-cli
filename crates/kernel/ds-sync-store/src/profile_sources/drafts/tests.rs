use super::*;

const SOURCE_BYTES: &[u8] = b"exact synthetic source";
// Opaque storage must not parse/normalize signed zero or large numeric tokens.
const CHECKPOINT: &[u8] =
    br#"{ "value": -0.0, "integer": 9007199254740993, "opaque": "checkpoint" }"#;
pub(super) fn private_directory() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    temp
}
pub(super) fn source(n: u8) -> ProfileSourceHeader {
    ProfileSourceHeader {
        scope: ProfileSourceScope {
            principal: "owner".into(),
            lane: "canary".into(),
            deployment: "fixture".into(),
            install: "install".into(),
            project: "project".into(),
        },
        schema: "ds.core.profile-source/v1".into(),
        session_id: format!("{n:032x}"),
        model_id: "model".into(),
        package_sha256: sha256_hex(SOURCE_BYTES),
        package_bytes: SOURCE_BYTES.len() as u64,
        initial_history: serde_json::json!({"model_revision":"rev","initial_revision":"rev","undo_depth":0,"redo_depth":0,"history_pin":"pin"}),
        source_revision: 1,
        coordinate_system: "EPSG:3395".into(),
        engine: serde_json::json!({"selected_sha256":"a".repeat(64),"protocol":1}),
    }
}
fn history(n: u64) -> Value {
    serde_json::json!({"model_revision":format!("revision-{n}"),"initial_revision":"rev","undo_depth":n,"redo_depth":0,"history_pin":format!("pin-{n}")})
}
fn draft(source: &ProfileSourceHeader, history: Value, bytes: &[u8]) -> ProfileDraftHeader {
    ProfileDraftHeader {
        schema: "ds.core.profile-draft/v1".into(),
        source: source.clone(),
        history,
        checkpoint_sha256: sha256_hex(bytes),
        checkpoint_bytes: bytes.len() as u64,
    }
}
pub(super) fn retain_source(store: &mut Store, source: &ProfileSourceHeader) {
    store
        .retain_profile_source(&source.scope, source, SOURCE_BYTES, &[], 1)
        .unwrap();
}
pub(super) fn retain_draft(store: &mut Store, source: &ProfileSourceHeader) -> ProfileDraftHeader {
    let header = draft(source, history(1), CHECKPOINT);
    store
        .retain_profile_draft(
            &source.scope,
            source,
            &source.initial_history,
            &header,
            CHECKPOINT,
            2,
        )
        .unwrap();
    header
}
fn opaque(draft: Option<ProfileDraft>) -> Option<(ProfileDraftHeader, Vec<u8>)> {
    draft.map(|draft| match draft {
        ProfileDraft::Checkpoint { header, bytes } => (header, bytes),
        ProfileDraft::Journal(_) => panic!("expected an opaque v1 checkpoint"),
    })
}
fn recovered(store: &Store, source: &ProfileSourceHeader) -> ProfileRecovery {
    store
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
        .unwrap()
}
fn row_count(store: &Store, table: &str) -> i64 {
    store
        .connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn assert_refuses_all(
    store: &mut Store,
    source: &ProfileSourceHeader,
    header: &ProfileDraftHeader,
) {
    assert!(store
        .profile_recovery(&source.scope, &source.session_id)
        .is_err());
    assert!(store
        .profile_source(&source.scope, &source.session_id)
        .is_err());
    assert!(store
        .touch_profile_source(&source.scope, source, 8)
        .is_err());
    assert!(store.remove_profile_source(&source.scope, source).is_err());
    assert!(store
        .remove_profile_recovery(&source.scope, source, &header.history)
        .is_err());
    assert!(store
        .retain_profile_draft(
            &source.scope,
            source,
            &header.history,
            header,
            CHECKPOINT,
            8
        )
        .is_err());
    assert!(store
        .retain_profile_source(&source.scope, source, SOURCE_BYTES, &[], 8)
        .is_err());
    assert_eq!(row_count(store, "profile_sources"), 1);
}

#[test]
fn absence_reads_do_not_create_schema_or_modify_other_owners() {
    let store = Store::in_memory().unwrap();
    let source = source(1);
    assert!(store
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
        .is_none());
    assert!(!has_table(&store.connection, "profile_sources").unwrap());
    assert!(!has_table(&store.connection, "profile_drafts").unwrap());
    for table in ["artifacts", "heads", "receipts", "leases"] {
        assert_eq!(row_count(&store, table), 0);
    }
}

#[test]
fn exact_opaque_bytes_initial_checkpoint_replacement_and_reopen() {
    let temp = private_directory();
    let path = temp.path().join("store.sqlite");
    let source = source(1);
    let initial = draft(&source, source.initial_history.clone(), CHECKPOINT);
    {
        let mut store = Store::open(&path).unwrap();
        retain_source(&mut store, &source);
        let original = recovered(&store, &source);
        assert_eq!(original.source_bytes, SOURCE_BYTES);
        assert!(original.draft.is_none());
        store
            .retain_profile_draft(
                &source.scope,
                &source,
                &source.initial_history,
                &initial,
                CHECKPOINT,
                2,
            )
            .unwrap();
    }
    let readonly = Store::open_read_only(&path).unwrap().unwrap();
    assert_eq!(
        opaque(recovered(&readonly, &source).draft),
        Some((initial.clone(), CHECKPOINT.to_vec()))
    );
    drop(readonly);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        opaque(recovered(&store, &source).draft),
        Some((initial.clone(), CHECKPOINT.to_vec()))
    );
    assert!(store
        .profile_source(&source.scope, &source.session_id)
        .is_err());
    assert!(store
        .touch_profile_source(&source.scope, &source, 3)
        .is_err());
    assert!(store.remove_profile_source(&source.scope, &source).is_err());
    assert!(store
        .retain_profile_source(&source.scope, &source, SOURCE_BYTES, &[], 3)
        .is_err());
    let next_bytes = b"\0opaque\xff authored checkpoint";
    let next = draft(&source, history(1), next_bytes);
    store
        .retain_profile_draft(
            &source.scope,
            &source,
            &initial.history,
            &next,
            next_bytes,
            3,
        )
        .unwrap();
    let result = recovered(&store, &source);
    assert_eq!(result.source, source);
    assert_eq!(result.source_bytes, SOURCE_BYTES);
    assert_eq!(
        opaque(result.draft),
        Some((next.clone(), next_bytes.to_vec()))
    );
    assert_eq!(row_count(&store, "profile_drafts"), 1);
    assert_eq!(crate::schema_version(&store.connection).unwrap(), Some(2));
    for table in ["artifacts", "heads", "receipts", "leases"] {
        assert_eq!(row_count(&store, table), 0);
    }
    store
        .remove_profile_recovery(&source.scope, &source, &next.history)
        .unwrap();
    assert_eq!(row_count(&store, "profile_drafts"), 0);
    assert_eq!(row_count(&store, "profile_sources"), 0);
}

#[test]
fn every_history_field_is_an_exact_cas_fence() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let first = retain_draft(&mut store, &source);
    let next = draft(&source, history(2), CHECKPOINT);
    for key in [
        "model_revision",
        "initial_revision",
        "undo_depth",
        "redo_depth",
        "history_pin",
    ] {
        let mut stale = first.history.clone();
        stale[key] = if key.ends_with("depth") {
            9.into()
        } else {
            "changed".into()
        };
        assert!(
            store
                .retain_profile_draft(&source.scope, &source, &stale, &next, CHECKPOINT, 3)
                .is_err(),
            "{key}"
        );
        assert!(
            store
                .remove_profile_recovery(&source.scope, &source, &stale)
                .is_err(),
            "{key}"
        );
        assert_eq!(opaque(recovered(&store, &source).draft).unwrap().0, first);
    }
    store
        .retain_profile_draft(&source.scope, &source, &first.history, &next, CHECKPOINT, 3)
        .unwrap();
    assert!(store
        .retain_profile_draft(&source.scope, &source, &first.history, &next, CHECKPOINT, 4)
        .is_err());
    assert!(store
        .remove_profile_recovery(&source.scope, &source, &first.history)
        .is_err());
    store
        .remove_profile_recovery(&source.scope, &source, &next.history)
        .unwrap();
}

#[test]
fn scopes_and_full_source_engine_model_pins_are_fenced() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let first = retain_draft(&mut store, &source);
    for field in 0..5 {
        let mut scope = source.scope.clone();
        match field {
            0 => scope.principal = "other".into(),
            1 => scope.lane = "stable".into(),
            2 => scope.deployment = "other".into(),
            3 => scope.install = "other".into(),
            _ => scope.project = "other".into(),
        }
        assert!(store
            .profile_recovery(&scope, &source.session_id)
            .unwrap()
            .is_none());
        assert!(store
            .retain_profile_draft(&scope, &source, &first.history, &first, CHECKPOINT, 3)
            .is_err());
        assert!(store
            .remove_profile_recovery(&scope, &source, &first.history)
            .is_err());
    }
    for field in 0..6 {
        let mut wrong = source.clone();
        match field {
            0 => wrong.engine["selected_sha256"] = "b".repeat(64).into(),
            1 => wrong.model_id = "other".into(),
            2 => wrong.coordinate_system = "other".into(),
            3 => wrong.source_revision += 1,
            4 => wrong.initial_history["history_pin"] = "other".into(),
            _ => wrong.package_sha256 = "0".repeat(64),
        }
        let replacement = draft(&wrong, history(2), CHECKPOINT);
        assert!(store
            .retain_profile_draft(
                &wrong.scope,
                &wrong,
                &first.history,
                &replacement,
                CHECKPOINT,
                3
            )
            .is_err());
        assert!(store
            .remove_profile_recovery(&wrong.scope, &wrong, &first.history)
            .is_err());
    }
    assert_eq!(opaque(recovered(&store, &source).draft).unwrap().0, first);
}

#[test]
fn invalid_checkpoint_headers_and_bytes_refuse_before_invalidation() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    for field in 0..11 {
        let mut header = draft(&source, history(1), CHECKPOINT);
        match field {
            0 => header.schema = "future".into(),
            1 => {
                header
                    .history
                    .as_object_mut()
                    .unwrap()
                    .remove("history_pin");
            }
            2 => header.history["extra"] = true.into(),
            3 => header.history["undo_depth"] = (-1).into(),
            4 => header.history["initial_revision"] = "other".into(),
            5 => header.history["history_pin"] = "".into(),
            6 => header.checkpoint_sha256 = "A".repeat(64),
            7 => header.checkpoint_bytes = 0,
            8 => header.checkpoint_bytes = MAX_PROFILE_CHECKPOINT_BYTES as u64 + 1,
            9 => header.checkpoint_sha256 = "0".repeat(64),
            _ => header.history["redo_depth"] = "0".into(),
        }
        assert!(
            store
                .retain_profile_draft(
                    &source.scope,
                    &source,
                    &source.initial_history,
                    &header,
                    CHECKPOINT,
                    2
                )
                .is_err(),
            "{field}"
        );
        assert!(store
            .profile_source(&source.scope, &source.session_id)
            .unwrap()
            .is_some());
        assert!(!has_table(&store.connection, "profile_drafts").unwrap());
    }
    let header = draft(&source, history(1), CHECKPOINT);
    assert!(store
        .retain_profile_draft(
            &source.scope,
            &source,
            &source.initial_history,
            &header,
            b"changed",
            2
        )
        .is_err());
    assert!(store
        .profile_source(&source.scope, &source.session_id)
        .unwrap()
        .is_some());
}

#[test]
fn ineligible_original_without_checkpoint_cannot_be_repaired_or_closed() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    store
        .invalidate_profile_source(&source.scope, &source.session_id)
        .unwrap();
    let header = draft(&source, history(1), CHECKPOINT);
    assert_refuses_all(&mut store, &source, &header);
    assert!(store
        .retain_profile_draft(
            &source.scope,
            &source,
            &source.initial_history,
            &header,
            CHECKPOINT,
            2
        )
        .is_err());
    assert!(!has_table(&store.connection, "profile_drafts").unwrap());
}

#[test]
fn corrupt_or_inconsistent_drafts_never_fall_back_replace_or_erase_evidence() {
    for kind in 0..8 {
        let mut store = Store::in_memory().unwrap();
        let source = source(1);
        retain_source(&mut store, &source);
        let header = retain_draft(&mut store, &source);
        match kind {
            0 => {
                store
                    .connection
                    .execute("UPDATE profile_drafts SET checkpoint=x'00'", [])
                    .unwrap();
            }
            1 => {
                store
                    .connection
                    .execute("UPDATE profile_drafts SET header='{}'", [])
                    .unwrap();
            }
            2 => {
                store
                    .connection
                    .execute("DELETE FROM profile_drafts", [])
                    .unwrap();
            }
            3 => {
                store
                    .connection
                    .execute("UPDATE profile_sources SET eligible=1", [])
                    .unwrap();
            }
            4 => {
                store
                    .connection
                    .execute("UPDATE profile_sources SET package=x'00'", [])
                    .unwrap();
            }
            5 => {
                let mut wrong = header.clone();
                wrong.source.engine["selected_sha256"] = "b".repeat(64).into();
                let raw = serde_json::to_string(&wrong).unwrap();
                store
                    .connection
                    .execute(
                        "UPDATE profile_drafts SET header=?1,header_sha256=?2",
                        params![raw, sha256_hex(raw.as_bytes())],
                    )
                    .unwrap();
            }
            6 => {
                let mut wrong = source.clone();
                wrong.initial_history["history_pin"] = "changed".into();
                let raw = serde_json::to_string(&wrong).unwrap();
                store
                    .connection
                    .execute(
                        "UPDATE profile_sources SET header=?1,header_sha256=?2",
                        params![raw, sha256_hex(raw.as_bytes())],
                    )
                    .unwrap();
            }
            _ => {
                store
                    .connection
                    .execute(
                        "UPDATE profile_drafts SET checkpoint=zeroblob(?1)",
                        [MAX_PROFILE_CHECKPOINT_BYTES + 1],
                    )
                    .unwrap();
            }
        }
        assert_refuses_all(&mut store, &source, &header);
        assert_eq!(
            row_count(&store, "profile_drafts"),
            if kind == 2 { 0 } else { 1 }
        );
    }
}

#[test]
fn failed_first_retention_replacement_and_deletion_roll_back_both_rows() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    store.connection.execute_batch(SCHEMA).unwrap();
    store.connection.execute_batch("CREATE TRIGGER reject_checkpoint BEFORE INSERT ON profile_drafts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let first = draft(&source, history(1), CHECKPOINT);
    assert!(store
        .retain_profile_draft(
            &source.scope,
            &source,
            &source.initial_history,
            &first,
            CHECKPOINT,
            2
        )
        .is_err());
    assert!(recovered(&store, &source).draft.is_none());
    assert_eq!(row_count(&store, "profile_drafts"), 0);
    store
        .connection
        .execute_batch("DROP TRIGGER reject_checkpoint;")
        .unwrap();
    retain_draft(&mut store, &source);
    let before = recovered(&store, &source);
    store.connection.execute_batch("CREATE TRIGGER reject_replacement BEFORE UPDATE ON profile_drafts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let next = draft(&source, history(2), b"next");
    assert!(store
        .retain_profile_draft(&source.scope, &source, &first.history, &next, b"next", 99)
        .is_err());
    assert_eq!(recovered(&store, &source), before);
    assert_eq!(
        store
            .connection
            .query_row("SELECT touched FROM profile_sources", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    store.connection.execute_batch("CREATE TRIGGER reject_removal BEFORE DELETE ON profile_sources BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(store
        .remove_profile_recovery(&source.scope, &source, &first.history)
        .is_err());
    assert_eq!(recovered(&store, &source), before);
    assert_eq!(row_count(&store, "profile_drafts"), 1);
    store
        .connection
        .execute_batch("DROP TRIGGER reject_removal;")
        .unwrap();
    store
        .remove_profile_recovery(&source.scope, &source, &first.history)
        .unwrap();
    assert_eq!(row_count(&store, "profile_sources"), 0);
    assert_eq!(row_count(&store, "profile_drafts"), 0);
}

#[test]
fn independent_connections_cannot_overwrite_or_remove_a_newer_history() {
    let temp = private_directory();
    let path = temp.path().join("store.sqlite");
    let source = source(1);
    let mut first = Store::open(&path).unwrap();
    retain_source(&mut first, &source);
    let header = retain_draft(&mut first, &source);
    let mut second = Store::open(&path).unwrap();
    let previous = opaque(recovered(&second, &source).draft).unwrap().0;
    let next = draft(&source, history(2), b"next");
    first
        .retain_profile_draft(&source.scope, &source, &header.history, &next, b"next", 3)
        .unwrap();
    assert!(second
        .retain_profile_draft(
            &source.scope,
            &source,
            &previous.history,
            &header,
            CHECKPOINT,
            4
        )
        .is_err());
    assert!(second
        .remove_profile_recovery(&source.scope, &source, &previous.history)
        .is_err());
    assert_eq!(
        opaque(recovered(&second, &source).draft).unwrap(),
        (next, b"next".to_vec())
    );
}

#[test]
fn capacity_protects_drafts_even_if_eligibility_is_tampered() {
    let mut store = Store::in_memory().unwrap();
    for n in 1..=8 {
        retain_source(&mut store, &source(n));
    }
    let first = source(1);
    retain_draft(&mut store, &first);
    store
        .connection
        .execute(
            "UPDATE profile_sources SET eligible=1 WHERE session_id=?1",
            [&first.session_id],
        )
        .unwrap();
    let active: Vec<_> = (2..=8).map(|n| source(n).session_id).collect();
    let next = source(9);
    assert!(store
        .retain_profile_source(&next.scope, &next, SOURCE_BYTES, &active, 9)
        .is_err());
    assert_eq!(row_count(&store, "profile_sources"), 8);
    assert_eq!(row_count(&store, "profile_drafts"), 1);
    store
        .retain_profile_source(&next.scope, &next, SOURCE_BYTES, &active[1..], 9)
        .unwrap();
    assert!(store
        .profile_source(&first.scope, &source(2).session_id)
        .unwrap()
        .is_none());
    assert!(store
        .profile_recovery(&first.scope, &first.session_id)
        .is_err());
    assert_eq!(row_count(&store, "profile_drafts"), 1);
}

#[test]
fn orphan_checkpoint_refuses_and_cannot_be_blessed_by_reopening_source() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let header = retain_draft(&mut store, &source);
    store
        .connection
        .execute("DELETE FROM profile_sources", [])
        .unwrap();
    assert!(store
        .profile_recovery(&source.scope, &source.session_id)
        .is_err());
    assert!(store
        .retain_profile_source(&source.scope, &source, SOURCE_BYTES, &[], 3)
        .is_err());
    assert!(store
        .remove_profile_recovery(&source.scope, &source, &header.history)
        .is_err());
    assert_eq!(row_count(&store, "profile_drafts"), 1);
}

#[test]
fn exact_maximum_checkpoint_is_admitted_but_larger_is_not() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let mut bytes = vec![b'x'; MAX_PROFILE_CHECKPOINT_BYTES];
    let header = draft(&source, history(1), &bytes);
    store
        .retain_profile_draft(
            &source.scope,
            &source,
            &source.initial_history,
            &header,
            &bytes,
            2,
        )
        .unwrap();
    assert_eq!(opaque(recovered(&store, &source).draft).unwrap().1, bytes);
    bytes.push(b'y');
    let oversized = draft(&source, history(2), &bytes);
    assert!(store
        .retain_profile_draft(
            &source.scope,
            &source,
            &header.history,
            &oversized,
            &bytes,
            3
        )
        .is_err());
    assert_eq!(opaque(recovered(&store, &source).draft).unwrap().0, header);
}

#[test]
fn readonly_original_can_be_removed_only_with_its_full_initial_history() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    assert!(store
        .remove_profile_recovery(&source.scope, &source, &history(1))
        .is_err());
    store
        .remove_profile_recovery(&source.scope, &source, &source.initial_history)
        .unwrap();
    assert!(store
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
        .is_none());
    assert!(!has_table(&store.connection, "profile_drafts").unwrap());
}

fn retain_other_source(store: &mut Store, n: u8) -> ProfileSourceHeader {
    let mut other = source(n);
    let bytes = b"different original";
    other.package_bytes = bytes.len() as u64;
    other.package_sha256 = sha256_hex(bytes);
    store
        .retain_profile_source(&other.scope, &other, bytes, &[], 1)
        .unwrap();
    other
}

#[test]
fn package_lookup_is_draft_only_unique_and_scoped_without_touching_rows() {
    let mut store = Store::in_memory().unwrap();
    let first = source(1);
    assert!(store
        .profile_draft_for_package(&first.scope, &first.package_sha256)
        .unwrap()
        .is_none());
    assert!(!has_table(&store.connection, "profile_sources").unwrap());
    retain_source(&mut store, &first);
    retain_source(&mut store, &source(2));
    assert!(store
        .profile_draft_for_package(&first.scope, &first.package_sha256)
        .unwrap()
        .is_none());
    let draft = retain_draft(&mut store, &first);
    let other = retain_other_source(&mut store, 3);
    // An unrelated old dirty original remains unavailable, but cannot claim
    // this package or prevent its exact draft from being found.
    store
        .invalidate_profile_source(&other.scope, &other.session_id)
        .unwrap();
    let found = store
        .profile_draft_for_package(&first.scope, &first.package_sha256)
        .unwrap()
        .unwrap();
    assert_eq!(found.source, first);
    assert_eq!(opaque(found.draft), Some((draft, CHECKPOINT.to_vec())));
    for field in 0..5 {
        let mut scope = first.scope.clone();
        match field {
            0 => scope.principal = "other".into(),
            1 => scope.lane = "stable".into(),
            2 => scope.deployment = "other".into(),
            3 => scope.install = "other".into(),
            _ => scope.project = "other".into(),
        }
        assert!(store
            .profile_draft_for_package(&scope, &first.package_sha256)
            .unwrap()
            .is_none());
    }
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT touched FROM profile_sources WHERE session_id=?1",
                [&first.session_id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(row_count(&store, "profile_sources"), 3);
    assert_eq!(row_count(&store, "profile_drafts"), 1);
}

#[test]
fn package_lookup_never_selects_a_newest_ambiguous_draft() {
    let mut store = Store::in_memory().unwrap();
    let first = source(1);
    let second = source(2);
    retain_source(&mut store, &first);
    retain_source(&mut store, &second);
    retain_draft(&mut store, &first);
    retain_draft(&mut store, &second);
    store
        .connection
        .execute(
            "UPDATE profile_drafts SET touched=999 WHERE session_id=?1",
            [&second.session_id],
        )
        .unwrap();
    let error = store
        .profile_draft_for_package(&first.scope, &first.package_sha256)
        .unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
    assert_eq!(row_count(&store, "profile_sources"), 2);
    assert_eq!(row_count(&store, "profile_drafts"), 2);
}

#[test]
fn package_lookup_refuses_dirty_missing_corrupt_and_orphaned_evidence() {
    for kind in 0..8 {
        let mut store = Store::in_memory().unwrap();
        let first = source(1);
        retain_source(&mut store, &first);
        // A clean duplicate must not turn a bad matching record into None.
        retain_source(&mut store, &source(2));
        let other = retain_other_source(&mut store, 3);
        if kind == 0 {
            store
                .invalidate_profile_source(&first.scope, &first.session_id)
                .unwrap();
        } else {
            retain_draft(&mut store, &first);
            match kind {
                1 => {
                    store
                        .connection
                        .execute("UPDATE profile_drafts SET checkpoint=x'00'", [])
                        .unwrap();
                }
                2 => {
                    store
                        .connection
                        .execute("DELETE FROM profile_drafts", [])
                        .unwrap();
                }
                3 => {
                    store
                        .connection
                        .execute(
                            "UPDATE profile_sources SET header='{}' WHERE session_id=?1",
                            [&other.session_id],
                        )
                        .unwrap();
                }
                4 => {
                    retain_draft(&mut store, &other);
                    store
                        .connection
                        .execute(
                            "DELETE FROM profile_sources WHERE session_id=?1",
                            [&other.session_id],
                        )
                        .unwrap();
                }
                5 => {
                    store
                        .connection
                        .execute(
                            "UPDATE profile_sources SET eligible=1 WHERE session_id=?1",
                            [&first.session_id],
                        )
                        .unwrap();
                }
                6 => {
                    store
                        .connection
                        .execute(
                            "UPDATE profile_sources SET package=x'00' WHERE session_id=?1",
                            [&first.session_id],
                        )
                        .unwrap();
                }
                _ => {
                    retain_draft(&mut store, &other);
                    store
                        .connection
                        .execute(
                            "UPDATE profile_drafts SET header='{}' WHERE session_id=?1",
                            [&other.session_id],
                        )
                        .unwrap();
                }
            }
        }
        assert!(
            store
                .profile_draft_for_package(&first.scope, &first.package_sha256)
                .is_err(),
            "kind {kind}"
        );
        assert!(store
            .profile_source(&first.scope, &source(2).session_id)
            .unwrap()
            .is_some());
    }
}

#[test]
fn package_lookup_refuses_invalid_digest_and_logical_capacity_overflow() {
    let mut store = Store::in_memory().unwrap();
    let first = source(1);
    for digest in [
        "".to_owned(),
        "A".repeat(64),
        "x".repeat(64),
        "0".repeat(65),
    ] {
        assert!(store
            .profile_draft_for_package(&first.scope, &digest)
            .is_err());
    }
    for n in 1..=8 {
        retain_source(&mut store, &source(n));
    }
    let extra = source(9);
    let raw = header_bytes(&extra).unwrap();
    store
        .connection
        .execute(
            "INSERT INTO profile_sources VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,9)",
            params![
                extra.scope.principal,
                extra.scope.lane,
                extra.scope.deployment,
                extra.scope.install,
                extra.scope.project,
                extra.session_id,
                raw,
                sha256_hex(raw.as_bytes()),
                SOURCE_BYTES
            ],
        )
        .unwrap();
    let error = store
        .profile_draft_for_package(&first.scope, &first.package_sha256)
        .unwrap_err();
    assert!(error.to_string().contains("capacity"));
    assert_eq!(row_count(&store, "profile_sources"), 9);
}
