use super::*;
use crate::transactional_projection_tests::{
    describe, refresh, request_for, section_demand_fixture,
};
use std::time::Duration;

pub(super) fn prepare(runtime: &mut Runtime, worker: &GridSession) -> Value {
    runtime
        .execute(Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            expected_history: worker.history_state(),
        }))
        .unwrap()
}
pub(super) fn status(runtime: &mut Runtime, id: &str) -> Value {
    runtime
        .execute(Request::CalculationStatus(CalculationJobRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            job_id: id.into(),
        }))
        .unwrap()
}
pub(super) fn ready(runtime: &mut Runtime, id: &str) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let result = status(runtime, id);
        if result["status"] != "running" {
            assert_eq!(result["status"], "ready", "{result}");
            return;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub(super) fn start(
    runtime: &mut Runtime,
    input: &ProjectRequest,
    worker: &GridSession,
    signature: String,
    id: &str,
) -> Result<Value, String> {
    let base = runtime.held["one"]
        .session
        .current_revision()
        .revision_id
        .to_string();
    let mut request = refresh(runtime, input, &base, worker);
    request.publication_id = Some(id.into());
    runtime.execute(Request::StartDeltaCalculation(Box::new(
        StartDeltaCalculationRequest {
            base_signature: signature,
            calculation: StartCalculationRequest {
                job_id: id.into(),
                request,
            },
        },
    )))
}

#[test]
fn explicit_delta_jobs_preserve_complete_fresh_geometry_display_and_publication() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    let initial = runtime.execute(input.clone()).unwrap();
    let mut receiver =
        CalculatedSceneBase::new(serde_json::from_value(initial["scene"].clone()).unwrap());
    for (i, text) in ["A", "B", "A", "B"].into_iter().enumerate() {
        let revision = runtime.held["one"]
            .session
            .current_revision()
            .revision_id
            .to_string();
        describe(&mut worker, &format!("edit-{i}"), text);
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &revision, &worker,
            ))))
            .unwrap();
        let prepared = prepare(&mut runtime, &worker);
        assert_eq!(
            prepared["computed_revision"],
            receiver.scene().model_revision.as_str()
        );
        assert_eq!(prepared["metrics"]["calculated_cases"], 0);
        assert!(serde_json::to_vec(&prepared).unwrap().len() < 2048);
        let id = format!("delta-{i}");
        let ticket = start(
            &mut runtime,
            &input,
            &worker,
            prepared["base_signature"].as_str().unwrap().into(),
            &id,
        )
        .unwrap();
        assert!(serde_json::to_vec(&ticket).unwrap().len() < 2048);
        ready(&mut runtime, &id);
        let result = runtime
            .execute(Request::AdmitCalculation(AdmitCalculationRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
                job_id: id,
                publication_id: ticket["job_id"].as_str().map(str::to_string),
            }))
            .unwrap();
        assert!(result["scene"].is_null());
        assert!(result["scene_delta_json"].is_string());
        let delta = serde_json::from_str(result["scene_delta_json"].as_str().unwrap()).unwrap();
        receiver = receiver
            .admit(
                &delta,
                &worker.current_revision().revision_id,
                &worker.current_revision().roots.engineering_input_root,
            )
            .unwrap();
        let fresh = Runtime::default()
            .execute(request_for(worker.snapshot().clone()))
            .unwrap();
        let fresh_scene: ds_grid_engine::ProfileAtlasScene =
            serde_json::from_value(fresh["scene"].clone()).unwrap();
        assert_eq!(
            serde_json::to_vec(receiver.scene()).unwrap(),
            serde_json::to_vec(&fresh_scene).unwrap()
        );
        assert_eq!(result["display"], fresh["scene"]["display"]);
        let pin = serde_json::from_value(result["publication"].clone()).unwrap();
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: "one".into(),
                publication: pin,
            }))
            .unwrap();
        assert_eq!(
            prepare(&mut runtime, &worker)["base_signature"],
            receiver.signature()
        );
    }
    assert_eq!(worker.history_state().undo_depth, 4);
}

