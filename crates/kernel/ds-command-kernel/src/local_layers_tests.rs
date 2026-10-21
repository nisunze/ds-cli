use super::*;

fn ask(op: Value) -> Value {
    let request = serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).expect("encodes");
    let answer = evaluate(&request).unwrap_or_else(|error| panic!("{error}"));
    serde_json::from_str(&answer).expect("the answer is JSON")
}
fn refused(op: Value) -> String {
    let answer = ask(op);
    assert_eq!(answer["outcome"], "refused", "{answer}");
    answer["refusal"]["code"]
        .as_str()
        .expect("a refusal names its code")
        .to_owned()
}
fn rejected(op: Value) -> String {
    let request = serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).expect("encodes");
    evaluate(&request).expect_err("the request itself is malformed")
}
fn browser() -> Value {
    json!({"host": "browser"})
}
fn store(layers: Vec<Value>) -> Value {
    json!({"schema": 1, "revision": 7, "scope": browser(), "layers": layers})
}
fn empty() -> Value {
    json!({"schema": 1, "revision": 0, "scope": browser(), "layers": []})
}
fn register(registry: Value, draft: Value) -> Value {
    json!({"kind": "register", "registry": registry, "scope": browser(),
           "now_ms": 1_700_000_000_000i64, "sequence": 4, "colorIndex": 3, "draft": draft})
}
fn manual_draft() -> Value {
    json!({"name": "Access road", "geometryType": "LineString"})
}
/// One admitted layer, as the registry holds it.
fn admitted(draft: Value) -> Value {
    ask(register(empty(), draft))["registry"]["layers"][0].clone()
}
fn update(registry: Value, id: &str, change: Value) -> Value {
    json!({"kind": "update", "registry": registry, "scope": browser(), "id": id, "change": change})
}

#[test]
fn register_fills_every_default_the_host_no_longer_decides() {
    let answer = ask(register(empty(), manual_draft()));
    let layer = &answer["registry"]["layers"][0];
    assert_eq!(layer["id"], "sketch-1700000000000-4");
    assert_eq!(layer["name"], "Access road");
    assert_eq!(layer["sourceName"], MANUAL_SOURCE_NAME);
    assert_eq!(layer["sourceLayerName"], "Access road");
    assert_eq!(layer["sourceKind"], "manual");
    assert_eq!(layer["geometryType"], "LineString");
    assert_eq!(layer["color"], SKETCH_COLORS[3]);
    assert_eq!(layer["lineWidth"], 2.5);
    assert_eq!(layer["pointRadius"], 5.0);
    assert_eq!(layer["fillOpacity"], 0.15);
    assert_eq!(layer["schema"], json!([]));
    assert_eq!(layer["featureCount"], 0);
    assert_eq!(layer["visible"], true);
    assert_eq!(layer["createdAt"], 1_700_000_000_000i64);
    assert!(layer.get("styleDoc").is_none() && layer.get("sourceMetadata").is_none());
    assert_eq!(
        &answer["receipt"], layer,
        "the receipt is the admitted layer"
    );
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["registry"]["revision"], 1);
}

#[test]
fn the_palette_wraps_and_belongs_to_the_kernel() {
    for index in [0u64, 9, 10, 23] {
        let mut op = register(empty(), manual_draft());
        op["colorIndex"] = json!(index);
        assert_eq!(
            ask(op)["registry"]["layers"][0]["color"],
            json!(SKETCH_COLORS[(index % 10) as usize])
        );
    }
}

#[test]
fn a_source_name_defaults_by_kind_and_never_repeats_a_manual_layers_own_name() {
    for (kind, expected) in [
        ("manual", MANUAL_SOURCE_NAME),
        ("import", IMPORT_SOURCE_NAME),
        ("derived", DERIVED_SOURCE_NAME),
        ("pinned", PINNED_SOURCE_NAME),
    ] {
        let mut draft = manual_draft();
        draft["sourceKind"] = json!(kind);
        // A pinned layer is named by its pair wherever it is registered.
        if kind == "pinned" {
            draft["pinned"] = json!({"transformer": "Poles A", "className": "mv_line"});
        }
        assert_eq!(admitted(draft)["sourceName"], expected);
    }
    let mut draft = manual_draft();
    draft["sourceName"] = json!("  Access road  ");
    assert_eq!(admitted(draft)["sourceName"], MANUAL_SOURCE_NAME);
    let mut draft = manual_draft();
    draft["sourceName"] = json!("Field survey");
    draft["sourceLayerName"] = json!("  roads  ");
    let layer = admitted(draft);
    assert_eq!(layer["sourceName"], "Field survey");
    assert_eq!(layer["sourceLayerName"], "roads");
}

#[test]
fn imported_file_metadata_belongs_only_to_an_imported_layer() {
    let metadata = json!({
        "kind": "import_file", "fileName": "roads.zip", "archiveInnerName": "roads",
        "sourcePath": "", "sizeBytes": 12.5, "lastModified": 0, "lastRefreshAt": 42,
    });
    let mut draft = manual_draft();
    draft["sourceMetadata"] = metadata.clone();
    assert!(
        admitted(draft).get("sourceMetadata").is_none(),
        "a manual layer has no imported file behind it"
    );
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("import");
    draft["sourceMetadata"] = metadata;
    assert_eq!(
        admitted(draft)["sourceMetadata"],
        json!({
            "kind": "import_file", "fileName": "roads.zip", "sourceName": "roads.zip",
            "archiveInnerName": "roads", "sizeBytes": 12, "lastRefreshAt": 42,
        }),
        "an empty path and a non-positive timestamp are absent, not zeroed"
    );
}

#[test]
fn project_work_is_refused_by_name_wherever_it_is_offered() {
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("project_work");
    assert_eq!(refused(register(empty(), draft)), UNSUPPORTED_SOURCE_KIND);
    let layer = admitted(manual_draft());
    let id = layer["id"].as_str().expect("id").to_owned();
    assert_eq!(
        refused(update(
            store(vec![layer]),
            &id,
            json!({"kind": "rename_source", "sourceKind": "project_work",
                   "sourceName": MANUAL_SOURCE_NAME, "name": "Anything"}),
        )),
        UNSUPPORTED_SOURCE_KIND
    );
}

