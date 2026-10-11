//! Demand-driven JSON materialization is independent of engineering freshness.
use super::*;
use crate::transactional_projection_tests::{describe, refresh, request_for};
use ds_grid_engine::profile_calculated_delta::CalculatedSceneBase;
use tests::{execute, fixture, request};

fn retained_runtime() -> (Runtime, ProjectRequest, RetainedIndexWork) {
    let input = request_for(fixture());
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let held = runtime.held.get_mut(&input.scope).unwrap();
    let base = Arc::new(CalculatedSceneBase::from_shared(
        held.calculated_scene.as_ref().unwrap().clone(),
    ));
    let (index, work) = Index::new_retained(&held.session, base.clone(), &held.demand_cache);
    held.result_index = Arc::new(index);
    held.calculated_base = Some(base);
    (runtime, input, work)
}

fn materialized(index: &Index) -> u64 {
    index
        .facts
        .values()
        .flat_map(|facts| &facts.rows)
        .find_map(|fact| fact.value.work.as_ref())
        .map_or(0, |work| work.materialized.load(Ordering::Relaxed))
}

#[test]
fn unused_facts_are_unserialized_and_bounded_consumers_share_exact_cells() {
    let (mut runtime, input, work) = retained_runtime();
    assert_eq!(work.captured_report_rows, fixture().structures.len());
    assert!(work.deferred_facts > work.captured_report_rows as u64);
    assert_eq!(work.materialized_facts, 0);
    let held = &runtime.held[&input.scope];
    let review = held.review.clone();
    let cache_work = held.demand_cache.last_demand_work();
    let history = held.session.history_state();
    let index = held.result_index.clone();
    let mut query = request(&runtime, &input.scope, QueryKind::Usage);
    query.limit = 1;
    let first = execute(&mut runtime, query.clone()).unwrap();
    assert_eq!(first["total"], work.captured_report_rows);
    assert_eq!(first["metrics"]["materialized_facts"], 1);
    assert_eq!(materialized(&index), 1);
    let repeated = execute(&mut runtime, query.clone()).unwrap();
    assert_eq!(first["items"], repeated["items"]);
    assert_eq!(repeated["metrics"]["materialized_facts"], 0);
    assert_eq!(repeated["metrics"]["reused_materialized_facts"], 1);
    query.kind = QueryKind::Qualification;
    let qualification = execute(&mut runtime, query).unwrap();
    assert_eq!(first["items"], qualification["items"]);
    assert_eq!(qualification["metrics"]["materialized_facts"], 0);
    assert_eq!(qualification["metrics"]["reused_materialized_facts"], 1);
    assert_eq!(materialized(&index), 1);
    let held = &runtime.held[&input.scope];
    assert_eq!(held.demand_cache.last_demand_work(), cache_work);
    assert_eq!(held.session.history_state(), history);
    assert!(Arc::ptr_eq(&held.review, &review));
}

#[test]
fn retained_typed_report_survives_cache_replacement_and_matches_eager_facts() {
    let (mut runtime, input, _) = retained_runtime();
    let mut eager = Runtime::default();
    eager.execute(input.clone()).unwrap();
    // Deliberately remove the mutable acquisition/cache source. Reading exact
    // retained facts cannot re-enter screening or consult this replacement.
    runtime.held.get_mut(&input.scope).unwrap().demand_cache = Default::default();
    let cache_work = runtime.held[&input.scope].demand_cache.last_demand_work();
    for kind in [
        QueryKind::Usage,
        QueryKind::Failures,
        QueryKind::NegativeLoading,
        QueryKind::Qualification,
        QueryKind::ViolatingSections,
        QueryKind::Blockers,
    ] {
        let query = request(&runtime, &input.scope, kind);
        let actual = execute(&mut runtime, query).unwrap();
        let query = request(&eager, &input.scope, kind);
        let expected = execute(&mut eager, query).unwrap();
        for field in [
            "total",
            "items",
            "basis",
            "computed_revision",
            "computed_engineering_input_root",
            "freshness",
        ] {
            assert_eq!(actual[field], expected[field], "{kind:?} {field}");
        }
        assert_eq!(
            serde_json::to_vec(&actual["items"]).unwrap(),
            serde_json::to_vec(&expected["items"]).unwrap(),
            "{kind:?} serialized fact bytes"
        );
        // Retain the original eager constructor's last-member position when
        // the consuming feature graph preserves JSON insertion order.
        if kind == QueryKind::ViolatingSections
            && json!({"z":0,"a":0})
                .as_object()
                .unwrap()
                .keys()
                .next()
                .unwrap()
                == "z"
        {
            for item in actual["items"].as_array().unwrap() {
                assert_eq!(
                    item["fact"].as_object().unwrap().keys().next_back().unwrap(),
                    "capacity_failure_structures_truncated"
                );
            }
        }
        assert_eq!(actual["metrics"]["calculated_cases"], 0);
    }
    assert_eq!(
        runtime.held[&input.scope].demand_cache.last_demand_work(),
        cache_work
    );
}