#[test]
fn missing_stale_foreign_and_pending_bases_refuse_without_starting_a_worker() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    assert!(
        start(
            &mut runtime,
            &input,
            &worker,
            format!("sha256:{}", "0".repeat(64)),
            "missing"
        )
        .unwrap_err()
        .starts_with("profile_calculated_delta_base_unavailable:")
    );
    let prepared = prepare(&mut runtime, &worker);
    assert!(
        start(
            &mut runtime,
            &input,
            &worker,
            format!("sha256:{}", "0".repeat(64)),
            "stale"
        )
        .unwrap_err()
        .starts_with("profile_calculated_delta_stale:")
    );
    for id in ["missing", "stale"] {
        assert!(
            runtime
                .execute(Request::CalculationStatus(CalculationJobRequest {
                    scope: "one".into(),
                    model_id: "profile-test".into(),
                    job_id: id.into(),
                }))
                .unwrap_err()
                .starts_with("profile_calculation_unavailable:")
        );
    }
    let mut history = worker.history_state();
    history.undo_depth += 1;
    assert!(
        runtime
            .execute(Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
                expected_history: history,
            }))
            .unwrap_err()
            .starts_with("profile_replay_stale:")
    );
    assert!(
        runtime
            .execute(Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
                scope: "one".into(),
                model_id: "other".into(),
                expected_history: worker.history_state(),
            }))
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:")
    );
    assert_eq!(
        prepare(&mut runtime, &worker)["base_signature"],
        prepared["base_signature"]
    );
}

#[test]
fn delta_jobs_allow_edits_and_reject_stale_results_without_losing_the_base() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let before = prepare(&mut runtime, &worker);
    let retained = runtime.held["one"].calculated_base.clone().unwrap();
    let (release, entered) = runtime.calculation_jobs.pause_next();
    start(
        &mut runtime,
        &input,
        &worker,
        retained.signature().into(),
        "paused",
    )
    .unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let revision = worker.current_revision().revision_id.to_string();
    describe(&mut worker, "during-delta", "still editable");
    let observed = runtime
        .execute(Request::Observe(Box::new(refresh(
            &runtime, &input, &revision, &worker,
        ))))
        .unwrap();
    assert!(observed["authored_delta"].is_object());
    assert_eq!(observed["ran"], false);
    assert_eq!(
        observed["calculation"]["computed_revision"],
        before["computed_revision"]
    );
    assert_eq!(status(&mut runtime, "paused")["status"], "stale");
    assert!(
        runtime
            .execute(Request::AdmitCalculation(AdmitCalculationRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
                job_id: "paused".into(),
                publication_id: Some("paused".into()),
            }))
            .unwrap_err()
            .starts_with("profile_calculation_stale:")
    );
    assert!(Arc::ptr_eq(
        &retained,
        runtime.held["one"].calculated_base.as_ref().unwrap()
    ));
    assert_eq!(
        prepare(&mut runtime, &worker)["base_signature"],
        before["base_signature"]
    );
    assert_eq!(
        runtime.held["one"].session.history_state(),
        worker.history_state()
    );
    release.send(()).unwrap();
}