#[test]
fn a_pinned_layer_without_its_pair_is_refused() {
    // Without the pair the id would fall back to the clock and the counter,
    // and materialising the same room again would append a second row
    // instead of answering the one already held.
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("pinned");
    assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
    // Half a pair is no pair either, and the pair belongs to a pinned layer.
    let mut half = pinned_draft("Poles A", "mv_line", json!(["name"]));
    half["pinned"]["transformer"] = json!("  ");
    assert_eq!(refused(register(empty(), half)), MALFORMED_DESCRIPTOR);
    assert!(
        admitted(pinned_draft("Poles A", "mv_line", json!(["name"])))["id"]
            .as_str()
            .expect("an id")
            .starts_with("pinned-poles-a__mv-line-")
    );
}

#[test]
fn an_id_is_minted_once_and_a_second_layer_with_it_is_refused() {
    let first = admitted(manual_draft());
    assert_eq!(
        refused(register(store(vec![first]), manual_draft())),
        DUPLICATE_LAYER,
        "the same clock and sequence mint the same id"
    );
}

#[test]
fn a_descriptor_that_breaks_its_own_bounds_is_refused_and_changes_nothing() {
    let mut draft = manual_draft();
    draft["name"] = json!("n".repeat(MAX_NAME_CHARS + 1));
    assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
    let mut draft = manual_draft();
    draft["name"] = json!("   ");
    assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
    let mut draft = manual_draft();
    draft["schema"] = json!(vec![
        json!({"name": "f", "type": "text"});
        MAX_SCHEMA_FIELDS + 1
    ]);
    assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
}

#[test]
fn a_full_registry_admits_nothing_more() {
    let layer = admitted(manual_draft());
    let layers: Vec<Value> = (0..MAX_LAYERS)
        .map(|index| {
            let mut row = layer.clone();
            row["id"] = json!(format!("sketch-{index}"));
            row
        })
        .collect();
    assert_eq!(
        refused(register(store(layers), manual_draft())),
        MALFORMED_DESCRIPTOR
    );
}

#[test]
fn parked_chunk_storage_is_normalised_at_admission() {
    let mut draft = manual_draft();
    draft["featureCount"] = json!(9);
    draft["featureStorage"] = json!({
        "kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
        "parsedLayer": "rows", "renderMode": "preview", "previewLimit": 9_000_000,
    });
    let layer = admitted(draft);
    assert_eq!(layer["featureCount"], 9);
    assert_eq!(
        layer["featureStorage"],
        json!({
            "kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
            "parsedLayer": "rows", "idPrefix": "feat-sketch-1700000000000-4",
            "renderMode": "preview", "previewLimit": MAX_PREVIEW_LIMIT,
            "renderPreviewLoaded": false,
        })
    );
    let mut draft = manual_draft();
    draft["featureStorage"] = json!({"kind": "upload_chunks", "workspaceKey": "w1"});
    assert_eq!(
        refused(register(empty(), draft)),
        MALFORMED_DESCRIPTOR,
        "storage that cannot locate its chunks is a malformed descriptor, not a payload to lose"
    );
    let mut draft = manual_draft();
    draft["featureStorage"] = Value::Null;
    assert!(
        admitted(draft).get("featureStorage").is_none(),
        "null is no storage at all"
    );
}

#[test]
fn a_negative_feature_count_is_refused_wherever_a_host_asserts_it() {
    let mut draft = manual_draft();
    draft["featureCount"] = json!(-1);
    assert_eq!(refused(register(empty(), draft)), MALFORMED_DESCRIPTOR);
    let layer = admitted(manual_draft());
    let id = layer["id"].as_str().expect("id").to_owned();
    assert_eq!(
        refused(update(
            store(vec![layer.clone()]),
            &id,
            json!({"kind": "payload", "schema": [], "featureCount": -3, "featureStorage": null}),
        )),
        MALFORMED_DESCRIPTOR
    );
    assert_eq!(
        refused(update(
            store(vec![layer]),
            &id,
            json!({"kind": "payload", "schema": [], "featureCount": 3,
                   "featureStorage": {"kind": "upload_chunks", "workspaceKey": "w1"}}),
        )),
        MALFORMED_DESCRIPTOR,
        "an update never parks a payload in chunks it cannot locate"
    );
}

#[test]
fn list_answers_bounded_rows_with_the_origin() {
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("import");
    draft["sourceMetadata"] = json!({
        "kind": "import_file", "fileName": "roads.zip", "archiveInnerName": "roads",
        "sourcePath": "/home/me/gis/roads.zip", "lastRefreshAt": 11, "dirtySinceRefresh": 12,
    });
    let layer = admitted(draft);
    let answer = ask(
        json!({"kind": "list", "registry": store(vec![layer.clone()]),
                            "scope": browser(), "limit": 1}),
    );
    assert_eq!(answer["changed"], false);
    assert_eq!(answer["registry"]["revision"], 7, "a list is not a change");
    let row = &answer["receipt"]["layers"][0];
    assert_eq!(answer["receipt"]["count"], 1);
    assert_eq!(
        row["origin"],
        json!({"kind": "import_file", "label": "roads.zip → roads", "dirtySinceRefresh": 12})
    );
    assert!(!row.to_string().contains("/home/me"));
    for key in [
        "id",
        "name",
        "sourceName",
        "sourceLayerName",
        "sourceKind",
        "geometryType",
        "featureCount",
        "visible",
        "createdAt",
    ] {
        assert!(row.get(key).is_some(), "a row carries {key}");
    }
    for key in ["color", "schema", "styleDoc", "sourceMetadata"] {
        assert!(
            row.get(key).is_none(),
            "a row is bounded, {key} is not in it"
        );
    }
    let many = store(vec![layer.clone(), {
        let mut other = layer;
        other["id"] = json!("sketch-2");
        other
    }]);
    let answer = ask(json!({"kind": "list", "registry": many, "scope": browser(), "limit": 1}));
    assert_eq!(
        answer["receipt"]["layers"].as_array().expect("rows").len(),
        1
    );
    assert_eq!(answer["receipt"]["count"], 2, "the count is the catalogue");
}

fn restore(records: Vec<Value>) -> Value {
    ask(
        json!({"kind": "restore", "scope": browser(), "records": records,
               "now_ms": 1_700_000_000_000i64}),
    )
}

