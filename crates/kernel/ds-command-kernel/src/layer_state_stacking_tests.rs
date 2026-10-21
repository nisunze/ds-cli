use super::*;
#[test]
fn screen_mount_priorities_preserve_explicit_order_and_old_server_authority() {
    let rows = [
        (
            "snapshot",
            "circle",
            "design_tile",
            "master/lv_poles_vt",
            230,
        ),
        ("live", "line", "design", "master/lv_lines", 110),
        ("survey", "circle", "survey", "edcl_customers_survey", 200),
        ("existing", "line", "global_layer", "gt/mv_line", 100),
        ("reference", "fill", "global_layer", "gt/landform", 0),
    ];
    let ids: Vec<String> = rows.iter().map(|row| row.0.into()).collect();
    let mut raw = json!({"project_id":"p","layers":rows.iter().map(|(id,kind,class,reference,order)| json!({"id":id,"type":kind,"style_ref":reference,"metadata":{"layer_class":class,"order":order,"user_order":null}})).collect::<Vec<_>>()});
    for (row, tier) in raw["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip([4, 5, 2, 3, 0])
    {
        row["metadata"]["presentation_tier"] = json!(tier);
    }
    let document: Document = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        stack_order(&ids, Some(&document), true).unwrap(),
        vec!["reference", "survey", "existing", "snapshot", "live"]
    );
    assert_eq!(stack_order(&ids, Some(&document), false).unwrap(), ids);
    // A saved operator order is absolute across presentation tiers;
    // unedited rows retain their effective order alongside it.
    raw["layers"][3]["metadata"]["user_order"] = json!(999);
    raw["layers"][3]["metadata"]["order"] = json!(999);
    let saved: Document = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        stack_order(&ids, Some(&saved), true).unwrap(),
        vec!["reference", "live", "survey", "snapshot", "existing"]
    );
    // Older catalogues never declared provenance: preserve their array.
    raw["layers"][0]["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("user_order");
    let legacy: Document = serde_json::from_value(raw).unwrap();
    assert_eq!(stack_order(&ids, Some(&legacy), true).unwrap(), ids);
}
#[test]
fn casing_stays_under_primary_on_fresh_mount_and_refetch() {
    for ids in [
        vec![
            "survey",
            "lv__boundary",
            "lv",
            "lv__label",
            "mv__boundary",
            "mv",
        ],
        vec![
            "survey",
            "lv",
            "mv",
            "lv__label",
            "lv__boundary",
            "mv__boundary",
        ],
    ] {
        let ids: Vec<String> = ids.into_iter().map(str::to_owned).collect();
        let ordered = stack_order(&ids, None, false).unwrap();
        assert_eq!(
            ordered,
            vec![
                "survey",
                "lv__boundary",
                "lv",
                "lv__label",
                "mv__boundary",
                "mv"
            ]
        );
        assert_eq!(stack_order(&ordered, None, false).unwrap(), ordered);
    }
    assert!(stack_order(&[" invalid".into()], None, false).is_err());
    let document: Document = serde_json::from_value(
        json!({"project_id":"p","layers":[{"id":"park","type":"fill"}]}),
    )
    .unwrap();
    assert_eq!(
        stack_order(
            &["park__boundary".into(), "park".into()],
            Some(&document),
            false
        )
        .unwrap(),
        vec!["park", "park__boundary"]
    );
}