#[test]
fn delta_publication_discard_restores_exact_base_and_keeps_authored_history() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let original = prepare(&mut runtime, &worker);
    let original_base = runtime.held["one"].calculated_base.clone().unwrap();
    let revision = worker.current_revision().revision_id.to_string();
    describe(&mut worker, "unpublished-edit", "B");
    runtime
        .execute(Request::Observe(Box::new(refresh(
            &runtime, &input, &revision, &worker,
        ))))
        .unwrap();
    start(
        &mut runtime,
        &input,
        &worker,
        original_base.signature().into(),
        "discard",
    )
    .unwrap();
    ready(&mut runtime, "discard");
    let result = runtime
        .execute(Request::AdmitCalculation(AdmitCalculationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            job_id: "discard".into(),
            publication_id: Some("discard".into()),
        }))
        .unwrap();
    assert!(result["scene_delta_json"].is_string());
    assert!(
        runtime
            .execute(Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
                expected_history: worker.history_state(),
            }))
            .unwrap_err()
            .starts_with("profile_publication_pending:")
    );
    assert!(start(
        &mut runtime,
        &input,
        &worker,
        serde_json::from_str::<Value>(result["scene_delta_json"].as_str().unwrap()).unwrap()
            ["result_signature"]
            .as_str()
            .unwrap()
            .into(),
        "pending"
    )
    .unwrap_err()
    .starts_with("profile_publication_pending:"));
    runtime
        .execute(Request::DiscardPublication(DiscardPublicationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            publication_id: "discard".into(),
            target_history: worker.history_state(),
        }))
        .unwrap();
    assert!(Arc::ptr_eq(
        &original_base,
        runtime.held["one"].calculated_base.as_ref().unwrap()
    ));
    assert_eq!(
        prepare(&mut runtime, &worker)["base_signature"],
        original["base_signature"]
    );
    assert_eq!(
        runtime.held["one"].session.history_state(),
        worker.history_state()
    );
    assert!(!runtime.held["one"].pending_receipts.is_empty());
    let mut foreign_input = input.clone();
    foreign_input.scope = "other".into();
    runtime.execute(foreign_input).unwrap();
    assert!(
        runtime
            .execute(Request::StartDeltaCalculation(Box::new(
                StartDeltaCalculationRequest {
                    base_signature: original_base.signature().into(),
                    calculation: StartCalculationRequest {
                        job_id: "foreign".into(),
                        request: {
                            let base = worker.current_revision().revision_id.to_string();
                            let mut r = refresh(&runtime, &input, &base, &worker);
                            r.scope = "other".into();
                            r.publication_id = Some("foreign".into());
                            r
                        }
                    },
                }
            )))
            .unwrap_err()
            .starts_with("profile_calculated_delta_base_unavailable:")
    );
    assert!(
        runtime
            .execute(Request::StartDeltaCalculation(Box::new(
                StartDeltaCalculationRequest {
                    base_signature: "sha256:bad".into(),
                    calculation: StartCalculationRequest {
                        job_id: "malformed".into(),
                        request: refresh(
                            &runtime,
                            &input,
                            &worker.current_revision().revision_id.to_string(),
                            &worker
                        )
                    },
                }
            )))
            .unwrap_err()
            .starts_with("invalid_profile_view:")
    );
    assert_eq!(
        prepare(&mut runtime, &worker)["base_signature"],
        original["base_signature"]
    );
}

#[test]
fn delta_requests_are_closed_and_case_selection_invalidates_prepared_base() {
    for wire in [
        json!({"operation":"calculated_scene_base","scope":"one","model_id":"profile-test","expected_history":null,"save":true}),
        json!({"operation":"start_delta_calculation","base_signature":"x","calculation":null,"fallback":true}),
    ] {
        assert!(serde_json::from_value::<Request>(wire).is_err());
    }
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    prepare(&mut runtime, &worker);
    runtime
        .execute(Request::CaseLayer(CaseRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            expected_revision: input.expected_revision.clone(),
            expected_history: worker.history_state(),
            display_case: Default::default(),
            select: true,
            cached_only: true,
            sequence: 1,
        }))
        .unwrap();
    assert!(
        runtime
            .execute(Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
                expected_history: worker.history_state(),
            }))
            .unwrap_err()
            .starts_with("profile_calculated_delta_base_unavailable:")
    );
    assert_eq!(worker.history_state().undo_depth, 0);
}

