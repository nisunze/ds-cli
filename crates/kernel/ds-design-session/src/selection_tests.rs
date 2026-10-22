use super::*;
use serde_json::json;

fn props(id: &str) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert("id".to_string(), json!(id));
    m
}

fn tab(tab_id: &str, layer_id: &str, form_slug: Option<&str>, ids: &[&str]) -> TabIndexInput {
    TabIndexInput {
        tab_id: tab_id.to_string(),
        layer_id: layer_id.to_string(),
        form_slug: form_slug.map(|s| s.to_string()),
        domain_context: None,
        id_map: ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.to_string(), i))
            .collect(),
    }
}

#[test]
fn empty_coordinator_returns_none() {
    let c = SelectionCoordinator::new();
    let r = c.find_feature_in_tabs(FeatureLookupInput {
        properties: props("anything"),
        layer_id: None,
        form_slug: None,
        feature_id: None,
        domain_context: None,
    });
    assert!(r.is_none());
}

#[test]
fn single_tab_finds_by_id() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab(
        "survey-customers",
        "survey-customers",
        Some("customers"),
        &["a", "b", "c"],
    ));
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("b"),
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.tab_id, "survey-customers");
    assert_eq!(r.row_index, 1);
}

#[test]
fn pure_lookup_uses_inline_tabs() {
    let r = find_feature_in_tabs(FeatureLookupTabsInput {
        tabs: vec![tab(
            "survey-customers",
            "survey-customers",
            Some("customers"),
            &["a", "b", "c"],
        )],
        properties: props("c"),
        layer_id: None,
        form_slug: Some("customers".to_string()),
        feature_id: None,
        domain_context: None,
    })
    .unwrap();
    assert_eq!(r.tab_id, "survey-customers");
    assert_eq!(r.row_index, 2);
}

#[test]
fn no_match_returns_none() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab(
        "survey-customers",
        "survey-customers",
        Some("customers"),
        &["a"],
    ));
    let r = c.find_feature_in_tabs(FeatureLookupInput {
        properties: props("missing"),
        layer_id: None,
        form_slug: None,
        feature_id: None,
        domain_context: None,
    });
    assert!(r.is_none());
}

#[test]
fn form_slug_preference_picks_direct_match() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab(
        "survey-customers",
        "survey-customers",
        Some("customers"),
        &["dup"],
    ));
    c.set_tab(tab(
        "survey-assets",
        "survey-assets",
        Some("assets"),
        &["dup"],
    ));

    let pick_customers = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("dup"),
            layer_id: None,
            form_slug: Some("customers".to_string()),
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(pick_customers.tab_id, "survey-customers");

    let pick_assets = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("dup"),
            layer_id: None,
            form_slug: Some("assets".to_string()),
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(pick_assets.tab_id, "survey-assets");
}

#[test]
fn layer_id_preference_picks_direct_match() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("a-tab", "a-tab", None, &["dup"]));
    c.set_tab(tab("b-tab", "b-tab", None, &["dup"]));
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("dup"),
            layer_id: Some("b-tab".to_string()),
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.tab_id, "b-tab");
}

#[test]
fn design_layer_property_picks_lv_or_mv_tab() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("lv:lv_lines", "lv:lv_lines", None, &["pole-1"]));
    c.set_tab(tab(
        "survey-poles",
        "survey-poles",
        Some("poles"),
        &["pole-1"],
    ));

    let mut p = props("pole-1");
    p.insert(DESIGN_LAYER_PROP.to_string(), json!("lv_lines"));
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: p,
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.tab_id, "lv:lv_lines");
}

#[test]
fn fallback_picks_lex_first_when_no_preference() {
    let mut c = SelectionCoordinator::new();
    // Insert in reverse order to confirm BTreeMap ordering — not insertion.
    c.set_tab(tab("z-tab", "z-tab", None, &["dup"]));
    c.set_tab(tab("a-tab", "a-tab", None, &["dup"]));
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("dup"),
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.tab_id, "a-tab");
}

#[test]
fn id_falls_back_to_doc_id_then_feature_id() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("survey", "survey", None, &["abc"]));

    let mut p = HashMap::new();
    p.insert("doc_id".to_string(), json!("abc"));
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: p,
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.row_index, 0);

    let mut p2 = HashMap::new();
    p2.insert("feature_id".to_string(), json!("abc"));
    let r2 = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: p2,
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r2.row_index, 0);
}

#[test]
fn remove_tab_drops_lookup() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("t", "t", None, &["x"]));
    assert!(c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("x"),
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .is_some());
    c.remove_tab("t");
    assert!(c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("x"),
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .is_none());
}

#[test]
fn set_tab_replaces_id_map_completely() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("t", "t", None, &["a", "b"]));
    c.set_tab(tab("t", "t", None, &["b", "c"])); // 'a' must drop

    assert!(c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("a"),
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .is_none());
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: props("c"),
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.row_index, 1);
}

#[test]
fn clear_drops_all_tabs() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("a", "a", None, &["x"]));
    c.set_tab(tab("b", "b", None, &["y"]));
    c.clear();
    assert_eq!(c.tab_count(), 0);
}

