use super::super::tests::execute;
use super::tests::{configured_fixture, native, query};
use super::*;
use crate::transactional_projection_tests::{refresh, request_for};
use ds_grid_engine::{CommandEnvelope, GridCommand};
use ds_grid_model::*;

const SUPPORT_KINDS: [QueryKind; 5] = [
    QueryKind::Usage,
    QueryKind::Failures,
    QueryKind::NegativeLoading,
    QueryKind::Qualification,
    QueryKind::Blockers,
];

fn connected_fixture(points: usize) -> GridModelSnapshot {
    let mut snapshot = configured_fixture(points);
    snapshot.alignments[0].terrain_gap_tolerance_m = Some(1000.0);
    let mut support = snapshot.structures[2].clone();
    support.id = StructureId::new("after-end").unwrap();
    support.station_m = Some(450.0);
    support.x_m = 450.0;
    support.y_m = 20.0;
    snapshot.structures.push(support);
    let mut section = snapshot.tension_sections[0].clone();
    section.id = TensionSectionId::new("next-section").unwrap();
    snapshot.tension_sections.push(section);
    for (sequence, id) in ["str-3", "after-end"].into_iter().enumerate() {
        snapshot
            .tension_section_supports
            .push(TensionSectionSupportRow {
                section_id: TensionSectionId::new("next-section").unwrap(),
                sequence: sequence as u32,
                structure_id: StructureId::new(id).unwrap(),
                attachment_point_id: Some(snapshot.attachment_points[0].id.clone()),
                insulator_model: None,
            });
    }
    ds_grid_engine::recompute_stored_geometry(&mut snapshot);
    for (i, row) in snapshot.structures.iter_mut().enumerate() {
        row.engineering_number = Some((i + 1).to_string());
    }
    snapshot
}
fn apply(worker: &mut GridSession, id: &str, command: GridCommand) {
    worker
        .apply_command(CommandEnvelope::new(
            id,
            worker.current_revision().revision_id.clone(),
            command,
        ))
        .unwrap();
}
fn sag(worker: &mut GridSession, id: &str, section: &str, tension: f64) {
    let row = worker
        .snapshot()
        .tension_sections
        .iter()
        .find(|row| row.id.as_str() == section)
        .unwrap()
        .clone();
    apply(
        worker,
        id,
        GridCommand::SetTensionSectionSagMode {
            id: row.id,
            sag: AuthoredSag::HorizontalTension { tension_n: tension },
            sag_condition: row.sag_condition,
            sag_weather_state_id: row.sag_weather_state_id,
            criterion_set_id: row.criterion_set_id,
            ruling_span_m: row.ruling_span_m,
        },
    );
}
fn read(
    runtime: &mut Runtime,
    input: &ProjectRequest,
    kind: QueryKind,
    ids: &[&str],
) -> Result<Value, String> {
    let mut request = query(runtime, input, kind);
    request.entity_ids = ids.iter().map(|id| (*id).into()).collect();
    execute(runtime, request)
}
fn observe(runtime: &mut Runtime, input: &ProjectRequest, worker: &GridSession) {
    let base = runtime.held[&input.scope]
        .session
        .current_revision()
        .revision_id
        .to_string();
    let replay = refresh(runtime, input, &base, worker);
    runtime.execute(Request::Observe(Box::new(replay))).unwrap();
}
fn calculate(runtime: &mut Runtime, input: &ProjectRequest, worker: &GridSession, analyze: bool) {
    let base = runtime.held[&input.scope]
        .session
        .current_revision()
        .revision_id
        .to_string();
    let mut replay = refresh(runtime, input, &base, worker);
    replay.analyze = analyze;
    runtime.execute(replay).unwrap();
}
fn require_calculation(result: Result<Value, String>) {
    assert!(result
        .unwrap_err()
        .starts_with("profile_calculation_required:"));
}
fn assert_fresh_support_facts(runtime: &mut Runtime, input: &ProjectRequest, ids: &[&str]) {
    let (report, facts) = native(runtime, input);
    let fresh = Index::model_analysis(&runtime.held[&input.scope].session, &report, facts);
    let strings: Vec<_> = ids.iter().map(|id| (*id).into()).collect();
    let work = runtime.held[&input.scope].demand_cache.last_demand_work();
    let retained = runtime.held[&input.scope].analysis_index.clone().unwrap();
    let history = runtime.held[&input.scope].session.history_state();
    for kind in SUPPORT_KINDS {
        let result = read(runtime, input, kind, ids).unwrap_or_else(|error| panic!(
            "{kind:?} {ids:?}: {error}; invalidation {:?}, defaults {:?}, current defaults {:?}, available {}, changes {:?}",
            runtime.held[&input.scope].analysis_invalidation, retained.default_selection,
            ds_grid_engine::default_analysis::model_analysis_defaults_key(runtime.held[&input.scope].session.snapshot()),
            retained.usage_available, runtime.held[&input.scope].pending_receipts.iter().map(|receipt| &receipt.delta.changed_tables).collect::<Vec<_>>(),
        ));
        let source = &fresh.facts[&kind];
        let expected: Vec<_> = source
            .positions(&strings)
            .into_iter()
            .map(|position| {
                let fact = &source.rows[position];
                json!({"entity_id":fact.entity_id,"fact":fact.value.as_ref()})
            })
            .collect();
        let mut actual = result["items"].as_array().unwrap().clone();
        let mut cursor = result["next_cursor"].clone();
        while !cursor.is_null() {
            let mut request = query(runtime, input, kind);
            request.entity_ids = strings.clone();
            request.cursor = Some(serde_json::from_value(cursor).unwrap());
            let page = execute(runtime, request).unwrap();
            assert_eq!(page["metrics"]["calculated_cases"], 0);
            assert!(serde_json::to_vec(&page).unwrap().len() <= MAX_BYTES);
            actual.extend(page["items"].as_array().unwrap().iter().cloned());
            cursor = page["next_cursor"].clone();
        }
        assert_eq!(
            actual.len(),
            expected.len(),
            "fresh {kind:?} count at {ids:?}"
        );
        for (position, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(
                actual, expected,
                "fresh {kind:?} at {ids:?}, row {position}"
            );
        }
        assert_eq!(result["total"], expected.len());
        assert_eq!(result["metrics"]["calculated_cases"], 0);
        assert_eq!(result["freshness"], "unchanged_dependencies");
        assert_eq!(result["computed_revision"], json!(retained.revision));
        assert_eq!(result["computed_engineering_input_root"], retained.root);
        assert_ne!(
            result["engineering_input_root"],
            result["computed_engineering_input_root"]
        );
    }
    assert!(Arc::ptr_eq(
        &retained,
        runtime.held[&input.scope].analysis_index.as_ref().unwrap()
    ));
    assert_eq!(runtime.held[&input.scope].session.history_state(), history);
    assert_eq!(
        runtime.held[&input.scope].demand_cache.last_demand_work(),
        work
    );
}

