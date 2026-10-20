use super::*;
use crate::durable_store::{self, Call};
use rusqlite::Connection;

fn identity(principal: &str) -> Identity {
    Identity {
        principal: Principal::Account(principal.into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}

/// One call through the Kernel's one transaction, exactly as every host runs it.
fn run(db: &mut Connection, who: &str, operation: Value) -> Result<Value, String> {
    let call: Call =
        serde_json::from_value(json!({"request": {"store": STORE, "operation": operation}}))
            .map_err(|e| e.to_string())?;
    durable_store::execute(db, Some(&identity(who)), call, None)
        .map(|answer| serde_json::to_value(&answer).unwrap())
}

fn layers() -> Value {
    json!({
        "tr": {"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[29.7,-2.7]},"properties":{"name":"old","kva":100}}]},
        "lv_poles": {"type":"FeatureCollection","features":[
            {"type":"Feature","id":"p1","geometry":{"type":"Point","coordinates":[29.73,-2.76,1.5]},"properties":null},
            {"type":"Feature","geometry":null}]},
        "empty": {"type":"FeatureCollection"}
    })
}

fn edit(db: &mut Connection, transformer: &str, layers: Value, extra: Value) -> Value {
    let mut room = json!({"transformer": transformer, "layers": layers});
    room.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    run(
        db,
        "owner",
        json!({"action":"write","project":"kigali","kind":"edited","rooms":[room],"now":1000}),
    )
    .unwrap()
}

fn get(db: &mut Connection, transformer: &str) -> Value {
    run(
        db,
        "owner",
        json!({"action":"get","project":"kigali","transformer":transformer}),
    )
    .unwrap()
}

fn header(db: &mut Connection, transformer: &str) -> Value {
    let answer = run(
        db,
        "owner",
        json!({"action":"headers","project":"kigali","transformers":[transformer]}),
    )
    .unwrap();
    answer["headers"][0].clone()
}

fn patch(db: &mut Connection, patch: Value) -> Value {
    run(
        db,
        "owner",
        json!({"action":"patch","project":"kigali","patches":[patch],"now":5}),
    )
    .unwrap()
}

fn acquire(expected: Value, consume: Value) -> Value {
    json!({"action":"acquire","project":"kigali","expected_revision":expected,"consume":consume,"now":2000,
        "room":{"transformer":"tr_1","layers":{"tr":{"type":"FeatureCollection","features":[]}},"server_version":4}})
}

#[test]
fn the_header_digest_is_ds_webs_transmitted_content_digest() {
    // Pinned against ds-web's own modules (content-digest.ts, transformer-name.ts
    // and api/design.ts withTransformerDocumentFields) over the same room.
    let layers = layers();
    let layers = layers.as_object().unwrap();
    for (name, digest, bytes) in [
        (
            "tr_1",
            "01f764799f5561997348ad32855fd6ed90e776b951e58e0f9c4e17a95b7285b1",
            469,
        ),
        (
            "kigali_ex_12",
            "3e28882208f21f01555503e23a3992569bf15ada17381e7eb787eaa9893b1efd",
            521,
        ),
        (
            "mv_data",
            "fa72afa68986cfa800bd320de74a0ee3f31402150dd704030e99801e9ea94522",
            391,
        ),
    ] {
        let summary = summarize(name, layers, None);
        assert_eq!(
            summary.local_content_digest.as_deref(),
            Some(digest),
            "{name}"
        );
        assert_eq!(summary.estimated_payload_bytes, bytes, "{name}");
        assert_eq!((summary.feature_count, summary.has_layers), (3, true));
    }
    // An analysed room travels unchanged: the shared cross-language fixture.
    let fixture: Map<String, Value> = serde_json::from_str(DIGEST_FIXTURE).unwrap();
    let analysis = Map::from_iter([("schema".to_string(), json!("x"))]);
    let analysed = summarize("tr_1", &fixture, Some(&analysis));
    assert_eq!(
        analysed.local_content_digest.as_deref(),
        Some("41fed8ceb44492eb3c8c9168d76ae0bfa8436e40a2f2d58e51b6da3b37661a24")
    );
}

#[test]
fn document_names_follow_ds_webs_clean_name() {
    for (raw, document) in [
        ("tr_1", "TR_1"),
        ("Kigali EX-12", "KIGALI_FILL_IN_12"),
        ("éléphant__x", "L_PHANT_X"),
        ("a_-b^!c", "ABC"),
        ("123abc", "COL_123ABC"),
        ("___", ""),
        ("tx_exempt", "TX_FILL_INEMPT"),
        ("MV Data", "MV_DATA"),
        ("a\tb", "A_B"),
        ("x_ _y", "X_Y"),
        ("trf_sud", "TRF_SUD"),
        ("Tr 7 / Gisagara (old)", "TR_7_GISAGARA_OLD"),
        ("😀tr", "TR"),
    ] {
        let name = ds_system_clean_name(raw)
            .to_uppercase()
            .replace("EX", "FILL_IN");
        assert_eq!(name, document, "{raw:?}");
    }
}

#[test]
fn an_edited_room_round_trips_with_its_header_metadata_and_generation() {
    let mut db = Connection::open_in_memory().unwrap();
    let written = edit(
        &mut db,
        "TR 1",
        layers(),
        json!({"process_metadata": {"status":"success"}}),
    );
    assert_eq!(
        written["written"],
        json!([{"transformer_name":"tr_1","revision":1}])
    );
    assert_eq!(written["generation"], 1);
    let answer = get(&mut db, "tr_1");
    assert_eq!(
        answer["generation"], 1,
        "a read does not advance the generation"
    );
    let room = &answer["room"];
    assert_eq!(room["layers"], layers());
    assert_eq!(
        (&room["local_dirty"], &room["server_known"]),
        (&json!(true), &json!(false))
    );
    assert_eq!(
        (&room["process_metadata"], &room["updated_at"]),
        (&json!({"status":"success"}), &json!(1000))
    );
    let header = header(&mut db, "Tr_1");
    assert_eq!(
        header["local_content_digest"],
        "01f764799f5561997348ad32855fd6ed90e776b951e58e0f9c4e17a95b7285b1"
    );
    assert_eq!(
        header["server_content_digest"],
        Value::Null,
        "an unsaved edit has no server digest"
    );
    assert_eq!(
        (
            &header["requires_analysis_receipt"],
            &header["feature_count"]
        ),
        (&json!(true), &json!(3))
    );
    let listed = run(
        &mut db,
        "owner",
        json!({"action":"headers","project":"kigali","source":"lv"}),
    )
    .unwrap();
    let row = &listed["headers"][0];
    assert!(
        row.get("layers").is_none(),
        "metadata never carries geometry"
    );
    assert_eq!(
        row["layer_feature_counts"],
        json!({"tr":1,"lv_poles":2,"empty":0})
    );
    let count = run(
        &mut db,
        "owner",
        json!({"action":"count","project":"kigali"}),
    )
    .unwrap();
    assert_eq!(count["rooms"], 1);
    let newest = run(
        &mut db,
        "owner",
        json!({"action":"list","project":"kigali","source":"lv","limit":1}),
    )
    .unwrap();
    assert_eq!(newest["rooms"][0]["layers"], layers());
}

#[test]
fn omitted_flags_keep_the_held_rooms_and_null_clears_process_metadata() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(
        &mut db,
        "tr_1",
        layers(),
        json!({"server_known":true,"local_dirty":false,"server_version":7,"process_metadata":{"a":1}}),
    );
    edit(&mut db, "tr_1", layers(), json!({}));
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        (&room["server_known"], &room["local_dirty"]),
        (&json!(true), &json!(false))
    );
    assert_eq!(
        (&room["server_version"], &room["process_metadata"]),
        (&json!(7), &json!({"a":1}))
    );
    assert_eq!(room["revision"], 2);
    edit(
        &mut db,
        "tr_1",
        layers(),
        json!({"process_metadata": null, "local_dirty": true}),
    );
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        (&room["process_metadata"], &room["local_dirty"]),
        (&Value::Null, &json!(true))
    );
}

