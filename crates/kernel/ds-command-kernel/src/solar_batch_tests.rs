use super::*;
fn request() -> Request {
    Request {
        schema: SCHEMA.into(),
        batch_id: "batch-test".into(),
        root: "root".into(),
        portfolios: (0..3)
            .map(|i| Portfolio {
                id: format!("p{i}"),
                name: format!("P{i}"),
                cities: vec!["a".into(), "b".into()],
                membership_revision: format!("sha256:{}", "a".repeat(64)),
            })
            .collect(),
        parallel: 2,
        graph_strategy: "round_robin".into(),
        force: false,
        render_charts: true,
        report_drafts: vec!["apd".into()],
        language: "fr".into(),
        placement: Placement::Native,
    }
}
fn sources() -> Vec<Source> {
    vec![Source {
        run_id: "source".into(),
        city_ids: vec!["a".into(), "b".into()],
        status: "succeeded".into(),
    }]
}
#[test]
fn batch_rejects_ids_outside_the_contract_grammar() {
    for candidate in ["-x", "_x", "a.b", &"a".repeat(129)] {
        let mut request = request();
        request.portfolios[0].id = candidate.into();
        assert!(request.validate().is_err(), "{candidate}");
    }
}

#[test]
fn bounded_parallelism_and_failure_isolation() {
    let first = start(request(), &sources(), 1).unwrap();
    assert_eq!(first.effects.len(), 2);
    assert_eq!(first.state.rows[2].phase, Phase::Queued);
    let next = transition(
        first.state,
        Event::Failed {
            index: 0,
            run_id: "batch-test-0-portfolio".into(),
            detail: "bad input".into(),
        },
        2,
    )
    .unwrap();
    assert_eq!(next.effects.len(), 1);
    assert_eq!(next.state.rows[0].phase, Phase::Failed);
    assert_eq!(next.state.rows[2].phase, Phase::Portfolio);
}

#[test]
fn effects_carry_owner_resolved_graph_selection_and_failed_sources_never_aggregate() {
    for (strategy, expected, city, offset) in [
        ("first", "first", None, None),
        ("round_robin", "round_robin", None, Some(0)),
        ("city:b", "explicit", Some("b"), None),
    ] {
        let mut request = request();
        request.graph_strategy = strategy.into();
        let output = start(request, &sources(), 1).unwrap();
        match &output.effects[0] {
            Effect::Portfolio { selection, .. } => {
                assert_eq!(selection.representative_strategy, expected);
                assert_eq!(selection.representative_city_id.as_deref(), city);
                assert_eq!(selection.round_robin_start_index, offset);
            }
            _ => panic!("expected portfolio effect"),
        }
    }
    let mut request = request();
    request.portfolios.truncate(1);
    let output = start(request, &[], 1).unwrap();
    let output = transition(
        output.state,
        Event::SourceCompleted {
            index: 0,
            run_id: "batch-test-0-source".into(),
            status: "failed".into(),
            detail: Some("missing city".into()),
        },
        2,
    )
    .unwrap();
    assert!(output.effects.is_empty());
    assert_eq!(output.state.rows[0].phase, Phase::Failed);
    assert_eq!(output.state.rows[0].detail.as_deref(), Some("missing city"));
}
#[test]
fn source_order_force_and_governed_revision_are_exact() {
    let mut source = sources();
    source[0].city_ids.reverse();
    assert_eq!(
        start(request(), &source, 1).unwrap().state.rows[0].phase,
        Phase::Cities
    );
    let mut force = request();
    force.force = true;
    assert_eq!(
        start(force, &sources(), 1).unwrap().state.rows[0].phase,
        Phase::Cities
    );
    let mut changed = request().portfolios;
    changed[0].cities.reverse();
    assert!(request().validate_catalog(&changed).is_err());
    assert!(request().validate_catalog(&request().portfolios).is_ok());
}
// Migrated from ds-web/solar/portfolio-source-run.test.ts. Source selection
// has one implementation and test authority for native CLI and WASM hosts.
#[test]
fn source_selection_rejects_ineligible_runs_and_uses_newest_exact_membership() {
    let mut candidates = vec![
        Source {
            run_id: "running".into(),
            city_ids: vec!["a".into(), "b".into()],
            status: "running".into(),
        },
        Source {
            run_id: "partial".into(),
            city_ids: vec!["a".into()],
            status: "succeeded".into(),
        },
        Source {
            run_id: "reordered".into(),
            city_ids: vec!["b".into(), "a".into()],
            status: "succeeded".into(),
        },
        Source {
            run_id: " padded ".into(),
            city_ids: vec!["a".into(), "b".into()],
            status: "succeeded".into(),
        },
    ];
    // Each negative control must be ineligible on its own too.
    for candidate in &candidates {
        let output = start(request(), std::slice::from_ref(candidate), 1).unwrap();
        assert!(output
            .state
            .rows
            .iter()
            .all(|row| row.source_run_id.is_none()));
        assert!(output
            .effects
            .iter()
            .all(|effect| matches!(effect, Effect::Source { .. })));
    }
    let output = start(request(), &candidates, 1).unwrap();
    assert!(output
        .state
        .rows
        .iter()
        .all(|row| row.source_run_id.is_none()));
    for run_id in ["newest-exact", "older-exact"] {
        candidates.push(Source {
            run_id: run_id.into(),
            city_ids: vec!["a".into(), "b".into()],
            status: "succeeded".into(),
        });
    }
    let output = start(request(), &candidates, 1).unwrap();
    assert!(output
        .state
        .rows
        .iter()
        .all(|row| row.source_run_id.as_deref() == Some("newest-exact")));
    assert_eq!(output.effects.len(), 2);
    assert!(output
        .effects
        .iter()
        .all(|effect| matches!(effect, Effect::Portfolio { .. })));
    assert_eq!(output.state.rows[2].phase, Phase::Queued);
}

