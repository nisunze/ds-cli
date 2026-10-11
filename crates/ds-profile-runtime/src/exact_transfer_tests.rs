use super::tests::{prepare, ready};
use super::*;
use crate::transactional_projection_tests::{
    describe, refresh, request_for, section_demand_fixture,
};
use ds_grid_engine::profile_calculated_delta::CalculatedSceneBase;

fn control(worker: &GridSession, request: CalculationArguments) -> CalculationControlRequest {
    CalculationControlRequest {
        scope: "one".into(),
        model_id: "profile-test".into(),
        expected_history: Some(worker.history_state()),
        request,
    }
}

#[test]
fn present_retains_the_pinned_base_and_discard_restores_exact_geometry_and_display() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let prepared = prepare(&mut runtime, &worker);
    let before = runtime.held["one"].calculated_base.clone().unwrap();
    let bytes = serde_json::to_vec(before.scene()).unwrap();
    let authored = serde_json::to_vec(worker.snapshot()).unwrap();
    let state = runtime.views["one"].clone();
    let case_work = runtime.held["one"].review.case_work();
    let mut request = refresh(&runtime, &input, &input.expected_revision, &worker);
    request.calculated_base_signature = Some(before.signature().into());
    request.publication_id = Some("present-pinned".into());
    request.patch.visibility.insert(
        ds_command_kernel::profile_display::DisplayOption::Wire,
        false,
    );
    let result = runtime
        .execute(Request::Present(Box::new(request)))
        .unwrap();
    assert!(result["scene"].is_null());
    assert!(result["scene_json"].is_null());
    let delta = serde_json::from_str(result["scene_delta_json"].as_str().unwrap()).unwrap();
    let receiver = before
        .admit(
            &delta,
            &worker.current_revision().revision_id,
            &worker.current_revision().roots.engineering_input_root,
        )
        .unwrap();
    assert_eq!(serde_json::to_vec(receiver.scene()).unwrap(), bytes);
    assert_eq!(receiver.signature(), before.signature());
    assert_eq!(result["ran"], false);
    assert_eq!(result["calculation"]["required"], false);
    assert_eq!(result["history"], json!(worker.history_state()));
    assert_eq!(
        serde_json::to_value(runtime.held["one"].review.case_work()).unwrap(),
        serde_json::to_value(case_work).unwrap()
    );
    assert_eq!(
        runtime.held["one"].session.history_state(),
        worker.history_state()
    );
    assert_eq!(
        serde_json::to_vec(runtime.held["one"].session.snapshot()).unwrap(),
        authored
    );
    assert_eq!(
        runtime.held["one"]
            .calculated_base
            .as_ref()
            .unwrap()
            .signature(),
        before.signature()
    );
    assert_eq!(result["metrics"]["calculated_delta"], 1);
    assert_eq!(result["metrics"]["exact_scene_bytes"], 0);
    assert!(runtime.delta_transfers.is_empty());
    assert!(runtime.exact_transfers.is_empty());
    assert_ne!(runtime.views["one"], state);
    runtime
        .execute(Request::DiscardPublication(DiscardPublicationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            publication_id: "present-pinned".into(),
            target_history: worker.history_state(),
        }))
        .unwrap();
    assert_eq!(runtime.views["one"], state);
    assert!(Arc::ptr_eq(
        runtime.held["one"].calculated_base.as_ref().unwrap(),
        &before
    ));
    let restored = prepare(&mut runtime, &worker);
    assert_eq!(restored["base_signature"], prepared["base_signature"]);
    assert_eq!(restored["metrics"]["base_rebuilt"], 0);
    assert_eq!(restored["metrics"]["calculated_cases"], 0);
}

