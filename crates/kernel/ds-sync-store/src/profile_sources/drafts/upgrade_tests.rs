//! Engine re-pinning of retained recovery: only the engine changes, v1
//! checkpoints become the verified journal, and stale or crossed fences refuse.
use super::tests::{retain_draft, retain_source, source};
use super::*;

const E1: &[u8] = br#"{"command_id":"a","value":-0.0}"#;
const E2: &[u8] = br#"{"command_id":"b"}"#;

fn history(undo: u64) -> Value {
    serde_json::json!({"model_revision":format!("revision-{undo}"),"initial_revision":"rev",
        "undo_depth":undo,"redo_depth":0,"history_pin":format!("pin-{undo}")})
}
fn repinned(source: &ProfileSourceHeader) -> ProfileSourceHeader {
    let mut upgraded = source.clone();
    upgraded.engine = serde_json::json!({"selected_sha256":"b".repeat(64),"protocol":1});
    upgraded
}
fn journal(
    source: &ProfileSourceHeader,
    history: Value,
    entries: &[&[u8]],
) -> (ProfileJournalDraftHeader, Vec<u8>) {
    let metadata = serde_json::to_vec(&serde_json::json!({"history":history})).unwrap();
    (
        ProfileJournalDraftHeader {
            schema: PROFILE_JOURNAL_DRAFT_SCHEMA.into(),
            source: source.clone(),
            history,
            entry_count: entries.len() as u64,
            entry_bytes: entries.iter().map(|e| e.len() as u64).sum(),
            chain_sha256: sha256_hex(&entries.concat()),
            metadata_sha256: sha256_hex(&metadata),
            metadata_bytes: metadata.len() as u64,
        },
        metadata,
    )
}
fn recovered(store: &Store, source: &ProfileSourceHeader) -> ProfileRecovery {
    store
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
        .unwrap()
}

#[test]
fn opaque_checkpoint_upgrades_to_the_new_engine_journal_atomically() {
    let directory = super::tests::private_directory();
    let path = directory.path().join("store.sqlite");
    let mut store = Store::open(&path).unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let v1 = retain_draft(&mut store, &source);
    let upgraded = repinned(&source);
    let (header, metadata) = journal(&upgraded, v1.history.clone(), &[E1]);
    store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &v1.history,
            &upgraded,
            Some((&header, &metadata, &[E1])),
            10,
        )
        .unwrap();
    let recovery = recovered(&store, &upgraded);
    assert_eq!(recovery.source, upgraded);
    match recovery.draft.unwrap() {
        ProfileDraft::Journal(draft) => {
            assert_eq!(draft.header, header);
            assert_eq!(draft.metadata, metadata);
            assert_eq!(draft.entries.len(), 1);
            assert_eq!(draft.entries[0].bytes, E1);
        }
        ProfileDraft::Checkpoint { .. } => panic!("the opaque checkpoint must be replaced"),
    }
    // The former pin is no longer the retained original.
    let (next, next_metadata) = journal(&source, history(2), &[E1, E2]);
    assert!(
        store
            .retain_profile_journal_draft(
                &source.scope,
                &source,
                &v1.history,
                Some(&header.chain_sha256),
                &next,
                &next_metadata,
                1,
                &[E2],
                11
            )
            .is_err()
    );
    let (next, next_metadata) = journal(&upgraded, history(2), &[E1, E2]);
    store
        .retain_profile_journal_draft(
            &upgraded.scope,
            &upgraded,
            &v1.history,
            Some(&header.chain_sha256),
            &next,
            &next_metadata,
            1,
            &[E2],
            12,
        )
        .unwrap();
    // A selected compatible engine may recover an already-authored journal.
    let mut again = repinned(&upgraded);
    again.engine = serde_json::json!({"selected_sha256":"c".repeat(64),"protocol":1});
    let (header, metadata) = journal(&again, history(2), &[E1, E2]);
    store
        .upgrade_profile_recovery(
            &upgraded.scope,
            &upgraded,
            &next.history,
            &again,
            Some((&header, &metadata, &[E1, E2])),
            13,
        )
        .unwrap();
    drop(store);
    let store = Store::open_read_only(&path).unwrap().unwrap();
    let recovery = recovered(&store, &again);
    assert_eq!(recovery.source.model_id, source.model_id);
    assert_eq!(recovery.source.coordinate_system, source.coordinate_system);
    assert_eq!(recovery.source.source_revision, source.source_revision);
    let Some(ProfileDraft::Journal(draft)) = recovery.draft else {
        panic!("the authored recovery must survive reopen")
    };
    assert_eq!(draft.header.source, again);
    assert_eq!(draft.header.history, history(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&draft.metadata).unwrap(),
        serde_json::from_slice::<Value>(&next_metadata).unwrap()
    );
    assert_eq!(
        draft
            .entries
            .iter()
            .map(|entry| serde_json::from_slice::<Value>(&entry.bytes).unwrap())
            .collect::<Vec<_>>(),
        vec![
            serde_json::from_slice::<Value>(E1).unwrap(),
            serde_json::from_slice::<Value>(E2).unwrap()
        ]
    );
}