#[test]
fn accounts_lanes_and_projects_never_share_a_room() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_1", layers(), json!({}));
    let other = run(
        &mut db,
        "someone",
        json!({"action":"get","project":"kigali","transformer":"tr_1"}),
    )
    .unwrap();
    assert_eq!(other["room"], Value::Null);
    assert_eq!(
        run(
            &mut db,
            "owner",
            json!({"action":"get","project":"huye","transformer":"tr_1"})
        )
        .unwrap()["room"],
        Value::Null
    );
    let stable = Identity {
        lane: "stable".into(),
        ..identity("owner")
    };
    let call: Call = serde_json::from_value(
        json!({"request":{"store":STORE,"operation":{"action":"count","project":"kigali"}}}),
    )
    .unwrap();
    let answer =
        serde_json::to_value(durable_store::execute(&mut db, Some(&stable), call, None).unwrap())
            .unwrap();
    assert_eq!(answer["rooms"], 0);
}

#[test]
fn closed_refusals_name_their_code() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_1", layers(), json!({}));
    let create = json!({"action":"write","project":"kigali","kind":"create","rooms":[{"transformer":"TR_1","layers":{}}],"now":1});
    assert!(run(&mut db, "owner", create)
        .unwrap_err()
        .starts_with(EXISTS));
    for refused in [
        json!({"action":"write","project":"kigali","kind":"edited","rooms":[{"transformer":"combined_transformer","layers":{}}],"now":1}),
        json!({"action":"write","project":"kigali","kind":"edited","rooms":[{"transformer":"collisions","layers":{}}],"now":1}),
        json!({"action":"write","project":"kigali","kind":"edited","rooms":[{"transformer":"tr_2","layers":{"tr":[]}}],"now":1}),
        json!({"action":"write","project":"kigali","kind":"edited","rooms":[],"now":1}),
        json!({"action":"write","project":"a/b","kind":"edited","rooms":[{"transformer":"tr_2","layers":{}}],"now":1}),
        json!({"action":"patch","project":"kigali","patches":[{"transformer":"tr_1","server_content_digest":"XYZ"}],"now":1}),
        json!({"action":"list","project":"kigali","limit":17}),
        json!({"action":"layer_features","project":"kigali","layer":"tr","limit":0}),
        json!({"action":"import_rooms","account_email":"not-an-email","rooms":[],"now":1}),
    ] {
        let error = run(&mut db, "owner", refused.clone()).unwrap_err();
        assert!(error.starts_with(INVALID), "{refused}: {error}");
    }
    // A call that is not the closed vocabulary never reaches the store.
    for malformed in [
        json!({"action":"drop_everything","project":"kigali"}),
        json!({"action":"write","project":"kigali","kind":"edited","rooms":[{"transformer":"tr_2","layers":{},"process_metadata":[1]}],"now":1}),
        json!({"action":"write","project":"kigali","kind":"acquired","rooms":[],"now":1}),
    ] {
        assert!(run(&mut db, "owner", malformed).is_err());
    }
    let call: Call = serde_json::from_value(
        json!({"request":{"store":STORE,"operation":{"action":"count","project":"kigali"}}}),
    )
    .unwrap();
    let bytes =
        durable_store::execute(&mut db, Some(&identity("owner")), call, Some(vec![1])).unwrap_err();
    assert!(bytes.starts_with(INVALID), "{bytes}");
}