#[test]
fn restore_repairs_a_legacy_row_instead_of_losing_it() {
    let answer = restore(vec![json!({
        "name": "Old layer", "geometryType": "Polygon", "lineWidth": 99, "pointRadius": 0,
        "fillOpacity": -1, "features": [{"id": "f1"}, {"id": "f2"}],
    })]);
    let layer = &answer["registry"]["layers"][0];
    assert_eq!(
        layer["id"], "sketch-1700000000000-0",
        "minted from clock and place"
    );
    assert_eq!(layer["color"], SKETCH_COLORS[0], "the palette answers");
    assert_eq!(layer["lineWidth"], MAX_LINE_WIDTH);
    assert_eq!(layer["pointRadius"], MIN_POINT_RADIUS);
    assert_eq!(layer["fillOpacity"], 0.0);
    assert_eq!(layer["visible"], true, "an absent visible reads as visible");
    assert_eq!(layer["featureCount"], 2, "counted from the legacy payload");
    assert_eq!(layer["createdAt"], 1_700_000_000_000i64);
    assert_eq!(layer["sourceName"], MANUAL_SOURCE_NAME);
    assert_eq!(layer["sourceLayerName"], "Old layer");
    assert_eq!(answer["receipt"]["restored"], 1);
    assert_eq!(answer["changed"], true, "the host must rewrite its store");
    let repaired = &answer["receipt"]["repaired"][0];
    assert_eq!(repaired["id"], "sketch-1700000000000-0");
    assert_eq!(
        repaired["fields"],
        json!([
            "id",
            "sourceName",
            "sourceLayerName",
            "sourceKind",
            "color",
            "lineWidth",
            "pointRadius",
            "fillOpacity",
            "schema",
            "featureCount",
            "visible",
            "createdAt"
        ]),
        "every descriptor key the row did not already carry as admitted"
    );
    assert!(
        !repaired["fields"].to_string().contains("name\""),
        "the name the row carried is the name it keeps"
    );
}

#[test]
fn a_row_that_needs_no_repair_is_not_reported_as_repaired() {
    let layer = admitted(manual_draft());
    let answer = restore(vec![layer.clone()]);
    assert_eq!(answer["registry"]["layers"][0], layer);
    assert_eq!(answer["receipt"]["repaired"], json!([]));
    assert_eq!(answer["changed"], false);
    assert_eq!(answer["registry"]["revision"], 0);
}

#[test]
fn colourless_rows_take_the_palette_in_turn() {
    let answer = restore(vec![
        json!({"id": "a", "color": "#123456"}),
        json!({"id": "b"}),
        json!({"id": "c"}),
    ]);
    let layers = &answer["registry"]["layers"];
    assert_eq!(layers[0]["color"], "#123456");
    assert_eq!(layers[1]["color"], SKETCH_COLORS[0]);
    assert_eq!(layers[2]["color"], SKETCH_COLORS[1]);
}

#[test]
fn project_work_rows_are_retired_with_their_keys_named_for_release() {
    let answer = restore(vec![
        json!({"id": "keep", "name": "Kept"}),
        json!({"id": "pw", "sourceKind": "project_work", "featureStorage": {
            "kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l",
        }}),
        json!({"sourceKind": "project_work"}),
    ]);
    assert_eq!(answer["receipt"]["restored"], 1);
    assert_eq!(
        answer["receipt"]["retired"],
        json!(["pw", "sketch-1700000000000-2"])
    );
    assert_eq!(
        answer["receipt"]["release"],
        json!([
            {"kind": "features_payload", "id": "pw"},
            {"kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l"},
            {"kind": "features_payload", "id": "sketch-1700000000000-2"},
        ])
    );
    assert_eq!(
        answer["registry"]["layers"]
            .as_array()
            .expect("layers")
            .len(),
        1
    );
}

#[test]
fn a_row_whose_id_is_malformed_is_carried_verbatim_and_never_re_keyed() {
    // The id carries a control character, so it is not admissible — and
    // repairing it would key the reopened row somewhere the host never
    // parked its payload. The good row beside it still reopens.
    let bad = json!({"id": "sketch-1\u{7}-2", "name": "Broken", "featureStorage": {
        "kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l",
    }});
    let kept = admitted(manual_draft());
    let answer = restore(vec![kept.clone(), bad.clone()]);
    let layers = answer["registry"]["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 1, "only the admissible row is reopened");
    assert_eq!(layers[0], kept, "and it reopens exactly as it was");
    assert_eq!(answer["receipt"]["restored"], 1);
    assert_eq!(answer["receipt"]["carried"][0]["raw"], bad, "verbatim");
    assert_eq!(
        answer["receipt"]["carried"][0]["reason"],
        "a local layer id is never repaired, and this row's id carries a control character"
    );
    assert_eq!(
        answer["receipt"]["release"],
        json!([]),
        "a carried row is not destroyed, so nothing of it is released"
    );
    assert_eq!(answer["receipt"]["retired"], json!([]));
    assert_eq!(answer["receipt"]["repaired"], json!([]));
    assert_eq!(
        answer["changed"], false,
        "the host's bytes are what they were: admitted rows plus this one"
    );
}

#[test]
fn every_id_rule_carries_its_row_rather_than_refusing_the_whole_store() {
    // Each shape `LocalLayer::validate` refuses, and what used to happen to
    // it: a traversal id refused the WHOLE store (every layer in it lost to
    // its owner), an over-long or untrimmed one was silently truncated or
    // trimmed into a different record.
    for (id, reason) in [
        ("../escape", "is not one path segment"),
        ("a/b", "is not one path segment"),
        (" padded ", "is not trimmed"),
        (
            "x".repeat(MAX_NAME_CHARS + 1).as_str(),
            "is longer than 200 characters",
        ),
    ] {
        let answer = restore(vec![
            admitted(manual_draft()),
            json!({"id": id, "name": "Carried"}),
        ]);
        assert_eq!(
            answer["registry"]["layers"]
                .as_array()
                .expect("layers")
                .len(),
            1,
            "{id}: the rest of the store still reopens"
        );
        assert_eq!(answer["receipt"]["carried"][0]["raw"]["id"], id);
        assert!(
            answer["receipt"]["carried"][0]["reason"]
                .as_str()
                .is_some_and(|said| said.ends_with(reason)),
            "{id}: the receipt says which rule the id broke"
        );
    }
}

#[test]
fn a_carried_row_is_carried_again_on_every_reopen_and_keeps_its_chunks() {
    // Stability: the host persists the admitted rows and the carried raw
    // rows together, and reopening THAT store answers the same thing again.
    // A retired row parked in the same chunks releases nothing either — the
    // carried row still parks its payload there.
    let carried = json!({"id": "sketch-\u{7}", "name": "Broken", "featureStorage": {
        "kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l",
    }});
    let first = restore(vec![
        json!({"id": "keep", "name": "Kept"}),
        carried.clone(),
        json!({"id": "pw", "sourceKind": "project_work", "featureStorage": {
            "kind": "upload_chunks", "workspaceKey": "w", "fileName": "f", "parsedLayer": "l",
        }}),
    ]);
    assert_eq!(
        first["receipt"]["release"],
        json!([{"kind": "features_payload", "id": "pw"}]),
        "the chunks the carried row parks in are not the retired row's to release"
    );
    let mut stored: Vec<Value> = first["registry"]["layers"]
        .as_array()
        .expect("layers")
        .clone();
    stored.push(first["receipt"]["carried"][0]["raw"].clone());
    let again = restore(stored);
    assert_eq!(again["receipt"]["carried"][0]["raw"], carried);
    assert_eq!(again["receipt"]["restored"], 1);
    assert_eq!(
        again["changed"], false,
        "a reopen of an answer is no change"
    );
}

#[test]
fn a_duplicate_id_refuses_the_whole_store_rather_than_dropping_a_layer() {
    let answer = ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1,
                            "records": [json!({"id": "a"}), json!({"id": "a"})]}));
    assert_eq!(answer["refusal"]["code"], DUPLICATE_LAYER);
    assert!(answer.get("registry").is_none(), "nothing is reopened");
}

