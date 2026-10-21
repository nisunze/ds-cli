use super::*;

fn room(name: &str, lon: f64) -> Room {
    Room {
        transformer: name.into(),
        version: Some(1),
        content_digest: None,
        layers: BTreeMap::from([
            (
                "tr".into(),
                json!({"type":"FeatureCollection","features":[{
                    "type":"Feature","geometry":{"type":"Point","coordinates":[lon,-2.0]},
                    "properties":{"transfo":name,"tr_size":"100 kVA","private_note":"never print"}
                }]}),
            ),
            (
                "lv_lines".into(),
                json!({"features":[{"type":"Feature","geometry":null}]}),
            ),
        ]),
    }
}

#[test]
fn neighbor_context_contains_only_other_points_and_public_names() {
    let markers = BTreeMap::from([
        ("own".into(), marker(&room("own", 29.7)).unwrap()),
        ("other".into(), marker(&room("other", 29.701)).unwrap()),
    ]);
    let attached = attach("own", &markers, None).unwrap().unwrap();
    let document: Value = serde_json::from_slice(&attached.bytes).unwrap();
    assert_eq!(
        document["layers"][LAYER]["features"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        document["layers"][LAYER]["features"][0]["properties"],
        json!({"transfo":"other"})
    );
    assert!(document["layers"]["lv_lines"].is_null());
    assert!(document["layers"]["lv_poles"].is_null());
    assert!(document["layers"]["lv_print_info"].is_null());
    assert!(
        !String::from_utf8(attached.bytes)
            .unwrap()
            .contains("private_note")
    );
}

#[test]
fn governed_layout_accepts_the_neighbor_layer_binding() {
    use ds_command_kernel::printing::{default_layout, elements};

    let layout = elements::set_layer_style_ref(
        default_layout(),
        LAYER,
        "master/neighbor_transformers_print",
        &[elements::StyleFacts {
            style_ref: "master/neighbor_transformers_print".into(),
            style_target: "print".into(),
        }],
    )
    .unwrap();
    assert_eq!(
        layout.style_refs[LAYER],
        "master/neighbor_transformers_print"
    );
    ds_command_kernel::printing::validate(&layout).unwrap();
}

#[test]
fn a_solo_project_adds_no_layer_and_malformed_points_refuse() {
    let own = marker(&room("own", 29.7)).unwrap();
    assert!(
        attach("own", &BTreeMap::from([("own".into(), own)]), None)
            .unwrap()
            .is_none()
    );
    let mut malformed = room("bad", 29.7);
    malformed.layers.get_mut("tr").unwrap()["features"][0]["geometry"]["type"] =
        json!("LineString");
    assert!(marker(&malformed).unwrap_err().contains("point geometry"));
}

#[test]
fn current_project_rooms_supply_one_catalogue_without_network_reads() {
    let temp = tempfile::tempdir().unwrap();
    let hold = Hold::at(temp.path().into(), "principal", "project-a");
    hold.hold_room(&room("own", 29.7)).unwrap();
    hold.hold_room(&room("other", 29.701)).unwrap();
    let status = BTreeMap::from([
        ("own".into(), json!({"metadata":{"version":1}})),
        ("other".into(), json!({"metadata":{"version":1}})),
    ]);
    let identity =
        ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "principal").unwrap();
    let points = catalogue(
        "canary",
        "project-a",
        &identity,
        &["own".into(), "other".into()],
        Some(&status),
        &hold,
        &mut Link::default(),
        true,
    )
    .unwrap();
    let (points, rooms) = points;
    assert_eq!(points.len(), 2);
    assert_eq!(
        networks(rooms).unwrap().len(),
        2,
        "circuits read no room again"
    );
    assert_eq!(points["other"]["properties"], json!({"transfo":"other"}));
    assert!(hold.room("other").unwrap().is_some());
}

#[test]
fn adding_neighbors_keeps_other_context_coverage_and_layers() {
    let coverage = json!({"type":"Polygon","coordinates":[[[29.69,-2.01],[29.71,-2.01],[29.71,-1.99],[29.69,-1.99],[29.69,-2.01]]]});
    let sources = json!([{"layer":"roads","kind":"catalog"}]);
    let layers = BTreeMap::from([(
        "roads".into(),
        json!({"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[29.69,-2.0],[29.71,-2.0]]},"properties":{}}]}),
    )]);
    let bytes =
        ds_command_kernel::report_export::print_context_document(&coverage, &sources, &layers)
            .unwrap();
    let held = PrintContextBytes {
        sha256: ds_command_kernel::report_export::sha256_hex(&bytes),
        bytes,
        layers: vec!["roads".into()],
        omitted: vec!["rivers".into()],
    };
    let markers = BTreeMap::from([
        ("own".into(), marker(&room("own", 29.7)).unwrap()),
        ("other".into(), marker(&room("other", 29.701)).unwrap()),
    ]);
    let attached = attach("own", &markers, Some(held)).unwrap().unwrap();
    let document: Value = serde_json::from_slice(&attached.bytes).unwrap();
    assert_eq!(document["coverage"], coverage);
    assert_eq!(document["layers"]["roads"], layers["roads"]);
    assert_eq!(attached.omitted, ["rivers"]);
    assert!(attached.layers.contains(&LAYER.to_string()));
}