#[test]
fn a_refused_write_rolls_the_whole_batch_back() {
    let mut db = Connection::open_in_memory().unwrap();
    let batch = json!({"action":"write","project":"kigali","kind":"edited","rooms":[
        {"transformer":"tr_1","layers":layers()},
        {"transformer":"tr_2","layers":{"bad": 5}}],"now":1});
    assert!(run(&mut db, "owner", batch).is_err());
    let count = run(
        &mut db,
        "owner",
        json!({"action":"count","project":"kigali"}),
    )
    .unwrap();
    assert_eq!(
        (&count["rooms"], &count["generation"]),
        (&json!(0), &json!(0))
    );
}

#[test]
fn an_acquisition_commits_only_over_the_revision_it_captured() {
    let mut db = Connection::open_in_memory().unwrap();
    // A dirty edit landed after the download captured "nothing held".
    edit(&mut db, "tr_1", layers(), json!({}));
    let error = run(&mut db, "owner", acquire(Value::Null, json!([]))).unwrap_err();
    assert!(
        error.starts_with(SUPERSEDED) && error.contains("transformer_acquisition_superseded"),
        "{error}"
    );
    assert_eq!(
        get(&mut db, "tr_1")["room"]["layers"],
        layers(),
        "the edit survives"
    );
    // A save in flight also wins over the download.
    patch(
        &mut db,
        json!({"transformer":"tr_1","pending_operation_id":"op-1"}),
    );
    assert!(run(&mut db, "owner", acquire(json!(1), json!([])))
        .unwrap_err()
        .starts_with(SUPERSEDED));
    patch(
        &mut db,
        json!({"transformer":"tr_1","pending_operation_id":null}),
    );
    // The exact captured revision (a discard-and-reload) commits a clean saved copy.
    run(&mut db, "owner", acquire(json!(1), json!([]))).unwrap();
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        (
            &room["local_dirty"],
            &room["server_known"],
            &room["server_version"]
        ),
        (&json!(false), &json!(true), &json!(4))
    );
    let header = header(&mut db, "tr_1");
    assert_eq!(
        header["server_content_digest"],
        header["local_content_digest"]
    );
}