/// Profile contract §0: an edit lands whole. A scoped post-edit update that
/// still carries whole-model facts from an older revision is never presented
/// as the edit's result; the runtime keeps the worker's base and the host
/// reconciles, receiving the complete calculation as a delta from that base.
#[test]
fn an_observed_edit_lands_whole_through_reconcile_never_as_a_partial_update() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    let initial = runtime.execute(input.clone()).unwrap();
    let receiver =
        CalculatedSceneBase::new(serde_json::from_value(initial["scene"].clone()).unwrap());
    let section = worker.snapshot().tension_sections[0].clone();
    let sag = |tension_n: f64| ds_grid_engine::GridCommand::SetTensionSectionSagMode {
        id: section.id.clone(),
        sag: ds_grid_model::AuthoredSag::HorizontalTension { tension_n },
        sag_condition: section.sag_condition,
        sag_weather_state_id: section.sag_weather_state_id.clone(),
        criterion_set_id: section.criterion_set_id.clone(),
        ruling_span_m: section.ruling_span_m,
    };
    let base = runtime.held["one"]
        .session
        .current_revision()
        .revision_id
        .to_string();
    worker
        .apply_command(ds_grid_engine::CommandEnvelope::new(
            "sag-unsigned",
            worker.current_revision().revision_id.clone(),
            sag(6000.),
        ))
        .unwrap();
    // A host that names no worker base gets no calculated update.
    let unsigned = runtime
        .execute(Request::Observe(Box::new(refresh(
            &runtime, &input, &base, &worker,
        ))))
        .unwrap();
    assert!(unsigned["calculated_update"].is_null());
    assert_eq!(unsigned["calculation"]["required"], true);

    let base = runtime.held["one"]
        .session
        .current_revision()
        .revision_id
        .to_string();
    worker
        .apply_command(ds_grid_engine::CommandEnvelope::new(
            "sag-signed",
            worker.current_revision().revision_id.clone(),
            sag(7000.),
        ))
        .unwrap();
    let mut request = refresh(&runtime, &input, &base, &worker);
    request.calculated_base_signature = Some(receiver.signature().to_string());
    let result = runtime
        .execute(Request::Observe(Box::new(request)))
        .unwrap();
    let partial = &result["calculated_update"]["reconcile_required"];
    assert_eq!(partial["code"], "partial", "{result}");
    assert!(
        partial["section_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == section.id.as_str())
    );
    assert!(!partial["pending_reconcile"].as_array().unwrap().is_empty());
    assert!(result["calculated_update"]["scene_delta_json"].is_null());
    assert_eq!(result["calculation"]["required"], true);
    // The runtime still holds exactly the worker's base.
    let prepared = prepare(&mut runtime, &worker);
    assert_eq!(prepared["base_signature"], receiver.signature());

    // The host's reconcile lands the complete calculation of this head.
    let ticket = start(
        &mut runtime,
        &input,
        &worker,
        receiver.signature().to_string(),
        "reconcile",
    )
    .unwrap();
    ready(&mut runtime, "reconcile");
    let admitted = runtime
        .execute(Request::AdmitCalculation(AdmitCalculationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            job_id: "reconcile".into(),
            publication_id: ticket["job_id"].as_str().map(str::to_string),
        }))
        .unwrap();
    assert_eq!(admitted["calculation"]["required"], false);
    let delta = serde_json::from_str(admitted["scene_delta_json"].as_str().unwrap()).unwrap();
    let head = worker.current_revision();
    let whole = receiver
        .admit(
            &delta,
            &head.revision_id,
            &head.roots.engineering_input_root,
        )
        .unwrap();
    let fresh = Runtime::default()
        .execute(request_for(worker.snapshot().clone()))
        .unwrap();
    let fresh_scene: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_value(fresh["scene"].clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(whole.scene()).unwrap(),
        serde_json::to_vec(&fresh_scene).unwrap()
    );
}

/// A delta is only a cheaper transport. Without its exact base it becomes
/// the exact full scene; a calculation is never refused for its transport.
#[test]
fn a_delta_transfer_without_its_base_lands_the_exact_full_scene() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    let initial = runtime.execute(input).unwrap();
    prepare(&mut runtime, &worker);
    let scene: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_value(initial["scene"].clone()).unwrap();
    runtime
        .delta_transfers
        .insert("one".into(), format!("sha256:{}", "0".repeat(64)));
    let (wire, exact, delta, _) = runtime
        .transfer_calculated_scene("one", scene.clone())
        .unwrap();
    assert!(wire.is_null());
    assert!(delta.is_none());
    assert_eq!(exact.unwrap(), serde_json::to_string(&scene).unwrap());
    assert!(runtime.held["one"].calculated_base.is_none());
}