#[test]
fn repeated_local_calculation_preserves_exact_analysis_facts_beyond_section_ends() {
    let snapshot = connected_fixture(3);
    let mut input = request_for(snapshot.clone());
    input.analyze = true;
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let original = runtime.held[&input.scope].analysis_index.clone().unwrap();
    let represented = read(&mut runtime, &input, QueryKind::Usage, &["after-end"]).unwrap();
    assert!(
        represented["items"][0]["fact"]["row"]["maximum_signed_weight_span_m"]
            .as_f64()
            .is_some(),
        "the neighboring section carries represented forces, not only unavailable facts"
    );
    for (i, tension) in [21_000., 23_000., 21_000., 23_000.].into_iter().enumerate() {
        sag(&mut worker, &format!("sag-{i}"), "query-section", tension);
        observe(&mut runtime, &input, &worker);
        assert_fresh_support_facts(&mut runtime, &input, &["after-end", "detached"]);
        // The shared endpoint needs fresh adjacent incident force assembly.
        for id in ["str-1", "str-2", "str-3"] {
            for kind in SUPPORT_KINDS {
                require_calculation(read(&mut runtime, &input, kind, &[id]));
            }
        }
        for kind in SUPPORT_KINDS {
            require_calculation(read(&mut runtime, &input, kind, &[]));
        }
        for (kind, id) in [
            (QueryKind::Findings, "finding-0"),
            (QueryKind::ViolatingSections, "next-section"),
            (QueryKind::Blockers, "next-section"),
        ] {
            require_calculation(read(&mut runtime, &input, kind, &[id]));
        }
        calculate(&mut runtime, &input, &worker, false);
        assert!(runtime.held[&input.scope].pending_receipts.is_empty());
        assert_fresh_support_facts(&mut runtime, &input, &["after-end", "detached"]);
        assert!(Arc::ptr_eq(
            &original,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        require_calculation(read(&mut runtime, &input, QueryKind::Usage, &["str-3"]));
    }
    // Independent Analyze is the only action that renews the analysis index.
    calculate(&mut runtime, &input, &worker, true);
    assert!(!Arc::ptr_eq(
        &original,
        runtime.held[&input.scope].analysis_index.as_ref().unwrap()
    ));
    assert_eq!(
        read(&mut runtime, &input, QueryKind::Usage, &[]).unwrap()["freshness"],
        "current_inputs"
    );
}

#[test]
fn analysis_invalidation_accumulates_across_profile_calculation_and_navigation() {
    let snapshot = connected_fixture(2);
    let mut input = request_for(snapshot.clone());
    input.analyze = true;
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    sag(&mut worker, "first", "query-section", 23_000.);
    // Direct explicit Refresh also admits characterized receipt lineage.
    calculate(&mut runtime, &input, &worker, false);
    assert_fresh_support_facts(&mut runtime, &input, &["after-end"]);
    sag(&mut worker, "next", "next-section", 24_000.);
    observe(&mut runtime, &input, &worker);
    calculate(&mut runtime, &input, &worker, false);
    require_calculation(read(&mut runtime, &input, QueryKind::Usage, &["after-end"]));
    assert_fresh_support_facts(&mut runtime, &input, &["detached"]);
    worker.undo().unwrap();
    observe(&mut runtime, &input, &worker);
    require_calculation(read(&mut runtime, &input, QueryKind::Usage, &["detached"]));
    worker.redo().unwrap();
    observe(&mut runtime, &input, &worker);
    require_calculation(read(&mut runtime, &input, QueryKind::Usage, &["detached"]));
    worker.undo().unwrap();
    observe(&mut runtime, &input, &worker);
    sag(&mut worker, "branch", "query-section", 25_000.);
    observe(&mut runtime, &input, &worker);
    calculate(&mut runtime, &input, &worker, false);
    require_calculation(read(&mut runtime, &input, QueryKind::Usage, &["detached"]));
    calculate(&mut runtime, &input, &worker, true);
    assert_eq!(
        read(&mut runtime, &input, QueryKind::Usage, &["detached"]).unwrap()["freshness"],
        "current_inputs"
    );
}

#[test]
fn terrain_weather_unavailable_and_untraced_package_changes_cannot_reuse_analysis_facts() {
    for change in ["terrain", "weather", "package", "unavailable", "criterion"] {
        let mut snapshot = connected_fixture(2);
        if change == "unavailable" {
            snapshot.criterion_sets.push(CriterionSetRow {
                id: CriterionSetId::new("other").unwrap(),
                label: "Ambiguous".into(),
            });
        }
        let mut input = request_for(snapshot.clone());
        input.analyze = true;
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        match change {
            "criterion" => {
                let row = worker.snapshot().tension_sections[0].clone();
                apply(
                    &mut worker,
                    "criterion",
                    GridCommand::SetTensionSectionSagMode {
                        id: row.id,
                        sag: row.sag,
                        sag_condition: row.sag_condition,
                        sag_weather_state_id: row.sag_weather_state_id,
                        criterion_set_id: None,
                        ruling_span_m: row.ruling_span_m,
                    },
                );
            }
            "terrain" => apply(
                &mut worker,
                "terrain",
                GridCommand::SetTerrainPointElevation {
                    id: TerrainPointId::new("finding-0").unwrap(),
                    elevation_m: 101.,
                },
            ),
            "weather" => {
                let mut row = worker.snapshot().weather_states[0].clone();
                row.temperature_c += 1.;
                apply(
                    &mut worker,
                    "weather",
                    GridCommand::UpdateWeatherState {
                        id: row.id.clone(),
                        row,
                    },
                );
            }
            _ => sag(&mut worker, "sag", "query-section", 25_000.),
        }
        if change == "package" {
            let mut untraced = request_for(worker.snapshot().clone());
            untraced.scope = input.scope.clone();
            runtime.execute(untraced).unwrap();
        } else {
            observe(&mut runtime, &input, &worker);
        }
        for kind in SUPPORT_KINDS {
            require_calculation(read(&mut runtime, &input, kind, &["after-end"]));
        }
    }
}

#[test]
fn local_structure_move_invalidates_incident_supports_without_renewing_unchanged_facts() {
    let snapshot = connected_fixture(2);
    let mut input = request_for(snapshot.clone());
    input.analyze = true;
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let row = worker.snapshot().structures[1].clone();
    apply(
        &mut worker,
        "move",
        GridCommand::MoveStructure {
            id: row.id,
            x_m: row.x_m - 1.,
            y_m: row.y_m,
            z_m: row.z_m + 2.,
            orientation_rad: row.orientation_rad,
        },
    );
    observe(&mut runtime, &input, &worker);
    assert_fresh_support_facts(&mut runtime, &input, &["after-end", "detached"]);
    require_calculation(read(
        &mut runtime,
        &input,
        QueryKind::Qualification,
        &["str-3"],
    ));
    let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
    calculate(&mut runtime, &input, &worker, false);
    assert_fresh_support_facts(&mut runtime, &input, &["after-end", "detached"]);
    assert!(Arc::ptr_eq(
        &index,
        runtime.held[&input.scope].analysis_index.as_ref().unwrap()
    ));
}

#[test]
fn profile_only_async_admission_and_discard_preserve_independent_analysis_fences() {
    use std::time::{Duration, Instant};
    let snapshot = connected_fixture(2);
    let mut input = request_for(snapshot.clone());
    input.analyze = true;
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    sag(&mut worker, "local", "query-section", 23_000.);
    observe(&mut runtime, &input, &worker);
    let original = runtime.held[&input.scope].analysis_index.clone().unwrap();
    for (job_id, confirm) in [("retained-discard", false), ("retained-confirm", true)] {
        let control = |request| CalculationControlRequest {
            scope: input.scope.clone(),
            model_id: "profile-test".into(),
            expected_history: Some(worker.history_state()),
            request,
        };
        runtime
            .execute(control(CalculationArguments::Start {
                job_id: job_id.into(),
                display_case: None,
                analyze: false,
            }))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = runtime
                .execute(control(CalculationArguments::Status {
                    job_id: job_id.into(),
                }))
                .unwrap();
            if status["status"] != "running" {
                assert_eq!(status["status"], "ready");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let projected = runtime
            .execute(control(CalculationArguments::Admit {
                job_id: job_id.into(),
            }))
            .unwrap();
        assert!(read(&mut runtime, &input, QueryKind::Usage, &["after-end"])
            .unwrap_err()
            .starts_with("profile_publication_pending:"));
        if confirm {
            runtime
                .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                    scope: input.scope.clone(),
                    publication: serde_json::from_value(projected["publication"].clone()).unwrap(),
                }))
                .unwrap();
        } else {
            runtime
                .execute(Request::DiscardPublication(DiscardPublicationRequest {
                    scope: input.scope.clone(),
                    model_id: "profile-test".into(),
                    publication_id: job_id.into(),
                    target_history: worker.history_state(),
                }))
                .unwrap();
        }
        assert!(Arc::ptr_eq(
            &original,
            runtime.held[&input.scope].analysis_index.as_ref().unwrap()
        ));
        assert_fresh_support_facts(&mut runtime, &input, &["after-end"]);
        require_calculation(read(&mut runtime, &input, QueryKind::Usage, &["str-3"]));
    }
}

