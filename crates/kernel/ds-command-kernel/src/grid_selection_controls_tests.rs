use super::*;
#[test]
fn native_preferences_remember_layers_and_compose_ordered_ids() {
    let mut r = Request {
        scope: "project/model".into(),
        space: Space::Plan,
        available_families: vec![Family::Structures, Family::TerrainPoints],
        request: Action::Read,
    };
    let (s, v) = execute(&State::default(), &r).unwrap();
    assert_eq!(v["state"]["predicate"], "within");
    assert_eq!(v["state"]["families"], Value::Null);
    r.request = Action::Configure {
        patch: serde_json::from_value(json!({"families":["structures"],"predicate":"outside"}))
            .unwrap(),
    };
    let (s, _) = execute(&s, &r).unwrap();
    r.space = Space::Profile;
    r.request = Action::Read;
    assert_eq!(
        execute(&s, &r).unwrap().1["resolved_families"],
        json!(["structures"])
    );
    r.request = Action::Configure {
        patch: serde_json::from_value(json!({"families":null})).unwrap(),
    };
    assert!(execute(&s, &r).unwrap().0.families.is_none());
    r.request = Action::Compose {
        mode: Mode::Intersect,
        current: vec!["a".into(), "b".into()],
        hits: vec!["b".into(), "c".into()],
    };
    assert_eq!(execute(&s, &r).unwrap().1["entity_ids"], json!(["b"]));
}
#[test]
fn admission_retains_native_defaults_and_refuses_invalid_pairs() {
    let mut r = Request {
        scope: "p/m".into(),
        space: Space::Profile,
        available_families: vec![Family::Structures],
        request: Action::Admit {
            query: json!({"model_id":"m","expected_revision":"r","space":"profile","polygon":[[0,0],[10,0],[10,10]]}),
        },
    };
    assert_eq!(
        execute(&State::default(), &r).unwrap().1["query"]["predicate"],
        "within"
    );
    r.request = Action::Admit {
        query: json!({"model_id":"m","expected_revision":"r","space":"profile","polygon":[[0,0,0],[10,0],[10,10]]}),
    };
    assert!(execute(&State::default(), &r).is_err());
}

#[test]
fn all_composition_modes_preserve_order_duplicates_and_inputs() {
    for (current, hits, expected) in [
        (
            vec!["ground-1", "pole-1", "pole-2"],
            vec!["pole-2", "pole-3", "pole-3"],
            [
                vec!["pole-2", "pole-3"],
                vec!["ground-1", "pole-1", "pole-2", "pole-3"],
                vec!["ground-1", "pole-1"],
                vec!["pole-2"],
            ],
        ),
        (
            vec!["c", "a", "c", "b"],
            vec!["b", "d", "b"],
            [
                vec!["b", "d"],
                vec!["c", "a", "b", "d"],
                vec!["c", "a"],
                vec!["b"],
            ],
        ),
    ] {
        let current: Vec<String> = current.into_iter().map(String::from).collect();
        let hits: Vec<String> = hits.into_iter().map(String::from).collect();
        for (mode, expected) in [Mode::Replace, Mode::Add, Mode::Remove, Mode::Intersect]
            .into_iter()
            .zip(expected)
        {
            let state = State {
                predicate: Predicate::Outside,
                mode: Mode::Add,
                families: Some(vec![Family::TerrainPoints]),
            };
            let before = serde_json::to_value(&state).unwrap();
            let request = Request {
                scope: "project/model".into(),
                space: Space::Profile,
                available_families: vec![Family::Structures, Family::TerrainPoints],
                request: Action::Compose {
                    mode,
                    current: current.clone(),
                    hits: hits.clone(),
                },
            };
            let (next, result) = execute(&state, &request).unwrap();
            assert_eq!(result["entity_ids"], json!(expected), "{mode:?}");
            assert_eq!(result["primary"], json!(expected.last()), "{mode:?}");
            assert_eq!(serde_json::to_value(&state).unwrap(), before);
            assert_eq!(serde_json::to_value(next).unwrap(), before);
            assert_eq!(result["state"], before);
            let Action::Compose {
                current: after_current,
                hits: after_hits,
                ..
            } = &request.request
            else {
                unreachable!()
            };
            assert_eq!(after_current, &current);
            assert_eq!(after_hits, &hits);
        }
    }
}

#[test]
fn composition_refuses_invalid_identities_without_changing_native_preferences_or_inputs() {
    for invalid in [String::new(), "x".repeat(1025)] {
        let state = State::default();
        let before = serde_json::to_value(&state).unwrap();
        let current = vec!["retained".into()];
        let hits = vec![invalid];
        let request = Request {
            scope: "project/model".into(),
            space: Space::Profile,
            available_families: vec![Family::Structures],
            request: Action::Compose {
                mode: Mode::Add,
                current: current.clone(),
                hits: hits.clone(),
            },
        };
        assert_eq!(
            execute(&state, &request).unwrap_err(),
            "invalid_grid_lasso: selection identities exceed native bounds"
        );
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
        let Action::Compose {
            current: after_current,
            hits: after_hits,
            ..
        } = &request.request
        else {
            unreachable!()
        };
        assert_eq!(after_current, &current);
        assert_eq!(after_hits, &hits);
    }
}