#[test]
fn an_edit_made_during_a_save_stays_dirty() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(
        &mut db,
        "tr_1",
        layers(),
        json!({"server_known":true,"server_version":3}),
    );
    let sent = header(&mut db, "tr_1")["local_content_digest"].clone();
    patch(
        &mut db,
        json!({"transformer":"tr_1","pending_operation_id":"op-1"}),
    );
    // The person keeps editing while the save request is in flight.
    let newer = json!({"tr":{"type":"FeatureCollection","features":[]}});
    edit(&mut db, "tr_1", newer.clone(), json!({}));
    let answer = patch(
        &mut db,
        json!({"transformer":"tr_1","local_dirty":false,"server_known":true,
        "server_version":4,"server_content_digest":sent,"pending_operation_id":null}),
    );
    assert_eq!(answer["kept_dirty"], json!(["tr_1"]));
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        (&room["local_dirty"], &room["layers"]),
        (&json!(true), &newer)
    );
    let header = header(&mut db, "tr_1");
    assert_eq!(
        (
            &header["base_server_version"],
            &header["server_content_digest"]
        ),
        (&json!(4), &sent)
    );
    assert_ne!(
        header["local_content_digest"],
        header["server_content_digest"]
    );
}

#[test]
fn an_acknowledged_save_marks_the_unchanged_room_clean() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_1", layers(), json!({}));
    patch(
        &mut db,
        json!({"transformer":"tr_1","pending_operation_id":"op-1"}),
    );
    assert_eq!(
        header(&mut db, "tr_1")["revision"],
        1,
        "a pending marker is not a room change"
    );
    let answer = patch(
        &mut db,
        json!({"transformer":"tr_1","local_dirty":false,"server_known":true,"server_version":1,
        "pending_operation_id":null,"voltage_drop_metadata":{"state":"missing"}}),
    );
    assert_eq!(
        (&answer["patched"], &answer["kept_dirty"]),
        (&json!(["tr_1"]), &json!([]))
    );
    let header = header(&mut db, "tr_1");
    assert_eq!(
        (&header["local_dirty"], &header["pending_operation_id"]),
        (&json!(false), &Value::Null)
    );
    assert_eq!(
        header["server_content_digest"],
        header["local_content_digest"]
    );
    assert_eq!(header["voltage_drop_metadata"], json!({"state":"missing"}));
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        (&room["voltage_drop_metadata"], &room["server_version"]),
        (&json!({"state":"missing"}), &json!(1))
    );
}

