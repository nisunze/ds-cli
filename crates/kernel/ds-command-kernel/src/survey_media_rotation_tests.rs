use super::*;

const PATH: &str = "p1/forms/poles/e1/a.jpg";

fn data(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn apply(data: &mut Map<String, Value>, turned: &Turned) {
    for (key, value) in turned.updated_data.as_object().unwrap() {
        data.insert(key.clone(), value.clone());
    }
}

#[test]
fn four_clockwise_turns_return_to_no_record() {
    let mut entry = data(json!({"pole": "x"}));
    let mut seen = Vec::new();
    for _ in 0..4 {
        let turned = turn(&entry, PATH, "1729", 90).unwrap();
        seen.push((turned.previous_quarter_turns, turned.quarter_turns));
        apply(&mut entry, &turned);
    }
    assert_eq!(seen, [(0, 1), (1, 2), (2, 3), (3, 0)]);
    assert_eq!(entry[KEY], json!({}), "a full circle leaves no record");
    assert_eq!(quarter_turns(&entry, PATH, Some("1729")), 0);
}

#[test]
fn turns_accumulate_and_counter_clockwise_is_three_quarters() {
    let mut entry = Map::new();
    let turned = turn(&entry, PATH, "1729", -90).unwrap();
    assert_eq!(turned.quarter_turns, 3);
    assert_eq!(
        turned.updated_data,
        json!({KEY: {PATH: {"quarter_turns": 3, "generation": "1729"}}})
    );
    apply(&mut entry, &turned);
    // A second turn while the first is still queued simply adds.
    let again = turn(&entry, PATH, "1729", 180).unwrap();
    assert_eq!((again.previous_quarter_turns, again.quarter_turns), (3, 1));
}

#[test]
fn a_record_for_other_bytes_is_not_applied_and_others_are_kept() {
    let entry = data(json!({KEY: {
        PATH: {"quarter_turns": 2, "generation": "11"},
        "p1/forms/poles/e1/b.jpg": {"quarter_turns": 1, "generation": "12"}
    }}));
    assert_eq!(quarter_turns(&entry, PATH, Some("11")), 2);
    assert_eq!(quarter_turns(&entry, PATH, Some("99")), 0);
    assert_eq!(quarter_turns(&entry, PATH, None), 2);
    let turned = turn(&entry, PATH, "99", 90).unwrap();
    assert_eq!((turned.previous_quarter_turns, turned.quarter_turns), (0, 1));
    assert_eq!(
        turned.updated_data[KEY]["p1/forms/poles/e1/b.jpg"],
        json!({"quarter_turns": 1, "generation": "12"})
    );
}

#[test]
fn thumbnails_foreign_paths_bad_generations_and_malformed_maps_are_refused() {
    let entry = Map::new();
    assert!(turn(&entry, "p1/forms/poles/e1/a_thunder.jpeg", "1", 90)
        .unwrap_err()
        .contains("Thumbnail"));
    assert!(turn(&entry, "/p1/forms/poles/e1/a.jpg", "1", 90).is_err());
    assert!(turn(&entry, "not a path", "1", 90).is_err());
    for generation in ["", "0", "01", "x1", "123456789012345678901"] {
        assert!(turn(&entry, PATH, generation, 90).is_err(), "{generation}");
    }
    for degrees in [0, 45, 360, -360, 450] {
        assert!(turn(&entry, PATH, "1", degrees).is_err(), "{degrees}");
    }
    let bad = data(json!({KEY: {PATH: {"quarter_turns": 5, "generation": "1"}}}));
    assert!(turn(&bad, PATH, "1", 90).unwrap_err().contains("malformed"));
    assert_eq!(quarter_turns(&bad, PATH, None), 0);
    let bad = data(json!({KEY: "90"}));
    assert!(turn(&bad, PATH, "1", 90).is_err());
}

#[test]
fn the_door_reads_a_feature_and_turns_it() {
    let feature = json!({"type": "Feature", "properties": {"data": {KEY: {PATH: {"quarter_turns": 1, "generation": "5"}}}}});
    let read: Value = serde_json::from_str(
        &evaluate(
            &serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "read", "entry": feature, "path": PATH, "generation": "5"}}))
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(read["quarter_turns"], 1);
    let turned: Value = serde_json::from_str(
        &evaluate(
            &serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "turn", "entry": feature, "path": PATH, "generation": "5", "degrees": 90}}))
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(turned["quarter_turns"], 2);
    assert_eq!(turned["thumbnail_object_path"], "p1/forms/poles/e1/a_thunder.jpeg");
    assert!(evaluate(br#"{"schema":"other","op":{"kind":"read","entry":{},"path":"x"}}"#).is_err());
}
