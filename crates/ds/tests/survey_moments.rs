//! The survey photos a machine holds, and its local data, answered off a
//! Server state root with no credential, no gateway and no running Server.
//!
//! The store is seeded exactly as `ds survey photo rotate` seeds it — a
//! `MediaRecord` row in `store.sqlite` beside a bundle under `survey-media/`
//! — so these proofs cover the read side, the refusals and the clean without
//! reaching the bucket. The gateway half (rotate held, publish, settle) is
//! proven by hand against a testing project; see the survey reference.

use std::path::Path;

use ds_command_kernel::survey_moments::{self, MediaRecord, RECORD_SCHEMA, SyncState};
use ds_command_kernel::sync_store::Fence;
use ds_sync_store::Store;
use serde_json::Value;

mod common;

const PROJECT: &str = "agct38sq_sample";

fn fence() -> Fence {
    Fence {
        account: "owner".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install-1".into(),
    }
}

fn record(form: &str, entry: &str, file: &str, state: SyncState, cached_at_ms: u64) -> MediaRecord {
    let path = format!("{PROJECT}/forms/{form}/{entry}/{file}");
    MediaRecord {
        schema: RECORD_SCHEMA.into(),
        project: PROJECT.into(),
        thumbnail_path: ds_command_kernel::survey_photo_outbox::thumbnail_path_for(&path).unwrap(),
        path,
        media_type: "image/jpeg".into(),
        size: 100,
        sha256: "a".repeat(64),
        thumbnail_size: 10,
        thumbnail_sha256: "b".repeat(64),
        width: 4,
        height: 3,
        cached_at_ms,
        updated_at_ms: cached_at_ms,
        state,
        degrees: 90,
        source_generation: "1700000000000000".into(),
        published_generation: (state == SyncState::Synced).then(|| "1700000000000001".into()),
        published_thumbnail_generation: None,
        account: "owner".into(),
        entry: None,
    }
}

fn seed(state: &Path, records: &[MediaRecord]) {
    let mut store = Store::open(&state.join("store.sqlite")).unwrap();
    for record in records {
        store.survey_media_put(&fence(), record).unwrap();
        let dir = state
            .join(survey_moments::MEDIA_ROOT)
            .join(PROJECT)
            .join(survey_moments::media_key(&record.path));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("original.bin"), vec![0u8; record.size as usize]).unwrap();
        std::fs::write(
            dir.join("thumbnail.jpeg"),
            vec![0u8; record.thumbnail_size as usize],
        )
        .unwrap();
        std::fs::write(dir.join("manifest.json"), b"{}").unwrap();
    }
}

fn ds(state: &Path, args: &[&str]) -> Value {
    let root = state.to_str().unwrap();
    let mut full: Vec<&str> = args.to_vec();
    full.extend([
        "--lane",
        "canary",
        "--server-state-dir",
        root,
        "--output",
        "json",
    ]);
    common::json(&full).0
}

#[test]
fn moments_are_listed_filtered_and_read_off_the_store_without_a_session() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();

    // Nothing held: an empty gallery, not an error.
    let empty = ds(&state, &["survey", "moments", "list", "--project", PROJECT]);
    assert_eq!(empty["status"], "ok");
    assert_eq!(empty["data"]["total"], 0);

    seed(
        &state,
        &[
            record(
                "poles",
                "e1",
                "IMG_0001.jpg",
                SyncState::Waiting,
                1_789_900_000_000,
            ),
            record(
                "lines",
                "e2",
                "span_0002.jpg",
                SyncState::Synced,
                1_789_800_000_000,
            ),
            record(
                "poles",
                "e3",
                "IMG_0003.jpg",
                SyncState::Synced,
                1_789_700_000_000,
            ),
        ],
    );

    let all = ds(&state, &["survey", "moments", "list", "--project", PROJECT]);
    assert_eq!(all["status"], "ok", "{all}");
    let data = &all["data"];
    assert_eq!(
        (
            data["count"].as_u64(),
            data["total"].as_u64(),
            data["more"].as_bool()
        ),
        (Some(3), Some(3), Some(false))
    );
    assert_eq!(
        (data["waiting"].as_u64(), data["synced"].as_u64()),
        (Some(1), Some(2))
    );
    let files: Vec<&str> = data["moments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        ["IMG_0001.jpg", "span_0002.jpg", "IMG_0003.jpg"],
        "newest first"
    );
    assert_eq!(data["forms"][0]["form"], "poles");
    assert_eq!(data["forms"][0]["count"], 2);
    assert_eq!(data["moments"][0]["state"], "waiting");
    assert_eq!(
        data["moments"][0]["thumbnail_path"],
        format!("{PROJECT}/forms/poles/e1/IMG_0001_thunder.jpeg")
    );
    assert_eq!(data["moments"][0]["navigate"]["reason"], "entry_not_local");

    let filtered = ds(
        &state,
        &[
            "survey",
            "moments",
            "list",
            "--project",
            PROJECT,
            "--form",
            "poles",
            "--sync",
            "synced",
            "--text",
            "img",
            "--limit",
            "1",
        ],
    );
    assert_eq!(filtered["data"]["count"], 1);
    assert_eq!(filtered["data"]["matched"], 1);
    assert_eq!(
        filtered["data"]["total"], 3,
        "the total is the machine's holdings"
    );
    assert_eq!(filtered["data"]["moments"][0]["file"], "IMG_0003.jpg");
    assert_eq!(filtered["data"]["filter"]["form"], "poles");

    let bad = ds(
        &state,
        &[
            "survey",
            "moments",
            "list",
            "--project",
            PROJECT,
            "--since",
            "19/09/2026",
        ],
    );
    assert_eq!(bad["status"], "error");
    assert_eq!(bad["error"]["code"], "invalid_filter");

    let path = format!("{PROJECT}/forms/poles/e1/IMG_0001.jpg");
    let read = ds(
        &state,
        &[
            "survey",
            "moments",
            "read",
            "--project",
            PROJECT,
            "--path",
            &path,
        ],
    );
    assert_eq!(read["status"], "ok", "{read}");
    assert_eq!(read["data"]["moment"]["state"], "waiting");
    assert_eq!(read["data"]["degrees"], 90);
    assert!(Path::new(read["data"]["original"].as_str().unwrap()).is_file());
    assert!(
        read["data"]["bundle"]
            .as_str()
            .unwrap()
            .contains(&survey_moments::media_key(&path))
    );

    let missing = ds(
        &state,
        &[
            "survey",
            "moments",
            "read",
            "--project",
            PROJECT,
            "--path",
            "agct38sq_sample/forms/x/y/z.jpg",
        ],
    );
    assert_eq!(missing["error"]["code"], "moment_not_held");

    // The rotation gate (`not_rotatable` for a thumbnail, a URL or another
    // project's photo; `moment_not_waiting` for a publish of a path nothing is
    // waiting for) sits behind the native-profile availability gate, which
    // this harness has no catalog for; the kernel's unit tests pin the gate
    // and the reference records the hand proof against the gateway.
}