#[test]
fn restore_reads_only_objects_and_only_a_bounded_store() {
    assert_eq!(
        ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1, "records": [7]}))["refusal"]
            ["code"],
        MALFORMED_DESCRIPTOR
    );
    let records: Vec<Value> = (0..=MAX_LAYERS)
        .map(|i| json!({"id": i.to_string()}))
        .collect();
    assert_eq!(
        ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1, "records": records}))
            ["refusal"]["code"],
        MALFORMED_DESCRIPTOR
    );
}

#[test]
fn a_replaced_payload_names_the_chunks_it_left_behind() {
    let mut draft = manual_draft();
    draft["featureStorage"] = json!({"kind": "upload_chunks", "workspaceKey": "w1",
                                     "fileName": "big.csv", "parsedLayer": "rows"});
    let layer = admitted(draft);
    let id = layer["id"].as_str().expect("id").to_owned();
    let answer = ask(update(
        store(vec![layer.clone()]),
        &id,
        json!({"kind": "payload", "schema": [{"name": "road", "type": "text"}],
               "featureCount": 3, "featureStorage": null}),
    ));
    let next = &answer["registry"]["layers"][0];
    assert_eq!(next["featureCount"], 3);
    assert_eq!(next["schema"], json!([{"name": "road", "type": "text"}]));
    assert!(next.get("featureStorage").is_none());
    assert_eq!(
        answer["receipt"],
        json!({"id": id, "applied": "payload", "release": [
            {"kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
             "parsedLayer": "rows"}]})
    );
    assert_eq!(answer["registry"]["revision"], 8);
    // Re-parking the same chunks releases nothing: the payload is the one just written.
    let same = ask(update(
        store(vec![layer.clone()]),
        &id,
        json!({"kind": "payload", "schema": [], "featureCount": 3,
               "featureStorage": {"kind": "upload_chunks", "workspaceKey": "w1",
                                  "fileName": "big.csv", "parsedLayer": "rows"}}),
    ));
    assert_eq!(same["receipt"]["release"], json!([]));
    // Chunks another layer still parks its payload in are that layer's storage.
    let mut sibling = layer.clone();
    sibling["id"] = json!("sketch-sibling");
    let shared = ask(update(
        store(vec![layer, sibling]),
        &id,
        json!({"kind": "payload", "schema": [], "featureCount": 3, "featureStorage": null}),
    ));
    assert_eq!(
        shared["receipt"]["release"],
        json!([]),
        "a release never names another layer's storage"
    );
    assert!(shared["registry"]["layers"][1]
        .get("featureStorage")
        .is_some());
}

#[test]
fn a_rename_follows_the_source_layer_name_only_for_a_manual_layer() {
    let manual = admitted(manual_draft());
    let id = manual["id"].as_str().expect("id").to_owned();
    let answer = ask(update(
        store(vec![manual]),
        &id,
        json!({"kind": "rename", "name": " New "}),
    ));
    let next = &answer["registry"]["layers"][0];
    assert_eq!(next["name"], "New");
    assert_eq!(next["sourceLayerName"], "New");
    assert_eq!(answer["receipt"]["applied"], "rename");

    let mut draft = manual_draft();
    draft["sourceKind"] = json!("import");
    draft["sourceName"] = json!("roads.zip");
    draft["sourceLayerName"] = json!("roads");
    let imported = admitted(draft);
    let answer = ask(update(
        store(vec![imported]),
        &id,
        json!({"kind": "rename", "name": "Renamed"}),
    ));
    let next = &answer["registry"]["layers"][0];
    assert_eq!(next["name"], "Renamed");
    assert_eq!(
        next["sourceLayerName"], "roads",
        "the name inside the source is unchanged"
    );
    assert_eq!(next["sourceName"], "roads.zip");
    assert_eq!(
        refused(update(
            store(vec![]),
            &id,
            json!({"kind": "rename", "name": " "})
        )),
        UNKNOWN_LAYER
    );
}

#[test]
fn renaming_a_source_renames_every_layer_filed_under_it() {
    let first = admitted(manual_draft());
    let id = first["id"].as_str().expect("id").to_owned();
    let mut second = first.clone();
    second["id"] = json!("sketch-2");
    second["name"] = json!("Other");
    let mut elsewhere = first.clone();
    elsewhere["id"] = json!("sketch-3");
    elsewhere["sourceName"] = json!("Field survey");
    let answer = ask(update(
        store(vec![first, second, elsewhere]),
        &id,
        json!({"kind": "rename_source", "sourceKind": "manual",
               "sourceName": MANUAL_SOURCE_NAME, "name": "  Drawn today  "}),
    ));
    let layers = &answer["registry"]["layers"];
    assert_eq!(layers[0]["sourceName"], "Drawn today");
    assert_eq!(layers[1]["sourceName"], "Drawn today");
    assert_eq!(
        layers[2]["sourceName"], "Field survey",
        "another source is untouched"
    );
    assert_eq!(answer["receipt"]["applied"], "rename_source");
}

