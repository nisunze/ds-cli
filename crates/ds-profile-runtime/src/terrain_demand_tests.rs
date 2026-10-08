use crate::transactional_projection_tests::{refresh, request_for, section_demand_fixture};
use crate::{Request, Runtime};
use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession};
use ds_grid_model::{TerrainPointId, TerrainPointRow};

#[test]
fn terrain_observe_never_solves_and_explicit_refresh_reuses_unchanged_section_demands_with_fresh_scene_parity(
) {
    for exact_endpoint in [false, true] {
        let mut snapshot = section_demand_fixture();
        snapshot.alignments[0].terrain_gap_tolerance_m = Some(1000.0);
        snapshot.alignments[0].terrain_corridor_half_width_m = Some(100.0);
        for (i, (x_m, y_m)) in [(0.0, 0.0), (300.0, 0.0), (600.0, 40.0)]
            .into_iter()
            .enumerate()
        {
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("terrain-cache-ground-{i}")).unwrap(),
                x_m,
                y_m,
                z_m: 100.0,
                feature_class: "GROUND".into(),
                description: None,
                required_clearance_m: None,
                source_id: None,
            });
        }
        // The shared terminal must lie exactly on its native route endpoint. The
        // common fixture's rounded station is just before it and correctly receives
        // a tiny interpolated terrain change from the edited middle observation.
        let endpoint =
            ds_grid_engine::alignment_station_point(&snapshot, &snapshot.alignments[0].id, 0.0)
                .unwrap()
                .alignment_length_m;
        if exact_endpoint {
            snapshot.structures[2].station_m = Some(endpoint);
        }
        let cases = snapshot.analysis_cases.len();
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        let base = input.expected_revision.clone();
        let before = runtime.held[&input.scope].demand_cache.last_demand_work();
        worker
            .apply_command(CommandEnvelope::new(
                "terrain-cache-edit",
                worker.current_revision().revision_id.clone(),
                GridCommand::SetTerrainPointElevation {
                    id: TerrainPointId::new("terrain-cache-ground-1").unwrap(),
                    elevation_m: 98.0,
                },
            ))
            .unwrap();
        let observed = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &base, &worker,
            ))))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            before
        );
        assert_eq!(observed["ran"], false);
        assert!(observed["scene"].is_null());
        assert_eq!(
            observed["calculation"]["computed_revision"],
            initial["model_revision"]
        );
        let base = worker.current_revision().revision_id.to_string();
        let result = runtime
            .execute(refresh(&runtime, &input, &base, &worker))
            .unwrap();
        let computed = if exact_endpoint { cases } else { cases * 2 };
        let reused = cases * 2 - computed;
        assert_eq!(result["metrics"]["last_demand_computed_cases"], computed);
        assert_eq!(result["metrics"]["last_demand_reused_cases"], reused);
        assert_eq!(result["metrics"]["last_demand_resolved_support_paths"], 2);
        assert_eq!(
            result["metrics"]["last_demand_terrain_geometry_checks"],
            cases * 2
        );
        assert_eq!(
            result["metrics"]["last_demand_terrain_geometry_reused_cases"],
            reused
        );
        let fresh = Runtime::default()
            .execute(request_for(worker.snapshot().clone()))
            .unwrap();
        assert_eq!(
            result["scene"], fresh["scene"],
            "full current native Profile includes exact fresh engineering and incident forces"
        );
        assert_eq!(
            worker.checkpoint().sequence,
            1,
            "only this explicit Save advances its sequence"
        );
    }
}
