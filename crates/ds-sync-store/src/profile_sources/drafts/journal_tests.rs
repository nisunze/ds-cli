//! v2 authored journal rows: incremental exact retention, CAS fences,
//! corruption refusal and the v1 opaque-checkpoint transition.
use super::tests::{private_directory, retain_draft, retain_source, source};
use super::*;

// Opaque rows must not be parsed or normalized: signed zero, huge integers.
const E1: &[u8] = br#"{"command_id":"a","value":-0.0,"n":9007199254740993}"#;
const E2: &[u8] = br#"{"command_id":"b"}"#;
const E3: &[u8] = br#"{"command_id":"c"}"#;
const E4: &[u8] = br#"{"command_id":"d"}"#;

fn history(undo: u64, redo: u64) -> Value {
    serde_json::json!({"model_revision":format!("revision-{undo}-{redo}"),"initial_revision":"rev",
        "undo_depth":undo,"redo_depth":redo,"history_pin":format!("pin-{}", undo + redo)})
}
fn metadata(history: &Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"opaque":"metadata","history":history})).unwrap()
}
fn chain(entries: &[&[u8]]) -> String {
    sha256_hex(&entries.concat())
}
fn header(
    source: &ProfileSourceHeader,
    history: Value,
    entries: &[&[u8]],
) -> (ProfileJournalDraftHeader, Vec<u8>) {
    let metadata = metadata(&history);
    (
        ProfileJournalDraftHeader {
            schema: PROFILE_JOURNAL_DRAFT_SCHEMA.into(),
            source: source.clone(),
            history,
            entry_count: entries.len() as u64,
            entry_bytes: entries.iter().map(|e| e.len() as u64).sum(),
            chain_sha256: chain(entries),
            metadata_sha256: sha256_hex(&metadata),
            metadata_bytes: metadata.len() as u64,
        },
        metadata,
    )
}
fn journal(recovery: &ProfileRecovery) -> &ProfileJournalDraft {
    match recovery.draft.as_ref().unwrap() {
        ProfileDraft::Journal(draft) => draft,
        ProfileDraft::Checkpoint { .. } => panic!("expected a journal draft"),
    }
}
fn recovered(store: &Store, source: &ProfileSourceHeader) -> ProfileRecovery {
    store
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
        .unwrap()
}
fn rowids(store: &Store) -> Vec<(i64, i64)> {
    let mut query = store
        .connection
        .prepare("SELECT ordinal,rowid FROM profile_draft_entries ORDER BY ordinal")
        .unwrap();
    let rows = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    rows.map(|row| row.unwrap()).collect()
}
#[allow(clippy::too_many_arguments)]
fn retain(
    store: &mut Store,
    source: &ProfileSourceHeader,
    expected: &Value,
    expected_chain: Option<&str>,
    header: &ProfileJournalDraftHeader,
    metadata: &[u8],
    keep: u64,
    appended: &[&[u8]],
) -> Result<()> {
    store.retain_profile_journal_draft(
        &source.scope,
        source,
        expected,
        expected_chain,
        header,
        metadata,
        keep,
        appended,
        10,
    )
}
fn entries_of(draft: &ProfileJournalDraft) -> Vec<Vec<u8>> {
    draft.entries.iter().map(|e| e.bytes.clone()).collect()
}