#[test]
fn style_is_admitted_in_bounds_refused_outside_them_and_a_null_document_clears_it() {
    let layer = admitted(manual_draft());
    let id = layer["id"].as_str().expect("id").to_owned();
    for out_of_range in [
        json!({"kind": "style", "lineWidth": 900}),
        json!({"kind": "style", "lineWidth": 0.4}),
        json!({"kind": "style", "pointRadius": 0}),
        json!({"kind": "style", "pointRadius": 25}),
        json!({"kind": "style", "fillOpacity": 2}),
        json!({"kind": "style", "fillOpacity": -0.1}),
        json!({"kind": "style", "color": "#000000", "fillOpacity": 2}),
    ] {
        assert_eq!(
            refused(update(store(vec![layer.clone()]), &id, out_of_range)),
            MALFORMED_DESCRIPTOR,
            "only restore clamps; an update outside the range is refused whole"
        );
    }
    let answer = ask(update(
        store(vec![layer]),
        &id,
        json!({"kind": "style", "color": " #000000 ", "lineWidth": MAX_LINE_WIDTH,
               "pointRadius": MIN_POINT_RADIUS, "fillOpacity": 1, "styleDoc": {"paint": {}}}),
    ));
    let next = &answer["registry"]["layers"][0];
    assert_eq!(next["color"], "#000000");
    assert_eq!(next["lineWidth"], MAX_LINE_WIDTH);
    assert_eq!(next["pointRadius"], MIN_POINT_RADIUS);
    assert_eq!(next["fillOpacity"], 1.0);
    assert_eq!(next["styleDoc"], json!({"paint": {}}));
    let cleared = ask(update(
        store(vec![next.clone()]),
        &id,
        json!({"kind": "style", "styleDoc": null}),
    ));
    assert!(cleared["registry"]["layers"][0].get("styleDoc").is_none());
    let untouched = ask(update(
        store(vec![next.clone()]),
        &id,
        json!({"kind": "style"}),
    ));
    assert_eq!(
        untouched["registry"]["layers"][0]["styleDoc"],
        json!({"paint": {}})
    );
    assert_eq!(
        untouched["changed"], false,
        "a style change that changes nothing is not a revision"
    );
    assert_eq!(untouched["registry"]["revision"], 7);
    assert_eq!(
        refused(update(
            store(vec![next.clone()]),
            &id,
            json!({"kind": "style", "color": "  "})
        )),
        MALFORMED_DESCRIPTOR
    );
    assert_eq!(
        refused(update(
            store(vec![next.clone()]),
            &id,
            json!({"kind": "style", "styleDoc": 4})
        )),
        MALFORMED_DESCRIPTOR
    );
}

#[test]
fn visibility_and_source_metadata_are_persisted_decisions() {
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("import");
    let layer = admitted(draft);
    let id = layer["id"].as_str().expect("id").to_owned();
    let hidden = ask(update(
        store(vec![layer]),
        &id,
        json!({"kind": "visible", "visible": false}),
    ));
    assert_eq!(hidden["registry"]["layers"][0]["visible"], false);
    assert_eq!(
        hidden["receipt"],
        json!({"id": id, "applied": "visible", "release": []})
    );
    let attached = ask(update(
        store(vec![hidden["registry"]["layers"][0].clone()]),
        &id,
        json!({"kind": "source_metadata", "sourceMetadata": {
            "kind": "import_file", "fileName": "roads.geojson", "lastRefreshAt": 99}}),
    ));
    assert_eq!(
        attached["registry"]["layers"][0]["sourceMetadata"]["fileName"],
        "roads.geojson"
    );
    let detached = ask(update(
        store(vec![attached["registry"]["layers"][0].clone()]),
        &id,
        json!({"kind": "source_metadata", "sourceMetadata": null}),
    ));
    assert!(detached["registry"]["layers"][0]
        .get("sourceMetadata")
        .is_none());
}

#[test]
fn metadata_with_no_refresh_time_and_no_clock_is_not_metadata() {
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("import");
    let layer = admitted(draft);
    let id = layer["id"].as_str().expect("id").to_owned();
    let metadata = json!({"kind": "import_file", "fileName": "roads.geojson"});
    let blind = ask(update(
        store(vec![layer.clone()]),
        &id,
        json!({"kind": "source_metadata", "sourceMetadata": metadata.clone()}),
    ));
    assert!(blind["registry"]["layers"][0]
        .get("sourceMetadata")
        .is_none());
    let mut op = update(
        store(vec![layer]),
        &id,
        json!({"kind": "source_metadata", "sourceMetadata": metadata}),
    );
    op["now_ms"] = json!(1_700_000_000_000i64);
    assert_eq!(
        ask(op)["registry"]["layers"][0]["sourceMetadata"]["lastRefreshAt"],
        1_700_000_000_000i64
    );
}

#[test]
fn remove_names_the_layers_own_records_and_nothing_else() {
    let mut draft = manual_draft();
    draft["sourceKind"] = json!("import");
    draft["sourceMetadata"] = json!({"kind": "import_file", "fileName": "roads.geojson",
                                     "sourcePath": "/home/me/gis/roads.geojson",
                                     "lastRefreshAt": 9});
    draft["featureStorage"] = json!({"kind": "upload_chunks", "workspaceKey": "w1",
                                     "fileName": "big.csv", "parsedLayer": "rows"});
    let layer = admitted(draft);
    let id = layer["id"].as_str().expect("id").to_owned();
    let mut other = layer.clone();
    other["id"] = json!("sketch-other");
    other["featureStorage"]["parsedLayer"] = json!("other-rows");
    let mut sharing = layer.clone();
    sharing["id"] = json!("sketch-sharing");
    let answer = ask(
        json!({"kind": "remove", "registry": store(vec![layer.clone(), sharing]),
                            "scope": browser(), "id": id}),
    );
    assert_eq!(
        answer["receipt"]["release"],
        json!([{"kind": "features_payload", "id": id}]),
        "chunks another layer still parks in are never named"
    );
    let answer = ask(
        json!({"kind": "remove", "registry": store(vec![layer, other]),
                            "scope": browser(), "id": id}),
    );
    assert_eq!(
        answer["receipt"],
        json!({"id": id, "removed": true, "release": [
            {"kind": "features_payload", "id": id},
            {"kind": "upload_chunks", "workspaceKey": "w1", "fileName": "big.csv",
             "parsedLayer": "rows"}]})
    );
    let release = answer["receipt"]["release"].to_string();
    assert!(
        !release.contains("/home/me"),
        "a source file is never named for deletion"
    );
    assert!(
        !release.contains("sketch-other"),
        "another layer is never named"
    );
    assert_eq!(
        answer["registry"]["layers"]
            .as_array()
            .expect("layers")
            .len(),
        1
    );
    assert_eq!(answer["registry"]["revision"], 8);
}

