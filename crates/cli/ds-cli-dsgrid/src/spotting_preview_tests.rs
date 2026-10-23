use super::*;
use ds_network_contracts::grid::NativeGridSourcePin;

fn planned_item(alignment: &str) -> Value {
    json!({"request_index":0,"alignment_id":alignment,"stringing_basis_section_id":"ts-1",
        "estimated_peak_bytes":1024,"status":"planned","plan":{"request":{"alignment_id":alignment},"plan_digest":format!("digest-{alignment}")}})
}
fn refused_item(alignment: &str) -> Value {
    json!({"request_index":1,"alignment_id":alignment,"stringing_basis_section_id":"ts-2",
        "estimated_peak_bytes":1024,"status":"refused","error":{"kind":"no_feasible_layout"}})
}
fn result(items: Vec<Value>, refused: u64) -> Value {
    json!({"operation_id":"plan_whole_model_spotting","model_revision":"rev:a","settings_digest":"s",
        "requested_alignment_ids":["al-branch","al-main"],"passes":1,
        "batch":{"operation_id":"plan_optimum_spotting_batch","model_revision":"rev:a","available_processors":4,
            "worker_count":1,"requested_alignments":2,"completed_plans":2 - refused,"refused_requests":refused,"items":items},
        "ruling_span_history":[]})
}
fn query(value: Value) -> NativeGridQueryResultV1 {
    NativeGridQueryResultV1 {
        schema: "ds.network.grid-query-result/v1".into(),
        source: NativeGridSourcePin {
            package_sha256: "a".repeat(64),
            model_id: ds_grid_model::EntityId::new("model-a").unwrap(),
            expected_revision: "rev:a".into(),
            engineering_input_root: "root".into(),
        },
        model_revision: 4,
        engine_version: "engine-x".into(),
        operation: json!({"operation_id":"plan_whole_model_spotting"}),
        result: value,
    }
}
fn failure(code: &'static str, owner: Option<&str>) -> NetworkFailure {
    NetworkFailure {
        code,
        message: "native refusal".into(),
        owner_error: None,
        owner_code: owner.map(str::to_owned),
        owner_detail: Some(Box::new(json!({"refusal":{"kind":"crossing_layout"}}))),
        owner_remedy: None,
        owner_invalid_input: None,
    }
}

#[test]
fn admits_exact_pins_and_scopes_before_any_read() {
    admit(&"a".repeat(64), "rev:a", &[]).unwrap();
    admit(&"0f".repeat(32), "rev:a", &["al-branch".into(), "al-main".into()]).unwrap();
    for (sha, revision, alignments) in [
        ("A".repeat(64), "rev:a".to_owned(), vec![]),
        ("a".repeat(63), "rev:a".to_owned(), vec![]),
        ("a".repeat(64), String::new(), vec![]),
        ("a".repeat(64), "r".repeat(201), vec![]),
        ("a".repeat(64), "rev:a".to_owned(), vec![String::new()]),
        ("a".repeat(64), "rev:a".to_owned(), vec![" al-main".to_owned()]),
        ("a".repeat(64), "rev:a".to_owned(), vec!["x".repeat(513)]),
    ] {
        assert_eq!(
            admit(&sha, &revision, &alignments).unwrap_err().code(),
            "spotting_preview_input_invalid"
        );
    }
}

#[test]
fn a_complete_planning_result_has_no_conflict_and_keeps_requested_plan_order() {
    let value = result(vec![planned_item("al-branch"), planned_item("al-main")], 0);
    assert!(planning_conflicts(&value).is_empty());
    let plans = planned(&value);
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0]["request"]["alignment_id"], "al-branch");
    assert_eq!(plans[1]["plan_digest"], "digest-al-main");
}

#[test]
fn refused_plans_derivations_and_fixed_structures_each_block_with_their_typed_conflict() {
    let mut value = result(vec![planned_item("al-branch"), refused_item("al-main")], 1);
    value["refused_derivations"] = json!([{"alignment_id":"al-spur","reason":"no_stringing_basis"}]);
    value["unresolved_fixed_structure_ids"] = json!(["str-missing"]);
    let conflicts = planning_conflicts(&value);
    let codes: Vec<_> = conflicts.iter().map(|c| c["code"].as_str().unwrap()).collect();
    assert_eq!(
        codes,
        [
            "alignment_plan_refused",
            "alignment_request_refused",
            "fixed_structure_missing"
        ]
    );
    assert_eq!(conflicts[0]["detail"], value["batch"]["items"]);
    assert_eq!(conflicts[1]["detail"], value["refused_derivations"]);
    assert_eq!(conflicts[2]["detail"], json!(["str-missing"]));
    for conflict in &conflicts {
        assert!(conflict["message"].as_str().is_some_and(|m| !m.is_empty()));
        assert!(conflict["remedy"].as_str().is_some_and(|m| !m.is_empty()));
    }
    // Only the planned item is ever offered to the application owner.
    assert_eq!(planned(&value).len(), 1);
}

#[test]
fn only_the_owners_typed_operation_refusal_becomes_an_application_conflict() {
    let conflict = application_conflict(&failure("network_engine_refused", Some("operation_failed")))
        .expect("typed owner refusal");
    assert_eq!(conflict["code"], "layout_application_refused");
    assert_eq!(conflict["detail"], json!({"refusal":{"kind":"crossing_layout"}}));
    for (code, owner) in [
        ("network_engine_missing", None),
        ("network_engine_incompatible", None),
        ("network_engine_refused", Some("stale_revision")),
    ] {
        assert!(application_conflict(&failure(code, owner)).is_none());
    }
}

#[test]
fn a_preview_is_ready_only_with_an_application_and_no_conflict() {
    let plan = query(result(vec![planned_item("al-main")], 0));
    let application = json!({"apply_digest":"d","model_revision":"rev:a","structures":[],"section_paths":[],"spotting_warnings":[],"alignments":[]});
    let ready = assemble(&plan, &"a".repeat(64), 3, Some(application.clone()), vec![]);
    assert_eq!(ready["schema"], SCHEMA);
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["application"], application);
    assert_eq!(
        ready["source"],
        json!({"model_id":"model-a","package_revision":4,"authored_revision":"rev:a","package_sha256":"a".repeat(64),"byte_len":3})
    );
    assert_eq!(ready["result"], plan.result);
    let conflict = json!({"code":"layout_application_refused","message":"m","remedy":"r","detail":null});
    let blocked = assemble(&plan, &"a".repeat(64), 3, Some(application), vec![conflict.clone()]);
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(blocked["application"], Value::Null);
    assert_eq!(blocked["conflicts"], json!([conflict]));
    assert_eq!(assemble(&plan, &"a".repeat(64), 3, None, vec![])["status"], "blocked");
}

#[test]
fn the_descriptor_stages_exact_package_bytes_and_a_new_json_result() {
    let model = COMMAND.args.iter().find(|arg| arg.name == "model").unwrap();
    assert!(model.required && model.value.ends_with("file>") && !model.value.contains("json"));
    let out = COMMAND.args.iter().find(|arg| arg.name == "out").unwrap();
    assert!(!out.required && out.value == "<absolute-new-json-file>");
    let codes: Vec<_> = COMMAND.refusals.iter().map(|refusal| refusal.code).collect();
    let mut unique = codes.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), codes.len(), "refusal codes are unique");
}
