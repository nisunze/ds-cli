use super::*;
use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
use serde_json::json;

fn catalogue(resolved_at: i64) -> Value {
    json!({
        "project_id": "p", "resolved_at": resolved_at, "styles": {"roads": {"type": "line"}},
        "style_editors": [], "warnings": [], "style_properties": {}, "style_presets": {},
        "style_resolution": {"revision_id": format!("r{resolved_at}")},
        "label_editor": {}, "fill_pattern": {}, "platform_config": {}, "symbol_catalog": {},
        "design_layer_catalog": {}, "required_images": []
    })
}

fn answer(data: Value) -> Value {
    json!({"success": true, "data": data})
}

fn document(resolved_at: i64) -> Value {
    let mut document = catalogue(resolved_at);
    document["sources"] = json!({"s": {"type": "geojson"}});
    document["layers"] = json!([{"id": "roads", "type": "line", "source": "s"}]);
    document["sidebar"] = json!({"groups": [{"key": "g", "layers": ["roads"]}]});
    document["survey_layers"] = json!([]);
    document["design_layers"] = json!([]);
    document
}

#[test]
fn a_held_catalogue_is_read_once_until_it_is_invalidated() {
    let edge = TestEdge::new();
    edge.json(answer(catalogue(1)));
    assert_eq!(block_on(style_catalog(&edge, "p", false)).unwrap(), catalogue(1));
    assert_eq!(block_on(style_catalog(&edge, "p", false)).unwrap(), catalogue(1));
    assert_eq!(edge.requests().len(), 1, "the held catalogue reads no Brain");
    assert!(block_on(is_current(&edge, "p", &catalogue(1))));

    block_on(invalidate(&edge, None));
    assert!(!block_on(is_current(&edge, "p", &catalogue(1))));
    edge.json(answer(catalogue(2)));
    assert_eq!(block_on(style_catalog(&edge, "p", false)).unwrap()["resolved_at"], 2);
    assert_eq!(edge.requests().len(), 2);
    assert_eq!(edge.requests()[1].body.as_ref().unwrap()["action"], "get_style_catalog");

    edge.json(answer(catalogue(3)));
    assert_eq!(block_on(style_catalog(&edge, "p", true)).unwrap()["resolved_at"], 3);
    assert_eq!(edge.requests().len(), 3, "a refresh always reads Brain");
}

#[test]
fn a_layer_document_read_holds_its_metadata_as_the_catalogue() {
    let edge = TestEdge::new();
    edge.json(answer(document(4)));
    let read = block_on(layer_document(&edge, "p", false)).unwrap();
    assert!(read.get("layers").is_some(), "the caller gets the whole document");
    assert_eq!(block_on(style_catalog(&edge, "p", false)).unwrap(), catalogue(4));
    assert_eq!(edge.requests().len(), 1);
    assert!(block_on(is_current(&edge, "p", &json!({"style_resolution": {"revision_id": "r4"}}))));
    assert!(!block_on(is_current(&edge, "p", &json!({"style_resolution": {"revision_id": "r3"}}))));
    assert!(!block_on(is_current(&edge, "q", &catalogue(4))), "another project holds nothing");
}

#[test]
fn another_account_never_reads_a_held_catalogue() {
    let edge = TestEdge::new();
    edge.json(answer(catalogue(1)));
    block_on(style_catalog(&edge, "p", false)).unwrap();
    edge.sign_in("someone_else");
    assert!(!block_on(is_current(&edge, "p", &catalogue(1))));
    edge.json(answer(catalogue(5)));
    assert_eq!(block_on(style_catalog(&edge, "p", false)).unwrap()["resolved_at"], 5);
    assert_eq!(edge.requests().len(), 2);
}
