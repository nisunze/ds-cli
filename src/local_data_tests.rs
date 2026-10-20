use super::*;

fn reading(id: &str, count: u64, bytes: Option<u64>) -> Reading {
    Reading {
        id: id.into(),
        count,
        bytes,
    }
}

#[test]
fn one_row_per_store_in_roster_order_with_count_and_retention() {
    let readings = [
        reading("survey", 12, None),
        reading("design_rooms", 3, None),
        reading("version_downloads", 2, None),
    ];
    let rows = rows(Host::Window, &readings);
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["survey", "design_rooms", "version_downloads"]
    );
    assert_eq!(
        rows.iter().map(|row| row.count).collect::<Vec<_>>(),
        [12, 3, 2]
    );
    assert!(rows
        .iter()
        .all(|row| !row.size_tracked && row.bytes.is_none()));
    assert_eq!(rows[2].retention, Retention::Cleanable);
    assert!(rows[..2]
        .iter()
        .all(|row| row.retention == Retention::Retained));
}

#[test]
fn a_store_the_host_did_not_read_is_shown_empty_not_missing() {
    let rows = rows(Host::Server, &[reading("sync_downloads", 4, Some(4096))]);
    assert_eq!(rows.len(), SERVER_STORES.len());
    let downloads = rows.iter().find(|row| row.id == "sync_downloads").unwrap();
    assert_eq!(
        (downloads.count, downloads.bytes, downloads.size_tracked),
        (4, Some(4096), true)
    );
    assert!(rows
        .iter()
        .filter(|row| row.id != "sync_downloads")
        .all(|row| row.count == 0));
}

#[test]
fn the_answer_names_what_a_clean_would_remove_and_the_storage_fraction() {
    let readings = [
        reading("sync_downloads", 4, Some(4096)),
        reading("survey_media_synced", 2, Some(1000)),
        reading("survey_media_waiting", 1, Some(500)),
        reading("report_artifacts", 0, Some(0)),
    ];
    let storage = Storage {
        usage_bytes: Some(50),
        quota_bytes: Some(200),
        root: Some("/state".into()),
    };
    let answer = answer(Host::Server, "p", &readings, storage, 7);
    assert_eq!(answer.cleanable, ["survey_media_synced", "sync_downloads"]);
    assert_eq!(
        (answer.cleanable_count, answer.cleanable_bytes),
        (6, Some(5096))
    );
    assert_eq!(answer.storage_fraction, Some(0.25));
    assert_eq!(answer.refreshed_at_ms, 7);
    let unknown = super::answer(Host::Window, "p", &[], Storage::default(), 1);
    assert_eq!(unknown.storage_fraction, None);
    assert!(unknown.cleanable.is_empty() && unknown.cleanable_bytes.is_none());
    let clamped = super::answer(
        Host::Window,
        "p",
        &[],
        Storage {
            usage_bytes: Some(300),
            quota_bytes: Some(200),
            root: None,
        },
        1,
    );
    assert_eq!(clamped.storage_fraction, Some(1.0));
}

#[test]
fn a_clean_is_refused_before_anything_moves() {
    let readings = [
        reading("version_downloads", 2, None),
        reading("survey", 9, None),
    ];
    let plan = clean_plan(Host::Window, "p", &readings, &[], false).unwrap_err();
    assert_eq!(plan.code, CONFIRMATION_REQUIRED);
    assert!(plan.message.contains("2 item(s)") && plan.message.contains("version_downloads"));
    let plan = clean_plan(Host::Window, "p", &readings, &[], true).unwrap();
    assert_eq!(plan.stores.len(), 1);
    assert_eq!((plan.count, plan.bytes), (2, None));
    assert_eq!(
        clean_plan(Host::Window, "p", &readings, &["survey".into()], true)
            .unwrap_err()
            .code,
        STORE_RETAINED
    );
    assert_eq!(
        clean_plan(Host::Window, "p", &readings, &["nope".into()], true)
            .unwrap_err()
            .code,
        UNKNOWN_STORE
    );
    assert_eq!(
        clean_plan(
            Host::Window,
            "p",
            &[reading("version_downloads", 0, None)],
            &[],
            true
        )
        .unwrap_err()
        .code,
        NOTHING_TO_CLEAN
    );
    // A Server clean of one named store leaves the others alone.
    let server = [
        reading("sync_downloads", 4, Some(40)),
        reading("survey_media_synced", 1, Some(9)),
    ];
    let plan = clean_plan(
        Host::Server,
        "p",
        &server,
        &["survey_media_synced".into()],
        true,
    )
    .unwrap();
    assert_eq!(
        plan.stores
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["survey_media_synced"]
    );
    assert_eq!(plan.bytes, Some(9));
}

#[test]
fn the_json_door_answers_status_and_plans() {
    let input = json!({"op": "status", "host": "window", "project": "p", "readings": [{"id": "survey", "count": 3}], "now_ms": 5});
    let answer: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
    assert_eq!(answer["data"]["stores"][0]["count"], 3);
    assert_eq!(answer["data"]["stores"][0]["size_tracked"], false);
    let input = json!({"op": "clean_plan", "host": "window", "project": "p", "readings": [], "stores": []});
    let answer: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
    assert_eq!(answer["refusal"]["code"], NOTHING_TO_CLEAN);
}
