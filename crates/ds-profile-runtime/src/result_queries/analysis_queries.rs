//! Complete model-bound Analyze facts. Construction is explicit calculation;
//! querying this immutable index never calls an engineering owner.
use super::*;
use ds_grid_engine::support_resistance::{SupportResistanceCheck, SupportResistanceOutcome};
use ds_grid_engine::{DefaultAnalysisFacts, DefaultAnalysisReport};

fn resistance_fact(check: &SupportResistanceCheck) -> Arc<Value> {
    let mut value = json!(check);
    value.as_object_mut().unwrap().remove("missing_inputs");
    value["missing_input_count"] = json!(check.missing_inputs.len());
    Arc::new(json!({"kind":"support_resistance","check":value}))
}

impl Index {
    fn empty_analysis(session: &GridSession, basis: Value) -> Self {
        let snapshot = session.snapshot();
        let revision = session.current_revision();
        let structures: BTreeSet<_> = snapshot
            .structures
            .iter()
            .map(|row| row.id.to_string())
            .collect();
        let sections: BTreeSet<_> = snapshot
            .tension_sections
            .iter()
            .map(|row| row.id.to_string())
            .collect();
        Self {
            id: Self::next_id(),
            revision: revision.revision_id.clone(),
            root: revision.roots.engineering_input_root.clone(),
            case: Default::default(),
            basis,
            source: QuerySource::ModelAnalysis,
            usage_available: false,
            findings_available: false,
            entities: structures
                .iter()
                .chain(&sections)
                .cloned()
                .chain(snapshot.alignments.iter().map(|row| row.id.to_string()))
                .chain(snapshot.terrain_points.iter().map(|row| row.id.to_string()))
                .collect(),
            structures,
            sections,
            facts: [
                QueryKind::Usage,
                QueryKind::Failures,
                QueryKind::NegativeLoading,
                QueryKind::ViolatingSections,
                QueryKind::Qualification,
                QueryKind::Blockers,
                QueryKind::Findings,
            ]
            .into_iter()
            .map(|kind| (kind, Facts::default()))
            .collect(),
        }
    }
    pub(crate) fn unavailable_analysis(
        session: &GridSession,
        refusal: Value,
        message: &str,
    ) -> Self {
        let mut index = Self::empty_analysis(
            session,
            json!({"source":"model_analysis",
            "model_analysis_available":false,"refusal":refusal,"count_unit":"native_fact"}),
        );
        index.blocker(None, "model_analysis_unavailable", Some(refusal), message);
        index
    }
    pub(crate) fn model_analysis(
        session: &GridSession,
        report: &DefaultAnalysisReport,
        facts: DefaultAnalysisFacts,
    ) -> Self {
        assert_eq!(
            report.model_revision,
            session.current_revision().revision_id
        );
        assert_eq!(
            report.engineering_input_root,
            session.current_revision().roots.engineering_input_root
        );
        let clearance = facts.clearance.as_ref();
        let basis = json!({"source":"model_analysis","model_analysis_available":true,
            "usage":{"criterion_set_id":report.criterion_set_id,"analysis_case_ids":report.analysis_case_ids,
                "weight_span_basis":report.weight_span_basis,"basis_origin":report.basis_origin},
            "usage_classification":report.structure_classification,"usage_summary":report.structure_summary,
            "usage_status_counts":report.structure_status_counts,"blocking_findings":report.blocking_findings,
            "support_resistance":{"classification":report.support_resistance_checks.classification,
                "pass_case_state_count":report.support_resistance_checks.pass_case_state_count,
                "fail_case_state_count":report.support_resistance_checks.fail_case_state_count,
                "unknown_case_state_count":report.support_resistance_checks.unknown_case_state_count},
            "clearance":clearance.map(|report| json!({"classification":report.classification,
                "verification_level":report.verification_level,"criterion_set_id":report.criterion_set_id,
                "evaluated_vertical_cases":report.evaluated_vertical_cases,"evaluated_horizontal_cases":report.evaluated_horizontal_cases,
                "finding_count":report.finding_count,"points_checked":report.points_checked,
                "points_unresolved":report.points_unresolved,"vertical_rule_point_count":report.vertical_rule_point_count,
                "horizontal_rule_point_count":report.horizontal_rule_point_count,
                "minimum_vertical_margin":report.minimum_vertical_margin,"minimum_horizontal_margin":report.minimum_horizontal_margin,
                "sections_evaluated":report.sections_evaluated,"sections_unavailable":report.sections_unavailable.len(),
                "warning_count":report.warnings.len()})),
            "clearance_unavailable":report.clearance_unavailable,
            "qualification_scope":"model_bound_structure_usage_and_support_resistance",
            "clearance_scope":"model_bound_design_cases","count_unit":"native_fact",
            "clearance_complete_for_checked_corridors":report.clearance.as_ref().map(|clearance| clearance.complete_for_checked_corridors),
            "design_clearance_section_scope":"native_governing_vertical_wire",
            "negative_loading_scope":"signed_section_weight_span_or_complete_net_support_uplift"});
        let mut index = Self::empty_analysis(session, basis);
        index.usage_available = facts.structures.len() == session.snapshot().structures.len();
        index.findings_available = clearance.is_some();
        let mut sections = BTreeMap::<String, Value>::new();
        for row in &facts.structures {
            let id = row.structure_id.to_string();
            let value = usage_fact(row);
            index.add(QueryKind::Usage, Some(id.clone()), value.clone());
            index.add(QueryKind::Qualification, Some(id.clone()), value.clone());
            if row.engineering_status == Some(EngineeringStatus::Fail) {
                index.add(QueryKind::Failures, Some(id.clone()), value.clone());
                for section in &row.failing_tension_section_ids {
                    add_section_contributor(
                        &mut sections,
                        &section.to_string(),
                        "capacity_failure_structure",
                        &id,
                    );
                }
            }
            if row.uplift_case_present || row.minimum_signed_weight_span_m.is_some_and(|v| v < 0.0)
            {
                index.add(QueryKind::NegativeLoading, Some(id.clone()), value);
            }
            for (kind, messages) in [
                ("structure_demand_or_capacity", &row.blockers),
                ("unevaluated_structure_check", &row.unevaluated_checks),
            ] {
                for message in messages {
                    index.blocker(Some(id.clone()), kind, None, message);
                }
            }
        }
        for check in &facts.support_resistance_checks {
            let id = check.structure_id.to_string();
            let value = resistance_fact(check);
            index.add(QueryKind::Qualification, Some(id.clone()), value.clone());
            if check.outcome == SupportResistanceOutcome::Fail {
                index.add(QueryKind::Failures, Some(id.clone()), value.clone());
            }
            if check.net_upward_force_n.is_some_and(|v| v > 0.0) {
                index.add(QueryKind::NegativeLoading, Some(id.clone()), value);
            }
            for message in &check.missing_inputs {
                let mut value = bounded_text(message);
                value["kind"] = json!("support_resistance_input");
                value["analysis_case_id"] = json!(check.analysis_case_id);
                value["wind_sign"] = json!(check.wind_sign);
                index.add(QueryKind::Blockers, Some(id.clone()), Arc::new(value));
            }
        }
        for row in &facts.demand_unavailable {
            let mut value = bounded_text(&row.reason);
            value["kind"] = json!("section_demand_unavailable");
            value["analysis_case_id"] = json!(row.analysis_case_id);
            index.add(
                QueryKind::Blockers,
                Some(row.tension_section_id.to_string()),
                Arc::new(value),
            );
        }
        if let Some(report) = clearance {
            let points: BTreeMap<_, _> = session
                .snapshot()
                .terrain_points
                .iter()
                .map(|row| (row.id.as_str(), row))
                .collect();
            for finding in &report.findings {
                let value = Arc::new(json!({"kind":"design_clearance_finding","finding":finding,
                    "geometry":points.get(finding.terrain_point_id.as_str()).map(|point| json!({"x_m":point.x_m,"y_m":point.y_m,"z_m":point.z_m}))}));
                let facts = index.facts.get_mut(&QueryKind::Findings).unwrap();
                let position = facts.rows.len();
                facts.insert(Some(finding.terrain_point_id.clone()), value);
                // Exact native references allow a selected section, alignment or
                // incident support to retrieve its point findings without a scan.
                for id in std::iter::once(Some(finding.alignment_id.as_str()))
                    .chain(std::iter::once(finding.section_id.as_deref()))
                    .chain(std::iter::once(
                        finding
                            .from_structure
                            .as_ref()
                            .map(|row| row.structure_id.as_str()),
                    ))
                    .chain(std::iter::once(
                        finding
                            .to_structure
                            .as_ref()
                            .map(|row| row.structure_id.as_str()),
                    ))
                    .flatten()
                {
                    facts.by_entity.entry(id.into()).or_default().push(position);
                }
                // The report's section_id identifies the vertical governing
                // wire. A horizontal-only failure cannot be assigned to it.
                if finding.vertical_violation {
                    if let Some(section) = &finding.section_id {
                        add_section_contributor(
                            &mut sections,
                            section,
                            "design_clearance_point",
                            &finding.terrain_point_id,
                        );
                    }
                }
            }
            for row in &report.sections_unavailable {
                let mut value = bounded_text(&row.reason);
                value["kind"] = json!("design_clearance_section_unavailable");
                value["case"] = json!(row.case);
                value["analysis_case_id"] = json!(row.analysis_case_id);
                index.add(
                    QueryKind::Blockers,
                    Some(row.section_id.clone()),
                    Arc::new(value),
                );
            }
            for warning in &report.warnings {
                let mut value = bounded_text(&warning.detail);
                value["kind"] = json!("design_clearance_warning");
                value["code"] = json!(warning.code);
                value["count"] = json!(warning.count);
                value["calculation_note"] = json!(warning.calculation_note);
                index.add(
                    QueryKind::Blockers,
                    warning
                        .calculation_note
                        .as_ref()
                        .map(|note| note.section_id.clone()),
                    Arc::new(value),
                );
            }
        }
        if let Some(unavailable) = &report.clearance_unavailable {
            index.blocker(
                None,
                "design_clearance_unavailable",
                Some(unavailable.refusal.clone()),
                &unavailable.message,
            );
        }
        for (id, value) in sections {
            index.add(QueryKind::ViolatingSections, Some(id), Arc::new(value));
        }
        index
    }
}