#[test]
fn an_unknown_layer_and_another_scope_change_nothing() {
    let layer = admitted(manual_draft());
    let id = layer["id"].as_str().expect("id").to_owned();
    let registry = store(vec![layer]);
    assert_eq!(
        refused(update(
            registry.clone(),
            "sketch-nope",
            json!({"kind": "visible", "visible": false})
        )),
        UNKNOWN_LAYER
    );
    assert_eq!(
        refused(
            json!({"kind": "remove", "registry": registry.clone(), "scope": browser(),
                       "id": "sketch-nope"})
        ),
        UNKNOWN_LAYER
    );
    let native = json!({"host": "native", "lane": "run", "uid": "u1"});
    for op in [
        json!({"kind": "list", "registry": registry.clone(), "scope": native}),
        json!({"kind": "update", "registry": registry.clone(), "scope": native, "id": id,
               "change": {"kind": "visible", "visible": true}}),
        json!({"kind": "remove", "registry": registry, "scope": native, "id": id}),
    ] {
        assert_eq!(refused(op), SCOPE_MISMATCH);
    }
}

#[test]
fn a_registry_that_is_not_a_catalogue_is_refused_before_anything_is_read() {
    let layer = admitted(manual_draft());
    let mut twice = store(vec![layer.clone(), layer.clone()]);
    assert_eq!(
        refused(json!({"kind": "list", "registry": twice.clone(), "scope": browser()})),
        DUPLICATE_LAYER
    );
    twice["layers"][1]["id"] = json!("sketch-2");
    twice["layers"][1]["lineWidth"] = json!(99);
    assert_eq!(
        refused(json!({"kind": "list", "registry": twice, "scope": browser()})),
        MALFORMED_DESCRIPTOR
    );
    let mut retired = store(vec![layer]);
    retired["layers"][0]["sourceKind"] = json!("project_work");
    assert_eq!(
        refused(json!({"kind": "list", "registry": retired, "scope": browser()})),
        UNSUPPORTED_SOURCE_KIND
    );
}

#[test]
fn a_malformed_request_is_the_hosts_mistake_not_a_refusal() {
    assert!(evaluate(b"{").is_err());
    assert!(evaluate(
        &serde_json::to_vec(
            &json!({"schema": "ds.other/v1", "op": {"kind": "list", "registry": empty(),
                "scope": browser()}})
        )
        .expect("encodes")
    )
    .is_err());
    assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    assert!(rejected(json!({"kind": "sideways"})).contains("invalid local layer request"));
    let mut op = json!({"kind": "list", "registry": empty(), "scope": browser()});
    op["extra"] = json!(true);
    assert!(rejected(op).contains("unknown field"));
    let mut op = register(empty(), manual_draft());
    op["now_ms"] = json!(0);
    assert!(rejected(op).contains("clock"));
    assert!(rejected(json!({"kind": "list", "registry": empty(),
                            "scope": {"host": "browser", "lane": "run"}}))
    .contains("browser scope"));
    assert!(
        rejected(json!({"kind": "list", "registry": empty(), "scope": {"host": "native"}}))
            .contains("native scope")
    );
}

#[test]
fn a_retired_row_never_names_a_kept_layers_record_for_release() {
    // The same id on a retired and a kept row is one id twice: the store
    // refuses whole rather than releasing the kept layer's payload.
    for records in [
        vec![
            json!({"id": "a", "sourceKind": "project_work"}),
            json!({"id": "a", "name": "Kept"}),
        ],
        vec![
            json!({"id": "a", "name": "Kept"}),
            json!({"id": "a", "sourceKind": "project_work"}),
        ],
        vec![
            json!({"id": "a", "sourceKind": "project_work"}),
            json!({"id": "a", "sourceKind": "project_work"}),
        ],
    ] {
        let answer = ask(json!({"kind": "restore", "scope": browser(), "now_ms": 1,
                                "records": records}));
        assert_eq!(answer["refusal"]["code"], DUPLICATE_LAYER);
        assert!(answer.get("registry").is_none());
    }
    // Chunks a kept layer still parks in are its storage, whichever row
    // named them first.
    let storage = json!({"kind": "upload_chunks", "workspaceKey": "w", "fileName": "f",
                         "parsedLayer": "l"});
    let answer = restore(vec![
        json!({"id": "pw", "sourceKind": "project_work", "featureStorage": storage}),
        json!({"id": "kept", "name": "Kept", "featureStorage": storage}),
    ]);
    assert_eq!(answer["receipt"]["retired"], json!(["pw"]));
    assert_eq!(
        answer["receipt"]["release"],
        json!([{"kind": "features_payload", "id": "pw"}])
    );
    assert_eq!(
        answer["registry"]["layers"][0]["featureStorage"]["parsedLayer"],
        "l"
    );
}

#[test]
fn renaming_a_source_never_files_a_manual_layer_under_its_own_name() {
    let first = admitted(manual_draft());
    let id = first["id"].as_str().expect("id").to_owned();
    let mut second = first.clone();
    second["id"] = json!("sketch-2");
    second["name"] = json!("Other");
    let answer = ask(update(
        store(vec![first, second]),
        &id,
        json!({"kind": "rename_source", "sourceKind": "manual",
               "sourceName": MANUAL_SOURCE_NAME, "name": "Access road"}),
    ));
    let layers = &answer["registry"]["layers"];
    assert_eq!(
        layers[0]["sourceName"], MANUAL_SOURCE_NAME,
        "the layer named Access road is not filed under Access road"
    );
    assert_eq!(layers[1]["sourceName"], "Access road");
    // The admitted registry reopens as itself: nothing left to repair.
    let reopened = restore(layers.as_array().expect("layers").clone());
    assert_eq!(reopened["receipt"]["repaired"], json!([]));
}

