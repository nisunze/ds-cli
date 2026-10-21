use super::*;
fn head(model: &str) -> Value {
    json!({"model_id":model,"display_name":"Copied original name","model_kind":"mv_line","lifecycle":"promoted","state":"active","head_revision_id":"head","head_version":1,"head_model_digest":"a".repeat(64),"manifest_model_id":model})
}
fn inventory() -> Inventory {
    let context = json!({"account":"owner","project":"template","generation":1});
    let rows = (0..11).map(|n| { let model=format!("model-{}",n%3); json!({"model_id":format!("local-{n}"),"name":"Opened copy","selected":n==0,"read_only":false,"persisted":false,"host_revision":"rev:dirty","workspace_owner":"mv-design","project_binding":{"dsProject":if n<3 {"template"} else {"other"},"modelId":model,"revisionId":"head","version":1,"digest":"a".repeat(64)},"saving":false,"pending_operations":0,"physical_worker_active":false,"native":{"manifest_model_id":model,"revision":"rev:dirty","model_revision":7,"coordinate_system":"EPSG:32736","history":{"model_revision":"rev:dirty","initial_revision":"rev:base","undo_depth":4,"redo_depth":2,"history_pin":"pin"}}}) }).collect::<Vec<_>>();
    grid_held_models::admit(json!({"captured_context":context,"context":context,"total":rows.len(),"rows":rows,"mutated":false})).unwrap()
}
fn listing() -> Value {
    json!({"project":"template","models":(0..3).map(|n| head(&format!("model-{n}"))).collect::<Vec<_>>(),"more":false})
}
#[test]
fn three_project_heads_remain_three_choices_despite_eleven_dirty_open_copies() {
    let held = inventory();
    let before = serde_json::to_value(&held).unwrap();
    let result = project("template", "owner", &listing(), Some(held.clone())).unwrap();
    assert_eq!(result["models"].as_array().unwrap().len(), 3);
    for row in result["models"].as_array().unwrap() {
        assert_eq!(row["state"], "held");
        assert_eq!(row["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(row["source"]["project_id"], "template");
    }
    assert_eq!(serde_json::to_value(held).unwrap(), before);
    let source: SourceBinding =
        serde_json::from_value(result["models"][0]["source"].clone()).unwrap();
    assert!(admit_source("template", &result, &source, "rev:dirty").is_ok());
    assert!(admit_head("template", "owner", &listing(), &source, "rev:dirty").is_ok());
    assert!(admit_head("foreign", "owner", &listing(), &source, "rev:dirty").is_err());
    assert!(admit_head("template", "owner", &listing(), &source, "rev:changed").is_err());
    assert_eq!(
        scene_revision(br#"{"model_revision":"rev:dirty","large_data":[1,2,3]}"#).unwrap(),
        "rev:dirty"
    );
    assert!(admit_source("other", &result, &source, "rev:dirty").is_err());
    assert!(admit_source("template", &result, &source, "rev:new").is_err());
}
#[test]
fn exact_copy_ambiguity_requires_selection_and_old_or_unbound_copies_do_not_select_a_head() {
    let mut held = inventory();
    let mut duplicate = held.rows[0].clone();
    duplicate.model_id = "another-copy".into();
    duplicate.selected = false;
    held.rows.push(duplicate);
    held.total += 1;
    let result = project("template", "owner", &listing(), Some(held.clone())).unwrap();
    assert_eq!(result["models"][0]["state"], "choose_working_copy");
    assert!(result["models"][0]["source"].is_null());
    for row in &mut held.rows {
        row.project_binding.as_mut().unwrap()["revisionId"] = json!("old-head");
    }
    let result = project("template", "owner", &listing(), Some(held)).unwrap();
    assert!(result["models"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["state"] == "load_needed"));
}
#[test]
fn crossed_accounts_incomplete_catalogues_and_duplicate_heads_refuse() {
    assert!(project("template", "peer", &listing(), Some(inventory())).is_err());
    let mut value = listing();
    value["more"] = json!(true);
    assert!(project("template", "owner", &value, None).is_err());
    let mut value = listing();
    value["models"]
        .as_array_mut()
        .unwrap()
        .push(head("model-0"));
    assert!(project("template", "owner", &value, None).is_err());
    let mut value = listing();
    value["models"][0]["head_model_digest"] = json!("missing");
    assert!(project("template", "owner", &value, None).is_err());
}