fn add_section_contributor(
    sections: &mut BTreeMap<String, Value>,
    section: &str,
    kind: &str,
    id: &str,
) {
    let value = sections
        .entry(section.into())
        .or_insert_with(|| json!({"kind":"violating_section","section_id":section}));
    let count_key = format!("{kind}_count");
    let ids_key = format!("{kind}_ids");
    let count = value[&count_key].as_u64().unwrap_or(0);
    value[&count_key] = json!(count + 1);
    if count == 0 {
        value[&ids_key] = json!([]);
    }
    if count < 50 {
        value[&ids_key].as_array_mut().unwrap().push(json!(id));
    }
    value[format!("{kind}_ids_truncated")] = json!(count >= 50);
}

#[cfg(test)]
mod tests {
    use super::super::tests::{execute, fixture, request};
    use super::*;
    use crate::transactional_projection_tests::{describe, refresh, request_for};
    use ds_grid_model::*;

    fn configured_fixture(point_count: usize) -> GridModelSnapshot {
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
    fn query(runtime: &Runtime, input: &ProjectRequest, kind: QueryKind) -> QueryResultsRequest {
        let mut query = request(runtime, &input.scope, kind);
        query.source = QuerySource::ModelAnalysis;
        query
    }
    fn native(
        runtime: &Runtime,
        input: &ProjectRequest,
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
    fn paged(runtime: &mut Runtime, input: &ProjectRequest, kind: QueryKind) -> Vec<Value> {
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
        runtime.execute(analyzed).unwrap();
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
            "native_governing_vertical_wire"
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
        runtime.execute(input.clone()).unwrap();
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
        for id in ["finding-0", "str-1", "detached"] {
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
            runtime.held[&input.scope].analysis_index.is_none(),
            "Profile-only refresh cannot renew old model-analysis facts"
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
