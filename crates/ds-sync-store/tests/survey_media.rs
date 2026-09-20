//! The survey-media records: written when a rotation is prepared, settled
//! when it is published, read by the gallery, removed by a clean — and
//! absent, never an error, in a store written before the table existed.

use ds_command_kernel::survey_moments::{MediaRecord, SyncState, RECORD_SCHEMA};
use ds_command_kernel::sync_store::Fence;
use ds_sync_store::Store;

fn fence() -> Fence {
    Fence {
        account: "owner".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install-1".into(),
    }
}

fn record(project: &str, path: &str, state: SyncState, updated: u64) -> MediaRecord {
    MediaRecord {
        schema: RECORD_SCHEMA.into(),
        project: project.into(),
        path: path.into(),
        thumbnail_path: format!("{}_thunder.jpeg", path.trim_end_matches(".jpg")),
        media_type: "image/jpeg".into(),
        size: 100,
        sha256: "a".repeat(64),
        thumbnail_size: 10,
        thumbnail_sha256: "b".repeat(64),
        width: 4,
        height: 3,
        cached_at_ms: 1,
        updated_at_ms: updated,
        state,
        degrees: 90,
        source_generation: "1700000000000000".into(),
        published_generation: None,
        published_thumbnail_generation: None,
        account: "owner".into(),
        entry: None,
    }
}

#[test]
fn records_are_written_replaced_read_and_removed_per_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let mut store = Store::open(&path).unwrap();
    let a = record("p", "p/forms/poles/e1/a.jpg", SyncState::Waiting, 5);
    let b = record("p", "p/forms/poles/e2/b.jpg", SyncState::Synced, 6);
    let other = record("q", "q/forms/poles/e1/a.jpg", SyncState::Synced, 7);
    store.survey_media_put(&fence(), &a).unwrap();
    store.survey_media_put(&fence(), &b).unwrap();
    store.survey_media_put(&fence(), &other).unwrap();

    let held = store.survey_media_of_project("p").unwrap();
    assert_eq!(held.len(), 2);
    assert_eq!(held[0].path, a.path);
    assert_eq!(store.survey_media_record("p", &a.path).unwrap(), Some(a.clone()));
    assert_eq!(store.survey_media_record("p", "p/forms/none.jpg").unwrap(), None);

    // Replacing settles the same key rather than adding a row.
    let mut settled = a.clone();
    settled.state = SyncState::Synced;
    settled.published_generation = Some("1700000000000001".into());
    settled.updated_at_ms = 9;
    store.survey_media_put(&fence(), &settled).unwrap();
    assert_eq!(store.survey_media_of_project("p").unwrap().len(), 2);
    assert_eq!(
        store.survey_media_record("p", &a.path).unwrap().unwrap().state,
        SyncState::Synced
    );

    // A clean removes the synced records of one project and names them.
    let removed = store.survey_media_remove_state("p", SyncState::Synced).unwrap();
    assert_eq!(removed, vec![a.path.clone(), b.path.clone()]);
    assert!(store.survey_media_of_project("p").unwrap().is_empty());
    assert_eq!(store.survey_media_of_project("q").unwrap().len(), 1);
    assert_eq!(store.survey_media_remove("q", &[other.path.clone()]).unwrap(), 1);
    assert_eq!(store.survey_media_remove("q", &[other.path]).unwrap(), 0);

    // The read-only opener answers the same rows without writing.
    store.survey_media_put(&fence(), &b).unwrap();
    drop(store);
    let reader = Store::open_read_only(&path).unwrap().unwrap();
    assert_eq!(reader.survey_media_of_project("p").unwrap().len(), 1);
    let row = reader.survey_media_of_project("p").unwrap().remove(0).moment_row();
    assert_eq!(row.state, SyncState::Synced);
    assert!(row.thumbnail);
}

#[test]
fn a_store_without_the_table_holds_no_media() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                 INSERT INTO meta(key, value) VALUES ('schema_version', 1);",
            )
            .unwrap();
    }
    let reader = Store::open_read_only(&path).unwrap().unwrap();
    assert!(reader.survey_media_of_project("p").unwrap().is_empty());
    assert_eq!(reader.survey_media_record("p", "x").unwrap(), None);
}