#[test]
fn refused_identity_bounds_cursor_and_header_do_not_materialize_facts() {
    let (mut runtime, input, _) = retained_runtime();
    let index = runtime.held[&input.scope].result_index.clone();
    let valid = request(&runtime, &input.scope, QueryKind::Usage);
    let mut negatives = Vec::new();
    let mut bad = valid.clone();
    bad.limit = 0;
    negatives.push((bad, "invalid_profile_query:"));
    let mut bad = valid.clone();
    bad.model_id = "foreign".into();
    negatives.push((bad, "profile_replay_model_mismatch:"));
    let mut bad = valid.clone();
    bad.expected_engineering_input_root = "foreign".into();
    negatives.push((bad, "profile_query_stale:"));
    let mut bad = valid.clone();
    bad.expected_history.history_pin = "foreign".into();
    negatives.push((bad, "profile_query_stale:"));
    let mut bad = valid.clone();
    bad.entity_ids = vec!["absent".into()];
    negatives.push((bad, "invalid_profile_query:"));
    let mut bad = valid.clone();
    bad.kind = QueryKind::Findings;
    negatives.push((bad, "invalid_profile_query:"));
    let mut bad = valid.clone();
    bad.source = QuerySource::ModelAnalysis;
    negatives.push((bad, "profile_calculation_required:"));
    let mut bad = valid.clone();
    bad.cursor = Some(QueryCursor {
        result_id: index.id.clone(),
        scope: valid.scope.clone(),
        model_id: valid.model_id.clone(),
        history: valid.expected_history.clone(),
        kind: valid.kind,
        source: valid.source,
        entity_ids: vec![],
        offset: index.facts[&QueryKind::Usage].rows.len() + 1,
    });
    negatives.push((bad.clone(), "invalid_profile_query:"));
    bad.cursor.as_mut().unwrap().result_id = "old-index".into();
    negatives.push((bad, "profile_query_stale:"));
    for (query, code) in negatives {
        assert!(execute(&mut runtime, query).unwrap_err().starts_with(code));
        assert_eq!(materialized(&index), 0);
    }
    Arc::make_mut(&mut runtime.held.get_mut(&input.scope).unwrap().result_index).basis =
        json!({"oversized_native_basis":"x".repeat(MAX_BYTES)});
    assert!(execute(&mut runtime, valid)
        .unwrap_err()
        .starts_with("profile_query_output_limit:"));
    assert_eq!(materialized(&index), 0);
}