#[test]
fn authored_rebind_refuses_changed_commands_metadata_and_history_without_writes() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let (retained_header, metadata) = journal(&source, history(2), &[E1, E2]);
    store
        .retain_profile_journal_draft(
            &source.scope,
            &source,
            &source.initial_history,
            None,
            &retained_header,
            &metadata,
            0,
            &[E1, E2],
            1,
        )
        .unwrap();
    let before = recovered(&store, &source);
    let upgraded = repinned(&source);
    let (header, _) = journal(&upgraded, history(2), &[E1, E2]);
    let altered_command = br#"{"command_id":"b","value":12}"#.as_slice();
    let (changed_rows, _) = journal(&upgraded, history(2), &[E1, altered_command]);
    let (changed_history, changed_metadata) = journal(&upgraded, history(3), &[E1, E2]);
    let mut changed_chain = header.clone();
    changed_chain.chain_sha256 = "f".repeat(64);
    let altered_metadata =
        serde_json::to_vec(&serde_json::json!({"history":history(2),"extra":true})).unwrap();
    let mut changed_metadata_header = header.clone();
    changed_metadata_header.metadata_sha256 = sha256_hex(&altered_metadata);
    changed_metadata_header.metadata_bytes = altered_metadata.len() as u64;
    for (candidate, metadata, entries, expected) in [
        (
            &changed_rows,
            metadata.as_slice(),
            vec![E1, altered_command],
            history(2),
        ),
        (
            &changed_history,
            changed_metadata.as_slice(),
            vec![E1, E2],
            history(2),
        ),
        (
            &changed_chain,
            metadata.as_slice(),
            vec![E1, E2],
            history(2),
        ),
        (
            &changed_metadata_header,
            altered_metadata.as_slice(),
            vec![E1, E2],
            history(2),
        ),
        (&header, metadata.as_slice(), vec![E1, E2], history(1)),
    ] {
        assert!(
            store
                .upgrade_profile_recovery(
                    &source.scope,
                    &source,
                    &expected,
                    &upgraded,
                    Some((candidate, metadata, &entries)),
                    2
                )
                .is_err()
        );
        assert_eq!(
            recovered(&store, &source),
            before,
            "a refused rebind must leave the original retained state unchanged"
        );
    }
}

#[test]
fn upgrade_refuses_crossed_sources_fences_and_rows_without_writes() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let v1 = retain_draft(&mut store, &source);
    let before = recovered(&store, &source);
    let upgraded = repinned(&source);
    let (header, metadata) = journal(&upgraded, v1.history.clone(), &[E1]);
    let mut renamed = upgraded.clone();
    renamed.model_id = "another-model".into();
    let (renamed_header, renamed_metadata) = journal(&renamed, v1.history.clone(), &[E1]);
    let mut wrong_history = header.clone();
    wrong_history.history = history(1);
    wrong_history.history["history_pin"] = serde_json::json!("other-pin");
    let mut tampered = metadata.clone();
    tampered.push(b' ');
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &v1.history,
            &source,
            Some((&header, &metadata, &[E1])),
            1
        )
        .is_err());
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &v1.history,
            &renamed,
            Some((&renamed_header, &renamed_metadata, &[E1])),
            1
        )
        .is_err());
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &source.initial_history,
            &upgraded,
            Some((&header, &metadata, &[E1])),
            1
        )
        .is_err());
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &v1.history,
            &upgraded,
            Some((&wrong_history, &metadata, &[E1])),
            1
        )
        .is_err());
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &v1.history,
            &upgraded,
            Some((&header, &tampered, &[E1])),
            1
        )
        .is_err());
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &v1.history,
            &upgraded,
            Some((&header, &metadata, &[E1, E2])),
            1
        )
        .is_err());
    assert_eq!(recovered(&store, &source), before);
}

#[test]
fn opaque_checkpoint_rebind_preserves_saved_values_and_history_after_reopen() {
    let directory = super::tests::private_directory();
    let path = directory.path().join("store.sqlite");
    let mut store = Store::open(&path).unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let header = retain_draft(&mut store, &source);
    let before = recovered(&store, &source);
    let upgraded = repinned(&source);
    store.upgrade_profile_recovery(&source.scope, &source, &header.history, &upgraded, None, 3).unwrap();
    drop(store);
    let store = Store::open_read_only(&path).unwrap().unwrap();
    let after = recovered(&store, &upgraded);
    assert_eq!(after.source, upgraded);
    let (Some(ProfileDraft::Checkpoint { header: previous, bytes: previous_values }),
        Some(ProfileDraft::Checkpoint { header, bytes: values })) = (before.draft, after.draft)
    else { panic!("legacy recovery must keep its supported format") };
    assert_eq!(header.source, upgraded);
    assert_eq!(header.history, previous.history);
    assert_eq!(header.checkpoint_sha256, previous.checkpoint_sha256);
    assert_eq!(serde_json::from_slice::<Value>(&values).unwrap(),
        serde_json::from_slice::<Value>(&previous_values).unwrap());
}

#[test]
fn original_without_a_draft_is_repinned_in_place() {
    let mut store = Store::in_memory().unwrap();
    let source = source(1);
    retain_source(&mut store, &source);
    let upgraded = repinned(&source);
    let (header, metadata) = journal(&upgraded, source.initial_history.clone(), &[]);
    assert!(store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &source.initial_history,
            &upgraded,
            Some((&header, &metadata, &[])),
            1
        )
        .is_err());
    store
        .upgrade_profile_recovery(
            &source.scope,
            &source,
            &source.initial_history,
            &upgraded,
            None,
            2,
        )
        .unwrap();
    let (retained, _) = store
        .profile_source(&source.scope, &source.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(retained, upgraded, "still an eligible readonly original");
    assert!(recovered(&store, &upgraded).draft.is_none());
}