#[test]
fn numeric_id_is_stringified() {
    let mut c = SelectionCoordinator::new();
    c.set_tab(tab("t", "t", None, &["42"]));
    let mut p = HashMap::new();
    p.insert("id".to_string(), json!(42));
    let r = c
        .find_feature_in_tabs(FeatureLookupInput {
            properties: p,
            layer_id: None,
            form_slug: None,
            feature_id: None,
            domain_context: None,
        })
        .unwrap();
    assert_eq!(r.row_index, 0);
}

#[test]
fn duplicate_native_ids_require_the_exact_model_revision_and_source_layer() {
    let context = |model: &str, revision: &str, layer: &str| {
        Some(SelectionDomainContext::Grid {
            model_id: model.to_string(),
            model_revision: revision.to_string(),
            source_layer: layer.to_string(),
        })
    };
    let mut a1 = tab("grid-a1", "structures", None, &["structure-47"]);
    a1.domain_context = context("model-a", "revision:1", "structures");
    let mut a2 = tab("grid-a2", "structures", None, &["structure-47"]);
    a2.domain_context = context("model-a", "revision:2", "structures");
    let mut b2 = tab("grid-b2", "structures", None, &["structure-47"]);
    b2.domain_context = context("model-b", "revision:2", "structures");

    let result = find_feature_in_tabs(FeatureLookupTabsInput {
        tabs: vec![a1, a2, b2],
        properties: HashMap::new(),
        layer_id: Some("ds-grid:model-a:structures".into()),
        form_slug: None,
        feature_id: Some("structure-47".into()),
        domain_context: context("model-a", "revision:2", "structures"),
    })
    .unwrap();
    assert_eq!(result.tab_id, "grid-a2");
}

#[test]
fn all_identity_aliases_and_runtime_sketch_layers_are_resolved_in_rust() {
    let mut properties = HashMap::new();
    properties.insert("id".into(), json!("different-public-id"));
    properties.insert("_sketches_id".into(), json!("stake-2"));
    let result = find_feature_in_tabs(FeatureLookupTabsInput {
        tabs: vec![tab(
            "sketch-staking_table",
            "sketch-staking_table",
            None,
            &["stake-1", "stake-2"],
        )],
        properties,
        layer_id: Some("sketches-staking_table-circle".into()),
        form_slug: None,
        feature_id: Some("opaque-rendered-id".into()),
        domain_context: None,
    })
    .unwrap();
    assert_eq!(result.tab_id, "sketch-staking_table");
    assert_eq!(result.row_index, 1);
}

#[test]
fn lookup_response_admission_retains_none_and_exact_held_rows() {
    let held = vec!["customers".to_owned()];
    assert_eq!(admit_feature_lookup_result(None, &held).unwrap(), None);
    let result = FeatureLookupResult {
        tab_id: "customers".into(),
        row_index: 47,
    };
    assert_eq!(
        admit_feature_lookup_result(Some(result.clone()), &held).unwrap(),
        Some(result)
    );
    assert_eq!(held, ["customers"]);
}

#[test]
fn lookup_response_admission_refuses_empty_foreign_and_unsafe_identities() {
    let held = vec!["customers".to_owned()];
    for tab_id in ["", "another-window"] {
        let result = FeatureLookupResult {
            tab_id: tab_id.into(),
            row_index: 0,
        };
        assert!(admit_feature_lookup_result(Some(result), &held)
            .unwrap_err()
            .starts_with("selection_response_incompatible:"));
    }
    #[cfg(target_pointer_width = "64")]
    assert!(admit_feature_lookup_result(
        Some(FeatureLookupResult {
            tab_id: "customers".into(),
            row_index: 9_007_199_254_740_992,
        }),
        &held,
    )
    .is_err());
    assert_eq!(held, ["customers"]);
}

#[test]
fn lookup_owner_json_admission_keeps_required_fields_and_additive_results() {
    for value in [
        json!({}),
        json!({"properties": []}),
        json!({"properties": {}, "tabs": [{"tab_id":"t", "layer_id":"l", "id_map":[["id", -1]]}]}),
        json!({"properties": {}, "domain_context":{"kind":"grid", "model_id":"m"}}),
    ] {
        assert!(serde_json::from_value::<FeatureLookupTabsInput>(value).is_err());
    }
    let input: FeatureLookupTabsInput = serde_json::from_value(json!({
        "properties": {"id":"x"}, "future_input": true,
        "tabs": [{"tab_id":"t", "layer_id":"l", "id_map":[["x", 3]], "future_tab":true}]
    }))
    .unwrap();
    assert_eq!(find_feature_in_tabs(input).unwrap().row_index, 3);
    let result: FeatureLookupResult = serde_json::from_value(json!({
        "tab_id":"t", "row_index":3, "contract":27, "future_fact":true
    }))
    .unwrap();
    assert_eq!(
        admit_feature_lookup_result(Some(result), &["t".into()]).unwrap(),
        Some(FeatureLookupResult {
            tab_id: "t".into(),
            row_index: 3,
        })
    );
}
