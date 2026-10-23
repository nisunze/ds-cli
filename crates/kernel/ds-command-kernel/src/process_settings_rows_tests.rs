//! Parity with the retired TypeScript rules (`process-config-contract.ts`,
//! `project-process-settings.ts`): the same cases now answer from the Kernel.
use super::super::evaluate;
use super::*;
use serde_json::json;

fn settings(value: Value) -> Settings {
    value.as_object().unwrap().clone()
}

fn missing(settings_value: Value, sheets: Value) -> Vec<&'static str> {
    readiness(
        &settings(settings_value),
        Some(&json!({ "sheets": sheets })),
    )
    .missing
}

#[test]
fn readiness_names_the_sheets_a_run_needs_exactly_as_before() {
    assert_eq!(
        missing(
            json!({"build_network":true,"connect_unconnected_customers":true}),
            json!({})
        ),
        ["project_settings", "transfo_sizes"]
    );
    assert_eq!(
        missing(
            json!({"build_network":false,"delete_strange_cols":true}),
            json!({})
        ),
        ["know_columns"]
    );
    assert!(missing(
        json!({"build_network":false,"delete_strange_cols":true}),
        json!({"know_columns":[{}]})
    )
    .is_empty());
    assert_eq!(
        missing(
            json!({"keep_lv_lines_sizes":false,"keep_service_cables_sizes":false,
                "assign_struct_types":true,"consider_cable_size_in_pole_placement":true}),
            json!({"project_settings":[{}]})
        ),
        [
            "transfo_sizes",
            "lv_lines",
            "cust_category",
            "service_cable_sizes",
            "lv_poles_rules"
        ]
    );
    assert!(missing(
        json!({"build_network":false,"keep_service_cables_sizes":false}),
        json!({"customers_categories_info_df":[{}],"service_cable_sizes":[{}]})
    )
    .is_empty());
    assert!(missing(
        json!({"build_network":false,"assign_struct_types":true}),
        json!({"lv_poles_rules":{"rule_edcl":[{}],"rule_tchd":[{}]}})
    )
    .is_empty());
    assert_eq!(
        missing(
            json!({"build_network":false,"assign_assemblies":true}),
            json!({"lv_poles_rules":{}})
        ),
        ["lv_poles_rules"]
    );
    assert_eq!(
        missing(
            json!({"build_network":false,"assign_assemblies":true}),
            json!({"lv_poles_rules":{"rule_edcl":[]}})
        ),
        ["lv_poles_rules"]
    );
    assert!(missing(
        json!({"build_network":false}),
        json!({"project_settings":[{"parameter":"use_struct_sizes","value":true},
            {"parameter":"use_assembly_translation","value":"yes"}]})
    )
    .is_empty());
}

