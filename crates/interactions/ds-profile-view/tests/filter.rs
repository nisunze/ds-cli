use ds_profile_view::{
    filter::{compose, operators},
    panel_model::FilterOp,
};
fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).into()).collect()
}
#[test]
fn native_matches_compose_stably_without_mutating_or_duplicating_ids() {
    let current = strings(&["ground-1", "pole-1", "pole-2"]);
    let matched = strings(&["pole-2", "pole-3", "pole-3"]);
    assert_eq!(
        compose("replace", &current, &matched),
        strings(&["pole-2", "pole-3"])
    );
    assert_eq!(
        compose("add", &current, &matched),
        strings(&["ground-1", "pole-1", "pole-2", "pole-3"])
    );
    assert_eq!(
        compose("remove", &current, &matched),
        strings(&["ground-1", "pole-1"])
    );
    assert_eq!(
        compose("intersect", &current, &matched),
        strings(&["pole-2"])
    );
    assert_eq!(matched, strings(&["pole-2", "pole-3", "pole-3"]));
}
#[test]
fn conditions_follow_native_column_types() {
    assert!(
        operators("float")
            .iter()
            .any(|(op, _)| *op == FilterOp::Between)
    );
    assert!(
        !operators("boolean")
            .iter()
            .any(|(op, _)| *op == FilterOp::Contains)
    );
    assert!(
        operators("string")
            .iter()
            .any(|(op, _)| *op == FilterOp::Contains)
    );
}

#[test]
fn incomplete_controls_refuse_without_evaluating_native_rows() {
    use ds_profile_view::{
        filter::validate,
        panel_model::{Filter, FilterColumn},
    };
    let columns = vec![FilterColumn {
        name: "height".into(),
        data_type: "float".into(),
    }];
    let mut filters = vec![Filter {
        column: "height".into(),
        op: FilterOp::Between,
        value: "12".into(),
        value2: None,
    }];
    assert_eq!(
        validate(&filters, &columns),
        Err("Enter both bounds for Between.".into())
    );
    filters[0].value2 = Some("18".into());
    assert!(validate(&filters, &columns).is_ok());
    filters[0].value = " ".into();
    assert_eq!(
        validate(&filters, &columns),
        Err("Enter a value for every condition.".into())
    );
    filters[0].op = FilterOp::IsEmpty;
    assert!(validate(&filters, &columns).is_ok());
    filters[0].column = "stale".into();
    assert_eq!(
        validate(&filters, &columns),
        Err("Unknown field: stale".into())
    );
}