#[test]
fn signed_observation_pending_discard_and_stale_queries_preserve_unmaterialized_sources() {
    let (mut runtime, input, _) = retained_runtime();
    let original = runtime.held[&input.scope].result_index.clone();
    let original_query = request(&runtime, &input.scope, QueryKind::Usage);
    let original_base = runtime.held[&input.scope]
        .calculated_base
        .as_ref()
        .unwrap()
        .clone();
    let mut worker = GridSession::open(fixture());
    describe(&mut worker, "lazy-pending", "candidate description");
    let mut replay = refresh(&runtime, &input, &input.expected_revision, &worker);
    replay.calculated_base_signature = Some(original_base.signature().into());
    replay.publication_id = Some("lazy-publication".into());
    let result = runtime.execute(Request::Observe(Box::new(replay))).unwrap();
    assert_eq!(result["calculation"]["required"], false);
    assert_eq!(result["metrics"]["profile_result_fact_materializations"], 0);
    assert!(
        result["metrics"]["profile_result_fact_deferred_facts"]
            .as_u64()
            .unwrap()
            > 0
    );
    let candidate = runtime.held[&input.scope].result_index.clone();
    let query = request(&runtime, &input.scope, QueryKind::Usage);
    assert!(execute(&mut runtime, query)
        .unwrap_err()
        .starts_with("profile_publication_pending:"));
    assert_eq!(materialized(&candidate), 0);
    runtime
        .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
            scope: input.scope.clone(),
            publication: serde_json::from_value(result["publication"].clone()).unwrap(),
        }))
        .unwrap();
    assert!(execute(&mut runtime, original_query.clone())
        .unwrap_err()
        .starts_with("profile_query_stale:"));
    assert_eq!(materialized(&original), 0);
    assert_eq!(materialized(&candidate), 0);
    runtime
        .execute(Request::DiscardPublication(DiscardPublicationRequest {
            scope: input.scope.clone(),
            model_id: original_query.model_id.clone(),
            publication_id: "lazy-publication".into(),
            target_history: worker.history_state(),
        }))
        .unwrap();
    assert!(Arc::ptr_eq(
        &runtime.held[&input.scope].result_index,
        &original
    ));
    assert!(Arc::ptr_eq(
        runtime.held[&input.scope].calculated_base.as_ref().unwrap(),
        &original_base
    ));
    let restored = execute(&mut runtime, original_query).unwrap();
    assert_eq!(
        restored["metrics"]["materialized_facts"],
        fixture().structures.len()
    );
    assert_eq!(materialized(&candidate), 0);
}

#[test]
fn pending_authored_dependencies_refuse_without_materialization() {
    let (mut runtime, input, _) = retained_runtime();
    let index = runtime.held[&input.scope].result_index.clone();
    let mut worker = GridSession::open(fixture());
    describe(
        &mut worker,
        "lazy-authored",
        "requires explicit calculation",
    );
    let replay = refresh(&runtime, &input, &input.expected_revision, &worker);
    runtime.execute(Request::Observe(Box::new(replay))).unwrap();
    let query = request(&runtime, &input.scope, QueryKind::Usage);
    assert!(execute(&mut runtime, query)
        .unwrap_err()
        .starts_with("profile_calculation_required:"));
    assert_eq!(materialized(&index), 0);
    let mut unaffected = request(&runtime, &input.scope, QueryKind::Usage);
    unaffected.entity_ids = vec!["detached".into()];
    let result = execute(&mut runtime, unaffected).unwrap();
    assert_eq!(result["freshness"], "unchanged_dependencies");
    assert_eq!(result["metrics"]["materialized_facts"], 1);
}

#[test]
fn oversized_offered_fact_materializes_once_and_still_refuses_the_wire() {
    let (mut runtime, input, _) = retained_runtime();
    let index = Arc::make_mut(&mut runtime.held.get_mut(&input.scope).unwrap().result_index);
    let work = Arc::new(MaterializationWork::default());
    let mut facts = Facts::default();
    facts.insert_fact(
        Some("str-1".into()),
        FactValue::deferred(&work, || {
            Arc::new(json!({"native_evidence":"x".repeat(MAX_BYTES)}))
        }),
    );
    index.facts.insert(QueryKind::Usage, facts);
    let query = request(&runtime, &input.scope, QueryKind::Usage);
    for _ in 0..2 {
        assert!(execute(&mut runtime, query.clone())
            .unwrap_err()
            .starts_with("profile_query_output_limit:"));
        assert_eq!(work.materialized.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn concurrent_consumers_materialize_one_shared_native_cell() {
    let work = Arc::new(MaterializationWork::default());
    let row = fixture_report_row();
    let fact = FactValue::deferred(&work, move || usage_fact(&row));
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let fact = fact.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                fact.get().clone()
            })
        })
        .collect();
    let values: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(values.iter().all(|value| Arc::ptr_eq(value, &values[0])));
    assert_eq!(work.materialized.load(Ordering::Relaxed), 1);
}

fn fixture_report_row() -> ds_grid_engine::StructureUsageScreeningRow {
    let input = request_for(fixture());
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let held = &runtime.held[&input.scope];
    let scene = held.review.base_scene().unwrap();
    held.demand_cache
        .cached_report(
            &scene.model_revision,
            scene.engineering.usage_request.as_ref().unwrap(),
        )
        .unwrap()
        .rows[0]
        .clone()
}