#[test]
fn explicit_span_edits_missing_lineage_and_changed_defaults_cannot_borrow_derived_span_fences() {
    let mut worker = GridSession::open(connected_fixture(2));
    let row = worker.snapshot().tension_sections[0].clone();
    let receipt = worker
        .apply_command(CommandEnvelope::new(
            "receipt",
            worker.current_revision().revision_id.clone(),
            GridCommand::SetTensionSectionSagMode {
                id: row.id,
                sag: AuthoredSag::HorizontalTension { tension_n: 23_000. },
                sag_condition: row.sag_condition,
                sag_weather_state_id: row.sag_weather_state_id,
                criterion_set_id: row.criterion_set_id,
                ruling_span_m: row.ruling_span_m,
            },
        ))
        .unwrap();
    let ids = vec!["after-end".into()];
    let structures = worker
        .snapshot()
        .structures
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    let mut allowed = crate::analysis_retention::Invalidation::default();
    allowed.advance(false, std::slice::from_ref(&receipt), true);
    assert!(allowed.permits_supports(&ids, &structures));
    let mut authored_span = receipt.clone();
    authored_span.delta.changed_tables.push(TableKind::Spans);
    authored_span
        .delta
        .declared_write_tables
        .push(TableKind::Spans);
    for (reconcile, receipts, defaults) in [
        (false, vec![authored_span], true),
        (false, vec![], true),
        (true, vec![receipt.clone()], true),
        (false, vec![receipt], false),
    ] {
        let mut refused = crate::analysis_retention::Invalidation::default();
        refused.advance(reconcile, &receipts, defaults);
        assert!(!refused.permits_supports(&ids, &structures));
    }
}

