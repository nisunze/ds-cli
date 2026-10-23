use super::*;
fn run(action: &str, domain: Value, value: Value) -> Value {
    let result = evaluate(
        serde_json::from_value(json!({"action":action,"domain":domain,"value":value})).unwrap(),
    )
    .unwrap();
    serde_json::to_value(result).unwrap()["value"].clone()
}
#[test]
fn single_values_share_cleaning_rules() {
    let d =
        json!({"type":"categorical_number","values":[50,100],"aliases":{"small":50},"unit":"kVA"});
    assert_eq!(run("clean", d.clone(), json!("SMALL")), json!(50.0));
    assert_eq!(run("confirms", d.clone(), json!("50kVA")), json!(true));
    assert_eq!(run("other", d.clone(), json!("75 kVA")), json!(75.0));
    assert_eq!(run("other", d.clone(), json!(0)), Value::Null);
    assert_eq!(run("format", d, json!(50)), json!("50 kVA"));
    assert_eq!(
        run("coerce", json!({"type":"categorical_int"}), json!("12foo")),
        json!("12foo")
    );
    assert_eq!(
        run("coerce", json!({"type":"categorical_int"}), json!("1.0")),
        json!("1.0")
    );
    assert_eq!(
        run("coerce", json!({"type":"boolean"}), json!("yes")),
        json!(true)
    );
}
