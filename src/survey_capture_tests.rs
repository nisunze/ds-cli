use super::*;

fn form() -> Value {
    json!({
        "slug": "poles", "display_name": "Poles", "version": 4,
        "fields": [
            {"key": "height", "type": "number", "label": "Height", "required": true, "validation": {"min": 1}},
            {"key": "material", "type": "select", "defaultValue": "wood"},
            {"key": "note", "type": "text", "visibility": {"conditions": [{"field": "material", "operator": "equals", "value": "steel"}]}},
            {"key": "photo", "type": "photo"}
        ],
        "entry_document_schema": {"schema_version": "v2",
            "top_level_keys": [{"key": "data"}, {"key": "metadata"}, {"key": "geometry"}]}
    })
}
fn context() -> Context {
    Context {
        project: "kigali".into(),
        form: form(),
        ..Context::default()
    }
}
fn submit(uid: &str) -> Submit {
    Submit {
        now_ms: 1_760_000_000_000,
        doc_id: "e_0123456789abcdef0123456789abcdef".into(),
        mutation_id: "survey_00000000-0000-4000-8000-000000000001".into(),
        uid: uid.into(),
        email: "Surveyor@Example.test".into(),
        grants: json!({"entries.create": true, "entries.delete": true, "entries.edit_all": true}),
    }
}
fn located(session: &mut Session) {
    session.locate(json!({"longitude": 30.06, "latitude": -1.95, "accuracy": 4.0}), "gps");
}

#[test]
fn a_form_opens_on_its_defaults_and_pinned_revision_and_shows_what_is_visible() {
    let session = Session::open("u1", context()).unwrap();
    assert_eq!((session.revision.as_str(), session.values["material"].clone()), ("4:v2", json!("wood")));
    let shown = view(&session);
    let keys: Vec<&Value> = shown["fields"].as_array().unwrap().iter().map(|field| &field["key"]).collect();
    assert_eq!(keys, vec!["height", "material", "photo"], "a hidden field is not shown");
    assert_eq!(shown["can_submit"], true);
    let mut steel = session.clone();
    steel.set_value("material", json!("steel"));
    assert_eq!(view(&steel)["fields"].as_array().unwrap().len(), 4);
    assert!(Session::open("", context()).is_err(), "no account, no form");
    assert_eq!(view(&Session::default()), json!({"open": false}));
}

#[test]
fn a_draft_keeps_values_but_never_picked_files_and_restores_only_on_its_revision() {
    let mut session = Session::open("u1", context()).unwrap();
    session.set_value("height", json!(9));
    session.set_value("photo", json!({FILE: "t1", "name": "pole.jpg"}));
    located(&mut session);
    let draft = session.draft(5).unwrap();
    assert_eq!(draft["values"], json!({"height": 9, "material": "wood"}));
    assert_eq!(session.file_tokens(), vec!["t1".to_string()]);

    let mut reopened = Session::open("u1", context()).unwrap();
    reopened.restore(&draft);
    assert!(reopened.restored_draft);
    assert_eq!((reopened.values["height"].clone(), reopened.geometry["type"].clone()), (json!(9), json!("Point")));

    let mut other = Session::open("u2", context()).unwrap();
    other.restore(&draft);
    assert!(!other.restored_draft, "another account's draft is not theirs");

    let mut newer = form();
    newer["version"] = json!(5);
    let mut changed = Session::open("u1", Context { form: newer, ..context() }).unwrap();
    changed.restore(&draft);
    assert!(changed.incompatible_draft && !changed.restored_draft);
}

#[test]
fn a_submission_plans_its_uploads_and_the_create_its_queue_carries() {
    let mut session = Session::open("u1", context()).unwrap();
    session.set_value("height", json!(0));
    located(&mut session);
    let Err(Refused::Invalid(errors)) = plan(&session, &submit("u1")) else {
        panic!("a field below its minimum refuses");
    };
    assert_eq!(errors["height"], "Minimum is 1");

    session.set_value("height", json!(9));
    session.set_value("photo", json!({FILE: "t1", "name": "Pole photo.JPG"}));
    let planned = plan(&session, &submit("u1")).unwrap();
    let doc = "e_0123456789abcdef0123456789abcdef";
    let path = format!("kigali/forms/poles/{doc}/photo-t1-Pole_photo.jpg");
    assert_eq!(planned.uploads, vec![Upload { token: "t1".into(), path: path.clone(), photo: true }]);
    let row = &planned.row;
    assert_eq!((row["project"].clone(), row["id"].clone()), (json!("kigali"), json!("survey_00000000-0000-4000-8000-000000000001")));
    assert_eq!(row["record"], json!(record("poles", doc)));
    assert_eq!(row["meta"], json!({"operation": "create", "form": "poles", "doc": doc}));
    let payload = &row["payload"];
    assert_eq!((payload["uid"].clone(), payload["operation"].clone()), (json!("u1"), json!("create")));
    assert_eq!(payload["wire"]["data"]["photo"], json!(path), "the entry names its photo's stored path");
    assert_eq!(payload["wire"]["data"]["height"], 9);
    assert!(payload["wire"]["data"].get("note").is_none(), "a hidden field is not submitted");
    assert!(!planned.feature.is_null());

    assert!(matches!(plan(&session, &submit("u2")), Err(Refused::Unavailable(_))), "another account never submits this form");
    let mut no_grant = submit("u1");
    no_grant.grants = json!({});
    assert!(matches!(plan(&session, &no_grant), Err(Refused::Unavailable(_))));
}

#[test]
fn a_correction_is_one_step_of_its_entrys_record() {
    let prior = json!({"type": "Feature", "properties": {"doc_id": "e_1", "form_slug": "poles",
        "metadata": {"client_version": 3, "created_by": "surveyor@example.test"}, "data": {"height": 9}}});
    let payload = json!({"project_id": "kigali", "form_id": "poles", "doc_id": "e_1", "operation": "update",
        "updated_data": {"height": 10}});
    let (row, feature) = plan_mutation(&payload, &prior, &submit("u1")).unwrap();
    assert_eq!(row["record"], json!(record("poles", "e_1")));
    assert_eq!((row["payload"]["operation"].clone(), row["payload"]["wire"]["expected_version"].clone()), (json!("update"), json!(3)));
    assert!(!feature.is_null());
    let wrong = json!({"project_id": "kigali", "form_id": "poles", "doc_id": "e_1", "operation": "archive"});
    assert!(plan_mutation(&wrong, &prior, &submit("u1")).is_err());
}

#[test]
fn a_record_fits_the_queues_token_bound_whatever_the_ids() {
    let long = "x".repeat(1500);
    assert_eq!(record("poles", &long).len(), 40);
    assert_ne!(record("poles", "e_1"), record("pole", "se_1"));
}
