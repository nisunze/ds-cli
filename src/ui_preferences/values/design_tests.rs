use super::*;
use serde_json::json;

#[test]
fn pins_and_the_active_transformer_are_lv_boxes_the_design_owner_admits() {
    assert_eq!(
        pinned_transformers(&json!(["TX-10", "TX-2", "TX-10"])),
        Some(json!(["TX-10", "TX-2"])),
        "each pin once, in the order given; the window sorts for display"
    );
    for refused in [
        json!(["mv_data"]),
        json!(["all_transformers"]),
        json!(["a/b"]),
        json!([" TX-1"]),
        json!([7]),
        json!("TX-1"),
    ] {
        assert!(pinned_transformers(&refused).is_none(), "{refused}");
    }
    assert_eq!(pinned_transformers(&json!([])), Some(json!([])));
    assert_eq!(
        active_lv_transformer(&json!("Nyamata 3")),
        Some(json!("Nyamata 3"))
    );
    for refused in [
        json!("collisions"),
        json!("collisions_2026"),
        json!(""),
        json!({"transformerName": "TX-1"}),
    ] {
        assert!(active_lv_transformer(&refused).is_none(), "{refused}");
    }
}

#[test]
fn the_process_preset_is_the_process_settings_vocabulary() {
    assert_eq!(process_preset(&json!("drafting")), Some(json!("drafting")));
    assert_eq!(process_preset(&json!("sketch")), Some(json!("sketch")));
    assert!(process_preset(&json!("Drafting")).is_none());
    assert!(process_preset(&json!({"preset": "sketch"})).is_none());
}

#[test]
fn the_register_sort_and_warning_filter_are_the_status_query_owners() {
    assert_eq!(
        status_sort(&json!({"key": "governance", "asc": false})),
        Some(json!({"key": "governance", "asc": false}))
    );
    assert_eq!(
        status_sort(&json!({"key": null, "asc": true})),
        Some(json!({"key": null, "asc": true}))
    );
    assert!(status_sort(&json!({"key": "colour", "asc": true})).is_none());
    assert!(status_sort(&json!({"key": "name"})).is_none());
    assert!(status_sort(&json!({"key": "name", "asc": 1})).is_none());
    assert_eq!(
        status_warning_types(&json!([" vd_fail", "orphan_pole", "vd_fail", ""])),
        Some(json!(["orphan_pole", "vd_fail"]))
    );
    assert_eq!(status_warning_types(&json!([])), Some(json!([])));
    assert!(status_warning_types(&json!([1])).is_none());
    assert!(status_warning_types(&json!("vd_fail")).is_none());
}

#[test]
fn map_data_sources_are_each_kept_once() {
    assert_eq!(
        create_transformer_map_sources(
            &json!({"selectedKeys": ["survey://poles", "temp://a", "survey://poles"]})
        ),
        Some(json!({"selectedKeys": ["survey://poles", "temp://a"]}))
    );
    assert!(create_transformer_map_sources(&json!({"selectedKeys": [""]})).is_none());
    assert!(create_transformer_map_sources(&json!({"selected": []})).is_none());
    assert_eq!(switch(&json!(false)), Some(json!(false)));
    assert!(switch(&json!("false")).is_none());
}

fn sources(addresses: Value, labels: Value) -> Value {
    json!({"addresses": addresses, "labels": labels, "updated_at": 1_760_000_000_000_u64})
}

#[test]
fn fast_mode_sources_are_admitted_by_the_design_source_resolver() {
    let customers = sources(
        json!(["design://layer/customers", "survey://customers"]),
        json!(["Design customers", "Survey"]),
    );
    assert_eq!(
        fast_mode_sources(SourceKind::Customers, &customers),
        Some(customers.clone())
    );
    for refused in [
        sources(json!([]), json!([])),
        sources(json!(["survey://a", "survey://a"]), json!(["A", "A"])),
        sources(json!(["survey://a"]), json!([])),
        sources(json!([" survey://a"]), json!(["A"])),
        json!({"addresses": ["survey://a"], "labels": ["A"]}),
    ] {
        assert!(
            fast_mode_sources(SourceKind::Customers, &refused).is_none(),
            "{refused}"
        );
    }
    let poles = sources(
        json!(["design://layer/customers", "survey://poles"]),
        json!(["Design customers", "Poles"]),
    );
    assert_eq!(
        fast_mode_sources(SourceKind::Poles, &poles),
        Some(sources(json!(["survey://poles"]), json!(["Poles"]))),
        "the design-customers sentinel never names a pole table"
    );
    let none = sources(json!([]), json!([]));
    assert_eq!(
        fast_mode_sources(SourceKind::Poles, &none),
        Some(none.clone()),
        "no borrowed pole table is a choice"
    );
}