#[test]
fn the_verdict_is_typed_for_empty_missing_and_ready_configurations() {
    let empty = readiness(
        &settings(json!({"build_network":false})),
        Some(&json!({"sheets":{}})),
    );
    // Settings that need no sheet may run without a configuration (the replay
    // path); a surface that requires a configured project renders the refusal.
    assert!(!empty.has_config && empty.ready);
    assert_eq!(empty.refusal.unwrap().code, CONFIG_EMPTY);
    let empty_needing = readiness(
        &settings(json!({"build_network":true})),
        Some(&json!({"sheets":{}})),
    );
    assert!(!empty_needing.ready);
    assert_eq!(empty_needing.refusal.unwrap().code, CONFIG_EMPTY);
    let absent = readiness(&settings(json!({"build_network":false})), None);
    assert_eq!(absent.refusal.unwrap().code, CONFIG_EMPTY);
    let short = readiness(
        &settings(json!({"build_network":true})),
        Some(&json!({"sheets":{"project_settings":[{}]}})),
    );
    let refusal = short.refusal.unwrap();
    assert_eq!(refusal.code, CONFIG_MISSING);
    assert!(
        refusal.message.contains("transfo_sizes"),
        "{}",
        refusal.message
    );
    let ready = readiness(
        &settings(json!({"build_network":true})),
        Some(&json!({"sheets":{"project_settings":[{}],"transfo_sizes":[{}]},"future":1})),
    );
    assert!(ready.ready && ready.refusal.is_none() && ready.missing.is_empty());
    let door: Value = serde_json::from_str(
        &evaluate(br#"{"op":"config_readiness","settings":{"build_network":true},"project_config":{"sheets":{"project_settings":[{}]}}}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(door["missing"], json!(["transfo_sizes"]));
    assert_eq!(door["refusal"]["code"], CONFIG_MISSING);
    assert!(evaluate(br#"{"op":"config_readiness","settings":{"x":"text"}}"#).is_err());
}

fn template() -> Value {
    json!({"sheets":{
        "transformer_settings":[
            {"parameter":"keep_lv_poles","value":false},
            {"parameter":"Keep LV Poles","value":"true","preset":"sketch"},
            {"parameter":"join_lv_cables_of_same_size","value":"yes","preset":"Drafting"},
            {"parameter":"weld_tolerance","value":"0.25","preset":"drafting"},
            {"parameter":"not_a_value","value":"maybe","preset":"drafting"},
            {"parameter":"customer_source_layers","value":"edcl_customers, edcl_hh;edcl_customers"},
            {"parameter":"pole_source_layers","value":["edcl_poles"]},
            {"parameter":"include_design_customers","value":"false"}
        ],
        "project_settings":[{"parameter":"Default Process Preset","value":"Drafting"}]
    }})
}

#[test]
fn sources_read_the_base_rows_exactly_as_before() {
    assert_eq!(
        sources(Some(&template())),
        Sources {
            customer_layers: vec!["edcl_customers".into(), "edcl_hh".into()],
            pole_layers: vec!["edcl_poles".into()],
            include_design_customers: Some(false),
            customer_layers_declared: true,
            pole_layers_declared: true,
        }
    );
    assert_eq!(
        sources(None),
        Sources {
            customer_layers: vec![],
            pole_layers: vec![],
            include_design_customers: None,
            customer_layers_declared: false,
            pole_layers_declared: false,
        }
    );
}

#[test]
fn the_default_preset_row_is_upserted_once() {
    let config = json!({"sheets":{"project_settings":[
        {"parameter":"country","value":"rw"},
        {"parameter":"Default Process Preset","value":"sketch"},
        {"parameter":"default_process_preset","value":"sketch"}]}});
    assert_eq!(
        Value::from(default_preset_rows(Some(&config), Preset2::Drafting)),
        json!([{"parameter":"country","value":"rw"},
            {"parameter":"default_process_preset","value":"drafting"}])
    );
    assert_eq!(
        Value::from(default_preset_rows(None, Preset2::Sketch)),
        json!([{"parameter":"default_process_preset","value":"sketch"}])
    );
}

#[test]
fn preset_rows_replace_one_preset_and_keep_every_other_row() {
    let next = preset_rows(
        Some(&template()),
        Preset2::Drafting,
        &settings(json!({"keep_lv_poles":false,"Weld Tolerance":0.5})),
    );
    let drafting: Vec<_> = next
        .iter()
        .filter(|row| row.get("preset") == Some(&json!("drafting")))
        .cloned()
        .collect();
    assert_eq!(
        Value::from(drafting),
        json!([{"parameter":"keep_lv_poles","value":false,"preset":"drafting"},
            {"parameter":"weld_tolerance","value":0.5,"preset":"drafting"}])
    );
    assert_eq!(
        next.iter()
            .filter(|row| row.get("preset") == Some(&json!("sketch")))
            .count(),
        1
    );
    assert!(next
        .iter()
        .any(|row| row["parameter"] == "keep_lv_poles" && row.get("preset").is_none()));
    // A non-setting value or an empty name is skipped, exactly as the retired writer did.
    let door: Value = serde_json::from_str(
        &evaluate(br#"{"op":"preset_rows","preset":"drafting","settings":{"bad":"x","--":true,"Keep LV Poles":true}}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        door,
        json!({"sheet":"transformer_settings","rows":[{"parameter":"keep_lv_poles","value":true,"preset":"drafting"}]})
    );
}

#[test]
fn source_rows_upsert_declarations_and_collapse_aliases() {
    let update: SourceUpdate = serde_json::from_value(
        json!({"customer_layers":["a","b"],"pole_layers":[],"include_design_customers":true}),
    )
    .unwrap();
    let sourced = source_rows(Some(&template()), &update);
    let value = |name: &str| {
        sourced
            .iter()
            .find(|row| row["parameter"] == name)
            .map(|row| row["value"].clone())
    };
    assert_eq!(value("customer_source_layers"), Some(json!("a,b")));
    assert_eq!(value("pole_source_layers"), Some(json!("")));
    assert_eq!(value("include_design_customers"), Some(json!(true)));
    assert_eq!(
        sourced
            .iter()
            .filter(|row| row.get("preset").is_some())
            .count(),
        4
    );

    let config = json!({"sheets":{"transformer_settings":[
        {"parameter":"Customer Source Layers","value":"old","note":"keep"},
        {"parameter":"customer_source_layers","value":"stale"},
        {"parameter":"include_design_customers","value":true},
        {"parameter":"Include Design Customers","value":true},
        {"parameter":"customer_source_layers","value":"preset","preset":"sketch"}]}});
    let update: SourceUpdate =
        serde_json::from_value(json!({"customer_layers":[],"include_design_customers":null}))
            .unwrap();
    let next = source_rows(Some(&config), &update);
    assert_eq!(
        Value::from(next.clone()),
        json!([{"parameter":"Customer Source Layers","value":"","note":"keep"},
            {"parameter":"customer_source_layers","value":"preset","preset":"sketch"}])
    );
    let read = sources(Some(&json!({"sheets":{"transformer_settings":next}})));
    assert!(read.customer_layers.is_empty() && read.customer_layers_declared);
    assert_eq!(read.include_design_customers, None);
    // An absent member leaves its rows untouched; an unknown member refuses.
    let untouched = source_rows(Some(&config), &SourceUpdate::default());
    assert_eq!(untouched.len(), 5);
    assert!(
        evaluate(br#"{"op":"source_rows","sources":{"customer_layers":[],"surprise":1}}"#).is_err()
    );
    let door: Value = serde_json::from_str(
        &evaluate(br#"{"op":"source_rows","sources":{"pole_layers":["p"]}}"#).unwrap(),
    )
    .unwrap();
    assert_eq!(
        door,
        json!({"sheet":"transformer_settings","rows":[{"parameter":"pole_source_layers","value":"p"}]})
    );
}
