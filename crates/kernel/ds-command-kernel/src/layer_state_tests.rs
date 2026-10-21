use super::*;

fn layer(id: &str, kind: &str, source: &str, metadata: Value) -> Value {
    json!({"id": id, "type": kind, "source": source, "source-layer": "s", "style_ref": id, "metadata": metadata, "paint": {"line-color": "#000"}})
}

/// Two canonical layers with the wire shape ds-brain's assembler emits: a
/// primary and its `__label` / `__boundary` / `__d3` companions.
fn document() -> Value {
    json!({
        "project_id": "p1",
        "sources": {"design_vt": {"type": "vector"}, "survey_geo": {"type": "geojson"}},
        "styles": {"roads": {}, "roads__label": {}, "roads__d3": {}, "poles": {}, "poles__boundary": {}, "lost": {}},
        "layers": [
            layer("roads", "line", "design_vt", json!({"config_layer_id": "design/roads", "label": "Roads", "layer_class": "design_tile", "geometry_type": "LineString", "order": 4, "source_key": "vt", "layer_key": "design/roads"})),
            layer("roads__label", "symbol", "design_vt", json!({"config_layer_id": "design/roads", "parent_layer": "roads", "label": "Roads Labels", "source_key": "vt", "layer_key": "design/roads"})).tap(|v| { v["layout"] = json!({"visibility": "none"}); }),
            layer("roads__d3", "fill-extrusion", "design_vt", json!({"config_layer_id": "design/roads", "parent_layer": "roads", "source_key": "vt", "layer_key": "design/roads"})),
            layer("poles", "circle", "survey_geo", json!({"config_layer_id": "survey/poles", "label": "Poles", "layer_class": "survey", "geometry_type": "Point", "order": 2, "source_key": "geojson", "layer_key": "survey/poles"})).tap(|v| { v["minzoom"] = json!(12); v["maxzoom"] = json!(20); }),
            layer("poles__boundary", "line", "survey_geo", json!({"config_layer_id": "survey/poles", "parent_layer": "poles", "source_key": "geojson", "layer_key": "survey/poles"})),
            layer("lost", "circle", "gone_source", json!({"config_layer_id": "survey/lost", "label": "Lost", "order": 9})),
            layer("notes", "circle", "survey_geo", json!({}))
        ],
        "matrix": {
            "design": {"rows": [{"key": "design/roads", "label": "Roads", "ids": ["roads"], "style_ref": "roads", "geometry_type": "LineString"}], "sources": [{"key": "vt", "label": "vector_tiles"}]},
            "survey": {"rows": [
                {"key": "survey/poles", "label": "Poles", "ids": ["poles", null], "style_refs": ["poles", null], "geometry_type": "Point"},
                {"key": "survey/empty", "label": "Empty", "ids": [null, null]}
            ], "sources": [{"key": "geojson", "label": "survey entries", "created_at": 5, "created_by": "a@b.c", "file_name": "x.geojson"}, {"key": "tiles", "label": "tiles"}]},
            "global": {"rows": []}
        },
        "platform_config": {"domain_labels": [{"key": "survey", "label": "Field survey"}]}
    })
}

trait Tap {
    fn tap(self, f: impl FnOnce(&mut Value)) -> Value;
}
impl Tap for Value {
    fn tap(mut self, f: impl FnOnce(&mut Value)) -> Value {
        f(&mut self);
        self
    }
}

fn ask(extra: Value, op: Value) -> Value {
    let mut request =
        json!({"schema": SCHEMA, "project": "p1", "document": document(), "op": op});
    for (key, value) in extra.as_object().unwrap() {
        request[key] = value.clone();
    }
    let out =
        evaluate(&serde_json::to_vec(&request).unwrap()).unwrap_or_else(|e| panic!("{e}"));
    serde_json::from_str(&out).unwrap()
}

fn refuse(request: Value) -> String {
    evaluate(&serde_json::to_vec(&request).unwrap()).unwrap_err()
}