#[test]
fn present_without_a_shared_pin_returns_exact_native_signed_zero_text() {
    let mut input = request_for(section_demand_fixture());
    let mut terrain = DisplayState::default().terrain;
    terrain.corridor_half_width_m = -0.0;
    input.patch.terrain = Some(terrain);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let worker = runtime.held["one"].session.clone();
    let original = runtime.held["one"].calculated_scene.clone().unwrap();
    assert_eq!(
        original.metadata.terrain_corridor_half_width_m.to_bits(),
        (-0.0_f64).to_bits()
    );
    let request = refresh(&runtime, &input, &input.expected_revision, &worker);
    let result = runtime
        .execute(Request::Present(Box::new(request)))
        .unwrap();
    assert!(result["scene"].is_null());
    assert!(result["scene_delta_json"].is_null());
    let envelope = serde_json::to_string(&result).unwrap();
    let decoded: Value = serde_json::from_str(&envelope).unwrap();
    let scene: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_str(decoded["scene_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        scene.metadata.terrain_corridor_half_width_m.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        serde_json::to_vec(&scene).unwrap(),
        serde_json::to_vec(original.as_ref()).unwrap()
    );
    assert_eq!(result["ran"], false);
    assert_eq!(result["calculation"]["required"], false);
    assert_eq!(result["history"], json!(worker.history_state()));
    assert_eq!(
        result["metrics"]["exact_scene_bytes"],
        decoded["scene_json"].as_str().unwrap().len()
    );
    assert!(runtime.delta_transfers.is_empty());
    assert!(runtime.exact_transfers.is_empty());
    assert_eq!(
        prepare(&mut runtime, &worker)["base_signature"],
        CalculatedSceneBase::new(scene).signature()
    );
}

#[test]
fn present_transport_refusals_preserve_native_state_and_leave_no_transfer_flags() {
    for (failure, expected) in [
        ("malformed", "invalid_profile_view:"),
        ("stale_signature", "profile_calculated_delta_stale:"),
        ("foreign_model", "profile_replay_model_mismatch:"),
        ("target_history", "profile_scene_unavailable:"),
        ("base_history", "profile_replay_stale:"),
        ("styles", "profile_styles_unavailable:"),
        ("missing_base", "profile_calculated_delta_base_unavailable:"),
    ] {
        let input = request_for(section_demand_fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let worker = runtime.held["one"].session.clone();
        let prepared = prepare(&mut runtime, &worker);
        let mut request = refresh(&runtime, &input, &input.expected_revision, &worker);
        request.calculated_base_signature =
            Some(prepared["base_signature"].as_str().unwrap().into());
        request.publication_id = Some(format!("refused-{failure}"));
        match failure {
            "malformed" => request.calculated_base_signature = Some("sha256:bad".into()),
            "stale_signature" => {
                request.calculated_base_signature = Some(format!("sha256:{}", "0".repeat(64)))
            }
            "foreign_model" => request.model_id = "another-model".into(),
            "target_history" => request.target_history.undo_depth += 1,
            "base_history" => request.base_history.history_pin = "another-history".into(),
            "styles" => request.style_document = json!({}),
            "missing_base" => runtime.held.get_mut("one").unwrap().calculated_base = None,
            _ => unreachable!(),
        }
        let before = runtime.held["one"].clone();
        let state = runtime.views["one"].clone();
        let error = runtime
            .execute(Request::Present(Box::new(request)))
            .unwrap_err();
        assert!(error.starts_with(expected), "{failure}: {error}");
        assert_eq!(runtime.views["one"], state, "{failure}");
        assert_eq!(
            runtime.held["one"].session.history_state(),
            before.session.history_state(),
            "{failure}"
        );
        assert_eq!(
            runtime.held["one"].session.snapshot(),
            before.session.snapshot(),
            "{failure}"
        );
        assert_eq!(
            runtime.held["one"]
                .calculated_base
                .as_ref()
                .map(|base| base.signature()),
            before.calculated_base.as_ref().map(|base| base.signature()),
            "{failure}"
        );
        assert!(runtime.publications.is_empty(), "{failure}");
        assert!(runtime.delta_transfers.is_empty(), "{failure}");
        assert!(runtime.exact_transfers.is_empty(), "{failure}");
        let recovered = runtime
            .execute(Request::Present(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        assert!(recovered["scene_json"].is_string(), "{failure}");
        assert!(recovered["scene_delta_json"].is_null(), "{failure}");
        assert_eq!(
            recovered["history"],
            json!(worker.history_state()),
            "{failure}"
        );
    }
}

#[test]
fn failed_candidate_discard_preserves_previous_confirmed_publication_and_original_error() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    runtime.execute(input.clone()).unwrap();
    let mut confirmed = refresh(&runtime, &input, &input.expected_revision, &worker);
    confirmed.publication_id = Some("previous-confirmed".into());
    let result = runtime
        .execute(Request::Refresh(Box::new(confirmed)))
        .unwrap();
    let pin: PublicationPin = serde_json::from_value(result["publication"].clone()).unwrap();
    runtime
        .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
            scope: "one".into(),
            publication: pin.clone(),
        }))
        .unwrap();
    let previous_scene = serde_json::to_value(runtime.held["one"].review.base_scene()).unwrap();
    let previous_state = runtime.views["one"].clone();
    let previous_history = worker.history_state();
    describe(
        &mut worker,
        "applied-before-failure",
        "authored edit remains committed",
    );
    assert_ne!(worker.history_state(), previous_history);
    let mut failed = refresh(&runtime, &input, &input.expected_revision, &worker);
    failed.publication_id = Some("failed-candidate".into());
    failed.style_document = json!({});
    let error = runtime
        .execute(Request::Refresh(Box::new(failed)))
        .unwrap_err();
    assert!(error.starts_with("profile_styles_unavailable:"), "{error}");
    let discard = DiscardPublicationRequest {
        scope: "one".into(),
        model_id: "profile-test".into(),
        publication_id: "failed-candidate".into(),
        target_history: worker.history_state(),
    };
    assert!(runtime
        .execute(Request::DiscardPublication(discard.clone()))
        .unwrap_err()
        .starts_with("profile_publication_unavailable:"));
    assert_eq!(runtime.publications["one"].pin, pin);
    assert!(runtime.publications["one"].confirmed);
    assert_eq!(
        runtime.held["one"].session.history_state(),
        previous_history
    );
    assert_eq!(
        serde_json::to_value(runtime.held["one"].review.base_scene()).unwrap(),
        previous_scene
    );
    assert_eq!(runtime.views["one"], previous_state);
    let mut foreign = discard;
    foreign.model_id = "another-model".into();
    assert!(runtime
        .execute(Request::DiscardPublication(foreign))
        .unwrap_err()
        .starts_with("profile_publication_stale:"));

    let mut recovery = refresh(&runtime, &input, &input.expected_revision, &worker);
    recovery.publication_id = Some("recovery".into());
    let recovered = runtime
        .execute(Request::Refresh(Box::new(recovery)))
        .unwrap();
    let recovery_pin: PublicationPin =
        serde_json::from_value(recovered["publication"].clone()).unwrap();
    // A different candidate may never discard an outstanding unconfirmed result.
    assert!(runtime
        .execute(Request::DiscardPublication(DiscardPublicationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            publication_id: "foreign-candidate".into(),
            target_history: worker.history_state(),
        }))
        .unwrap_err()
        .starts_with("profile_publication_stale:"));
    runtime
        .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
            scope: "one".into(),
            publication: recovery_pin,
        }))
        .unwrap();
    assert_eq!(
        runtime.held["one"].session.history_state(),
        worker.history_state()
    );
}

