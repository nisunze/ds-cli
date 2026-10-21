//! Missing authored LV pole categories are diagnostics, never a replacement
//! type or a rendering category inferred from a label, colour or neighbour.
use serde_json::Value;

pub const WARNING_CODE: &str = "print_pole_category_missing";

/// Design pole roles, including their exact source partitions. National
/// reference poles use a different schema and are not silently retyped here.
pub fn design_pole_layer(layer: &str) -> bool {
    matches!(
        layer.split("-source-").next().unwrap_or(layer),
        "lv_poles" | "tapping_poles" | "unconnected_lv_poles" | "neighbor_lv_poles"
    )
}

pub fn missing_type(layer: &str, feature: &Value) -> bool {
    design_pole_layer(layer)
        && feature["properties"]["struct_type"]
            .as_str()
            .is_none_or(|value| value.trim().is_empty() || value.len() > 160 || value == "unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_types_are_visible_diagnostics_without_inference_or_source_edits() {
        let mut pole = json!({"id":"generated_node_58", "properties":{
            "pole_number":"P035", "material":"wood", "existing_status":"new"
        }, "geometry":{"type":"Point","coordinates":[29.77,-2.55]}});
        let held = pole.clone();
        assert!(missing_type("lv_poles", &pole));
        assert!(missing_type(
            "neighbor_lv_poles-source-upgrade_ruturo",
            &pole
        ));
        assert_eq!(
            pole, held,
            "diagnostics never guess S140 from material or numbering"
        );
        for value in [
            Value::Null,
            json!(""),
            json!("  "),
            json!(140),
            json!({"type":"S140"}),
            json!("unknown"),
        ] {
            pole["properties"]["struct_type"] = value;
            assert!(missing_type("lv_poles", &pole));
        }
        for category in ["S255", "S140", "S190", "400daN", "650daN"] {
            pole["properties"]["struct_type"] = json!(category);
            assert!(!missing_type("lv_poles", &pole), "authored type {category}");
        }
        pole["properties"]
            .as_object_mut()
            .unwrap()
            .remove("struct_type");
        assert!(!missing_type("mv_pole", &pole));
        assert!(!missing_type("lv_pole", &pole));
        assert!(!missing_type("customers", &pole));
        assert!(!missing_type("lv_poles_other", &pole));
    }
}