#[test]
fn a_native_catalogue_is_fenced_by_lane_and_uid() {
    let owner = json!({"host": "native", "lane": "run", "uid": "u1"});
    let mut op = register(empty(), manual_draft());
    op["registry"]["scope"] = owner.clone();
    op["scope"] = owner.clone();
    let registry = ask(op)["registry"].clone();
    let id = registry["layers"][0]["id"].as_str().expect("id").to_owned();
    for stranger in [
        json!({"host": "native", "lane": "run", "uid": "u2"}),
        json!({"host": "native", "lane": "canary", "uid": "u1"}),
        json!({"host": "browser"}),
    ] {
        let mut register_op = register(registry.clone(), manual_draft());
        register_op["scope"] = stranger.clone();
        assert_eq!(refused(register_op), SCOPE_MISMATCH);
        assert_eq!(
            refused(json!({"kind": "list", "registry": registry.clone(), "scope": stranger})),
            SCOPE_MISMATCH
        );
        assert_eq!(
            refused(
                json!({"kind": "update", "registry": registry.clone(), "scope": stranger,
                           "id": id, "change": {"kind": "visible", "visible": false}})
            ),
            SCOPE_MISMATCH
        );
        assert_eq!(
            refused(
                json!({"kind": "remove", "registry": registry.clone(), "scope": stranger,
                           "id": id})
            ),
            SCOPE_MISMATCH
        );
    }
    let mut own = json!({"kind": "remove", "registry": registry, "scope": owner, "id": id});
    assert_eq!(ask(own.clone())["changed"], true);
    own["scope"]["uid"] = json!("u2");
    assert_eq!(refused(own), SCOPE_MISMATCH);
}

#[test]
fn a_native_store_is_the_same_catalogue_under_another_scope() {
    let scope = json!({"host": "native", "lane": "run", "uid": "u1"});
    let answer = ask(json!({"kind": "restore", "scope": scope, "records": [],
                            "now_ms": 1_700_000_000_000i64}));
    assert_eq!(
        answer["registry"],
        json!({"schema": 1, "revision": 0, "scope": scope,
                                          "layers": []})
    );
    let mut op = register(answer["registry"].clone(), manual_draft());
    op["scope"] = scope;
    let answer = ask(op);
    assert_eq!(
        answer["registry"]["layers"][0]["id"],
        "sketch-1700000000000-4"
    );
}
// ── what a fixing review found, each rule on its own ─────────────────

