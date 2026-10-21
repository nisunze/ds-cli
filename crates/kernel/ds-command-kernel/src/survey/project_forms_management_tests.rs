use super::*;

#[test]
fn an_unavailable_binding_needs_current_project_confirmation_to_publish_cleanup() {
    let mut request = json!({"mode":"orphan_views","project":"p","forms":[],
        "orphans":[{"project_id":"p","form_slug":"gone","reason":"missing_master","enabled":true}],
        "staged":{"draft_only":{"settings":{"x":1},"updatedAt":1}},"confirmations":{}});
    let views = evaluate(&request).unwrap();
    assert_eq!(views[0]["form_slug"], "draft_only");
    assert_eq!(views[0]["reportedByBackend"], false);
    assert_eq!(views[1]["confirmedForWrite"], false);
    request["confirmations"] = json!({"gone":{"projectId":"other","source":"refresh"}});
    assert_eq!(evaluate(&request).unwrap()[1]["confirmedForWrite"], false);
    request["confirmations"]["gone"]["projectId"] = "p".into();
    let views = evaluate(&request).unwrap();
    let plan = evaluate(&json!({"mode":"publish_plan","forms":[],"orphans":views,
        "staged":{"gone":{"enabled":false,"updatedAt":1},"draft_only":{"settings":{"x":1},"updatedAt":1}}})).unwrap();
    assert_eq!(
        plan["changes"],
        json!([{"form_slug":"gone","enabled":false}])
    );
    assert_eq!(plan["blockedSlugs"], json!(["draft_only"]));
}

#[test]
fn staged_settings_keep_their_exact_revision_and_newer_edits_survive_submission() {
    let staged = json!({"pole":{"settings":{"x":1},"editorVersion":2,"updatedAt":1}});
    let next = evaluate(&json!({"mode":"stage_draft","staged":staged,"slug":"pole",
        "patch":{"enabled":false},"original_enabled":true,"revision":9,"now":2}))
    .unwrap();
    assert_eq!(next["pole"]["editorVersion"], 2);
    let kept =
        evaluate(&json!({"mode":"remove_submitted","source":next,"submitted":staged})).unwrap();
    assert!(kept.get("pole").is_some());
    let removed =
        evaluate(&json!({"mode":"remove_submitted","source":staged,"submitted":staged})).unwrap();
    assert_eq!(removed, json!({}));
}

#[test]
fn missing_or_noncanonical_settings_revisions_never_create_a_publish_request() {
    for revision in [
        Value::Null,
        json!("07"),
        json!(-1),
        json!(9_007_199_254_740_992_u64),
    ] {
        let answer = evaluate(
            &json!({"mode":"publish_plan","forms":[{"slug":"pole"}],"orphans":[],
            "staged":{"pole":{"settings":{"x":1},"editorVersion":revision,"updatedAt":1}}}),
        )
        .unwrap();
        assert_eq!(answer["failure"]["code"], "project_forms_revision_missing");
        assert!(answer.get("changes").is_none());
    }
}

#[test]
fn refresh_and_editor_confirmations_require_the_exact_advertised_binding() {
    let confirmed = evaluate(
        &json!({"mode":"refresh_confirmations","project":"p","orphans":[
        {"project_id":"p","form_slug":"gone","reason":"missing","enabled":true},
        {"project_id":"q","form_slug":"other","reason":"missing","enabled":true}]}),
    )
    .unwrap();
    assert_eq!(
        confirmed,
        json!({"gone":{"projectId":"p","source":"refresh"}})
    );
    let mut editor = json!({"mode":"editor_confirmation","slug":"gone","editor":{
        "form":{"slug":"gone","enabled":true},"read_only":true,"unavailable":true}});
    assert_eq!(evaluate(&editor).unwrap(), true);
    editor["editor"]["form"]["slug"] = "other".into();
    assert_eq!(evaluate(&editor).unwrap(), false);
    editor.as_object_mut().unwrap().remove("slug");
    editor["editor"]["form"]
        .as_object_mut()
        .unwrap()
        .remove("slug");
    assert_eq!(evaluate(&editor).unwrap(), false);
}

#[test]
fn working_editor_and_clear_actions_preserve_an_independent_enable_change() {
    let staged = json!({"pole":{"enabled":false,"conflictKeys":["old"],"updatedAt":1}});
    let working = json!({"formSlug":"pole","valid":true,"settings":{"x":1},"baseSettings":{"x":0},"editorVersion":2});
    let merged =
        evaluate(&json!({"mode":"working_draft","staged":staged,"working":working,"now":3}))
            .unwrap();
    assert_eq!(merged["pole"]["enabled"], false);
    assert!(merged["pole"].get("conflictKeys").is_none());
    let cleared =
        evaluate(&json!({"mode":"clear_settings","staged":merged,"slug":"pole","now":4})).unwrap();
    assert_eq!(
        cleared,
        json!({"pole":{"enabled":false,"editorVersion":2,"updatedAt":4}})
    );
    let no_change = evaluate(&json!({"mode":"working_draft","staged":staged,
        "working":{"formSlug":"pole","valid":false,"settings":{"x":1}},"now":5}))
    .unwrap();
    assert_eq!(no_change, staged);
}

#[test]
fn malformed_conflict_context_preserves_the_base_revision_and_requires_deliberate_recovery() {
    let staged =
        json!({"pole":{"settings":{"x":2},"baseSettings":{"x":1},"editorVersion":3,"updatedAt":1}});
    assert_eq!(
        evaluate(&json!({"mode":"conflict_slug","staged":staged,"slug":" "})).unwrap(),
        "pole"
    );
    let answer = evaluate(&json!({"mode":"recover_conflict","staged":staged,"slug":"pole",
        "editor":null,"conflict":{"currentVersionAvailable":true,"currentVersion":"07", "currentSettingsAvailable":false},"now":9})).unwrap();
    assert_eq!(answer["fresh"]["pole"]["editorVersion"], 3);
    assert_eq!(answer["conflictKeys"], json!(["x"]));
    assert_eq!(answer["editorPatch"], Value::Null);
    let refreshed = evaluate(
        &json!({"mode":"recover_conflict","staged":staged,"slug":"pole",
        "editor":{"version":4,"current_settings":{"x":1}},"conflict":{},"now":10}),
    )
    .unwrap();
    assert_eq!(refreshed["fresh"]["pole"]["editorVersion"], 4);
    assert_eq!(refreshed["conflictKeys"], json!([]));
}
