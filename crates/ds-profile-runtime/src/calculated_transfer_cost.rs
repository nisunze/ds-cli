//! Opt-in, disk-backed measurement of complete explicit job result transport.
use super::tests::{prepare, ready, start};
use super::*;
use crate::transactional_projection_tests::{refresh, request_for, section_demand_fixture};
use ds_grid_engine::{CommandEnvelope, GridCommand};
use ds_grid_model::{AuthoredSag, TerrainPointId, TerrainPointRow};

#[test]
#[ignore = "bounded native calculated transfer measurement; run alone in release"]
fn dense_delta_job_costs_and_complete_fresh_parity() {
    let mut snapshot = section_demand_fixture();
    snapshot.alignments[0].terrain_gap_tolerance_m = Some(1000.);
    snapshot.alignments[0].terrain_corridor_half_width_m = Some(100.);
    for index in 0..10_000 {
        let x_m = 600. * index as f64 / 9_999.;
        snapshot.terrain_points.push(TerrainPointRow {
            id: TerrainPointId::new(format!("delta-job-ground-{index}")).unwrap(),
            x_m,
            y_m: (x_m - 300.).max(0.) * 40. / 300.,
            z_m: 100.,
            feature_class: "GROUND".into(),
            description: None,
            required_clearance_m: None,
            source_id: None,
        });
    }
    let input = request_for(snapshot.clone());
    let mut worker = GridSession::open(snapshot);
    let mut full = Runtime::default();
    let mut delta = Runtime::default();
    full.execute(input.clone()).unwrap();
    let initial = delta.execute(input.clone()).unwrap();
    let mut receiver =
        CalculatedSceneBase::new(serde_json::from_value(initial["scene"].clone()).unwrap());
    let cold_base = prepare(&mut delta, &worker);
    let mut samples = Vec::new();
    for i in 0..8 {
        let row = worker.snapshot().tension_sections[0].clone();
        let apply_started = Instant::now();
        worker
            .apply_command(CommandEnvelope::new(
                format!("delta-cost-{i}"),
                worker.current_revision().revision_id.clone(),
                GridCommand::SetTensionSectionSagMode {
                    id: row.id,
                    sag: AuthoredSag::HorizontalTension {
                        tension_n: if i % 2 == 0 { 30_000. } else { 31_000. },
                    },
                    sag_condition: row.sag_condition,
                    sag_weather_state_id: row.sag_weather_state_id,
                    criterion_set_id: row.criterion_set_id,
                    ruling_span_m: row.ruling_span_m,
                },
            ))
            .unwrap();
        let apply_ms = apply_started.elapsed().as_secs_f64() * 1000.;
        let mut pair = Vec::new();
        let mut results = Vec::new();
        for (is_delta, runtime) in [(false, &mut full), (true, &mut delta)] {
            let base_revision = runtime.held["one"]
                .session
                .current_revision()
                .revision_id
                .to_string();
            let observe = refresh(runtime, &input, &base_revision, &worker);
            let started = Instant::now();
            let observed = runtime
                .execute(Request::Observe(Box::new(observe)))
                .unwrap();
            let observe_bytes = serde_json::to_vec(&observed).unwrap().len();
            let observe_ms = started.elapsed().as_secs_f64() * 1000.;
            let prepared = is_delta.then(|| prepare(runtime, &worker));
            let id = format!("cost-{}-{i}", if is_delta { "delta" } else { "full" });
            let started = Instant::now();
            let ticket = if let Some(prepared) = &prepared {
                start(
                    runtime,
                    &input,
                    &worker,
                    prepared["base_signature"].as_str().unwrap().into(),
                    &id,
                )
                .unwrap()
            } else {
                let base = runtime.held["one"]
                    .session
                    .current_revision()
                    .revision_id
                    .to_string();
                let mut request = refresh(runtime, &input, &base, &worker);
                request.publication_id = Some(id.clone());
                runtime
                    .execute(Request::StartCalculation(Box::new(
                        StartCalculationRequest {
                            job_id: id.clone(),
                            request,
                        },
                    )))
                    .unwrap()
            };
            let start_with_journal_ms = started.elapsed().as_secs_f64() * 1000.;
            ready(runtime, &id);
            let result = runtime
                .execute(Request::AdmitCalculation(AdmitCalculationRequest {
                    scope: "one".into(),
                    model_id: "profile-test".into(),
                    job_id: id.clone(),
                    publication_id: Some(id),
                }))
                .unwrap();
            let native_job_elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
            let encode_started = Instant::now();
            let wire = serde_json::to_vec(&result).unwrap();
            let wire_encode_ms = encode_started.elapsed().as_secs_f64() * 1000.;
            let decode_started = Instant::now();
            let mut decoded: Value = serde_json::from_slice(&wire).unwrap();
            let scene_decode_or_admit_ms;
            if is_delta {
                let change =
                    serde_json::from_str(decoded["scene_delta_json"].as_str().unwrap()).unwrap();
                receiver = receiver
                    .admit(
                        &change,
                        &worker.current_revision().revision_id,
                        &worker.current_revision().roots.engineering_input_root,
                    )
                    .unwrap();
                scene_decode_or_admit_ms = decode_started.elapsed().as_secs_f64() * 1000.;
                decoded["scene"] = serde_json::to_value(receiver.scene()).unwrap();
                decoded["scene"]["display"] = decoded["display"].take();
            } else {
                let scene: ds_grid_engine::ProfileAtlasScene =
                    serde_json::from_value(decoded["scene"].clone()).unwrap();
                std::hint::black_box(scene);
                scene_decode_or_admit_ms = decode_started.elapsed().as_secs_f64() * 1000.;
            }
            let pin = serde_json::from_value(result["publication"].clone()).unwrap();
            runtime
                .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                    scope: "one".into(),
                    publication: pin,
                }))
                .unwrap();
            pair.push(json!({"mode":if is_delta{"delta"}else{"full"},"observe_ms":observe_ms,"observe_bytes":observe_bytes,
                "base_prepare_metrics":prepared.map(|p|p["metrics"].clone()),"start_with_journal_ms":start_with_journal_ms,
                "start_metrics":ticket,"native_job_elapsed_ms":native_job_elapsed_ms,"result_bytes":wire.len(),
                "wire_encode_ms":wire_encode_ms,"wire_decode_and_scene_admission_ms":scene_decode_or_admit_ms,
                "calculation_metrics":result["metrics"],
                "display_bytes":serde_json::to_vec(if is_delta{&result["display"]}else{&result["scene"]["display"]}).unwrap().len(),
                "geometry_bytes":if is_delta{result["scene_delta_json"].as_str().unwrap().len()}else{serde_json::to_vec(&result["scene"]).unwrap().len()}}));
            results.push(decoded);
        }
        assert_eq!(results[0]["scene"], results[1]["scene"]);
        assert_eq!(results[0]["analysis"], results[1]["analysis"]);
        assert_eq!(results[0]["command_view"], results[1]["command_view"]);
        let fresh = Runtime::default()
            .execute(request_for(worker.snapshot().clone()))
            .unwrap();
        assert_eq!(results[1]["scene"], fresh["scene"]);
        assert_eq!(results[1]["analysis"], fresh["analysis"]);
        samples.push(json!({"apply_ms":apply_ms,"pair":pair}));
    }
    let receipt = json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},
        "structures":worker.snapshot().structures.len(),"terrain_points":worker.snapshot().terrain_points.len(),
        "tension_sections":worker.snapshot().tension_sections.len(),"edits":8,"cold_base_metrics":cold_base["metrics"],
        "samples":samples,"full_delta_fresh_scene_and_display_parity":true,
        "includes":"apply separately; Observe+result encoding; job capture+journal, serial worker, polling, publication admission; full runtime result encoding/decoding and native geometry admission",
        "excludes":["initial projection","IPC","worker picking","renderer paint","installed acceptance","parity-only full scene reconstruction"],
        "boundaries":["display remains full","analysis remains full","initial scene-base preparation hashes full typed scene","full decode clones scene Value"]});
    if let Some(path) = std::env::var_os("DS_PROFILE_DELTA_JOB_COST_RECEIPT") {
        std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
    println!("{}", serde_json::to_string(&receipt).unwrap());
}
