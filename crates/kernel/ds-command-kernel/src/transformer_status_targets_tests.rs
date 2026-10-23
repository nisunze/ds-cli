//! Targeted status reads match rows by the Kernel's transformer identity and
//! state deletion only from a refreshed, complete observation.
use super::*;

fn held(rows: Value, needs_refresh: bool) -> Observation {
    let document = json!({ "transformers": rows });
    Observation {
        sha256: digest(&document).unwrap(),
        document,
        observed_at_ms: 42,
        needs_refresh,
        last_error: None,
        revision: 3,
    }
}

fn names(value: &Value) -> Vec<&str> {
    value["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect()
}

#[test]
fn a_stored_name_outside_a_client_spelling_still_answers_its_target() {
    // A client rule (lowercase, non-[a-z0-9_] -> '_') spells "TX-1" as
    // "tx_1" and "9-lines" as "9_lines"; the stored rows keep their own names.
    let rows = json!([{"name":"TX-1"},{"name":"9-lines"},{"name":"other"}]);
    for targets in [
        vec!["tx_1".to_string(), "9_lines".to_string()],
        vec!["TX-1".to_string(), "9-lines".to_string()],
    ] {
        let answer = view(Some(held(rows.clone(), false)), true, &targets).unwrap();
        assert_eq!(names(&answer), ["TX-1", "9-lines"], "{targets:?}");
        assert_eq!(answer["deleted"], json!([]));
    }
}

#[test]
fn absence_deletes_only_after_a_refreshed_complete_observation() {
    let rows = json!([{"name":"kept"}]);
    let targets = vec!["kept".to_string(), "gone".to_string()];
    let refreshed = view(Some(held(rows.clone(), false)), true, &targets).unwrap();
    assert_eq!(names(&refreshed), ["kept"]);
    assert_eq!(refreshed["deleted"], json!(["gone"]));
    // Held (not refreshed in this read), invalidated or failed observations
    // never prove a deletion.
    for (needs_refresh, refreshed) in [(false, false), (true, false)] {
        let answer = view(Some(held(rows.clone(), needs_refresh)), refreshed, &targets).unwrap();
        assert_eq!(names(&answer), ["kept"]);
        assert_eq!(answer["deleted"], json!([]), "{needs_refresh} {refreshed}");
    }
}

#[test]
fn a_host_projection_uses_the_same_rule_and_admits_additive_row_fields() {
    let refreshed = status_observation(&held(json!([]), false), true, None).unwrap();
    let request = json!({"call":"select",
        "rows":[{"name":"TX-1","future_field":{"x":1}},{"name":"b"}],
        "targets":["tx_1","missing"], "observation": refreshed});
    let answer: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap()).unwrap();
    assert_eq!(
        answer["data"],
        json!([{"name":"TX-1","future_field":{"x":1}}])
    );
    assert_eq!(answer["deleted"], json!(["missing"]));
    let held_observation = status_observation(&held(json!([]), true), false, None).unwrap();
    let request = json!({"call":"select","rows":[{"name":"b"}],"targets":["missing"],
        "observation": held_observation});
    let answer: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap()).unwrap();
    assert_eq!(answer["deleted"], json!([]));
    let all =
        json!({"call":"select","rows":[{"name":"a"},{"name":"b"}],"targets":[],"observation":null});
    let answer: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&all).unwrap()).unwrap()).unwrap();
    assert_eq!(answer["data"].as_array().unwrap().len(), 2);
}

#[test]
fn malformed_rows_observations_and_requests_refuse() {
    for request in [
        json!({"call":"select","rows":[{"name":" bad"}],"targets":[],"observation":null}),
        json!({"call":"select","rows":[{"name":"a"}],"targets":["a"],
            "observation":{"source":"refresh","state":"held","observed_at_ms":1,"sha256":"a".repeat(64),"last_error":null}}),
        json!({"call":"select","rows":[],"targets":[],"observation":null,"extra":true}),
        json!({"call":"identities","names":vec!["a"; 2001]}),
    ] {
        assert!(
            evaluate(&serde_json::to_vec(&request).unwrap()).is_err(),
            "{request}"
        );
    }
    let answer: Value = serde_json::from_str(
        &evaluate(br#"{"call":"identities","names":["TX-1","9-lines","All Transformers"]}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(answer, json!(["tx_1", "_9_lines", "combined_transformer"]));
}