#[test]
fn a_saved_analysis_attaches_only_to_the_unchanged_clean_room() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_1", layers(), json!({}));
    let attach = |version: u64, at: u64| {
        json!({"action":"attach_saved_analysis","project":"kigali","transformer":"tr_1","expected_server_version":version,
            "expected_updated_at":at,"document":{"rows":[1.0]},"metadata":{"state":"ready"},"raw_json":"{\"rows\":[1.0]}"})
    };
    assert!(
        run(&mut db, "owner", attach(0, 1000))
            .unwrap_err()
            .starts_with(NOT_CLEAN),
        "a dirty room refuses"
    );
    edit(
        &mut db,
        "tr_1",
        layers(),
        json!({"local_dirty":false,"server_known":true,"server_version":9}),
    );
    assert!(
        run(&mut db, "owner", attach(8, 1000))
            .unwrap_err()
            .starts_with(NOT_CLEAN),
        "another version refuses"
    );
    assert!(
        run(&mut db, "owner", attach(9, 999))
            .unwrap_err()
            .starts_with(NOT_CLEAN),
        "another room write refuses"
    );
    run(&mut db, "owner", attach(9, 1000)).unwrap();
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        (&room["voltage_drop"], &room["voltage_drop_json"]),
        (&json!({"rows":[1.0]}), &json!("{\"rows\":[1.0]}"))
    );
    assert_eq!(
        (&room["updated_at"], &room["local_dirty"]),
        (&json!(1000), &json!(false))
    );
    let mismatch = json!({"action":"attach_saved_analysis","project":"kigali","transformer":"tr_1","expected_server_version":9,
        "expected_updated_at":1000,"document":{"rows":[2]},"metadata":{},"raw_json":"{\"rows\":[1]}"});
    assert!(run(&mut db, "owner", mismatch)
        .unwrap_err()
        .starts_with(INVALID));
}

#[test]
fn latest_invalidations_are_kept_once_and_consumed_with_the_acquisition() {
    let mut db = Connection::open_in_memory().unwrap();
    let receipt =
        json!({"working_source_version":5,"working_content_digest":"d","change_id":"c-1"});
    let invalidate =
        json!({"action":"invalidate","project":"kigali","transformer":"TR_1","receipt":receipt});
    assert_eq!(
        run(&mut db, "owner", invalidate.clone()).unwrap()["added"],
        true
    );
    assert_eq!(run(&mut db, "owner", invalidate).unwrap()["added"], false);
    assert_eq!(get(&mut db, "tr_1")["invalidations"], json!([receipt]));
    run(&mut db, "owner", acquire(Value::Null, json!(["c-1"]))).unwrap();
    assert_eq!(get(&mut db, "tr_1")["invalidations"], json!([]));
}

#[test]
fn layer_features_read_one_design_layer_across_rooms() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_1", layers(), json!({}));
    edit(
        &mut db,
        "tr_2",
        json!({"design/LV_Poles_vt":{"type":"FeatureCollection","features":[{"id":"x"}]}}),
        json!({}),
    );
    let answer = run(
        &mut db,
        "owner",
        json!({"action":"layer_features","project":"kigali","layer":"lv_poles","limit":2}),
    )
    .unwrap();
    assert_eq!(answer["features"].as_array().unwrap().len(), 2);
    assert_eq!(answer["truncated"], true);
}

#[test]
fn delete_discards_only_the_named_rooms() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_1", layers(), json!({}));
    edit(&mut db, "tr_2", layers(), json!({}));
    let answer = run(
        &mut db,
        "owner",
        json!({"action":"delete","project":"kigali","transformers":["TR 1","tr_9"]}),
    )
    .unwrap();
    assert_eq!(answer["deleted"], json!(["tr_1"]));
    assert_eq!(get(&mut db, "tr_1")["room"], Value::Null);
    assert_eq!(get(&mut db, "tr_2")["room"]["layers"], layers());
}

fn legacy(name: &str, dirty: Option<bool>, owner: Option<&str>) -> Value {
    let mut room = json!({"ds_project":"kigali","source":"lv","transformer_name":name,"layers":layers(),
        "updated_at":1_700_000_000_000u64,"cache_version":3,"server_known":true,"server_version":12,
        "local_process_metadata":{"status":"warning"}});
    if let Some(dirty) = dirty {
        room["local_dirty"] = json!(dirty);
    }
    if let Some(owner) = owner {
        room["owner_email"] = json!(owner);
    }
    json!({"room": room, "header": {"ds_project":"kigali","transformer_name":name,"source":"lv",
        "local_dirty":dirty.unwrap_or(false),"server_known":true,"pending_operation_id":"op-7",
        "local_content_digest":"b".repeat(64),"updated_at":1_700_000_000_500u64}})
}