#[test]
fn exact_initialization_is_transactional_and_preserves_native_publication() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let worker = GridSession::open(snapshot);
    let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
        &worker,
        ds_grid_model::EntityId::new("profile-test").unwrap(),
        Default::default(),
        &[],
    );
    let request = InitializeRequest {
        scope: "one".into(),
        publication_id: Some("exact-initialize".into()),
        seed_json: serde_json::to_string(&seed).unwrap(),
        restore_state: None,
        options: input.options.clone(),
        style_document: input.style_document.clone(),
        patch: Default::default(),
        analyze: false,
    };
    let mut runtime = Runtime::default();
    let mut invalid = request.clone();
    invalid.seed_json = "invalid".into();
    assert!(runtime
        .execute(Request::InitializeExact(Box::new(invalid)))
        .is_err());
    assert!(runtime.held.is_empty());
    assert!(runtime.exact_transfers.is_empty());
    let result = runtime
        .execute(Request::InitializeExact(Box::new(request.clone())))
        .unwrap();
    assert!(result["scene"].is_null());
    assert!(runtime.exact_transfers.is_empty());
    let exact: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_str(result["scene_json"].as_str().unwrap()).unwrap();
    let fresh = Runtime::default().execute(request).unwrap();
    let fresh_scene: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_value(fresh["scene"].clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&exact).unwrap(),
        serde_json::to_vec(&fresh_scene).unwrap()
    );
    assert_eq!(result["display"], fresh["scene"]["display"]);
    assert_eq!(
        runtime.held["one"].session.history_state(),
        worker.history_state()
    );
    let pin: PublicationPin = serde_json::from_value(result["publication"].clone()).unwrap();
    assert!(runtime
        .execute(control(&worker, CalculationArguments::PrepareSceneBase {}))
        .unwrap_err()
        .starts_with("profile_publication_pending:"));
    runtime
        .execute(Request::DiscardPublication(DiscardPublicationRequest {
            scope: "one".into(),
            model_id: pin.model_id,
            publication_id: pin.publication_id,
            target_history: pin.history,
        }))
        .unwrap();
    assert!(runtime.held.is_empty());
    assert!(runtime.publications.is_empty());
}

