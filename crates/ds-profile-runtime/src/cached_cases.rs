//! Case reads and selection consume prepared results; only explicit Refresh
//! calculates. Byte suppression does not grant permission to prepare a case.
use super::*;

impl Runtime {
    pub(super) fn case_layer(&mut self, request: CaseRequest) -> Result<Value, String> {
        let started = std::time::Instant::now();
        let held = self
            .held
            .get_mut(&request.scope)
            .ok_or("profile_case_base_unavailable: draw the active Profile first")?;
        if held.source.model_id != request.model_id {
            return Err("profile_replay_model_mismatch: case belongs to another model".into());
        }
        if held.session.current_revision().revision_id.to_string() != request.expected_revision {
            return Err(
                "profile_selection_stale: cached Profile differs from the held revision".into(),
            );
        }
        if held.session.history_state() != request.expected_history {
            return Err(
                "profile_selection_stale: case history changed, including undo/redo tails".into(),
            );
        }
        if request.select && request.sequence <= held.last_weather_sequence {
            return Err(
                "profile_selection_stale: a newer weather selection already took over".into(),
            );
        }
        let current = self
            .views
            .get(&request.scope)
            .ok_or("profile_case_base_unavailable: display state is missing")?;
        let patch = DisplayPatch {
            display_case: Some(
                serde_json::from_value(serde_json::to_value(&request.display_case).unwrap())
                    .unwrap(),
            ),
            ..Default::default()
        };
        let state = current.patched(&patch)?;
        // The transport's legacy cached_only flag controls bytes, never permission
        // to calculate. Both reads and selections consume an exact prepared layer.
        let layer = held
            .review
            .current_prepared_layer(&held.session, &request.display_case, held.options.clone())
            .map_err(|error| match error {
                ds_grid_engine::ProfileAtlasError::CaseCalculationRequired => {
                    format!("profile_calculation_required: {error}")
                }
                _ => format!("profile_case_unavailable: {error}"),
            })?;
        let materialize_started = std::time::Instant::now();
        let exact_base = if request.select {
            Some(
                held.calculated_base
                    .clone()
                    .or_else(|| {
                        held.calculated_scene.as_ref().map(|scene| {
                            Arc::new(
                                ds_grid_engine::profile_calculated_delta::CalculatedSceneBase::from_shared(scene.clone()),
                            )
                        })
                    })
                    .ok_or("profile_case_base_unavailable: exact calculated scene is missing")?,
            )
        } else {
            None
        };
        if let Some(base) = &exact_base {
            let head = held.session.current_revision();
            if base.scene().model_revision != head.revision_id
                || base.scene().engineering_input_root != head.roots.engineering_input_root
            {
                return Err("profile_case_base_unavailable: calculate the current authored head before selecting a case".into());
            }
        }
        let mut scene = if let Some(base) = &exact_base {
            base.scene().clone()
        } else {
            held.review
                .base_scene()
                .ok_or("profile_case_base_unavailable: scene is missing")?
                .clone()
        };
        layer.apply_to_scene(&mut scene);
        let selected_base = exact_base.map(|base| Arc::new(base.rebase_scene(scene.clone())));
        let materialize_ms = materialize_started.elapsed().as_secs_f64() * 1000.0;
        let decoration_started = std::time::Instant::now();
        let case_primitives = decorations::project(&scene, &state, &held.rows, &held.label_policy)
            .into_iter()
            .filter(|primitive| {
                matches!(
                    primitive.option,
                    ds_command_kernel::profile_display::DisplayOption::SpanDistances
                        | ds_command_kernel::profile_display::DisplayOption::SectionLabels
                )
            })
            .collect::<Vec<_>>();
        let decoration_ms = decoration_started.elapsed().as_secs_f64() * 1000.0;
        let encode_started = std::time::Instant::now();
        let bytes = if request.cached_only {
            None
        } else {
            Some(layer.to_bytes().map_err(|e| e.to_string())?)
        };
        let layer_encode_ms = encode_started.elapsed().as_secs_f64() * 1000.0;
        let layer_bytes = bytes.as_ref().map_or(0, Vec::len);
        // Only the held revision's analysis counts, so a weather switch after an edit
        // never reports the previous revision's analysis as current.
        let mut command_view = command_view::project(
            &state,
            Some(&held.rows),
            Some(&scene),
            self.analyses
                .get(&request.scope)
                .filter(|value| value["model_revision"] == request.expected_revision),
            !held.session.snapshot().structures.is_empty(),
        );
        command_view["review"]["display_case"] =
            serde_json::to_value(&layer.display_case.requested).map_err(|e| e.to_string())?;
        command_view["review"]["selected_case_index"] = json!(layer.selected_case_index);
        let mut receipt = json!({"scope":request.scope,"model_id":held.source.model_id,
            "history":held.session.history_state(),"model_revision":request.expected_revision,"display_case":request.display_case,
            "axis_pin_digest":layer.axis_pin.digest,"bytes":bytes,"case_primitives":case_primitives,"display_state":state,"command_view":command_view});
        let index_started = std::time::Instant::now();
        if request.select {
            let index = Arc::new(result_queries::Index::new(
                &held.session,
                &scene,
                &held.demand_cache,
            ));
            Arc::make_mut(&mut held.review)
                .select_prepared_case(&held.session, request.display_case, held.options.clone())
                .expect("validated prepared layer");
            held.result_index = index;
            held.last_weather_sequence = request.sequence;
            // The existing native case layer advances this exact scene on
            // both sides; retain its new signature rather than the old base.
            held.calculated_scene = None;
            held.calculated_base = selected_base;
            self.views.insert(request.scope, state);
        }
        receipt["metrics"] = json!({"case_ms":started.elapsed().as_secs_f64()*1000.0,
            "materialize_ms":materialize_ms,"decorations_ms":decoration_ms,
            "layer_encode_ms":layer_encode_ms,"layer_bytes":layer_bytes,
            "index_ms":index_started.elapsed().as_secs_f64()*1000.0,
            "index_rebuilt":request.select,"calculated_cases":0});
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactional_projection_tests::{describe, refresh, request};
    use ds_grid_engine::profile_display_case::ProfileDisplayCase;

    fn case(
        runtime: &Runtime,
        input: &FixtureInput,
        display_case: ProfileDisplayCase,
        select: bool,
        cached_only: bool,
        sequence: u64,
    ) -> CaseRequest {
        CaseRequest {
            scope: input.scope.clone(),
            model_id: runtime.held[&input.scope].source.model_id.clone(),
            expected_history: runtime.held[&input.scope].session.history_state(),
            expected_revision: input.expected_revision.clone(),
            display_case,
            select,
            cached_only,
            sequence,
        }
    }
    fn query(runtime: &Runtime, input: &FixtureInput) -> QueryResultsRequest {
        let held = &runtime.held[&input.scope];
        QueryResultsRequest {
            scope: input.scope.clone(),
            model_id: held.source.model_id.clone(),
            expected_history: held.session.history_state(),
            expected_engineering_input_root: held
                .session
                .current_revision()
                .roots
                .engineering_input_root
                .clone(),
            kind: QueryKind::Blockers,
            source: QuerySource::Profile,
            entity_ids: vec![],
            limit: 1,
            cursor: None,
        }
    }
    #[test]
    fn reads_and_selection_refuse_unprepared_cases_without_changing_results_or_context() {
        let mut runtime = Runtime::default();
        let input = request();
        runtime.execute(input.clone()).unwrap();
        let unknown = ProfileDisplayCase::AnalysisCase {
            analysis_case_id: common::fixture().analysis_cases[0].id.clone(),
        };
        let index = runtime.held[&input.scope].result_index.clone();
        let context = runtime.calculation_contexts[&input.scope].clone();
        let review = runtime.held[&input.scope].review.clone();
        let state = json!(runtime.views[&input.scope]);
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        for select in [false, true] {
            for cached_only in [false, true] {
                let error = runtime
                    .execute(case(
                        &runtime,
                        &input,
                        unknown.clone(),
                        select,
                        cached_only,
                        1,
                    ))
                    .unwrap_err();
                assert!(
                    error.starts_with("profile_calculation_required:"),
                    "{error}"
                );
                assert!(Arc::ptr_eq(
                    &index,
                    &runtime.held[&input.scope].result_index
                ));
                assert!(Arc::ptr_eq(&review, &runtime.held[&input.scope].review));
                assert!(Arc::ptr_eq(
                    &context,
                    &runtime.calculation_contexts[&input.scope]
                ));
                assert_eq!(json!(runtime.views[&input.scope]), state);
                assert_eq!(
                    runtime.held[&input.scope].demand_cache.last_demand_work(),
                    work
                );
            }
        }
        let read = runtime
            .execute(case(
                &runtime,
                &input,
                ProfileDisplayCase::GreatestSag,
                false,
                false,
                0,
            ))
            .unwrap();
        assert!(read["bytes"].is_array());
        assert!(Arc::ptr_eq(
            &context,
            &runtime.calculation_contexts[&input.scope]
        ));
        assert!(Arc::ptr_eq(
            &index,
            &runtime.held[&input.scope].result_index
        ));
        assert_eq!(runtime.held[&input.scope].last_weather_sequence, 0);
        assert_eq!(json!(runtime.views[&input.scope]), state);
    }
    #[test]
    fn prepared_case_retains_signed_base_and_next_edit_uses_exact_delta() {
        use crate::transactional_projection_tests::{request_for, section_demand_fixture};
        use ds_grid_engine::profile_calculated_delta::CalculatedSceneBase;
        let snapshot = section_demand_fixture();
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        let mut input = input;
        input.patch.display_case =
            Some(serde_json::from_value(json!({"mode":"sag_reference"})).unwrap());
        let initial = runtime.execute(input.clone()).unwrap();
        let mut expected: ds_grid_engine::ProfileAtlasScene =
            serde_json::from_value(initial["scene"].clone()).unwrap();
        let layer = runtime.held[&input.scope]
            .review
            .current_prepared_layer(
                &runtime.held[&input.scope].session,
                &ProfileDisplayCase::SagReference,
                runtime.held[&input.scope].options.clone(),
            )
            .unwrap();
        layer.apply_to_scene(&mut expected);
        let selected = runtime
            .execute(case(
                &runtime,
                &input,
                ProfileDisplayCase::SagReference,
                true,
                false,
                1,
            ))
            .unwrap();
        assert_eq!(selected["metrics"]["calculated_cases"], 0);
        let receiver = CalculatedSceneBase::new(expected);
        let prepared = runtime
            .execute(Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
                scope: input.scope.clone(),
                model_id: runtime.held[&input.scope].source.model_id.clone(),
                expected_history: worker.history_state(),
            }))
            .unwrap();
        assert_eq!(prepared["base_signature"], receiver.signature());
        assert_eq!(prepared["metrics"]["base_rebuilt"], 0);
        let revision = worker.current_revision().revision_id.to_string();
        describe(
            &mut worker,
            "case-next-edit",
            "uses the selected signed base",
        );
        let mut observe = refresh(&runtime, &input, &revision, &worker);
        observe.calculated_base_signature = Some(receiver.signature().to_string());
        let result = runtime
            .execute(Request::Observe(Box::new(observe)))
            .unwrap();
        assert_eq!(result["deferred"], false, "{result}");
        assert_eq!(result["calculation"]["required"], false, "{result}");
        let delta = serde_json::from_str(result["scene_delta_json"].as_str().unwrap()).unwrap();
        let admitted = receiver
            .admit(
                &delta,
                &worker.current_revision().revision_id,
                &worker.current_revision().roots.engineering_input_root,
            )
            .unwrap();
        // Case selection is explicit; use its retained native case for the
        // independent calculation rather than silently restoring a default.
        let mut fresh_input = request_for(worker.snapshot().clone());
        fresh_input.patch.display_case = input.patch.display_case.clone();
        let fresh = Runtime::default().execute(fresh_input).unwrap();
        let fresh_scene: ds_grid_engine::ProfileAtlasScene =
            serde_json::from_value(fresh["scene"].clone()).unwrap();
        crate::calculated_transfer::tests::assert_paint_and_retained_scene(
            &runtime, admitted.scene(), &fresh_scene,
        );
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
    }

    #[test]
    fn case_selection_cannot_promote_a_previous_calculated_head() {
        let mut runtime = Runtime::default();
        let input = request();
        runtime.execute(input.clone()).unwrap();
        let previous = runtime.held[&input.scope].calculated_scene.clone().unwrap();
        let mut worker = runtime.held[&input.scope].session.clone();
        let revision = worker.current_revision().revision_id.to_string();
        describe(&mut worker, "case-with-old-base", "current authored head");
        runtime
            .execute(Request::Refresh(Box::new(refresh(
                &runtime, &input, &revision, &worker,
            ))))
            .unwrap();
        let held = runtime.held.get_mut(&input.scope).unwrap();
        held.calculated_scene = Some(previous.clone());
        held.calculated_base = None;
        let index = held.result_index.clone();
        let context = runtime.calculation_contexts[&input.scope].clone();
        let state = json!(runtime.views[&input.scope]);
        let mut select = case(
            &runtime,
            &input,
            ProfileDisplayCase::GreatestSag,
            true,
            true,
            1,
        );
        select.expected_revision = worker.current_revision().revision_id.to_string();
        assert!(
            runtime
                .execute(select)
                .unwrap_err()
                .starts_with("profile_case_base_unavailable:")
        );
        assert!(Arc::ptr_eq(
            &index,
            &runtime.held[&input.scope].result_index
        ));
        assert!(Arc::ptr_eq(
            &previous,
            runtime.held[&input.scope]
                .calculated_scene
                .as_ref()
                .unwrap()
        ));
        assert!(Arc::ptr_eq(
            &context,
            &runtime.calculation_contexts[&input.scope]
        ));
        assert_eq!(json!(runtime.views[&input.scope]), state);
        assert_eq!(runtime.held[&input.scope].last_weather_sequence, 0);
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
    }

    #[test]
    fn native_case_fences_model_and_same_revision_history_aba() {
        let mut runtime = Runtime::default();
        let input = request();
        runtime.execute(input.clone()).unwrap();
        let original = case(
            &runtime,
            &input,
            ProfileDisplayCase::GreatestSag,
            true,
            true,
            1,
        );
        let mut foreign = original.clone();
        foreign.model_id = "another-model".into();
        assert!(runtime
            .execute(foreign)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        let mut worker = GridSession::open(common::fixture());
        describe(&mut worker, "case-aba", "different authored head");
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        let edited = worker.current_revision().revision_id.to_string();
        worker.undo().unwrap();
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &edited, &worker,
            ))))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string(),
            original.expected_revision
        );
        assert_ne!(
            runtime.held[&input.scope].session.history_state(),
            original.expected_history
        );
        assert!(runtime
            .execute(original)
            .unwrap_err()
            .starts_with("profile_selection_stale:"));
    }
    #[test]
    fn explicit_case_calculation_then_cached_selection_keeps_query_case_and_rollback_exact() {
        let mut runtime = Runtime::default();
        let input = request();
        runtime.execute(input.clone()).unwrap();
        let worker = GridSession::open(common::fixture());
        let mut calculate = refresh(&runtime, &input, &input.expected_revision, &worker);
        calculate.patch.display_case = Some(
            serde_json::from_value(json!({
                "mode":"analysis_case", "analysis_case_id":common::fixture().analysis_cases[0].id
            }))
            .unwrap(),
        );
        calculate.publication_id = Some("prepared-case".into());
        let candidate = runtime.execute(calculate).unwrap();
        let error = runtime
            .execute(case(
                &runtime,
                &input,
                ProfileDisplayCase::GreatestSag,
                false,
                false,
                1,
            ))
            .unwrap_err();
        assert!(error.starts_with("profile_publication_pending:"));
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: input.scope.clone(),
                publication: serde_json::from_value(candidate["publication"].clone()).unwrap(),
            }))
            .unwrap();
        let context = runtime.calculation_contexts[&input.scope].clone();
        let index = runtime.held[&input.scope].result_index.clone();
        // A pure prepared read cannot retire the confirmed rollback checkpoint
        // or change display/job context, even when reading a different case.
        runtime
            .execute(case(
                &runtime,
                &input,
                ProfileDisplayCase::GreatestSag,
                false,
                false,
                1,
            ))
            .unwrap();
        assert!(runtime.publications.contains_key(&input.scope));
        assert!(Arc::ptr_eq(
            &context,
            &runtime.calculation_contexts[&input.scope]
        ));
        assert!(Arc::ptr_eq(
            &index,
            &runtime.held[&input.scope].result_index
        ));
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: input.scope.clone(),
                model_id: candidate["model_id"].as_str().unwrap().into(),
                publication_id: "prepared-case".into(),
                target_history: worker.history_state(),
            }))
            .unwrap();
        let mut calculate = refresh(&runtime, &input, &input.expected_revision, &worker);
        calculate.patch.display_case = Some(
            serde_json::from_value(json!({
                "mode":"analysis_case", "analysis_case_id":common::fixture().analysis_cases[0].id
            }))
            .unwrap(),
        );
        runtime.execute(calculate).unwrap();
        let before = runtime
            .execute(Request::QueryResults(query(&runtime, &input)))
            .unwrap();
        let prior = runtime.held[&input.scope].calculated_scene.clone().unwrap();
        let mut selected_scene = (*prior).clone();
        runtime.held[&input.scope]
            .review
            .current_prepared_layer(
                &runtime.held[&input.scope].session,
                &ProfileDisplayCase::GreatestSag,
                runtime.held[&input.scope].options.clone(),
            )
            .unwrap()
            .apply_to_scene(&mut selected_scene);
        let expected_selected =
            ds_grid_engine::profile_calculated_delta::CalculatedSceneBase::new(selected_scene);
        let cached = runtime
            .execute(case(
                &runtime,
                &input,
                ProfileDisplayCase::GreatestSag,
                true,
                true,
                2,
            ))
            .unwrap();
        assert!(cached["bytes"].is_null());
        assert!(cached["case_primitives"].is_array());
        let retained = runtime.held[&input.scope].calculated_base.as_ref().unwrap();
        assert_eq!(retained.signature(), expected_selected.signature());
        assert_ne!(
            retained.signature(),
            ds_grid_engine::profile_calculated_delta::CalculatedSceneBase::from_shared(prior)
                .signature()
        );
        let after = runtime
            .execute(Request::QueryResults(query(&runtime, &input)))
            .unwrap();
        assert_eq!(
            after["display_case"],
            json!(ProfileDisplayCase::GreatestSag)
        );
        assert_ne!(after["result_id"], before["result_id"]);
        let mut stale = query(&runtime, &input);
        stale.cursor = serde_json::from_value(before["next_cursor"].clone()).unwrap();
        assert!(stale.cursor.is_some());
        assert!(runtime
            .execute(Request::QueryResults(stale))
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut edited = worker;
        describe(&mut edited, "case-edit", "authored change");
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &edited,
            ))))
            .unwrap();
        let mut read = case(
            &runtime,
            &input,
            ProfileDisplayCase::GreatestSag,
            false,
            false,
            3,
        );
        read.expected_revision = edited.current_revision().revision_id.to_string();
        assert!(runtime
            .execute(read)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
    }
}