fn import(db: &mut Connection, rooms: Vec<Value>) -> Result<Value, String> {
    run(
        db,
        "owner",
        json!({"action":"import_rooms","account_email":" Owner@DS.rw ","rooms":rooms,"now":9}),
    )
}

fn moved(answer: &Value) -> Vec<bool> {
    serde_json::from_value(answer["moved"].clone()).unwrap()
}

#[test]
fn a_held_dirty_room_survives_the_move_byte_exact() {
    let mut db = Connection::open_in_memory().unwrap();
    let answer = import(
        &mut db,
        vec![legacy("tr_1", Some(true), Some("owner@ds.rw"))],
    )
    .unwrap();
    assert_eq!(moved(&answer), [true]);
    let room = get(&mut db, "tr_1")["room"].clone();
    assert_eq!(
        room["layers"],
        layers(),
        "every feature of the unsaved work arrives"
    );
    assert_eq!(
        (&room["local_dirty"], &room["server_version"]),
        (&json!(true), &json!(12))
    );
    assert_eq!(
        (&room["process_metadata"], &room["updated_at"]),
        (&json!({"status":"warning"}), &json!(1_700_000_000_000u64))
    );
    let header = header(&mut db, "tr_1");
    assert_eq!(
        header["pending_operation_id"], "op-7",
        "an interrupted save stays reconcilable"
    );
    assert_eq!(
        header["local_content_digest"],
        "b".repeat(64),
        "the digest the server may hold is kept"
    );
    // The moved room is still the person's work: a later download cannot replace it.
    let error = run(&mut db, "owner", acquire(Value::Null, json!([]))).unwrap_err();
    assert!(error.starts_with(SUPERSEDED), "{error}");
}

#[test]
fn the_import_never_replaces_held_work_and_skips_what_is_not_this_accounts() {
    let mut db = Connection::open_in_memory().unwrap();
    edit(&mut db, "tr_held_dirty", json!({}), json!({}));
    edit(
        &mut db,
        "tr_held_clean",
        json!({}),
        json!({"local_dirty":false,"server_known":true}),
    );
    edit(
        &mut db,
        "tr_clean_kept",
        json!({}),
        json!({"local_dirty":false,"server_known":true}),
    );
    let mut stale = legacy("tr_stale", Some(true), None);
    stale["room"]["cache_version"] = json!(2);
    let answer = import(
        &mut db,
        vec![
            legacy("tr_held_dirty", Some(true), None),
            legacy("tr_held_clean", Some(true), None),
            legacy("tr_clean_kept", Some(false), None),
            legacy("tr_foreign", Some(true), Some("someone@ds.rw")),
            legacy("tr_unowned", None, None),
            stale,
            json!({"room": {"ds_project": "kigali"}}),
        ],
    )
    .unwrap();
    assert_eq!(
        moved(&answer),
        [false, true, false, false, true, false, false]
    );
    assert_eq!(
        get(&mut db, "tr_held_dirty")["room"]["layers"],
        json!({}),
        "the newer held work stays"
    );
    assert_eq!(
        get(&mut db, "tr_held_clean")["room"]["layers"],
        layers(),
        "unsaved legacy work replaces a clean copy"
    );
    assert_eq!(get(&mut db, "tr_clean_kept")["room"]["layers"], json!({}));
    assert_eq!(get(&mut db, "tr_foreign")["room"], Value::Null);
    assert_eq!(
        get(&mut db, "tr_unowned")["room"]["local_dirty"],
        false,
        "an unset flag reads dirty only when the server lacks it"
    );
}