#[test]
fn shared_controls_prepare_then_exact_full_and_delta_jobs_preserve_fresh_geometry() {
    let snapshot = section_demand_fixture();
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut runtime = Runtime::default();
    let initial = runtime.execute(input.clone()).unwrap();
    let job = "exact-full";
    runtime
        .execute(control(
            &worker,
            CalculationArguments::Start {
                job_id: job.into(),
                display_case: None,
                analyze: false,
                scene_transfer: CalculationTransfer::ExactJson {},
            },
        ))
        .unwrap();
    ready(&mut runtime, job);
    let result = runtime
        .execute(control(
            &worker,
            CalculationArguments::Admit { job_id: job.into() },
        ))
        .unwrap();
    assert!(result["scene"].is_null());
    assert!(result["scene_delta_json"].is_null());
    let typed: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_str(result["scene_json"].as_str().unwrap()).unwrap();
    let initial_typed: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_value(initial["scene"].clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&typed).unwrap(),
        serde_json::to_vec(&initial_typed).unwrap()
    );
    assert_eq!(result["display"], initial["scene"]["display"]);
    runtime
        .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
            scope: "one".into(),
            publication: serde_json::from_value(result["publication"].clone()).unwrap(),
        }))
        .unwrap();
    let mut receiver = CalculatedSceneBase::new(typed);
    for i in 0..4 {
        let revision = worker.current_revision().revision_id.to_string();
        describe(
            &mut worker,
            &format!("public-delta-{i}"),
            if i % 2 == 0 { "A" } else { "B" },
        );
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &revision, &worker,
            ))))
            .unwrap();
        let context = runtime.calculation_contexts["one"].clone();
        let base = runtime
            .execute(control(&worker, CalculationArguments::PrepareSceneBase {}))
            .unwrap();
        assert!(Arc::ptr_eq(&context, &runtime.calculation_contexts["one"]));
        assert_eq!(
            base["computed_revision"],
            receiver.scene().model_revision.as_str()
        );
        assert!(serde_json::to_vec(&base).unwrap().len() < 2048);
        let job = format!("public-delta-{i}");
        runtime
            .execute(control(
                &worker,
                CalculationArguments::Start {
                    job_id: job.clone(),
                    display_case: None,
                    analyze: false,
                    scene_transfer: CalculationTransfer::Delta {
                        base_signature: base["base_signature"].as_str().unwrap().into(),
                    },
                },
            ))
            .unwrap();
        ready(&mut runtime, &job);
        let result = runtime
            .execute(control(
                &worker,
                CalculationArguments::Admit { job_id: job },
            ))
            .unwrap();
        assert!(result["scene"].is_null());
        assert!(result["scene_json"].is_null());
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
        let fresh_typed: ds_grid_engine::ProfileAtlasScene =
            serde_json::from_value(fresh["scene"].clone()).unwrap();
        assert_eq!(
            serde_json::to_vec(receiver.scene()).unwrap(),
            serde_json::to_vec(&fresh_typed).unwrap()
        );
        assert_eq!(result["display"], fresh["scene"]["display"]);
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: "one".into(),
                publication: serde_json::from_value(result["publication"].clone()).unwrap(),
            }))
            .unwrap();
    }
    assert_eq!(worker.history_state().undo_depth, 4);
}