#[test]
fn local_data_status_and_clean_answer_the_server_state_root() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    seed(
        &state,
        &[
            record("poles", "e1", "IMG_0001.jpg", SyncState::Waiting, 1),
            record("lines", "e2", "span_0002.jpg", SyncState::Synced, 2),
        ],
    );
    let downloads = state
        .join("sync-downloads")
        .join(PROJECT)
        .join("network_reporter")
        .join("export-x");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("artifact.bin"), vec![1u8; 2048]).unwrap();

    let status = ds(
        &state,
        &["workstation", "local-data", "status", "--project", PROJECT],
    );
    assert_eq!(status["status"], "ok", "{status}");
    let rows: Vec<(&str, u64, Option<u64>, &str)> = status["data"]["stores"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["id"].as_str().unwrap(),
                row["count"].as_u64().unwrap(),
                row["bytes"].as_u64(),
                row["retention"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("sync_store", 0, None, "retained"),
            ("report_artifacts", 0, Some(0), "retained"),
            ("survey_media_waiting", 1, Some(110), "retained"),
            ("survey_media_synced", 1, Some(110), "cleanable"),
            ("sync_downloads", 1, Some(2048), "cleanable"),
        ]
    );
    assert_eq!(status["data"]["stores"][0]["size_tracked"], false);
    assert_eq!(
        status["data"]["cleanable"],
        serde_json::json!(["survey_media_synced", "sync_downloads"])
    );
    assert_eq!(status["data"]["cleanable_bytes"], 2158);
    assert_eq!(status["data"]["host"], "server");

    let unconfirmed = ds(
        &state,
        &["workstation", "local-data", "clean", "--project", PROJECT],
    );
    assert_eq!(unconfirmed["error"]["code"], "confirmation_required");
    let retained = ds(
        &state,
        &[
            "workstation",
            "local-data",
            "clean",
            "--project",
            PROJECT,
            "--store",
            "survey_media_waiting",
            "--yes",
        ],
    );
    assert_eq!(retained["error"]["code"], "store_retained");
    let unknown = ds(
        &state,
        &[
            "workstation",
            "local-data",
            "clean",
            "--project",
            PROJECT,
            "--store",
            "nope",
            "--yes",
        ],
    );
    assert_eq!(unknown["error"]["code"], "unknown_store");

    let cleaned = ds(
        &state,
        &[
            "workstation",
            "local-data",
            "clean",
            "--project",
            PROJECT,
            "--store",
            "survey_media_synced",
            "--yes",
        ],
    );
    assert_eq!(cleaned["status"], "ok", "{cleaned}");
    assert_eq!(cleaned["data"]["removed"][0]["id"], "survey_media_synced");
    assert_eq!(cleaned["data"]["removed"][0]["count"], 1);
    let after = &cleaned["data"]["status"]["stores"];
    assert_eq!(after[2]["count"], 1, "the waiting photo stays");
    assert_eq!(after[3]["count"], 0);
    assert_eq!(after[4]["count"], 1, "only the named store went");
    assert!(
        !state
            .join(survey_moments::MEDIA_ROOT)
            .join(PROJECT)
            .join(survey_moments::media_key(&format!(
                "{PROJECT}/forms/lines/e2/span_0002.jpg"
            )))
            .exists()
    );

    let rest = ds(
        &state,
        &[
            "workstation",
            "local-data",
            "clean",
            "--project",
            PROJECT,
            "--yes",
        ],
    );
    assert_eq!(rest["data"]["removed"][0]["id"], "sync_downloads");
    assert!(!state.join("sync-downloads").join(PROJECT).exists());
    let nothing = ds(
        &state,
        &[
            "workstation",
            "local-data",
            "clean",
            "--project",
            PROJECT,
            "--yes",
        ],
    );
    assert_eq!(nothing["error"]["code"], "nothing_to_clean");

    let list = ds(&state, &["survey", "moments", "list", "--project", PROJECT]);
    assert_eq!(
        list["data"]["total"], 1,
        "the waiting rotation survived every clean"
    );
}