#[test]
fn edits_append_only_new_rows_and_reopen_exactly() {
    let temp = private_directory();
    let path = temp.path().join("store.sqlite");
    let source = source(1);
    let mut store = Store::open(&path).unwrap();
    retain_source(&mut store, &source);
    let (first, meta) = header(&source, history(1, 0), &[E1]);
    retain(
        &mut store,
        &source,
        &source.initial_history,
        None,
        &first,
        &meta,
        0,
        &[E1],
    )
    .unwrap();
    let recovery = recovered(&store, &source);
    assert_eq!(journal(&recovery).header, first);
    assert_eq!(journal(&recovery).metadata, meta);
    assert_eq!(entries_of(journal(&recovery)), vec![E1.to_vec()]);
    assert_eq!(journal(&recovery).entries[0].sha256, sha256_hex(E1));
    assert!(store
        .profile_source(&source.scope, &source.session_id)
        .is_err());
    let original_row = rowids(&store);

    let (second, meta) = header(&source, history(3, 0), &[E1, E2, E3]);
    retain(
        &mut store,
        &source,
        &first.history,
        Some(&first.chain_sha256),
        &second,
        &meta,
        1,
        &[E2, E3],
    )
    .unwrap();
    let rows = rowids(&store);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0], original_row[0],
        "the retained prefix row is untouched"
    );

    // Undo changes only the cursor: no row is written.
    let (undone, meta) = header(&source, history(2, 1), &[E1, E2, E3]);
    retain(
        &mut store,
        &source,
        &second.history,
        Some(&second.chain_sha256),
        &undone,
        &meta,
        3,
        &[],
    )
    .unwrap();
    assert_eq!(rowids(&store), rows);

    // A new edit after undo replaces only the abandoned redo tail.
    let (branch, meta) = header(&source, history(3, 0), &[E1, E2, E4]);
    retain(
        &mut store,
        &source,
        &undone.history,
        Some(&undone.chain_sha256),
        &branch,
        &meta,
        2,
        &[E4],
    )
    .unwrap();
    let branched = rowids(&store);
    assert_eq!(branched[..2], rows[..2]);
    drop(store);
    let reopened = Store::open_read_only(&path).unwrap().unwrap();
    let recovery = recovered(&reopened, &source);
    assert_eq!(journal(&recovery).header, branch);
    assert_eq!(
        entries_of(journal(&recovery)),
        vec![E1.to_vec(), E2.to_vec(), E4.to_vec()]
    );
    assert_eq!(recovery.draft.as_ref().unwrap().history(), &branch.history);
    let found = reopened
        .profile_draft_for_package(&source.scope, &source.package_sha256)
        .unwrap()
        .unwrap();
    assert_eq!(found, recovery);
}

#[test]
fn stale_history_prefix_counts_and_metadata_refuse_without_changes() {
    let source = source(1);
    let mut store = Store::in_memory().unwrap();
    retain_source(&mut store, &source);
    let (first, meta) = header(&source, history(2, 0), &[E1, E2]);
    retain(
        &mut store,
        &source,
        &source.initial_history,
        None,
        &first,
        &meta,
        0,
        &[E1, E2],
    )
    .unwrap();
    let before = recovered(&store, &source);
    let (next, next_meta) = header(&source, history(3, 0), &[E1, E2, E3]);
    let wrong_chain = "f".repeat(64);
    let wrong_bytes = {
        let mut header = next.clone();
        header.entry_bytes += 1;
        header
    };
    let wrong_meta = {
        let mut header = next.clone();
        header.metadata_sha256 = "0".repeat(64);
        header
    };
    for (expected, chain, header, keep, appended) in [
        (
            &source.initial_history,
            Some(first.chain_sha256.as_str()),
            &next,
            2,
            vec![E3],
        ),
        (
            &first.history,
            Some(wrong_chain.as_str()),
            &next,
            2,
            vec![E3],
        ),
        (&first.history, None, &next, 0, vec![E1, E2, E3]),
        (
            &first.history,
            Some(first.chain_sha256.as_str()),
            &next,
            3,
            vec![],
        ),
        (
            &first.history,
            Some(first.chain_sha256.as_str()),
            &next,
            1,
            vec![E3],
        ),
        (
            &first.history,
            Some(first.chain_sha256.as_str()),
            &wrong_bytes,
            2,
            vec![E3],
        ),
        (
            &first.history,
            Some(first.chain_sha256.as_str()),
            &wrong_meta,
            2,
            vec![E3],
        ),
        (
            &first.history,
            Some(first.chain_sha256.as_str()),
            &next,
            2,
            vec![&b""[..]],
        ),
    ] {
        assert!(
            retain(&mut store, &source, expected, chain, header, &next_meta, keep, &appended)
                .is_err()
        );
        assert_eq!(recovered(&store, &source), before);
    }
    let mut keep_three = next.clone();
    keep_three.entry_count = 4;
    assert!(retain(
        &mut store,
        &source,
        &first.history,
        Some(&first.chain_sha256),
        &keep_three,
        &next_meta,
        3,
        &[E3],
    )
    .is_err());
    retain(
        &mut store,
        &source,
        &first.history,
        Some(&first.chain_sha256),
        &next,
        &next_meta,
        2,
        &[E3],
    )
    .unwrap();
    // A journal header whose depth differs from its rows is never admitted.
    let (mut skewed, meta) = header(&source, history(1, 0), &[E1, E2]);
    skewed.history = history(1, 0);
    assert!(retain(
        &mut store,
        &source,
        &next.history,
        Some(&next.chain_sha256),
        &skewed,
        &meta,
        2,
        &[]
    )
    .is_err());
    let oversized = vec![b'x'; MAX_PROFILE_JOURNAL_ENTRY_BYTES + 1];
    let (big, meta) = header(&source, history(4, 0), &[E1, E2, E3, &oversized]);
    assert!(retain(
        &mut store,
        &source,
        &next.history,
        Some(&next.chain_sha256),
        &big,
        &meta,
        3,
        &[&oversized]
    )
    .is_err());
    assert_eq!(journal(&recovered(&store, &source)).header, next);
}