fn ids(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

#[test]
fn identity_classifies_roles_families_and_suffixes() {
    let out = ask(
        json!({}),
        json!({"kind": "classify", "ids": ["sketch__d3", "sketch__label__2", "plain"]}),
    );
    let layers = out["layers"].as_array().unwrap();
    let by = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap();
    assert_eq!(by("roads")["role"], "primary");
    assert_eq!(by("roads__label")["role"], "label");
    assert_eq!(by("roads__d3")["role"], "d3");
    assert_eq!(by("poles__boundary")["role"], "boundary");
    assert_eq!(by("roads")["family"], by("roads__label")["family"]);
    assert_eq!(by("roads")["canonical_id"], "design/roads");
    assert_eq!(by("notes")["canonical_id"], Value::Null);
    assert_eq!(by("lost")["source_state"], "missing");
    assert_eq!(by("roads")["source_state"], "present");
    assert_eq!(by("roads__label")["authored_visible"], false);
    assert_eq!(
        out["ids"][0],
        json!({"id": "sketch__d3", "role": "d3", "known": false, "base": "sketch"})
    );
    assert_eq!(
        out["ids"][1]["role"], "companion",
        "an infix label marker is a companion, not a label (matches the TypeScript predicate)"
    );
    assert_eq!(out["ids"][2]["role"], "primary");
    assert_eq!(strip_companion_suffix("roads__label__2"), "roads");
    assert_eq!(strip_companion_suffix("roads__boundary"), "roads");
    assert_eq!(strip_companion_suffix("roads"), "roads");
}

#[test]
fn directional_marker_is_a_companion_even_without_parent_metadata() {
    let out = ask(
        json!({}),
        json!({"kind":"classify", "ids":["roads__marker", "roads__marker__2", "roads"]}),
    );
    assert_eq!(out["ids"][0]["role"], "companion");
    assert_eq!(out["ids"][1]["role"], "companion");
    assert_eq!(out["ids"][0]["base"], "roads");
    assert_eq!(out["ids"][1]["base"], "roads");
    assert_eq!(out["ids"][2]["role"], "primary");
}

#[test]
fn families_expand_by_id_base_parent_and_style_family_and_name_foreign_ids() {
    let out = ask(
        json!({}),
        json!({"kind": "families", "ids": ["roads", "", "ghost"]}),
    );
    assert_eq!(
        ids(&out["ids"]),
        ["roads", "ghost", "roads__label", "roads__d3"]
    );
    assert_eq!(ids(&out["foreign"]), ["ghost"]);
    // toggling a companion reaches its siblings through the id base
    let out = ask(
        json!({}),
        json!({"kind": "families", "ids": ["poles__boundary"]}),
    );
    assert_eq!(ids(&out["ids"]), ["poles__boundary", "poles"]);
}

#[test]
fn the_plan_mounts_records_and_hides_as_the_mount_always_did() {
    let out = ask(json!({}), json!({"kind": "plan"}));
    let layers = out["layers"].as_array().unwrap();
    let by = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap();
    // an authored-hidden label with nothing remembered is neither mounted nor recorded
    assert_eq!(by("roads__label")["mount"], false);
    assert_eq!(by("roads__label")["record"], false);
    assert_eq!(by("roads__label")["runtime"], "none");
    assert_eq!(
        by("roads__label")["reasons"],
        json!(["user_hidden", "authored_hidden"])
    );
    // a d3 companion mounts hidden: the 3D map is retired
    assert_eq!(by("roads__d3")["mount"], true);
    assert_eq!(by("roads__d3")["runtime"], "none");
    assert_eq!(by("roads__d3")["reasons"], json!(["3d_removed"]));
    assert_eq!(by("roads__d3")["record"], false);
    // a layer whose source is missing is skipped and says why
    assert_eq!(by("lost")["mount"], false);
    assert_eq!(by("lost")["source_state"], "missing");
    assert_eq!(
        by("lost")["runtime"],
        "visible",
        "runtime is the visibility decision; the source fact has its own field"
    );
    // primaries are always recorded, companions never without memory
    assert_eq!(
        out["preferences"],
        json!({"roads": true, "poles": true, "notes": true})
    );
    assert_eq!(out["preferences_source"], "persisted");
    assert_eq!(out["isolation_active"], false);
    assert!(out["writes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|w| w["id"] != "lost" && w["id"] != "roads__label"));

    // a remembered primary carries its family: the hidden label now mounts and is recorded
    let out = ask(
        json!({"preferences": {"roads": false}}),
        json!({"kind": "plan"}),
    );
    let layers = out["layers"].as_array().unwrap();
    let by = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap();
    assert_eq!(by("roads__label")["mount"], true);
    assert_eq!(by("roads__label")["desired"], false);
    assert_eq!(by("roads__label")["remembered"], false);
    assert_eq!(by("roads__label")["record"], true);
    assert_eq!(by("roads__d3")["record"], true);
    assert_eq!(out["preferences"]["roads__d3"], false);
    // a remembered true never beats an authored none
    let out = ask(
        json!({"preferences": {"roads__label": true}}),
        json!({"kind": "plan"}),
    );
    let label = out["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "roads__label")
        .unwrap()
        .clone();
    assert_eq!(label["desired"], true);
    assert_eq!(label["runtime"], "none");
    assert_eq!(label["reasons"], json!(["authored_hidden"]));
}

#[test]
fn governed_default_visibility_seeds_but_never_overrides_a_user_choice() {
    let mut doc = document();
    doc["layers"][3]["metadata"]["default_visible"] = json!(false);
    let evaluate_plan = |preferences: Value| {
        let request = json!({
            "schema": SCHEMA,
            "project": "p1",
            "document": doc.clone(),
            "preferences": preferences,
            "op": {"kind": "plan"}
        });
        serde_json::from_str::<Value>(
            &evaluate(&serde_json::to_vec(&request).unwrap()).unwrap(),
        )
        .unwrap()
    };

    let seeded = evaluate_plan(json!({}));
    let poles = seeded["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == "poles")
        .unwrap();
    assert_eq!(poles["remembered"], Value::Null);
    assert_eq!(poles["runtime"], "none");
    assert_eq!(poles["reasons"], json!(["default_hidden"]));

    let overridden = evaluate_plan(json!({"poles": true}));
    let poles = overridden["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == "poles")
        .unwrap();
    assert_eq!(poles["remembered"], true);
    assert_eq!(poles["runtime"], "visible");
    assert_eq!(poles["reasons"], json!([]));
}

#[test]
fn the_session_wins_over_persisted_only_for_the_same_project_and_zoom_is_reported() {
    let out = ask(
        json!({"preferences": {"poles": false}, "session": {"project": "p1", "preferences": {"poles": true}}, "zoom": 10.0}),
        json!({"kind": "plan"}),
    );
    assert_eq!(out["preferences_source"], "session");
    let poles = out["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "poles")
        .unwrap()
        .clone();
    assert_eq!(poles["desired"], true);
    assert_eq!(poles["in_zoom_range"], false);
    assert_eq!(
        poles["runtime"], "visible",
        "zoom is an answer, not a runtime write"
    );
    let out = ask(
        json!({"preferences": {"poles": false}, "session": {"project": "other", "preferences": {"poles": true}}, "zoom": 12.0}),
        json!({"kind": "plan"}),
    );
    assert_eq!(out["preferences_source"], "persisted");
    let poles = out["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "poles")
        .unwrap()
        .clone();
    assert_eq!(poles["desired"], false);
    assert_eq!(poles["in_zoom_range"], true);
    let refused = evaluate(
        &serde_json::to_vec(&json!({"schema": SCHEMA, "mode": {"renderer": "3d"}, "op": {"kind": "plan"}}))
            .unwrap(),
    );
    assert!(refused.is_err(), "the retired renderer mode is no input any more");
}

#[test]
fn folds_read_a_scope_and_say_what_a_parent_writes() {
    assert_eq!(
        fold([]),
        ScopeState {
            count: 0,
            any_visible: false,
            all_visible: false
        }
    );
    assert_eq!(
        fold([true, false]),
        ScopeState {
            count: 2,
            any_visible: true,
            all_visible: false
        }
    );
    assert!(!next_visibility(fold([true, false])));
    assert!(next_visibility(fold([false, false])));
    assert!(!next_visibility(fold([])));
    let merged = merge(&[fold([]), fold([true]), fold([false, false])]);
    assert_eq!(
        merged,
        ScopeState {
            count: 3,
            any_visible: true,
            all_visible: false
        }
    );
    let out = ask(
        json!({"preferences": {"roads": false, "roads__d3": false}}),
        json!({"kind": "scope", "ids": ["roads"], "expand": true}),
    );
    assert_eq!(ids(&out["ids"]), ["roads", "roads__label", "roads__d3"]);
    assert_eq!(out["count"], 3);
    assert_eq!(
        out["any_visible"], false,
        "the authored-hidden label follows its family, it is not visible by default"
    );
    assert_eq!(out["next"], true);
    let out = ask(
        json!({}),
        json!({"kind": "scope", "ids": ["ghost", "roads"]}),
    );
    assert_eq!(out["all_visible"], true);
    assert_eq!(ids(&out["foreign"]), ["ghost"]);
    let bare: Value = serde_json::from_str(
        &evaluate(
            &serde_json::to_vec(
                &json!({"schema": SCHEMA, "op": {"kind": "fold", "values": [true, true]}}),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(bare["all_visible"], true);
    assert_eq!(bare["next"], false);
}

#[test]
fn toggle_set_isolate_and_restore_move_preferences_and_name_the_writes() {
    let out = ask(json!({}), json!({"kind": "toggle", "ids": ["roads"]}));
    assert_eq!(out["outcome"], "applied");
    assert_eq!(out["visible"], false);
    assert_eq!(
        out["preferences"],
        json!({"roads": false, "roads__label": false, "roads__d3": false})
    );
    assert_eq!(
        ids(&out["changed"]),
        ["roads", "roads__d3"],
        "the authored-hidden label was already hidden; the d3 companion was desired-visible"
    );
    assert_eq!(
        out["writes"][0],
        json!({"id": "roads", "runtime": "none", "reasons": ["user_hidden"]})
    );
    let out = ask(
        json!({"preferences": {"roads": false, "roads__label": false, "roads__d3": false}}),
        json!({"kind": "toggle", "ids": ["roads"]}),
    );
    assert_eq!(out["visible"], true);
    assert_eq!(
        out["writes"][1],
        json!({"id": "roads__label", "runtime": "none", "reasons": ["authored_hidden"]})
    );
    assert_eq!(out["writes"][2]["reasons"], json!(["3d_removed"]));
    let out = ask(json!({}), json!({"kind": "toggle", "ids": [""]}));
    assert_eq!(out["outcome"], "noop");
    // a foreign id is toggled on its own and keeps the d3 suffix rule
    let out = ask(
        json!({"preferences": {"sketch__d3": true}}),
        json!({"kind": "toggle", "ids": ["sketch__d3"]}),
    );
    assert_eq!(out["preferences"]["sketch__d3"], false);
    assert_eq!(ids(&out["foreign"]), ["sketch__d3"]);
    let out = ask(
        json!({}),
        json!({"kind": "set", "ids": ["poles"], "visible": false}),
    );
    assert_eq!(
        ids(&out["ids"]),
        ["poles"],
        "set does not expand unless asked"
    );
    let out = ask(
        json!({}),
        json!({"kind": "set", "ids": ["poles"], "visible": false, "expand": true}),
    );
    assert_eq!(ids(&out["ids"]), ["poles", "poles__boundary"]);

    let out = ask(
        json!({"preferences": {"poles": false}}),
        json!({"kind": "isolate", "ids": ["roads"]}),
    );
    assert_eq!(out["outcome"], "isolated");
    let snapshot = out["isolation"].clone();
    assert_eq!(
        ids(&snapshot["layer_ids"]),
        [
            "roads",
            "roads__label",
            "roads__d3",
            "poles",
            "poles__boundary",
            "lost",
            "notes"
        ]
    );
    assert_eq!(snapshot["visibility"]["poles"], false);
    assert_eq!(
        snapshot["visibility"]["roads__label"], false,
        "the snapshot records the effective desired value"
    );
    assert_eq!(out["preferences"]["notes"], false);
    assert_eq!(out["preferences"]["roads__d3"], true);
    assert_eq!(out["preferences"]["poles__boundary"], false);
    assert_eq!(ids(&out["kept"]), ["roads", "roads__d3", "roads__label"]);
    let restored = ask(
        json!({"preferences": out["preferences"].clone(), "isolation": snapshot.clone()}),
        json!({"kind": "restore"}),
    );
    assert_eq!(restored["outcome"], "restored");
    assert_eq!(restored["isolation"], Value::Null);
    assert_eq!(restored["preferences"]["poles"], false);
    assert_eq!(restored["preferences"]["notes"], true);
    assert_eq!(restored["preferences"]["roads__label"], false);
    let plan = ask(
        json!({"isolation": snapshot.clone()}),
        json!({"kind": "plan"}),
    );
    assert_eq!(plan["isolation_active"], true);
    let state = ask(
        json!({"isolation": snapshot.clone()}),
        json!({"kind": "isolation"}),
    );
    assert_eq!(state, json!({"active": true, "snapshot": true}));
    assert_eq!(
        ask(json!({}), json!({"kind": "isolation"})),
        json!({"active": false, "snapshot": false})
    );
    // the layout word right now, for document and foreign layers alike, or forced on
    let out = ask(
        json!({"preferences": {"poles": false, "sketch": false}}),
        json!({"kind": "runtime", "ids": ["poles", "roads__label", "sketch", "other"]}),
    );
    assert_eq!(
        out["writes"][0],
        json!({"id": "poles", "runtime": "none", "reasons": ["user_hidden"]})
    );
    assert_eq!(
        out["writes"][1]["reasons"],
        json!(["user_hidden", "authored_hidden"])
    );
    assert_eq!(out["writes"][2]["runtime"], "none");
    assert_eq!(out["writes"][3]["runtime"], "visible");
    let out = ask(
        json!({"preferences": {"poles": false}}),
        json!({"kind": "runtime", "ids": ["poles", "roads__label"], "force_visible": true}),
    );
    assert_eq!(out["writes"][0]["runtime"], "visible");
    assert_eq!(
        out["writes"][1],
        json!({"id": "roads__label", "runtime": "none", "reasons": ["authored_hidden"]})
    );
    let mut stale = snapshot.clone();
    stale["layer_ids"].as_array_mut().unwrap().pop();
    let out = ask(json!({"isolation": stale}), json!({"kind": "restore"}));
    assert_eq!(out["outcome"], "stale_snapshot");
    assert_eq!(out["writes"], json!([]));
    let out = ask(json!({}), json!({"kind": "restore"}));
    assert_eq!(out["outcome"], "no_snapshot");
    // isolating a foreign layer (an analysis result) hides every managed layer and keeps it
    let out = ask(json!({}), json!({"kind": "isolate", "ids": ["ghost"]}));
    assert_eq!(out["outcome"], "isolated");
    assert!(out["preferences"]
        .as_object()
        .unwrap()
        .values()
        .all(|v| v == false));
    assert_eq!(ids(&out["kept"]), ["ghost"]);
    let out = ask(json!({}), json!({"kind": "isolate", "ids": [""]}));
    assert_eq!(out["outcome"], "noop");
    assert_eq!(out["reason"], "nothing_to_isolate");
}

#[test]
fn the_catalogue_groups_families_folds_visibility_and_truncates() {
    let out = ask(
        json!({"preferences": {"roads": false}, "zoom": 8.0}),
        json!({"kind": "catalog", "limit": 2}),
    );
    assert_eq!(out["layer_count"], 3);
    assert_eq!(out["more"], true);
    let roads = &out["layers"][0];
    assert_eq!(roads["id"], "design/roads");
    assert_eq!(roads["label"], "Roads");
    assert_eq!(roads["class"], "design_tile");
    assert_eq!(roads["order"], 4);
    assert_eq!(
        ids(&roads["runtime_ids"]),
        ["roads", "roads__label", "roads__d3"]
    );
    assert_eq!(roads["roles"]["roads__d3"], "d3");
    assert_eq!(roads["visibility"]["any_visible"], false);
    assert_eq!(roads["visibility"]["next"], true);
    assert_eq!(roads["source_state"], "present");
    assert_eq!(roads["in_zoom_range"], true);
    let poles = &out["layers"][1];
    assert_eq!(poles["visibility"]["all_visible"], true);
    assert_eq!(
        poles["in_zoom_range"], true,
        "the boundary companion has no zoom range, so the family renders at zoom 8"
    );
    let out = ask(json!({}), json!({"kind": "catalog", "limit": 10}));
    assert_eq!(out["layers"][2]["source_state"], "missing");
    assert_eq!(out["layers"][2]["class"], "unknown");
    assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": document(), "op": {"kind": "catalog", "limit": 0}})).contains("limit"));
}

#[test]
fn the_matrix_projects_domains_in_order_with_resolved_styles_and_provenance() {
    let out = ask(
        json!({"preferences": {"poles": false}}),
        json!({"kind": "matrix"}),
    );
    assert_eq!(out["has_data"], true);
    assert_eq!(out["total_rows"], 3);
    let domains = out["domains"].as_array().unwrap();
    assert_eq!(
        domains
            .iter()
            .map(|d| d["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["survey", "design"]
    );
    let survey = &domains[0];
    assert_eq!(survey["label"], "Field survey");
    assert_eq!(
        survey["entries"].as_array().unwrap().len(),
        1,
        "a row with no ids is dropped"
    );
    assert_eq!(survey["entries"][0]["style_ref"], "poles");
    assert_eq!(
        survey["entries"][0]["visibility"]["any_visible"], false,
        "the boundary follows its family"
    );
    let sources = survey["sources"].as_array().unwrap();
    assert_eq!(
        sources.len(),
        1,
        "a source column with no entries is dropped"
    );
    assert_eq!(sources[0]["label"], "Survey Entries");
    assert_eq!(sources[0]["created_by"], "a@b.c");
    assert_eq!(sources[0]["entries"][0]["key"], "geojson:survey/poles");
    assert_eq!(sources[0]["entries"][0]["file_name"], "x.geojson");
    assert_eq!(survey["grouping_choice"], false);
    assert_eq!(domains[1]["label"], "design");
    assert_eq!(domains[1]["sources"][0]["label"], "Vector Tiles");
    assert_eq!(format_label("lv_lines  extra"), "Lv Lines Extra");
    assert_eq!(format_label("  a-b "), "A-B");
}

#[test]
fn reorder_names_invalid_duplicate_unknown_and_partial_outcomes() {
    let out = ask(
        json!({}),
        json!({"kind": "reorder", "orders": [{"layer_id": "design/roads", "order": 10}]}),
    );
    assert_eq!(out["outcome"], "accepted");
    assert_eq!(out["complete"], false);
    assert_eq!(ids(&out["unlisted"]), ["survey/lost", "survey/poles"]);
    assert_eq!(out["canonical_count"], 3);
    let out = ask(
        json!({}),
        json!({"kind": "reorder", "orders": [{"layer_id": "roads", "order": 1}]}),
    );
    assert_eq!(out["outcome"], "refused");
    assert_eq!(out["code"], "unknown_layer");
    assert_eq!(
        ids(&out["ids"]),
        ["roads"],
        "a runtime id is never an order key"
    );
    let out = ask(
        json!({}),
        json!({"kind": "reorder", "orders": [{"layer_id": "design/roads", "order": 1}, {"layer_id": "design/roads", "order": 2}]}),
    );
    assert_eq!(out["code"], "duplicate_layer");
    let out = ask(
        json!({}),
        json!({"kind": "reorder", "orders": [{"layer_id": " design/roads", "order": 1}]}),
    );
    assert_eq!(out["code"], "invalid_order");
    let out = ask(
        json!({}),
        json!({"kind": "reorder", "orders": [{"layer_id": "design/roads", "order": 1_000_001}]}),
    );
    assert_eq!(out["code"], "invalid_order");
    let out = ask(json!({}), json!({"kind": "reorder", "orders": []}));
    assert_eq!(out["code"], "invalid_order");
}

#[test]
fn fences_refuse_a_stale_project_a_wrong_schema_and_a_missing_document() {
    for invalid in ["", " ", " roads", "roads ", "roads\nlabel", "roads\0label"] {
        let mut malformed = document();
        malformed["layers"][0]["id"] = json!(invalid);
        assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": malformed, "op": {"kind": "plan"}})).contains("invalid runtime id"));
    }
    let stale = refuse(
        json!({"schema": SCHEMA, "project": "p2", "document": document(), "op": {"kind": "plan"}}),
    );
    assert!(stale.starts_with("project_context_changed"), "{stale}");
    assert!(
        refuse(json!({"schema": "ds.other/v1", "op": {"kind": "fold", "values": []}}))
            .contains("schema")
    );
    assert!(refuse(json!({"schema": SCHEMA, "op": {"kind": "plan"}})).contains("document"));
    assert!(
        refuse(json!({"schema": SCHEMA, "document": document(), "op": {"kind": "plan"}}))
            .contains("project")
    );
    let mut duplicated = document();
    duplicated["layers"].as_array_mut().unwrap().push(layer(
        "roads",
        "line",
        "design_vt",
        json!({}),
    ));
    assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": duplicated, "op": {"kind": "plan"}})).contains("repeats"));
    assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": document(), "op": {"kind": "plan"}, "extra": 1})).contains("unknown field"));
    assert!(evaluate(&vec![b'x'; MAX_INPUT_BYTES + 1])
        .unwrap_err()
        .contains("bound"));
}
