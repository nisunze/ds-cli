use super::*;
#[test]
fn collapse_is_stable_for_geometry_collisions_and_preserves_original_provenance() {
    let result=plan(serde_json::from_value(json!({"mode":"layer","sources":[{"fileName":"one.zip","layers":[{"name":"a/roads.shp","geometryType":"Point","featureCount":1,"features":[{"properties":{"_ds_source_file":"original"}}],"columns":["id"]}]},{"fileName":"two.zip","layers":[{"name":"roads.shp","geometryType":"LineString","featureCount":2,"features":[],"columns":[]}]}]})).unwrap());
    assert_eq!(result[0]["name"], json!("roads_Point"));
    assert_eq!(result[1]["name"], json!("roads_LineString"));
    assert_eq!(
        result[0]["features"][0]["properties"]["_ds_source_file"],
        json!("original")
    );
    assert_eq!(
        result[0]["features"][0]["properties"]["_ds_source_layer"],
        json!("a/roads.shp")
    );
}