#[test]
fn corrupt_missing_and_orphan_rows_refuse_recovery_and_discovery() {
    let source = source(1);
    let corruptions: [&str; 4] = [
        "UPDATE profile_draft_entries SET entry=x'7b7d' WHERE ordinal=1",
        "DELETE FROM profile_draft_entries WHERE ordinal=1",
        "UPDATE profile_draft_entries SET ordinal=5 WHERE ordinal=1",
        "UPDATE profile_drafts SET checkpoint=x'7b7d'",
    ];
    for corruption in corruptions {
        let mut store = Store::in_memory().unwrap();
        retain_source(&mut store, &source);
        let (first, meta) = header(&source, history(2, 0), &[E1, E2]);
        retain(
            &mut store,
            &source,
            &source.initial_history,
            None,
            &first,
            &meta,
            0,
            &[E1, E2],
        )
        .unwrap();
        store.connection.execute_batch(corruption).unwrap();
        assert!(store
            .profile_recovery(&source.scope, &source.session_id)
            .is_err());
        assert!(store
            .profile_draft_for_package(&source.scope, &source.package_sha256)
            .is_err());
        assert!(store
            .remove_profile_recovery(&source.scope, &source, &first.history)
            .is_err());
    }
    // Rows without a v2 header are orphaned evidence, never silently dropped.
    let mut store = Store::in_memory().unwrap();
    retain_source(&mut store, &source);
    let v1 = retain_draft(&mut store, &source);
    store
        .connection
        .execute(
            "INSERT INTO profile_draft_entries VALUES(?1,?2,?3,?4,?5,?6,0,?7,?8)",
            params![
                source.scope.principal,
                source.scope.lane,
                source.scope.deployment,
                source.scope.install,
                source.scope.project,
                source.session_id,
                sha256_hex(E1),
                E1
            ],
        )
        .unwrap();
    assert!(store
        .profile_recovery(&source.scope, &source.session_id)
        .is_err());
    let (journal_header, meta) = header(&source, history(1, 0), &[E1]);
    assert!(retain(
        &mut store,
        &source,
        &v1.history,
        None,
        &journal_header,
        &meta,
        0,
        &[E1]
    )
    .is_err());
}

#[test]
fn opaque_checkpoint_upgrades_to_journal_and_removal_deletes_every_row() {
    let source = source(1);
    let mut store = Store::in_memory().unwrap();
    retain_source(&mut store, &source);
    let v1 = retain_draft(&mut store, &source);
    let (upgraded, meta) = header(&source, history(1, 0), &[E1]);
    // An existing v1 draft has no chain; claiming one refuses.
    assert!(retain(
        &mut store,
        &source,
        &v1.history,
        Some(&upgraded.chain_sha256),
        &upgraded,
        &meta,
        0,
        &[E1]
    )
    .is_err());
    retain(
        &mut store,
        &source,
        &v1.history,
        None,
        &upgraded,
        &meta,
        0,
        &[E1],
    )
    .unwrap();
    assert_eq!(journal(&recovered(&store, &source)).header, upgraded);
    // A journal draft requires its exact chain.
    let (next, next_meta) = header(&source, history(2, 0), &[E1, E2]);
    assert!(retain(
        &mut store,
        &source,
        &upgraded.history,
        None,
        &next,
        &next_meta,
        0,
        &[E1, E2]
    )
    .is_err());
    // A legacy checkpoint may still replace it; no journal row survives that.
    let legacy = ProfileDraftHeader {
        schema: "ds.core.profile-draft/v1".into(),
        source: source.clone(),
        history: history(5, 0),
        checkpoint_sha256: sha256_hex(b"legacy"),
        checkpoint_bytes: 6,
    };
    store
        .retain_profile_draft(
            &source.scope,
            &source,
            &upgraded.history,
            &legacy,
            b"legacy",
            11,
        )
        .unwrap();
    assert_eq!(rowids(&store).len(), 0);
    retain(
        &mut store,
        &source,
        &legacy.history,
        None,
        &next,
        &next_meta,
        0,
        &[E1, E2],
    )
    .unwrap();
    store
        .remove_profile_recovery(&source.scope, &source, &next.history)
        .unwrap();
    assert!(rowids(&store).is_empty());
    assert!(store
        .profile_recovery(&source.scope, &source.session_id)
        .unwrap()
        .is_none());
}