#[test]
#[ignore = "native retained-analysis cost gate; run alone with disk scratch"]
fn retained_analysis_support_query_costs_after_local_calculation() {
    let mut snapshot = connected_fixture(10_004);
    let detached = snapshot
        .structures
        .iter()
        .find(|row| row.id.as_str() == "detached")
        .unwrap()
        .clone();
    let mut selected = Vec::new();
    for i in 0..500 {
        let mut row = detached.clone();
        row.id = StructureId::new(format!("retained-support-{i}")).unwrap();
        row.engineering_number = Some((1000 + i).to_string());
        snapshot.structures.push(row);
        if i < 50 {
            selected.push(format!("retained-support-{i}"));
        }
    }
    ds_grid_engine::recompute_stored_geometry(&mut snapshot);
    let structures = snapshot.structures.len();
    let terrain = snapshot.terrain_points.len();
    let mut input = request_for(snapshot.clone());
    input.analyze = true;
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let index = runtime.held[&input.scope].analysis_index.clone().unwrap();
    let fact_count: usize = index.facts.values().map(|facts| facts.rows.len()).sum();
    let mut observations = Vec::new();
    let mut calculations = Vec::new();
    let mut applies = Vec::new();
    let mut calculated_bytes = 0;
    for i in 0..12 {
        let started = std::time::Instant::now();
        sag(
            &mut worker,
            &format!("cost-{i}"),
            "query-section",
            if i % 2 == 0 { 21_000. } else { 23_000. },
        );
        applies.push(started.elapsed().as_secs_f64() * 1000.);
        for calculate in [false, true] {
            let base = runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string();
            let request = refresh(&runtime, &input, &base, &worker);
            let mut wire = json!(request);
            wire["operation"] = json!(if calculate { "refresh" } else { "observe" });
            let wire = serde_json::to_vec(&wire).unwrap();
            let started = std::time::Instant::now();
            let result = runtime
                .execute(serde_json::from_slice::<Request>(&wire).unwrap())
                .unwrap();
            let bytes = serde_json::to_vec(&result).unwrap().len();
            let ms = started.elapsed().as_secs_f64() * 1000.;
            if calculate {
                calculations.push(ms);
                calculated_bytes = bytes;
            } else {
                observations.push(ms);
            }
            assert!(Arc::ptr_eq(
                &index,
                runtime.held[&input.scope].analysis_index.as_ref().unwrap()
            ));
        }
    }
    let ids: Vec<_> = selected.iter().map(String::as_str).collect();
    assert_fresh_support_facts(&mut runtime, &input, &ids[..3]);
    let mut queries = Vec::new();
    for kind in SUPPORT_KINDS {
        let mut times = Vec::new();
        let mut bytes = 0;
        let mut total = 0;
        for _ in 0..12 {
            let mut request = query(&runtime, &input, kind);
            request.entity_ids = selected.clone();
            let wire =
                serde_json::to_vec(&json!({"operation":"query_results","scope":request.scope,
                "model_id":request.model_id,"expected_history":request.expected_history,
                "expected_engineering_input_root":request.expected_engineering_input_root,
                "kind":request.kind,"source":request.source,"entity_ids":request.entity_ids,
                "limit":request.limit}))
                .unwrap();
            let started = std::time::Instant::now();
            let result = runtime
                .execute(serde_json::from_slice::<Request>(&wire).unwrap())
                .unwrap();
            bytes = serde_json::to_vec(&result).unwrap().len();
            total = result["total"].as_u64().unwrap();
            times.push(started.elapsed().as_secs_f64() * 1000.);
            assert_eq!(result["freshness"], "unchanged_dependencies");
            assert!(bytes <= MAX_BYTES);
        }
        times.sort_by(f64::total_cmp);
        queries.push(json!({"kind":kind,"median_ms":times[6],"result_bytes":bytes,"total_selected_facts":total}));
    }
    applies.sort_by(f64::total_cmp);
    observations.sort_by(f64::total_cmp);
    calculations.sort_by(f64::total_cmp);
    println!(
        "PROFILE_ANALYSIS_RETENTION_COST {}",
        json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},
        "structure_count":structures,"terrain_point_count":terrain,"section_count":2,"edits":12,
        "native_apply_median_ms":applies[6],"observe_wire_median_ms":observations[6],
        "profile_only_calculation_wire_median_ms":calculations[6],"calculated_result_bytes":calculated_bytes,
        "retained_analysis_fact_count":fact_count,"analysis_index_rebuilt":0,"selected_support_count":50,"queries":queries,
        "includes":"native apply separately; Observe/calculation DTO decode, native work and result JSON encode; query DTO decode and bounded result encode",
        "excludes":"journal capture before DTO decode, async capture/scheduling, IPC, worker picking, renderer paint and installed acceptance"})
    );
}
