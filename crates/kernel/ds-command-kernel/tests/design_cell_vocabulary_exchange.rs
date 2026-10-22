use ds_command_kernel::design_cell_vocabulary::{decode_vocabulary, design_cell_lock_vocabulary};
use ds_network_contracts::cell_authority::{
    construction_components_for_layer, Component, LockableCell,
};
use serde_json::json;

#[test]
fn kernel_response_and_decoder_share_the_actual_engine_facts() {
    let answer = design_cell_lock_vocabulary();
    let decoded = decode_vocabulary(&serde_json::to_vec(&answer).unwrap()).unwrap();
    assert_eq!(decoded, answer);
    for cell in LockableCell::ALL {
        assert!(decoded
            .lockable_cells
            .iter()
            .any(|name| name == cell.wire_name()));
    }
    for (layer, choices) in &decoded.construction_components {
        let engine = construction_components_for_layer(layer);
        assert_eq!(choices.len(), engine.len());
        for (choice, expected) in choices.iter().zip(engine) {
            assert_eq!(choice.key, expected.component.wire_name());
            assert_eq!(choice.statuses, expected.statuses);
            assert!(Component::ALL.contains(&expected.component));
        }
    }
}

#[test]
fn incompatible_payloads_refuse_before_a_caller_can_offer_false_locks() {
    for bad in [
        json!(null),
        json!({}),
        json!({"lockableCells": 2, "constructionComponents": {}}),
    ] {
        assert!(decode_vocabulary(&serde_json::to_vec(&bad).unwrap())
            .unwrap_err()
            .to_string()
            .contains("design_cell_vocabulary_response_invalid"));
    }
    for path in ["cells", "component", "statuses"] {
        let mut bad = serde_json::to_value(design_cell_lock_vocabulary()).unwrap();
        match path {
            "cells" => bad["lockableCells"] = json!(["cable_size", "cable_size"]),
            "component" => bad["constructionComponents"]["lv_lines"][0]["key"] = json!(null),
            _ => bad["constructionComponents"]["lv_lines"][0]["statuses"] = json!(["strung"]),
        }
        assert!(
            decode_vocabulary(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{path}"
        );
    }
    assert!(decode_vocabulary(b"{broken").is_err());
}

#[test]
fn additions_do_not_require_equal_versions_or_equal_vocabulary_lists() {
    let mut value = serde_json::to_value(design_cell_lock_vocabulary()).unwrap();
    value["future_metadata"] = json!({"hint": true});
    value["lockableCells"]
        .as_array_mut()
        .unwrap()
        .push(json!("new_cell"));
    value["constructionComponents"]["new_layer"] =
        json!([{"key": "new_component", "statuses": ["", "installed"], "label": "New"}]);
    let decoded = decode_vocabulary(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(decoded.lockable_cells.iter().any(|cell| cell == "new_cell"));
    assert_eq!(
        decoded.construction_components["new_layer"][0].key,
        "new_component"
    );
}
