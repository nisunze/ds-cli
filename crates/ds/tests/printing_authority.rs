//! An agent reaches complete print controls from the map authority without source reads.
mod common;
#[test]
fn map_print_discovery_is_compact_and_exposes_the_real_kernel_schemas() {
    let (index, code) = common::json(&["map", "print", "schema", "--output", "json"]);
    assert_eq!(code, 0);
    assert!(index.to_string().len() < 2000);
    assert_eq!(
        index["data"]["sections"],
        serde_json::json!(["request", "layout", "edit", "outputs"])
    );
    for section in ["request", "layout", "edit", "outputs"] {
        let (schema, code) = common::json(&[
            "map",
            "print",
            "schema",
            "--section",
            section,
            "--output",
            "json",
        ]);
        assert_eq!(code, 0);
        assert!(schema["data"]["$schema"].is_string());
        if section == "request" {
            for field in ["page_mode", "paper", "dpi", "layout", "codes"] {
                assert!(schema["data"]["properties"][field].is_object(), "{field}");
            }
        }
        if section == "layout" {
            for field in [
                "styles",
                "style_overrides",
                "context_layers",
                "elements",
                "layer_order",
            ] {
                assert!(schema["data"]["properties"][field].is_object(), "{field}");
            }
            let text = schema.to_string();
            for field in [
                "dash_mm",
                "width_mm",
                "label_pt",
                "halo_mm",
                "heading_mode",
                "overflow",
                "indent_mm",
            ] {
                assert!(text.contains(field), "{field}");
            }
        }
    }
}
