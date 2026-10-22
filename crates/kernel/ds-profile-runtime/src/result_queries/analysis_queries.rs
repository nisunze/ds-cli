//! Runtime integration tests use the shared immutable query owner.
use super::*;
#[cfg(test)]
#[path = "analysis_retention_tests.rs"]
mod retention_tests;

#[cfg(test)]
mod tests {
    use super::super::tests::{execute, fixture, request};
    use super::*;
    use crate::transactional_projection_tests::{describe, refresh, request_for};
    use ds_grid_model::*;

    pub(super) fn configured_fixture(point_count: usize) -> GridModelSnapshot {
        let mut session = GridSession::open(fixture());
        let standard =
            ds_grid_engine::feature_code_standard::FeatureCodeStandard::bundled().unwrap();
        let import = ds_grid_engine::feature_code_ops::plan_standard_import(
            session.snapshot(),
            &standard,
            &ds_grid_engine::feature_code_ops::StandardImportOptions {
                voltage_class: "MV".into(),
                merge: false,
            },
        )
        .unwrap();
        let apply = |session: &mut GridSession,
                     commands: Vec<ds_grid_engine::GridCommand>,
                     prefix: &str| {
            for (i, command) in commands.into_iter().enumerate() {
                session
                    .apply_command(ds_grid_engine::CommandEnvelope::new(
                        format!("{prefix}-{i}"),
                        session.current_revision().revision_id.clone(),
                        command,
                    ))
                    .unwrap();
            }
        };
        apply(&mut session, import.commands, "codes");
        let plan = ds_grid_engine::clearance_criteria::plan_clearance_set(
            session.snapshot(),
            &standard,
            &ds_grid_engine::clearance_criteria::ClearanceSetOptions {
                voltage_class: "MV".into(),
                vertical_case: "Every day".into(),
                horizontal_case: "High wind".into(),
                voltage_kv: None,
                criterion_set_id: None,
            },
        )
        .unwrap();
        apply(&mut session, plan.commands, "clearance");
        let mut snapshot = session.snapshot().clone();
        for i in 0..point_count {
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("finding-{i}")).unwrap(),
                x_m: 150.0,
                y_m: 0.5,
                z_m: 100.0,
                feature_class: "GROUND".into(),
                description: None,
                required_clearance_m: Some(1000.0),
                source_id: None,
            });
        }
        snapshot
    }
    pub(super) fn query(
        runtime: &Runtime,
        input: &FixtureInput,
        kind: QueryKind,
    ) -> QueryResultsRequest {
        let mut query = request(runtime, &input.scope, kind);
        query.source = QuerySource::ModelAnalysis;
        query
    }
    pub(super) fn native(
        runtime: &Runtime,
        input: &FixtureInput,
    ) -> (DefaultAnalysisReport, DefaultAnalysisFacts) {
        let session = &runtime.held[&input.scope].session;
        session
            .analyze_model_defaults_with_facts(
                &ds_grid_engine::DefaultAnalysisRequest {
                    expected_revision: session.current_revision().revision_id.clone(),
                    expected_engineering_input_root: session
                        .current_revision()
                        .roots
                        .engineering_input_root
                        .clone(),
                    max_rows: 2000,
                },
                &mut Default::default(),
            )
            .unwrap()
    }
    fn paged(runtime: &mut Runtime, input: &FixtureInput, kind: QueryKind) -> Vec<Value> {
        let mut query = query(runtime, input, kind);
        let mut result = Vec::new();
        query.limit = 200;
        loop {
            let reply = execute(runtime, query.clone()).unwrap();
            assert_eq!(reply["source"], "model_analysis");
            assert_eq!(reply["display_case"], Value::Null);
            assert_eq!(reply["metrics"]["calculated_cases"], 0);
            assert!(serde_json::to_vec(&reply).unwrap().len() <= MAX_BYTES);
            result.extend(reply["items"].as_array().unwrap().iter().cloned());
            if reply["next_cursor"].is_null() {
                break;
            }
            query.cursor = Some(serde_json::from_value(reply["next_cursor"].clone()).unwrap());
        }
        result
    }
    #[test]
    fn complete_model_analysis_queries_preserve_native_facts_before_caps_without_math() {
        let mut input = request_for(configured_fixture(2031));
        input.analyze = true;
        let mut runtime = Runtime::default();
        let admitted = runtime.execute(input.clone()).unwrap();
        let (report, fresh) = native(&runtime, &input);
        let clearance = fresh.clearance.as_ref().unwrap();
        assert!(clearance.findings.len() > 2000);
        assert!(
            admitted["analysis"]["report"]["report"]["clearance"]["findings"]["truncated"]
                .as_bool()
                .unwrap()
        );
        let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        let history = runtime.held[&input.scope].session.history_state();
        let review = runtime.held[&input.scope].review.clone();
        let points = paged(&mut runtime, &input, QueryKind::Findings);
        assert_eq!(
            points
                .iter()
                .map(|item| item["fact"]["finding"].clone())
                .collect::<Vec<_>>(),
            clearance
                .findings
                .iter()
                .map(|finding| json!(finding))
                .collect::<Vec<_>>()
        );
        let geometry: BTreeMap<_, _> = runtime.held[&input.scope]
            .session
            .snapshot()
            .terrain_points
            .iter()
            .map(|point| {
                (
                    point.id.to_string(),
                    json!({"x_m":point.x_m,"y_m":point.y_m,"z_m":point.z_m}),
                )
            })
            .collect();
        for item in &points {
            assert_eq!(
                item["fact"]["geometry"],
                geometry[item["entity_id"].as_str().unwrap()]
            );
        }
        let usage = paged(&mut runtime, &input, QueryKind::Usage);
        assert_eq!(
            usage
                .iter()
                .map(|item| item["fact"].clone())
                .collect::<Vec<_>>(),
            fresh
                .structures
                .iter()
                .map(|row| (*usage_fact(row)).clone())
                .collect::<Vec<_>>()
        );
        let qualified = paged(&mut runtime, &input, QueryKind::Qualification);
        let resistance: Vec<_> = qualified
            .iter()
            .filter(|item| item["fact"]["kind"] == "support_resistance")
            .map(|item| item["fact"].clone())
            .collect();
        assert_eq!(
            resistance,
            fresh
                .support_resistance_checks
                .iter()
                .map(|check| (*resistance_fact(check)).clone())
                .collect::<Vec<_>>()
        );
        assert!(fresh
            .support_resistance_checks
            .iter()
            .any(|check| check.outcome == SupportResistanceOutcome::Unknown));
        let fail = paged(&mut runtime, &input, QueryKind::Failures);
        assert!(fail
            .iter()
            .any(|item| item["fact"]["kind"] == "structure_usage"
                && item["fact"]["row"]["unevaluated_checks_count"]
                    .as_u64()
                    .unwrap()
                    > 0));
        for item in &fail {
            if item["fact"]["kind"] == "support_resistance" {
                assert_eq!(item["fact"]["check"]["outcome"], "fail");
            } else {
                assert_eq!(item["fact"]["row"]["engineering_status"], "fail");
            }
        }
        let sections = paged(&mut runtime, &input, QueryKind::ViolatingSections);
        assert!(sections
            .iter()
            .any(|item| item["fact"]["design_clearance_point_count"]
                .as_u64()
                .unwrap_or(0)
                > 2000));
        let blockers = paged(&mut runtime, &input, QueryKind::Blockers);
        assert!(blockers
            .iter()
            .any(|item| item["fact"]["kind"] == "support_resistance_input"));
        for id in ["finding-2030", "query-section", "al-main", "str-1"] {
            let mut query = query(&runtime, &input, QueryKind::Findings);
            query.entity_ids = vec![id.into()];
            let reply = execute(&mut runtime, query).unwrap();
            assert!(
                reply["total"].as_u64().unwrap() > 0,
                "native related identity {id}"
            );
        }
        assert_eq!(report.model_revision, history.model_revision);
        assert_eq!(runtime.held[&input.scope].session.history_state(), history);
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            work
        );
        assert!(Arc::ptr_eq(&review, &runtime.held[&input.scope].review));
        assert!(Arc::ptr_eq(
            &index,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
    }
    #[test]
    fn model_analysis_refuses_missing_source_clearance_and_wrong_query_families() {
        let input = request_for(fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let read = query(&runtime, &input, QueryKind::Blockers);
        assert!(execute(&mut runtime, read)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
        let mut analyzed = input.clone();
        analyzed.analyze = true;
        runtime.execute(analyzed.refresh(&runtime)).unwrap();
        let wrong_source = request(&runtime, &input.scope, QueryKind::Findings);
        assert!(execute(&mut runtime, wrong_source)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let read = query(&runtime, &input, QueryKind::Findings);
        assert!(execute(&mut runtime, read)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
        let read = query(&runtime, &input, QueryKind::Blockers);
        let blockers = execute(&mut runtime, read).unwrap();
        assert!(blockers["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["fact"]["kind"] == "design_clearance_unavailable"));
        let mut bad = query(&runtime, &input, QueryKind::Usage);
        bad.entity_ids = vec!["query-section".into()];
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut bad = query(&runtime, &input, QueryKind::Blockers);
        bad.entity_ids = vec!["absent".into()];
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut bad = query(&runtime, &input, QueryKind::Blockers);
        bad.model_id = "foreign".into();
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        let mut bad = query(&runtime, &input, QueryKind::Blockers);
        bad.expected_history.redo_depth += 1;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut bad = query(&runtime, &input, QueryKind::Blockers);
        bad.expected_engineering_input_root = "other".into();
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut read = query(&runtime, &input, QueryKind::Blockers);
        read.limit = 1;
        let reply = execute(&mut runtime, read.clone()).unwrap();
        read.cursor = Some(serde_json::from_value(reply["next_cursor"].clone()).unwrap());
        read.cursor.as_mut().unwrap().source = QuerySource::Profile;
        assert!(execute(&mut runtime, read)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        assert!(serde_json::from_value::<QueryArguments>(
            json!({"kind":"findings","source":"unknown"})
        )
        .is_err());
    }
    #[test]
    fn model_analysis_horizontal_only_findings_are_not_assigned_to_the_vertical_section() {
        let mut input = request_for(configured_fixture(2));
        input.analyze = true;
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let (report, mut facts) = native(&runtime, &input);
        for finding in &mut facts.clearance.as_mut().unwrap().findings {
            // Simulate an authoritative horizontal-only witness. Its section_id
            // is the vertical wire reference, not the failed horizontal wire.
            finding.horizontal_span = None; // no source horizontal wire evidence
            finding.vertical_violation = false;
            finding.horizontal_violation = true;
            finding.vertical_deficit_m = None;
            finding.available_vertical_m = Some(finding.required_vertical_m + 1.0);
        }
        let held = runtime.held.get_mut(&input.scope).unwrap();
        held.analysis_index = Some(Arc::new(Index::model_analysis(
            &held.session,
            &report,
            facts,
        )));
        let read = query(&runtime, &input, QueryKind::Findings);
        let findings = execute(&mut runtime, read).unwrap();
        assert!(findings["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |item| item["fact"]["finding"]["horizontal_violation"] == true
                    && item["fact"]["finding"]["vertical_violation"] == false
            ));
        let read = query(&runtime, &input, QueryKind::ViolatingSections);
        let sections = execute(&mut runtime, read).unwrap();
        assert!(sections["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["fact"]["design_clearance_point_count"].is_null()));
        assert_eq!(
            sections["basis"]["design_clearance_section_scope"],
            "native_independent_vertical_and_horizontal_wires"
        );
    }
    #[test]
    fn model_analysis_indexes_actual_horizontal_spans_without_vertical_attribution_or_double_counts(
    ) {
        let mut snapshot = configured_fixture(0);
        snapshot.structures[1].z_m = 100.0;
        snapshot
            .terrain_points
            .iter_mut()
            .for_each(|point| point.z_m = 100.0);
        snapshot.tension_sections[0].sag = AuthoredSag::CatenaryConstant { c_m: 1200.0 };
        let mut horizontal = snapshot.tension_sections[0].clone();
        horizontal.id = TensionSectionId::new("horizontal-section").unwrap();
        horizontal.sag = AuthoredSag::CatenaryConstant { c_m: 3600.0 };
        snapshot.tension_sections.push(horizontal);
        snapshot.attachment_points[1].local_x_m = snapshot.attachment_points[0].local_x_m;
        for mut support in snapshot.tension_section_supports.clone() {
            support.section_id = TensionSectionId::new("horizontal-section").unwrap();
            support.attachment_point_id = Some(AttachmentPointId::new("ap-right").unwrap());
            snapshot.tension_section_supports.push(support);
        }
        for i in 0..3 {
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("axes-{i}")).unwrap(),
                x_m: 150.0,
                y_m: 1.5,
                z_m: 100.0,
                feature_class: "BUILDING".into(),
                description: Some("h=2".into()),
                required_clearance_m: None,
                source_id: None,
            });
        }
        let mut input = request_for(snapshot);
        input.analyze = true;
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let (report, mut facts) = native(&runtime, &input);
        let native_findings = &facts.clearance.as_ref().unwrap().findings;
        let point = native_findings
            .iter()
            .find(|finding| finding.terrain_point_id == "axes-0")
            .unwrap();
        assert!(point.vertical_violation && point.horizontal_violation);
        assert_eq!(point.section_id.as_deref(), Some("query-section"));
        assert_eq!(
            point.horizontal_span.as_ref().unwrap().section_id,
            "horizontal-section"
        );
        let mut read = query(&runtime, &input, QueryKind::Findings);
        read.entity_ids = vec!["axes-0".into()];
        let page = execute(&mut runtime, read).unwrap();
        assert_eq!(page["total"], 1);
        assert_eq!(page["items"][0]["fact"]["finding"], json!(point));
        // Native horizontal section and incident support refs resolve the same
        // original findings; shared vertical/horizontal refs never duplicate a row.
        for id in ["horizontal-section", "str-1", "str-2"] {
            let mut read = query(&runtime, &input, QueryKind::Findings);
            read.entity_ids = vec![id.into()];
            let page = execute(&mut runtime, read).unwrap();
            let ids: BTreeSet<_> = page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["entity_id"].as_str().unwrap())
                .collect();
            assert_eq!(ids.len(), page["total"].as_u64().unwrap() as usize);
            assert!(ids.contains("axes-0"));
        }
        // Isolate point contributions from native capacity findings. Simulate
        // one both-axis point in the same actual section and one horizontal-only
        // point, retaining the native reported horizontal span.
        facts
            .clearance
            .as_mut()
            .unwrap()
            .findings
            .retain(|finding| finding.terrain_point_id.starts_with("axes-"));
        let findings = &mut facts.clearance.as_mut().unwrap().findings;
        findings[0].section_id = Some("horizontal-section".into());
        findings[1].vertical_violation = false;
        let held = runtime.held.get_mut(&input.scope).unwrap();
        held.analysis_index = Some(Arc::new(Index::model_analysis(
            &held.session,
            &report,
            facts,
        )));
        let read = query(&runtime, &input, QueryKind::ViolatingSections);
        let page = execute(&mut runtime, read).unwrap();
        let rows = page["items"].as_array().unwrap();
        let horizontal = rows
            .iter()
            .find(|row| row["entity_id"] == "horizontal-section")
            .unwrap();
        assert_eq!(
            horizontal["fact"]["design_clearance_point_count"], 3,
            "shared axis counts one point, horizontal-only point still counts"
        );
        let vertical = rows
            .iter()
            .find(|row| row["entity_id"] == "query-section")
            .unwrap();
        assert_eq!(
            vertical["fact"]["design_clearance_point_count"], 1,
            "horizontal-only point does not use its vertical ref"
        );
    }
    #[test]
    fn model_analysis_case_selection_presentation_and_unchanged_refresh_preserve_its_index() {
        let mut input = request_for(configured_fixture(2));
        input.analyze = true;
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
        let mut read = query(&runtime, &input, QueryKind::Findings);
        read.limit = 1;
        let first = execute(&mut runtime, read.clone()).unwrap();
        read.cursor = Some(serde_json::from_value(first["next_cursor"].clone()).unwrap());
        input.analyze = false;
        input.patch.display_case =
            Some(ds_command_kernel::profile_display::DisplayCase::SagReference);
        runtime.execute(input.refresh(&runtime)).unwrap();
        let held = &runtime.held[&input.scope];
        runtime
            .execute(Request::CaseLayer(CaseRequest {
                scope: input.scope.clone(),
                model_id: held.source.model_id.clone(),
                expected_history: held.session.history_state(),
                expected_revision: held.session.current_revision().revision_id.to_string(),
                display_case:
                    ds_grid_engine::profile_display_case::ProfileDisplayCase::GreatestSag {},
                select: true,
                cached_only: true,
                sequence: 1,
            }))
            .unwrap();
        let next = execute(&mut runtime, read).unwrap();
        assert_eq!(next["result_id"], first["result_id"]);
        assert!(Arc::ptr_eq(
            &index,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
    }
    #[test]
    fn model_analysis_edits_refresh_and_publication_rollback_never_admit_stale_facts() {
        let snapshot = configured_fixture(2);
        let mut input = request_for(snapshot.clone());
        input.analyze = true;
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
        let before = query(&runtime, &input, QueryKind::Findings);
        describe(&mut worker, "edit-analysis", "changed engineering input");
        let replay = refresh(&runtime, &input, &input.expected_revision, &worker);
        runtime.execute(Request::Observe(Box::new(replay))).unwrap();
        assert!(execute(&mut runtime, before)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        for id in ["finding-0", "str-1"] {
            let mut read = query(&runtime, &input, QueryKind::Blockers);
            read.entity_ids = vec![id.into()];
            assert!(execute(&mut runtime, read)
                .unwrap_err()
                .starts_with("profile_calculation_required:"));
        }
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        let mut calc = refresh(&runtime, &input, &base, &worker);
        calc.publication_id = Some("profile-only".into());
        let projected = runtime.execute(calc).unwrap();
        assert!(
            Arc::ptr_eq(
                &index,
                runtime.held[&input.scope].analysis_index.as_ref().unwrap()
            ),
            "Profile-only refresh retains the exact independently computed index"
        );
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: input.scope.clone(),
                model_id: "profile-test".into(),
                publication_id: "profile-only".into(),
                target_history: worker.history_state(),
            }))
            .unwrap();
        assert!(Arc::ptr_eq(
            &index,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        assert!(!projected["publication"].is_null());
        let mut calc = refresh(&runtime, &input, &base, &worker);
        calc.analyze = true;
        calc.publication_id = Some("analyzed".into());
        let projected = runtime.execute(calc).unwrap();
        let read = query(&runtime, &input, QueryKind::Findings);
        assert!(execute(&mut runtime, read)
            .unwrap_err()
            .starts_with("profile_publication_pending:"));
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: input.scope.clone(),
                publication: serde_json::from_value(projected["publication"].clone()).unwrap(),
            }))
            .unwrap();
        let read = query(&runtime, &input, QueryKind::Findings);
        let current = execute(&mut runtime, read).unwrap();
        assert_eq!(
            current["model_revision"],
            json!(worker.current_revision().revision_id)
        );
        assert_eq!(current["computed_revision"], current["model_revision"]);
        assert_eq!(current["freshness"], "current_inputs");
    }

    #[test]
    fn model_analysis_negative_loading_within_signed_bounds_does_not_become_a_failure() {
        let mut snapshot = fixture();
        for knot in &mut snapshot.span_limit_angle_knots {
            knot.minimum_signed_weight_span_m = -1_000_000.0;
            knot.allowable_wind_span_m = 1_000_000.0;
        }
        for maximum in &mut snapshot.span_limit_weight_maxima {
            maximum.maximum_signed_weight_span_m = 1_000_000.0;
        }
        let mut input = request_for(snapshot);
        input.analyze = true;
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let read = query(&runtime, &input, QueryKind::NegativeLoading);
        let negative = execute(&mut runtime, read).unwrap();
        assert!(negative["total"].as_u64().unwrap() > 0);
        for item in negative["items"].as_array().unwrap() {
            if item["fact"]["kind"] == "structure_usage" {
                assert_eq!(item["fact"]["row"]["uplift_failure"], false);
            } else {
                assert_ne!(item["fact"]["check"]["outcome"], "fail");
            }
        }
        let read = query(&runtime, &input, QueryKind::Failures);
        assert_eq!(execute(&mut runtime, read).unwrap()["total"], 0);
    }
    #[test]
    fn model_analysis_unavailable_envelope_retains_its_native_refusal_instead_of_zero_failures() {
        let mut snapshot = fixture();
        snapshot.criterion_sets.push(CriterionSetRow {
            id: CriterionSetId::new("ambiguous-set").unwrap(),
            label: "Other criterion".into(),
        });
        let mut input = request_for(snapshot);
        input.analyze = true;
        let mut runtime = Runtime::default();
        let admitted = runtime.execute(input.clone()).unwrap();
        assert_eq!(admitted["analysis"]["report"]["status"], "unavailable");
        let read = query(&runtime, &input, QueryKind::Blockers);
        let blocked = execute(&mut runtime, read).unwrap();
        assert_eq!(blocked["basis"]["model_analysis_available"], false);
        assert_eq!(blocked["total"], 1);
        assert_eq!(
            blocked["items"][0]["fact"]["code"],
            admitted["analysis"]["report"]["refusal"]
        );
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
            QueryKind::Findings,
            QueryKind::ViolatingSections,
        ] {
            let read = query(&runtime, &input, kind);
            assert!(execute(&mut runtime, read)
                .unwrap_err()
                .starts_with("profile_calculation_required:"));
        }
    }
    #[test]
    fn model_analysis_async_admission_discard_and_stale_worker_preserve_exact_index_ownership() {
        use std::time::{Duration, Instant};
        let snapshot = configured_fixture(2);
        let mut input = request_for(snapshot.clone());
        input.analyze = true;
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let original = runtime.held[&input.scope].analysis_index.clone().unwrap();
        let mut read = query(&runtime, &input, QueryKind::Findings);
        read.limit = 1;
        let first = execute(&mut runtime, read.clone()).unwrap();
        read.cursor = Some(serde_json::from_value(first["next_cursor"].clone()).unwrap());
        let reference = |id: &str| CalculationJobRequest {
            scope: input.scope.clone(),
            model_id: "profile-test".into(),
            job_id: id.into(),
        };
        let start = |runtime: &mut Runtime, worker: &GridSession, id: &str| {
            let base = runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string();
            let mut request = refresh(runtime, &input, &base, worker);
            request.analyze = true;
            request.publication_id = Some(format!("publication-{id}"));
            runtime
                .execute(Request::StartCalculation(Box::new(
                    StartCalculationRequest {
                        job_id: id.into(),
                        request,
                    },
                )))
                .unwrap();
        };
        let wait = |runtime: &mut Runtime, id: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = runtime
                    .execute(Request::CalculationStatus(reference(id)))
                    .unwrap();
                if status["status"] != "running" {
                    return status;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        let admit = |runtime: &mut Runtime, id: &str| {
            runtime.execute(Request::AdmitCalculation(AdmitCalculationRequest {
                scope: input.scope.clone(),
                model_id: "profile-test".into(),
                job_id: id.into(),
                publication_id: Some(format!("publication-{id}")),
            }))
        };
        let (release, entered) = runtime.calculation_jobs.pause_next();
        start(&mut runtime, &worker, "discard");
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        execute(&mut runtime, read.clone()).unwrap(); // query leaves captured job context unchanged
        assert!(Arc::ptr_eq(
            &original,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        release.send(()).unwrap();
        assert_eq!(wait(&mut runtime, "discard")["status"], "ready");
        admit(&mut runtime, "discard").unwrap();
        let current = query(&runtime, &input, QueryKind::Findings);
        assert!(execute(&mut runtime, current)
            .unwrap_err()
            .starts_with("profile_publication_pending:"));
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: input.scope.clone(),
                model_id: "profile-test".into(),
                publication_id: "publication-discard".into(),
                target_history: worker.history_state(),
            }))
            .unwrap();
        assert!(Arc::ptr_eq(
            &original,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        execute(&mut runtime, read.clone()).unwrap();
        start(&mut runtime, &worker, "confirm");
        assert_eq!(wait(&mut runtime, "confirm")["status"], "ready");
        let candidate = admit(&mut runtime, "confirm").unwrap();
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: input.scope.clone(),
                publication: serde_json::from_value(candidate["publication"].clone()).unwrap(),
            }))
            .unwrap();
        assert!(execute(&mut runtime, read)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
        let (release, entered) = runtime.calculation_jobs.pause_next();
        start(&mut runtime, &worker, "obsolete");
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        describe(&mut worker, "during-analysis", "new authored head");
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        let replay = refresh(&runtime, &input, &base, &worker);
        runtime.execute(Request::Observe(Box::new(replay))).unwrap();
        release.send(()).unwrap();
        wait(&mut runtime, "obsolete");
        assert!(admit(&mut runtime, "obsolete")
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        assert!(Arc::ptr_eq(
            &index,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        let current = query(&runtime, &input, QueryKind::Findings);
        assert!(execute(&mut runtime, current)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
    }
    #[test]
    #[ignore = "measured model-analysis query gate; run alone with disk scratch"]
    fn complete_model_analysis_query_costs() {
        let snapshot = configured_fixture(2031);
        let input = request_for(snapshot.clone());
        let worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut analyze_times = Vec::new();
        let mut analyze_bytes = 0;
        for _ in 0..12 {
            let base = runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string();
            let mut analyze = refresh(&runtime, &input, &base, &worker);
            analyze.analyze = true;
            let mut wire = json!(analyze);
            wire["operation"] = json!("refresh");
            let started = std::time::Instant::now();
            let response = runtime
                .execute(serde_json::from_value::<Request>(wire).unwrap())
                .unwrap();
            analyze_bytes = serde_json::to_vec(&response).unwrap().len();
            analyze_times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        analyze_times.sort_by(f64::total_cmp);
        let mut measured = Vec::new();
        let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
            QueryKind::ViolatingSections,
            QueryKind::Blockers,
            QueryKind::Findings,
        ] {
            let mut times = Vec::new();
            let mut bytes = 0;
            let mut total = 0;
            for _ in 0..12 {
                let read = query(&runtime, &input, kind);
                let started = std::time::Instant::now();
                let response = execute(&mut runtime, read).unwrap();
                bytes = serde_json::to_vec(&response).unwrap().len();
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                total = response["total"].as_u64().unwrap();
                assert!(bytes <= MAX_BYTES);
            }
            times.sort_by(f64::total_cmp);
            measured.push(json!({"kind":kind,"median_ms":times[6],"minimum_ms":times[0],"maximum_ms":times[11],"returned_bytes":bytes,"total":total}));
        }
        assert!(Arc::ptr_eq(
            &index,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            work
        );
        println!(
            "PROFILE_MODEL_ANALYSIS_COST {}",
            json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},
            "structure_count":4,"strung_support_count":3,"section_count":1,"added_design_points":2031,"page_limit":50,
            "explicit_unchanged_analyze_median_ms":analyze_times[6],"explicit_analyze_result_bytes":analyze_bytes,
            "queries":measured,"includes":"wire DTO decode, native calculation/query, native result materialization and final JSON byte encoding",
            "excludes":"IPC, renderer decode/paint, installed artifact and dense connected-model qualification"})
        );
    }
}