#[test]
fn restore_repairs_a_source_label_it_cannot_admit_instead_of_stranding_the_store() {
    // A legacy row filed under an unbounded, control-laden label used to
    // refuse the WHOLE reopen — and a browser that cannot reopen shows no
    // layer at all. The label is now repaired like every other text field.
    let long = "x".repeat(MAX_TEXT_CHARS + 100);
    let answer = restore(vec![
        json!({"id": "s1", "name": "Roads", "sourceName": format!("{long}\u{7}"),
               "geometryType": "LineString", "color": "#123456"}),
        json!({"id": "s2", "name": "Poles", "sourceName": "\u{1}\u{2}",
               "geometryType": "Point", "color": "#123456"}),
    ]);
    let layers = answer["registry"]["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 2, "no row is stranded: {answer}");
    assert_eq!(
        layers[0]["sourceName"].as_str().expect("a label").len(),
        MAX_TEXT_CHARS,
        "the label is trimmed to its bound, not refused"
    );
    assert_eq!(
        layers[1]["sourceName"], MANUAL_SOURCE_NAME,
        "a label with nothing left in it falls back to the kind's own"
    );
    for row in 0..2 {
        assert!(
            answer["receipt"]["repaired"][row]["fields"]
                .as_array()
                .expect("repaired fields")
                .contains(&json!("sourceName")),
            "the host is told to rewrite the label it carried"
        );
    }
    // The same rule for the rest of a row: metadata and storage that
    // cannot be admitted are dropped, never turned into a refusal.
    let answer = restore(vec![
        json!({"id": "s3", "name": "Roads", "geometryType": "Point",
                                     "color": "#123456",
                                     "sourceMetadata": {"kind": "gis_folder", "dsProject": "p1",
                                                        "handleKey": "\u{1}", "folderName": "gis",
                                                        "filePath": "gis/roads.geojson",
                                                        "lastRefreshAt": 9},
                                     "featureStorage": {"kind": "upload_chunks",
                                                        "workspaceKey": "\u{1}", "fileName": "r.geojson",
                                                        "parsedLayer": "roads"}}),
    ]);
    assert_eq!(
        answer["registry"]["layers"]
            .as_array()
            .expect("layers")
            .len(),
        1
    );
    assert_eq!(
        answer["registry"]["layers"][0]["sourceMetadata"],
        Value::Null
    );
    assert_eq!(
        answer["registry"]["layers"][0]["featureStorage"],
        Value::Null
    );
}

/// The draft a host offers for a pinned-transformer row.
fn pinned_draft(transformer: &str, class_name: &str, fields: Value) -> Value {
    json!({"name": "Poles", "geometryType": "Point", "sourceKind": "pinned",
           "sourceName": transformer, "sourceLayerName": class_name, "featureCount": 3,
           "pinned": {"transformer": transformer, "className": class_name, "fields": fields}})
}

#[test]
fn a_pinned_pair_names_its_layer_and_two_pairs_never_share_one_id() {
    let first = admitted(pinned_draft(
        "Poles A",
        "mv_line",
        json!(["name", "id", "name"]),
    ));
    let id = first["id"].as_str().expect("an id").to_owned();
    assert!(
        id.starts_with("pinned-poles-a__mv-line-"),
        "the id reads as the pair it was materialised from: {id}"
    );
    assert!(one_segment(&id) && bounded(&id, MAX_NAME_CHARS), "{id}");
    // The browser lower-cased and folded punctuation into one character,
    // so `Poles A` and `Poles-A` were ONE layer. The fingerprint is taken
    // over the pair itself, so the two pairs are two layers.
    let second = admitted(pinned_draft("Poles-A", "mv_line", json!(["name", "id"])));
    assert_ne!(first["id"], second["id"], "distinct pairs stay distinct");
    assert!(second["id"]
        .as_str()
        .expect("an id")
        .starts_with("pinned-poles-a__mv-line-"));
    // The class colour, the flat style and the columns are the kernel's.
    assert_eq!(first["color"], PINNED_COLOR);
    assert_eq!(first["lineWidth"], 2.5);
    assert_eq!(first["pointRadius"], 5.0);
    assert_eq!(first["fillOpacity"], 0.15);
    assert_eq!(
        first["schema"],
        json!([{"name": "id", "type": "text"}, {"name": "name", "type": "text"}]),
        "the property keys, sanitised, ordered and de-duplicated"
    );
    assert_eq!(first["sourceName"], "Poles A");
    assert_eq!(first["visible"], true);
}

#[test]
fn materialising_a_pinned_pair_again_answers_the_layer_already_held() {
    let mut layer = admitted(pinned_draft("Poles A", "mv_line", json!(["name"])));
    // What the user did to it since: hidden, and painted by the class.
    layer["visible"] = json!(false);
    layer["color"] = json!("#ff0000");
    let registry = store(vec![layer.clone()]);
    let answer = ask(register(
        registry.clone(),
        pinned_draft("Poles A", "mv_line", json!(["name", "phase"])),
    ));
    assert_eq!(answer["changed"], false, "one pair is one layer");
    assert_eq!(answer["receipt"], layer, "the descriptor already held");
    assert_eq!(answer["registry"], registry, "and nothing was rewritten");
    // A pinned identity is a pinned layer's; half a pair identifies
    // nothing; and the columns are derived, so a host offers one or the
    // other, never both.
    let mut wrong_kind = pinned_draft("Poles A", "mv_line", json!(["name"]));
    wrong_kind["sourceKind"] = json!("manual");
    assert_eq!(refused(register(empty(), wrong_kind)), MALFORMED_DESCRIPTOR);
    let mut half = pinned_draft("Poles A", "mv_line", json!(["name"]));
    half["pinned"]["className"] = json!("  ");
    assert_eq!(refused(register(empty(), half)), MALFORMED_DESCRIPTOR);
    let mut both = pinned_draft("Poles A", "mv_line", json!(["name"]));
    both["schema"] = json!([{"name": "name", "type": "text"}]);
    assert_eq!(refused(register(empty(), both)), MALFORMED_DESCRIPTOR);
    // And a pinned layer without a pair at all is refused rather than
    // minted from the clock: an id no pair derives can never be
    // rematerialised idempotently, which is the whole point of the pair
    // (`a_pinned_layer_without_its_pair_is_refused`).
    assert_eq!(
        refused(register(
            empty(),
            json!({"name": "Poles", "geometryType": "Point", "sourceKind": "pinned"})
        )),
        MALFORMED_DESCRIPTOR
    );
}

#[test]
fn an_id_is_one_path_segment_wherever_a_catalogue_is_read() {
    // An id keys a record on a host that keeps records in a file system.
    // A traversal-shaped one used to pass the kernel and be named in a
    // release receipt, leaving each host to neutralise it on its own.
    let mut layer = admitted(manual_draft());
    layer["id"] = json!("../../etc/original");
    let registry = store(vec![layer.clone()]);
    for op in [
        json!({"kind": "list", "registry": registry.clone(), "scope": browser()}),
        update(
            registry.clone(),
            "../../etc/original",
            json!({"kind": "visible", "visible": false}),
        ),
        json!({"kind": "remove", "registry": registry.clone(), "scope": browser(),
               "id": "../../etc/original"}),
        register(registry.clone(), manual_draft()),
    ] {
        assert_eq!(refused(op), MALFORMED_DESCRIPTOR);
    }
    for shape in ["a/b", "a\\b", "c:\\layers", ".", ".."] {
        let mut layer = layer.clone();
        layer["id"] = json!(shape);
        assert_eq!(
            refused(json!({"kind": "list", "registry": store(vec![layer]),
                           "scope": browser()})),
            MALFORMED_DESCRIPTOR,
            "{shape} is not one path segment"
        );
    }
    // Restore repairs what it can, but an id cannot be repaired: minting
    // another one would orphan the payload the row names. So the row is
    // carried instead — the store still reopens around it, and the host
    // keeps that row's raw bytes (see
    // `a_row_whose_id_is_malformed_is_carried_verbatim_and_never_re_keyed`).
    let answer = ask(json!({"kind": "restore", "scope": browser(),
                            "records": [{"id": "a/b", "name": "Roads"}],
                            "now_ms": 1_700_000_000_000i64}));
    assert_eq!(answer["registry"]["layers"], json!([]));
    assert_eq!(
        answer["receipt"]["carried"][0]["raw"],
        json!({"id": "a/b", "name": "Roads"})
    );
}

#[test]
fn imported_file_metadata_is_refused_on_a_manual_row_on_every_op() {
    // `register` and `restore` drop it, so no path here mints such a row —
    // but a registry a host hands in was read from its own store, and the
    // whole-registry validation now holds it to the same rule.
    let mut layer = admitted(manual_draft());
    layer["sourceMetadata"] = json!({"kind": "import_file", "fileName": "roads.geojson",
                                     "sourceName": "roads.geojson", "lastRefreshAt": 9});
    let registry = store(vec![layer.clone()]);
    let id = layer["id"].as_str().expect("an id").to_owned();
    for op in [
        json!({"kind": "list", "registry": registry.clone(), "scope": browser()}),
        update(
            registry.clone(),
            &id,
            json!({"kind": "visible", "visible": false}),
        ),
        json!({"kind": "remove", "registry": registry.clone(), "scope": browser(), "id": id}),
        register(registry.clone(), manual_draft()),
    ] {
        assert_eq!(refused(op), MALFORMED_DESCRIPTOR);
    }
    // The same row on an imported layer is exactly what it is for.
    let mut imported = layer;
    imported["sourceKind"] = json!("import");
    assert_eq!(
        ask(json!({"kind": "list", "registry": store(vec![imported]), "scope": browser()}))
            ["receipt"]["count"],
        1
    );
}