#[test]
fn the_import_resumes_in_batches_and_finishes_once() {
    let mut db = Connection::open_in_memory().unwrap();
    let rooms = vec![
        legacy("tr_1", Some(true), None),
        legacy("tr_2", Some(false), None),
    ];
    let needed = |db: &mut Connection, who: &str| {
        run(db, who, json!({"action":"legacy"})).unwrap()["import_needed"].clone()
    };
    assert_eq!(needed(&mut db, "owner"), true);
    assert_eq!(
        moved(&import(&mut db, rooms[..1].to_vec()).unwrap()),
        [true]
    );
    // An interrupted run reads everything again; what already moved stays.
    assert_eq!(
        moved(&import(&mut db, rooms.clone()).unwrap()),
        [false, true]
    );
    edit(&mut db, "tr_1", json!({}), json!({}));
    assert_eq!(
        moved(&import(&mut db, rooms.clone()).unwrap()),
        [false, false]
    );
    assert_eq!(
        get(&mut db, "tr_1")["room"]["layers"],
        json!({}),
        "newer held work is never replaced"
    );
    let finished = run(&mut db, "owner", json!({"action":"import_finish","now":11})).unwrap();
    assert_eq!(
        (
            &finished["receipt"]["count"],
            &finished["receipt"]["skipped"]
        ),
        (&json!(2), &json!(3))
    );
    let late = import(&mut db, rooms.clone()).unwrap_err();
    assert!(
        late.starts_with(crate::import_batches::INVALID),
        "an import runs once: {late}"
    );
    assert_eq!(
        needed(&mut db, "owner"),
        true,
        "the invalidations have not moved yet"
    );
    run(
        &mut db,
        "owner",
        json!({"action":"import_invalidations","entries":[],"now":12}),
    )
    .unwrap();
    assert_eq!(
        needed(&mut db, "owner"),
        false,
        "the host never reads the rooms again"
    );
    assert_eq!(
        needed(&mut db, "someone"),
        true,
        "every account moves its own rooms"
    );
}

#[test]
fn legacy_latest_invalidations_move_for_their_owner_only() {
    let mut db = Connection::open_in_memory().unwrap();
    let key = |owner: &str| {
        format!(
            "transformer-latest-invalidation:{}",
            json!({"owner":owner,"audience":"ds-prod","lane":"canary","project":"kigali","transformer":"tr_1"})
        )
    };
    let receipt =
        json!({"working_source_version":3,"working_content_digest":"d","change_id":"c-9"});
    let entries = json!([
        {"legacy_key": key("owner"), "value": [receipt]},
        {"legacy_key": key("someone"), "value": [receipt]},
        {"legacy_key": key("owner"), "value": [{"bad": 1}]},
        {"legacy_key": "unrelated", "value": []}]);
    let answer = run(
        &mut db,
        "owner",
        json!({"action":"import_invalidations","entries":entries,"now":3}),
    )
    .unwrap();
    assert_eq!(
        (&answer["receipt"]["count"], &answer["receipt"]["skipped"]),
        (&json!(1), &json!(3))
    );
    assert_eq!(get(&mut db, "tr_1")["invalidations"], json!([receipt]));
    let again = run(
        &mut db,
        "owner",
        json!({"action":"import_invalidations","entries":[],"now":4}),
    )
    .unwrap();
    assert_eq!(again["receipt"], answer["receipt"], "an import runs once");
}

/// ds-web `src/tests/fixtures/content-digest/room.json`, the cross-language digest fixture.
const DIGEST_FIXTURE: &str = r#"{"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":"p2","geometry":{"type":"Point","coordinates":[29.7318,-2.765,1.5]},"properties":{"transfo":"TR-1","pole_number":"P2","stay":1,"dev_angle":0.1,"tiny":1e-7,"huge":1e21,"z":"zulu \"quoted\" \\ back\tslash\n<&>","unicode":"élan – 日本 😀","nil":null,"flag":false}},{"type":"Feature","id":"p1","geometry":{"type":"Point","coordinates":[29.73,-2.76]},"properties":{"transfo":"TR-1","pole_number":"P1","stay":0,"height":9,"ratio":0.000001,"neg":-1234567.5}}]},"customers":{"type":"FeatureCollection","features":[]}}"#;