#[test]
fn exact_scene_and_delta_bodies_preserve_signed_zero_through_envelope_encoding() {
    let mut runtime = Runtime::default();
    let input = request_for(section_demand_fixture());
    runtime.execute(input).unwrap();
    let worker = runtime.held["one"].session.clone();
    let mut negative = runtime.held["one"]
        .calculated_scene
        .as_ref()
        .unwrap()
        .as_ref()
        .clone();
    negative.domain_min_m = -0.0;
    runtime.exact_transfers.insert("one".into());
    let (wire, exact, delta, _) = runtime.transfer_calculated_scene("one", negative).unwrap();
    assert!(wire.is_null());
    assert!(delta.is_none());
    let envelope = serde_json::to_string(&json!({"scene_json":exact})).unwrap();
    let decoded: Value = serde_json::from_str(&envelope).unwrap();
    let typed: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_str(decoded["scene_json"].as_str().unwrap()).unwrap();
    assert!(typed.domain_min_m.is_sign_negative());
    let receiver = CalculatedSceneBase::new(typed);
    let base = prepare(&mut runtime, &worker);
    assert_eq!(base["base_signature"], receiver.signature());
    let mut positive = receiver.scene().clone();
    positive.domain_min_m = 0.0;
    runtime
        .delta_transfers
        .insert("one".into(), receiver.signature().into());
    let (_, exact, delta, _) = runtime.transfer_calculated_scene("one", positive).unwrap();
    assert!(exact.is_none());
    let envelope = serde_json::to_string(&json!({"scene_delta_json":delta})).unwrap();
    let decoded: Value = serde_json::from_str(&envelope).unwrap();
    let delta = serde_json::from_str(decoded["scene_delta_json"].as_str().unwrap()).unwrap();
    let admitted = receiver
        .admit(
            &delta,
            &worker.current_revision().revision_id,
            &worker.current_revision().roots.engineering_input_root,
        )
        .unwrap();
    assert!(!admitted.scene().domain_min_m.is_sign_negative());
}

#[test]
fn missing_history_unprepared_base_and_malformed_transfer_controls_refuse() {
    for raw in [
        r#"{"action":"prepare_scene_base","job_id":"unwanted"}"#,
        r#"{"action":"start","job_id":"a","scene_transfer":{"mode":"delta","base_signature":null}}"#,
        r#"{"action":"start","job_id":"a","scene_transfer":{"mode":"exact_json","fallback":true}}"#,
        r#"{"action":"start","job_id":"a","scene_transfer":{"mode":"unrecognized"}}"#,
    ] {
        assert!(
            serde_json::from_str::<CalculationArguments>(raw).is_err(),
            "{raw}"
        );
    }
    let invalid: CalculationArguments =
        serde_json::from_value(json!({"action":"start","job_id":"a",
        "scene_transfer":{"mode":"delta","base_signature":"sha256:bad"}}))
        .unwrap();
    assert!(invalid.validate().is_err());
    let mut runtime = Runtime::default();
    let input = request_for(section_demand_fixture());
    runtime.execute(input).unwrap();
    let worker = runtime.held["one"].session.clone();
    let mut missing = control(&worker, CalculationArguments::PrepareSceneBase {});
    missing.expected_history = None;
    assert!(runtime
        .execute(missing)
        .unwrap_err()
        .starts_with("profile_replay_stale:"));
    assert!(runtime
        .execute(control(
            &worker,
            CalculationArguments::Start {
                job_id: "unprepared".into(),
                display_case: None,
                analyze: false,
                scene_transfer: CalculationTransfer::Delta {
                    base_signature: format!("sha256:{}", "0".repeat(64))
                },
            }
        ))
        .unwrap_err()
        .starts_with("profile_calculated_delta_base_unavailable:"));
    assert_eq!(
        runtime.held["one"].session.history_state(),
        worker.history_state()
    );
    assert_eq!(worker.history_state().undo_depth, 0);
}
