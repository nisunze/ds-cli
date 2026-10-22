//! Runtime lifecycle resolves context; Network owns immutable facts and query policy.
use super::*;
#[cfg(test)]
use ds_grid_physics::EngineeringStatus;
#[cfg(test)]
use ds_profile_presentation::result_queries::MAX_BYTES;
pub use ds_profile_presentation::result_queries::{
    Index, QueryArguments, QueryCursor, QueryKind, QueryResultsRequest, QuerySource,
    RetainedIndexWork,
};
#[cfg(test)]
mod analysis_queries;
#[cfg(test)]
mod lazy_fact_tests;

impl Runtime {
    pub(super) fn query_results(&self, request: QueryResultsRequest) -> Result<Value, String> {
        ds_profile_presentation::result_queries::validate(&request)?;
        let held = self.held.get(&request.scope).ok_or(
            "profile_results_unavailable: initialize and explicitly calculate this scope first",
        )?;
        let display_case = serde_json::from_value(json!(self.views[&request.scope].display_case))
            .expect("admitted case");
        ds_profile_presentation::result_queries::query(
            ds_profile_presentation::result_queries::QueryContext {
                session: &held.session,
                model_id: &held.source.model_id,
                publication_pending: self
                    .publications
                    .get(&request.scope)
                    .is_some_and(|publication| !publication.confirmed),
                display_case: &display_case,
                profile_index: Some(&held.result_index),
                analysis_index: held.analysis_index.as_deref(),
                query_scope: &held.query_scope,
                pending_reconcile: held.pending_reconcile,
                has_pending_receipts: !held.pending_receipts.is_empty(),
                analysis_invalidation: &held.analysis_invalidation,
            },
            request,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactional_projection_tests::{describe, refresh, request_for};
    use ds_grid_model::*;

    pub(super) fn fixture() -> GridModelSnapshot {
        let mut snapshot = common::fixture();
        snapshot.structures[1].z_m = 0.0; // A genuine valley support, with signed demand.
        for (i, (x_m, y_m, z_m)) in [(0.0, 0.0, 100.0), (300.0, 0.0, 0.0), (600.0, 40.0, 98.0)]
            .into_iter()
            .enumerate()
        {
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("query-ground-{i}")).unwrap(),
                x_m,
                y_m,
                z_m,
                feature_class: "ground".into(),
                description: None,
                required_clearance_m: None,
                source_id: None,
            });
        }
        let mut detached = snapshot.structures[0].clone();
        detached.id = StructureId::new("detached").unwrap();
        detached.station_m = Some(50.0);
        detached.x_m = 50.0;
        snapshot.structures.push(detached);
        snapshot.tension_sections.push(TensionSectionRow {
            id: TensionSectionId::new("query-section").unwrap(),
            cable_id: snapshot.cables[0].id.clone(),
            criterion_set_id: Some(snapshot.criterion_sets[0].id.clone()),
            sag: AuthoredSag::HorizontalTension {
                tension_n: 20_000.0,
            },
            sag_condition: Some(CableCondition::Initial),
            sag_weather_state_id: Some(snapshot.weather_states[0].id.clone()),
            ruling_span_m: None,
            details: Default::default(),
        });
        for (sequence, id) in ["str-1", "str-2", "str-3"].into_iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: TensionSectionId::new("query-section").unwrap(),
                    sequence: sequence as u32,
                    structure_id: StructureId::new(id).unwrap(),
                    attachment_point_id: Some(snapshot.attachment_points[0].id.clone()),
                    insulator_model: None,
                });
        }
        snapshot.capacity_artifacts[0].realization_digest =
            extract_element(&snapshot, snapshot.structure_types[0].id.entity())
                .unwrap()
                .snapshot_fingerprint();
        snapshot
    }
    pub(super) fn request(runtime: &Runtime, scope: &str, kind: QueryKind) -> QueryResultsRequest {
        let held = &runtime.held[scope];
        QueryResultsRequest {
            scope: scope.into(),
            model_id: held.source.model_id.clone(),
            expected_history: held.session.history_state(),
            expected_engineering_input_root: held
                .session
                .current_revision()
                .roots
                .engineering_input_root
                .clone(),
            kind,
            entity_ids: vec![],
            source: QuerySource::Profile,
            limit: 50,
            cursor: None,
        }
    }
    pub(super) fn execute(
        runtime: &mut Runtime,
        request: QueryResultsRequest,
    ) -> Result<Value, String> {
        let mut wire = json!(request);
        wire["operation"] = json!("query_results");
        runtime.execute(serde_json::from_value::<Request>(wire).unwrap())
    }

    #[test]
    fn signed_observation_keeps_all_queries_current_across_repeated_edits() {
        use ds_grid_engine::profile_calculated_delta::CalculatedSceneBase;
        let input = request_for(fixture());
        let mut worker = GridSession::open(fixture());
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        let mut receiver =
            CalculatedSceneBase::new(serde_json::from_value(initial["scene"].clone()).unwrap());
        for (index, description) in ["first edit", "next edit"].into_iter().enumerate() {
            let parent = worker.current_revision().revision_id.to_string();
            describe(&mut worker, &format!("whole-query-{index}"), description);
            let mut replay = refresh(&runtime, &input, &parent, &worker);
            replay.calculated_base_signature = Some(receiver.signature().into());
            let observed = runtime.execute(Request::Observe(Box::new(replay))).unwrap();
            assert_eq!(observed["calculation"]["required"], false, "{observed}");
            let delta =
                serde_json::from_str(observed["scene_delta_json"].as_str().unwrap()).unwrap();
            receiver = receiver
                .admit(
                    &delta,
                    &worker.current_revision().revision_id,
                    &worker.current_revision().roots.engineering_input_root,
                )
                .unwrap();
            let mut fresh = Runtime::default();
            let fresh_input = request_for(worker.snapshot().clone());
            fresh.execute(fresh_input.clone()).unwrap();
            for kind in [
                QueryKind::Usage,
                QueryKind::Failures,
                QueryKind::NegativeLoading,
                QueryKind::ViolatingSections,
                QueryKind::Blockers,
                QueryKind::Qualification,
            ] {
                let query = request(&runtime, &input.scope, kind);
                let actual = execute(&mut runtime, query).unwrap();
                let query = request(&fresh, &fresh_input.scope, kind);
                let expected = execute(&mut fresh, query).unwrap();
                assert_eq!(actual["items"], expected["items"]);
                assert_eq!(
                    actual["computed_revision"],
                    worker.current_revision().revision_id.to_string()
                );
            }
        }
    }

    #[test]
    fn all_query_kinds_match_fresh_typed_facts_without_solver_or_scene_mutation() {
        let input = request_for(fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let scene = runtime.held[&input.scope]
            .review
            .base_scene()
            .unwrap()
            .clone();
        let selected = scene.engineering.usage_request.clone().unwrap();
        let cache = &runtime.held[&input.scope].demand_cache;
        assert!(cache
            .cached_report(&scene.model_revision, &selected)
            .is_some());
        let mut wrong_envelope = selected.clone();
        wrong_envelope.analysis_case_ids.pop();
        assert!(cache
            .cached_report(&scene.model_revision, &wrong_envelope)
            .is_none());
        let fresh =
            ds_grid_engine::structure_usage::structure_usage_screening(&fixture(), &selected)
                .unwrap();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        let review = runtime.held[&input.scope].review.clone();
        let history = runtime.held[&input.scope].session.history_state();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
            QueryKind::ViolatingSections,
            QueryKind::Blockers,
        ] {
            let query = request(&runtime, &input.scope, kind);
            let reply = execute(&mut runtime, query).unwrap();
            assert_eq!(reply["freshness"], "current_inputs");
            assert_eq!(reply["metrics"]["calculated_cases"], 0);
            assert!(serde_json::to_vec(&reply).unwrap().len() <= MAX_BYTES);
            if matches!(
                kind,
                QueryKind::Usage
                    | QueryKind::Failures
                    | QueryKind::NegativeLoading
                    | QueryKind::Qualification
            ) {
                let expected: Vec<_> = fresh
                    .rows
                    .iter()
                    .filter(|row| match kind {
                        QueryKind::Failures => {
                            row.engineering_status == Some(EngineeringStatus::Fail)
                        }
                        QueryKind::NegativeLoading => {
                            row.uplift_case_present
                                || row.minimum_signed_weight_span_m.is_some_and(|v| v < 0.0)
                        }
                        _ => true,
                    })
                    .collect();
                assert_eq!(reply["total"], expected.len());
                for (item, row) in reply["items"].as_array().unwrap().iter().zip(expected) {
                    assert_eq!(item["entity_id"], row.structure_id.as_str());
                    assert_eq!(&item["fact"], usage_fact(row).as_ref());
                }
            }
        }
        let negative = fresh
            .rows
            .iter()
            .filter(|row| row.uplift_case_present)
            .count();
        assert!(
            negative > 0,
            "valley fixture must exercise actual native negative loading: {:?}",
            fresh.rows
        );
        assert!(
            fresh
                .rows
                .iter()
                .any(|row| row.engineering_status == Some(EngineeringStatus::Fail)),
            "applicable fixture capacity must exercise proven failure"
        );
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            work
        );
        assert!(Arc::ptr_eq(&runtime.held[&input.scope].review, &review));
        assert_eq!(runtime.held[&input.scope].session.history_state(), history);
    }

    #[test]
    fn paging_pins_result_head_kind_selection_and_bounds() {
        let input = request_for(fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut query = request(&runtime, &input.scope, QueryKind::Usage);
        query.limit = 1;
        let first = execute(&mut runtime, query.clone()).unwrap();
        assert_eq!(first["total"], 4);
        assert_eq!(first["items"].as_array().unwrap().len(), 1);
        query.cursor = serde_json::from_value(first["next_cursor"].clone()).unwrap();
        let second = execute(&mut runtime, query.clone()).unwrap();
        assert_eq!(second["offset"], 1);
        assert_ne!(first["items"], second["items"]);
        let mut bad = query.clone();
        bad.kind = QueryKind::Failures;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut bad = query.clone();
        bad.entity_ids = vec!["str-1".into()];
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut bad = query.clone();
        bad.cursor.as_mut().unwrap().offset = 5;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut bad = query.clone();
        bad.limit = MAX_ITEMS + 1;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut bad = query.clone();
        bad.model_id = "foreign".into();
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        let mut bad = query.clone();
        bad.expected_engineering_input_root = "foreign".into();
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        runtime.execute(input.refresh(&runtime)).unwrap(); // A fresh calculation at equal authored content.
        assert!(execute(&mut runtime, query)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut unknown = request(&runtime, "one", QueryKind::Usage);
        unknown.entity_ids = vec!["absent".into()];
        assert!(execute(&mut runtime, unknown)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut wrong_family = request(&runtime, "one", QueryKind::Failures);
        wrong_family.entity_ids = vec!["query-section".into()];
        assert!(execute(&mut runtime, wrong_family)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let captured = request(&runtime, "one", QueryKind::Usage);
        runtime
            .execute(Request::Release(ReleaseRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
            }))
            .unwrap();
        assert!(execute(&mut runtime, captured)
            .unwrap_err()
            .starts_with("profile_results_unavailable:"));
    }

    #[test]
    fn edits_invalidate_every_incident_row_but_preserve_unaffected_reads_and_rollback() {
        let snapshot = fixture();
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut untouched = request(&runtime, &input.scope, QueryKind::Usage);
        untouched.entity_ids = vec!["detached".into()];
        let old = execute(&mut runtime, untouched).unwrap();
        let captured = request(&runtime, &input.scope, QueryKind::Usage);
        describe(&mut worker, "query-edit", "pending edit");
        let replay = refresh(&runtime, &input, &input.expected_revision, &worker);
        runtime.execute(Request::Observe(Box::new(replay))).unwrap();
        assert!(execute(&mut runtime, captured)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        for id in ["str-1", "str-2", "str-3", "query-section"] {
            let mut query = request(&runtime, &input.scope, QueryKind::Blockers);
            query.entity_ids = vec![id.into()];
            assert!(
                execute(&mut runtime, query)
                    .unwrap_err()
                    .starts_with("profile_calculation_required:"),
                "{id}"
            );
        }
        let mut unaffected = request(&runtime, &input.scope, QueryKind::Usage);
        unaffected.entity_ids = vec!["detached".into()];
        let reused = execute(&mut runtime, unaffected).unwrap();
        assert_eq!(old["items"], reused["items"]);
        assert_eq!(reused["freshness"], "unchanged_dependencies");
        assert_eq!(old["computed_revision"], reused["computed_revision"]);
        assert_ne!(reused["model_revision"], reused["computed_revision"]);
        let mut calculate = refresh(
            &runtime,
            &input,
            &worker.current_revision().revision_id.to_string(),
            &worker,
        );
        calculate.publication_id = Some("query-publication".into());
        let calculated = runtime.execute(calculate).unwrap();
        let candidate = request(&runtime, &input.scope, QueryKind::Usage);
        assert!(execute(&mut runtime, candidate.clone())
            .unwrap_err()
            .starts_with("profile_publication_pending:"));
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: input.scope.clone(),
                model_id: "profile-test".into(),
                publication_id: "query-publication".into(),
                target_history: serde_json::from_value(calculated["history"].clone()).unwrap(),
            }))
            .unwrap();
        assert!(execute(&mut runtime, candidate)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
        let mut untouched = request(&runtime, &input.scope, QueryKind::Usage);
        untouched.entity_ids = vec!["detached".into()];
        assert_eq!(
            execute(&mut runtime, untouched).unwrap()["result_id"],
            old["result_id"]
        );
        runtime
            .execute(refresh(
                &runtime,
                &input,
                &worker.current_revision().revision_id.to_string(),
                &worker,
            ))
            .unwrap();
        let query = request(&runtime, &input.scope, QueryKind::Usage);
        assert_eq!(
            execute(&mut runtime, query).unwrap()["freshness"],
            "current_inputs"
        );
    }

    #[test]
    fn missing_envelope_blockers_are_readable_and_unknown_never_becomes_zero_failure() {
        let mut snapshot = common::fixture();
        snapshot.criterion_case_bindings.clear();
        let input = request_for(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
        ] {
            let query = request(&runtime, &input.scope, kind);
            assert!(execute(&mut runtime, query)
                .unwrap_err()
                .starts_with("profile_calculation_required:"));
        }
        let query = request(&runtime, &input.scope, QueryKind::Blockers);
        let reply = execute(&mut runtime, query).unwrap();
        assert!(reply["total"].as_u64().unwrap() > 0);
        assert_eq!(reply["basis"]["usage_available"], false);
    }

    #[test]
    fn negative_loading_within_declared_signed_bounds_is_not_a_failure() {
        let mut snapshot = fixture();
        for knot in &mut snapshot.span_limit_angle_knots {
            knot.minimum_signed_weight_span_m = -1_000_000.0;
            knot.allowable_wind_span_m = 1_000_000.0;
        }
        for maximum in &mut snapshot.span_limit_weight_maxima {
            maximum.maximum_signed_weight_span_m = 1_000_000.0;
        }
        let input = request_for(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let query = request(&runtime, &input.scope, QueryKind::NegativeLoading);
        let negative = execute(&mut runtime, query).unwrap();
        assert!(negative["total"].as_u64().unwrap() > 0);
        for item in negative["items"].as_array().unwrap() {
            assert_eq!(item["fact"]["row"]["uplift_failure"], false);
        }
        let query = request(&runtime, &input.scope, QueryKind::Failures);
        assert_eq!(execute(&mut runtime, query).unwrap()["total"], 0);
    }

    #[test]
    fn complete_query_counts_survive_analysis_caps_and_deleted_entity_history_aba() {
        let mut snapshot = common::fixture();
        for i in 0..2003 {
            let mut row = snapshot.structures[0].clone();
            row.id = StructureId::new(format!("extra-{i}")).unwrap();
            row.station_m = Some(i as f64 / 10.0);
            snapshot.structures.push(row);
        }
        let mut input = request_for(snapshot.clone());
        input.analyze = true;
        let mut runtime = Runtime::default();
        let analyzed = runtime.execute(input.clone()).unwrap();
        assert_eq!(
            analyzed["analysis"]["report"]["report"]["structures"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            2000
        );
        let held = runtime.held.get_mut(&input.scope).unwrap();
        let base = Arc::new(
            ds_grid_engine::profile_calculated_delta::CalculatedSceneBase::from_shared(
                held.calculated_scene.as_ref().unwrap().clone(),
            ),
        );
        let (index, work) = Index::new_retained(&held.session, base, &held.demand_cache);
        assert_eq!(work.materialized_facts, 0);
        assert_eq!(work.captured_report_rows, 2006);
        held.result_index = Arc::new(index);
        let mut query = request(&runtime, &input.scope, QueryKind::Usage);
        query.limit = 2;
        let page = execute(&mut runtime, query.clone()).unwrap();
        assert_eq!(page["total"], 2006);
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert_eq!(page["metrics"]["materialized_facts"], 2);
        query.cursor = serde_json::from_value(page["next_cursor"].clone()).unwrap();
        let mut worker = GridSession::open(snapshot);
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "delete-query-row",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::DeleteStructure {
                    id: StructureId::new("extra-0").unwrap(),
                },
            ))
            .unwrap();
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        let mut deleted = request(&runtime, &input.scope, QueryKind::Usage);
        deleted.entity_ids = vec!["extra-0".into()];
        assert!(execute(&mut runtime, deleted)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
        worker.undo().unwrap();
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &base, &worker,
            ))))
            .unwrap();
        assert!(execute(&mut runtime, query)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
    }

    #[test]
    #[ignore = "measured dense query gate; run alone with disk scratch"]
    fn dense_cached_query_costs() {
        let mut snapshot = fixture();
        for i in 0..10_000 {
            let mut row = snapshot.structures[3].clone();
            row.id = StructureId::new(format!("dense-query-{i}")).unwrap();
            row.station_m = Some(i as f64 / 100.0);
            snapshot.structures.push(row);
        }
        let input = request_for(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let index = runtime.held[&input.scope].result_index.clone();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        let mut measured = Vec::new();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
            QueryKind::ViolatingSections,
            QueryKind::Blockers,
        ] {
            let mut times = Vec::new();
            let mut transferred = 0;
            let mut total = 0;
            for _ in 0..12 {
                let mut query = request(&runtime, &input.scope, kind);
                query.limit = 50;
                let started = std::time::Instant::now();
                let response = execute(&mut runtime, query).unwrap();
                let bytes = serde_json::to_vec(&response).unwrap();
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                assert!(bytes.len() <= MAX_BYTES);
                transferred = bytes.len();
                total = response["total"].as_u64().unwrap();
            }
            times.sort_by(f64::total_cmp);
            measured.push(
                json!({"kind":kind,"median_ms":times[6],"minimum_ms":times[0],
                "maximum_ms":times[11],"returned_bytes":transferred,"total":total}),
            );
        }
        assert!(Arc::ptr_eq(
            &index,
            &runtime.held[&input.scope].result_index
        ));
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            work
        );
        println!(
            "PROFILE_QUERY_COST {}",
            json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},
            "structure_count":10_004,"page_limit":50,"queries":measured,
            "includes":"wire DTO decode, native query and JSON byte encoding",
            "excludes":"IPC, renderer, explicit-calculation index construction and installed artifact"})
        );
    }
}