#[test]
fn graph_city_must_belong_to_every_selected_portfolio() {
    let mut req = request();
    req.graph_strategy = "city:a".into();
    assert!(req.validate().is_ok());
    req.portfolios[1].cities = vec!["b".into(), "c".into()];
    assert_eq!(
        req.validate().unwrap_err(),
        "graph strategy must apply to every selected portfolio"
    );
    req.graph_strategy = "city:b".into();
    assert!(req.validate().is_ok());
    req.graph_strategy = "made_up".into();
    assert!(req.validate().is_err());
}
#[test]
fn cancellation_waits_for_registration_without_spinning_and_commit_wins() {
    let started = start(request(), &sources(), 1).unwrap();
    let cancelled = transition(started.state, Event::Cancel, 2).unwrap();
    assert_eq!(cancelled.state.rows[2].phase, Phase::Cancelled);
    assert_eq!(cancelled.effects.len(), 3);
    let unregistered = transition(
        cancelled.state,
        Event::CancelAcknowledged {
            index: 0,
            run_id: "batch-test-0-portfolio".into(),
            accepted: false,
        },
        3,
    )
    .unwrap();
    assert!(matches!(
        unregistered.effects.as_slice(),
        [Effect::Wake { .. }]
    ));
    let retry = transition(unregistered.state, Event::Tick, 4).unwrap();
    assert_eq!(retry.effects.len(), 2);
    let committed = transition(
        retry.state,
        Event::Succeeded {
            index: 0,
            run_id: "batch-test-0-portfolio".into(),
            detail: None,
        },
        5,
    )
    .unwrap();
    assert_eq!(committed.state.rows[0].phase, Phase::Ready);
    let stopped = transition(
        committed.state,
        Event::Failed {
            index: 1,
            run_id: "batch-test-1-portfolio".into(),
            detail: "cancelled".into(),
        },
        6,
    )
    .unwrap();
    assert!(stopped.state.complete);
    assert!(stopped.effects.is_empty());
}
#[test]
fn cancelled_source_never_starts_aggregation_and_restart_never_replays() {
    let first = start(request(), &[], 1).unwrap();
    let cancelled = transition(first.state, Event::Cancel, 2).unwrap();
    let source_done = transition(
        cancelled.state,
        Event::SourceSucceeded {
            index: 0,
            run_id: "batch-test-0-source".into(),
        },
        3,
    )
    .unwrap();
    assert_eq!(source_done.state.rows[0].phase, Phase::Cancelled);
    assert!(source_done
        .effects
        .iter()
        .all(|e| matches!(e, Effect::Wake { .. })));
    let recovered = transition(source_done.state, Event::Recover, 4).unwrap();
    assert!(recovered.state.complete);
    assert!(recovered.effects.is_empty());
}
#[test]
fn foreign_events_unknown_fields_and_invalid_limits_are_refused() {
    let first = start(request(), &[], 1).unwrap();
    assert!(transition(
        first.state,
        Event::SourceSucceeded {
            index: 0,
            run_id: "other".into()
        },
        2
    )
    .is_err());
    let mut r = request();
    r.parallel = 0;
    assert!(r.validate().is_err());
    let mut value = serde_json::to_value(request()).unwrap();
    value["fallback"] = true.into();
    assert!(serde_json::from_value::<Request>(value).is_err());
}
#[test]
fn shared_wire_matches_native_transition() {
    let r = request();
    let wire = serde_json::json!({"operation":"start", "request":r, "catalog":r.portfolios, "sources":sources(), "now":1});
    let actual: serde_json::Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&wire).unwrap()).unwrap()).unwrap();
    assert_eq!(
        actual,
        serde_json::to_value(start(request(), &sources(), 1).unwrap()).unwrap()
    );
}

/// Contract 4: a host built against a newer request revision of the same
/// family is served; the request stays closed (an unknown member refuses),
/// another family refuses and a missing required member is a decode refusal.
#[test]
fn a_newer_request_revision_is_served_and_the_request_stays_closed() {
    let mut newer = request();
    newer.schema = "ds-solar.portfolio-batch/v2".into();
    assert!(newer.validate().is_ok());
    let mut other = request();
    other.schema = "ds-solar.city-batch/v1".into();
    assert!(other.validate().is_err());

    let wire = serde_json::to_value(request()).unwrap();
    let mut additive = wire.clone();
    additive["future"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Request>(additive).is_err());
    let mut missing = wire;
    missing.as_object_mut().unwrap().remove("placement");
    assert!(serde_json::from_value::<Request>(missing).is_err());
}
