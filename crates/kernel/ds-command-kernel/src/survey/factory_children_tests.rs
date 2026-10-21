use super::*;
fn registry() -> Value {
    json!([
        {"key":"dropdown","display_name":"Choose","allowed_as_child":true,"needs_options":true},
        {"key":"website","display_name":"Website","allowed_as_child":true},
        {"key":"photo","display_name":"Photo","allowed_as_child":true},
        {"key":"compound","display_name":"Compound","allowed_as_child":false},
    ])
}
fn evaluate_field(field: Value, intent: Value) -> Value {
    evaluate(&json!({"field":field,"registry":registry(),"intent":intent}))
}
fn empty() -> Value {
    json!({"id":"parent","type":"compound","children":[]})
}
fn add(field: &Value, id: &str) -> Value {
    evaluate_field(field.clone(), json!({"action":"add","nonce":id}))
}
fn patch(field: &mut Value, answer: &Value) {
    for (key, value) in answer["patch"].as_object().unwrap() {
        field[key] = value.clone();
    }
}

#[test]
fn child_choices_are_the_backend_authority_and_defaults_are_the_existing_factorys() {
    let mut field = empty();
    let answer = add(&field, "01234567-89ab-cdef-0123-456789abcdef");
    assert!(answer["refusal"].is_null());
    assert_eq!(
        answer["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["value"].clone())
            .collect::<Vec<_>>(),
        vec![json!("dropdown"), json!("website"), json!("photo")]
    );
    patch(&mut field, &answer);
    assert_eq!(
        field["children"][0]["id"],
        "f_0123456789abcdef0123456789abcdef"
    );
    assert_eq!(field["children"][0]["label"], "Choose");
    assert_eq!(
        field["children"][0]["key"],
        "dropdown_f_0123456789abcdef0123456789abcdef"
    );
    assert_eq!(
        field["children"][0]["options"],
        json!(["Option 1", "Option 2"])
    );
    assert_eq!(answer["rows"][0]["show_options"], true);
    let duplicate = add(&field, "0123456789abcdef0123456789abcdef");
    assert_eq!(duplicate["refusal"]["code"], INVALID);
    assert!(duplicate["patch"].is_null());
}

#[test]
fn edits_and_order_are_admitted_against_the_current_children_and_refusals_do_not_patch() {
    let mut field = empty();
    patch(
        &mut field,
        &add(&empty(), "0123456789abcdef0123456789abcdef"),
    );
    let next = add(&field, "1123456789abcdef0123456789abcdef");
    patch(&mut field, &next);
    let moved = evaluate_field(field.clone(), json!({"action":"move","from":1,"to":0}));
    patch(&mut field, &moved);
    assert_eq!(
        field["children"][0]["id"],
        "f_1123456789abcdef0123456789abcdef"
    );
    let update = evaluate_field(
        field.clone(),
        json!({"action":"update","index":0,"patch":{"key":"Main label!","label":"Main","required":true}}),
    );
    patch(&mut field, &update);
    assert_eq!(field["children"][0]["key"], "main_label_");
    let options = evaluate_field(
        field.clone(),
        json!({"action":"options","index":0,"text":" One, , Two "}),
    );
    patch(&mut field, &options);
    assert_eq!(field["children"][0]["options"], json!(["One", "Two"]));
    for intent in [
        json!({"action":"move","from":0,"to":9}),
        json!({"action":"update","index":0,"patch":{"type":"compound"}}),
        json!({"action":"update","index":0,"patch":{"id":"replaced"}}),
    ] {
        let refused = evaluate_field(field.clone(), intent);
        assert_eq!(refused["refusal"]["code"], INVALID);
        assert!(refused["patch"].is_null());
    }
    let removed = evaluate_field(field.clone(), json!({"action":"remove","index":0}));
    patch(&mut field, &removed);
    assert_eq!(field["children"].as_array().unwrap().len(), 1);
}

#[test]
fn unavailable_types_stay_visible_until_the_person_chooses_an_admitted_type() {
    let mut field = json!({"type":"compound","children":[{"id":"kept","key":"old","label":"Old","type":"retired"}]});
    let view = evaluate_field(field.clone(), Value::Null);
    assert_eq!(view["rows"][0]["allowed"], false);
    assert_eq!(view["rows"][0]["field"]["type"], "retired");
    let changed = evaluate_field(
        field.clone(),
        json!({"action":"update","index":0,"patch":{"type":"dropdown"}}),
    );
    patch(&mut field, &changed);
    assert_eq!(field["children"][0]["id"], "kept");
    assert_eq!(field["children"][0]["key"], "old");
    assert_eq!(
        field["children"][0]["options"],
        json!(["Option 1", "Option 2"])
    );
}

#[test]
fn key_value_events_and_choice_absence_have_explicit_owner_results() {
    let field = json!({"type":"kv","kvPredefinedKeys":["old","name"]});
    assert_eq!(
        evaluate_field(field.clone(), Value::Null)["kv_keys_text"],
        "old\nname"
    );
    assert_eq!(
        evaluate_field(field.clone(), json!({"action":"kv_toggle","value":true}))["patch"],
        json!({"kvUsePredefinedKeys":true})
    );
    assert_eq!(
        evaluate_field(
            field.clone(),
            json!({"action":"kv_keys","text":" A\n\n B "})
        )["patch"],
        json!({"kvPredefinedKeys":["A","B"]})
    );
    assert!(evaluate_field(
        field,
        json!({"action":"add","nonce":"0123456789abcdef0123456789abcdef"})
    )["patch"]
        .is_null());
    let none = evaluate(
        &json!({"field":empty(),"registry":[],"intent":{"action":"add","nonce":"0123456789abcdef0123456789abcdef"}}),
    );
    assert_eq!(none["can_add"], false);
    assert_eq!(none["refusal"]["code"], INVALID);
}

#[test]
fn bounded_children_and_invalid_registry_refuse_without_mutating_the_document() {
    let field =
        json!({"type":"compound","children":vec![json!({"id":"held","type":"website"});MAX]});
    assert_eq!(
        add(&field, "0123456789abcdef0123456789abcdef")["refusal"]["code"],
        INVALID
    );
    let duplicate = evaluate(
        &json!({"field":empty(),"registry":[{"key":"text","allowed_as_child":true},{"key":"text","allowed_as_child":false}]}),
    );
    assert_eq!(duplicate["refusal"]["code"], INVALID);
    assert!(duplicate["patch"].is_null());
}
