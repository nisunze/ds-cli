use super::*;
use serde_json::json;
fn request() -> Request {
    let layers: BTreeMap<String, Value> = serde_json::from_value(json!({"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"id":"p1"}}]}})).unwrap();
    let sheets_json =
        json!({"project_settings":[{"parameter":"report_formats","value":"voltage_drop"}]})
            .to_string();
    Request { schema: SCHEMA.into(), project:"project-a".into(), lane:"canary".into(), uid:"owner".into(), credential_audience_sha256:"a".repeat(64), transformer:"tr_1".into(), server_version:7, server_known:true, local_dirty:false, content_digest:jcs::layers_content_digest(&layers).unwrap(), receipt:InputReceipt {local_print_recipe:None,schema:1,country:"Burundi".into(),sheets_sha256:super::super::sha256_hex(sheets_json.as_bytes()),sheets_json,reference_semantic_sha256:"b".repeat(64)}, layers, selection:serde_json::from_value(json!({"schema":"ds.design-output-selection/v1","prints":[],"geospatial":["shp","kmz"],"tabular":["xlsx"]})).unwrap(),media_grant:None }
}
#[test]
fn selected_gis_needs_no_analysis_and_preserves_bytes() {
    let r = request();
    let bytes = serde_json::to_vec(&r).unwrap();
    let admitted = Request::decode(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&admitted).unwrap(), bytes);
    assert!(admitted
        .saved_source(Some(7), Some(&r.content_digest))
        .is_ok());
    assert!(admitted.media_document().is_none());
}
#[test]
fn an_unselected_capture_runs_the_gis_defaults_and_never_requires_analysis() {
    let source = request();
    let capture = |selection: Option<DesignOutputSelection>| Capture {
        project: source.project.clone(),
        transformer: source.transformer.clone(),
        room: Some(Room {
            source: "lv".into(),
            transformer_name: source.transformer.clone(),
            server_known: true,
            local_dirty: false,
            server_version: Some(source.server_version),
            layers: source.layers.clone(),
        }),
        receipt: source.receipt.clone(),
        selection,
        media_grant: None,
    };
    // The project stores a voltage-drop selection; the GIS export still
    // completes its own outputs without asking for the analysis.
    let prepared = capture(None)
        .prepare("canary", "owner", &"a".repeat(64))
        .unwrap();
    let tokens = prepared.selection.tokens().unwrap();
    assert_eq!(tokens, ["shp", "kmz", "xlsx"]);
    assert!(!crate::report_formats::requires_voltage_drop_analysis(
        &tokens
    ));
    let explicit = source.selection.clone();
    let prepared = capture(Some(explicit.clone()))
        .prepare("canary", "owner", &"a".repeat(64))
        .unwrap();
    assert_eq!(prepared.selection, explicit);
}

#[test]
fn dirty_unsaved_and_changed_sources_refuse_before_execution() {
    for change in [0, 1, 2] {
        let mut r = request();
        match change {
            0 => r.local_dirty = true,
            1 => r.server_known = false,
            _ => r.server_version = 0,
        };
        assert_eq!(r.validate().unwrap_err().code, UNSAVED);
    }
    let r = request();
    assert_eq!(
        r.saved_source(Some(8), Some(&r.content_digest))
            .unwrap_err()
            .code,
        CHANGED
    );
    assert_eq!(
        r.saved_source(Some(7), Some(&"c".repeat(64)))
            .unwrap_err()
            .code,
        CHANGED
    );
    assert!(r
        .authority("project-b", "canary", "owner", &"a".repeat(64))
        .is_err());
}
#[test]
fn source_receipt_selection_and_unknown_fields_remain_closed() {
    let mut r = request();
    r.layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["id"] = json!("changed");
    assert!(r.validate().is_err());
    let mut r = request();
    r.receipt.sheets_json.push(' ');
    assert!(r.validate().is_err());
    let mut r = request();
    r.selection.tabular.push("voltage_drop".into());
    assert!(r.validate().is_err());
    let mut r = request();
    r.selection.geospatial.clear();
    r.selection.tabular.clear();
    assert!(r.validate().is_err());
    let mut raw = serde_json::to_value(request()).unwrap();
    raw["argv"] = json!(["bad"]);
    assert!(Request::decode(&serde_json::to_vec(&raw).unwrap()).is_err());
}
