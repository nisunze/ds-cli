use ds_profile_view::{camera::Camera, hit::HitIndex, model::*};
use serde_json::json;

fn scene(value: serde_json::Value) -> Scene {
    serde_json::from_value(value).expect("representative native scene is admitted")
}

fn observation(id: &str, x: f64) -> serde_json::Value {
    json!({
        "entity_id": id,
        "point": {"scene_x": x, "scene_y": 80., "elevation_m": 100.},
        "feature_clearance": {
            "feature_name": "Road", "feature_code": 7, "required_vertical_m": 6.,
            "baseline": {"scene_x": x, "scene_y": 80., "elevation_m": 100.},
            "threshold": {"scene_x": x, "scene_y": 60., "elevation_m": 110.},
            "display_margin_m": -0.4, "point_color": "#abcdef"
        }
    })
}

fn baseline(scene: &Scene, id: &str, x: f64) -> Point {
    scene.feature_baselines[&(id.to_owned(), x.to_bits())]
}

#[test]
fn feature_hairs_start_at_the_displayed_ground_guide_without_bridging_gaps() {
    let mut scene = scene(json!({
        "bands": [{"alignment_id": "a", "terrain_points": [
            observation("repeated", 25.), observation("repeated", 75.), observation("gap", 150.)
        ]}],
        "clearance_guides": [
            {"kind": "ground_offset", "offset_m": 6., "segments": [{"points": [
                {"scene_x": 0., "scene_y": 70.}, {"scene_x": 100., "scene_y": 50.}
            ]}]},
            {"kind": "lowest_wire_drop", "offset_m": 3., "segments": [{"points": [
                {"scene_x": 100., "scene_y": 1.}, {"scene_x": 200., "scene_y": 1.}
            ]}]}
        ]
    }));
    let observations = scene.bands[0].terrain_points.clone();

    scene.prepare_render();

    assert_eq!(baseline(&scene, "repeated", 25.).scene_y, 65.);
    assert_eq!(baseline(&scene, "repeated", 75.).scene_y, 55.);
    assert_eq!(baseline(&scene, "gap", 150.).scene_y, 80.);
    assert_eq!(scene.bands[0].terrain_points, observations);
}

#[test]
fn zero_ground_offset_keeps_the_observation_as_the_visual_baseline() {
    let mut scene = scene(json!({
        "bands": [{"terrain_points": [observation("road", 25.)]}],
        "clearance_guides": [{"kind": "ground_offset", "offset_m": 0., "segments": [{
            "points": [{"scene_x": 0., "scene_y": 70.}, {"scene_x": 100., "scene_y": 50.}]
        }]}]
    }));

    scene.prepare_render();

    assert_eq!(
        baseline(&scene, "road", 25.),
        scene.bands[0].terrain_points[0].point
    );
}

#[test]
fn preparing_replacement_guides_discards_old_feature_baselines() {
    let mut scene = scene(json!({
        "bands": [{"terrain_points": [observation("road", 25.)]}],
        "clearance_guides": [{"kind": "ground_offset", "offset_m": 6., "segments": [{
            "points": [{"scene_x": 0., "scene_y": 60.}, {"scene_x": 100., "scene_y": 60.}]
        }]}]
    }));
    scene.prepare_render();
    assert_eq!(baseline(&scene, "road", 25.).scene_y, 60.);

    scene.clearance_guides.clear();
    scene.prepare_render();

    assert_eq!(baseline(&scene, "road", 25.).scene_y, 80.);
    scene.bands[0].terrain_points.clear();
    scene.prepare_render();
    assert!(scene.feature_baselines.is_empty());
}

#[test]
fn folded_paths_keep_their_authored_order_instead_of_becoming_binary_searchable() {
    let mut scene = scene(json!({
        "bands": [{
            "ground": [{"points": [
                {"scene_x": 0.}, {"scene_x": 70.}, {"scene_x": 10.}, {"scene_x": 100.}
            ]}],
            "side_profiles": [{"segments": [{"points": [
                {"scene_x": 100.}, {"scene_x": 70.}, {"scene_x": 0.}
            ]}]}]
        }],
        "conductor_spans": [{"points": [
            {"scene_x": 0.}, {"scene_x": 0.}, {"scene_x": 100.}
        ]}],
        "clearance_guides": [{"segments": [{"points": [
            {"scene_x": 100.}, {"scene_x": 0.}
        ]}]}],
        "authored_connections": [{"points": [{"scene_x": -1000.}, {"scene_x": 1000.}]}],
        "display": {
            "primitives": [{"points": [{"scene_x": 0.}, {"scene_x": 50.}]}],
            "authored_structures": {"pole": {"primitives": [{"points": [
                {"scene_x": 0.}, {"scene_x": 50.}, {"scene_x": -10.}
            ]}]}}
        }
    }));

    scene.prepare_render();

    let folded = &scene.bands[0].ground[0].points;
    assert_eq!(
        folded.iter().map(|p| p.scene_x).collect::<Vec<_>>(),
        [0., 70., 10., 100.]
    );
    assert_eq!(folded[0].path_order, 0);
    assert_eq!(
        scene.bands[0].side_profiles[0].segments[0].points[0].path_order,
        -1
    );
    assert_eq!(scene.conductor_spans[0].points[0].path_order, 1);
    assert_eq!(
        scene.clearance_guides[0].segments[0].points[0].path_order,
        -1
    );
    assert_eq!(scene.authored_connections[0].points[0].path_order, 1);
    let display = scene.display.as_ref().unwrap();
    assert_eq!(display.primitives[0].points[0].path_order, 1);
    assert_eq!(
        display.authored_structures["pole"].primitives[0].points[0].path_order,
        0
    );
}

#[test]
fn native_axis_labels_symbols_and_review_disclosures_survive_render_preparation() {
    let mut scene = scene(json!({
        "model_revision": "head", "computed_revision": "head",
        "axis_pin": {"digest": "sha256:held-axis", "vertical_px_per_m": 2., "domain_min_m": 80., "domain_max_m": 120.},
        "scale": {"horizontal_scene_units_per_m": 0.5, "vertical_scene_units_per_m": 2.},
        "grid": {"target_screen_spacing_px": 72., "station_levels": [{"step_m": 100., "ticks": [
            {"value_m": 0., "scene_x": 10., "boundary": true}, {"value_m": 250., "scene_x": 135., "boundary": true}
        ]}]},
        "display": {
            "rows": [{"option": "structure_labels", "encoding": {"font": "11px monospace", "color": "#123456"}}],
            "primitives": [{"option": "structure_labels", "labels": [{
                "point": {"scene_x": 50., "scene_y": 20.}, "text": "P7 · S190", "rotation_radians": -std::f64::consts::FRAC_PI_2, "offset_px": [10., -4.]
            }]}],
            "symbols": [{"entity_id": "pole", "parent_id": null, "glyph": "◆", "point": {"scene_x": 50., "scene_y": 20.}}],
            "authored_structures": {"pole": {"symbols": [{"entity_id": "pole\0phase", "parent_id": "pole", "glyph": "■"}]}},
            "review_status_colors": {"unknown": "#888888"}
        },
        "engineering": {"markers": [{"entity_id": "pole", "kind": "structure_usage", "status": "unknown", "labels": [{
            "text": "—", "colour_role": "neutral", "hit_rect_px": [-10., -8., 20., 16.],
            "summary": {"lines": ["Evidence unavailable", "Choose capacity in the owning model"], "issues_label": "Open in Issues", "details_label": "Details"}
        }]}]}
    }));
    let axis = scene.axis_pin.clone();
    let scale = scene.scale.clone();
    let grid = scene.grid.clone();
    let display = scene.display.clone();
    let engineering = scene.engineering.clone();

    scene.prepare_render();

    assert_eq!(scene.axis_pin, axis);
    assert_eq!(scene.scale, scale);
    assert_eq!(scene.grid, grid);
    assert_eq!(scene.display, display);
    assert_eq!(scene.engineering, engineering);
    assert!(scene.current());
}

#[test]
fn stale_or_provisional_scenes_keep_authored_geometry_and_suppress_solved_wire_picks() {
    let base = scene(json!({
        "model_revision": "head", "computed_revision": "head",
        "bounds": {"min_x": 0., "max_x": 100., "min_y": 0., "max_y": 100.},
        "bands": [{"structures": [{"entity_id": "pole", "junction": true,
            "ground": {"scene_x": 20., "scene_y": 80.}, "top": {"scene_x": 20., "scene_y": 30.},
            "attachment_points": [{"attachment_point_id": "phase", "dead_end": true, "point": {"scene_x": 20., "scene_y": 30.}}]
        }]}],
        "conductor_spans": [{"section_id": "wire", "points": [{"scene_x": 30., "scene_y": 20.}, {"scene_x": 90., "scene_y": 20.}],
            "junction_hint": {"direction_arrow": {"shaft_start": {"scene_x": 20., "scene_y": 30.}, "shaft_end": {"scene_x": 24., "scene_y": 31.}}}
        }],
        "engineering": {"markers": [{"entity_id": "pole", "kind": "structure_usage", "labels": [{"text": "91.5%"}]}]}
    }));
    let camera = Camera {
        bounds: base.bounds,
        width: 800.,
        height: 400.,
        viewport: Viewport::default(),
    };
    let point = camera.project(Point::xy(60., 20.));
    let shown = Visibility::default();
    assert!(HitIndex::new(&base)
        .pick(&camera, point, &shown, 1.)
        .iter()
        .any(|c| c.entity_id == "wire"));

    for provisional in [false, true] {
        let mut scene = base.clone();
        if provisional {
            scene.provisional_geometry = true;
        } else {
            scene.computed_revision = Some("previous-head".into());
        }
        scene.prepare_render();

        assert!(
            !scene.current(),
            "held engineering must not be presented as current"
        );
        assert!(HitIndex::new(&scene)
            .pick(&camera, point, &shown, 1.)
            .is_empty());
        assert_eq!(scene.bands, base.bands);
        assert_eq!(
            scene.engineering, base.engineering,
            "retain evidence without relabeling it"
        );
        assert_eq!(
            scene.conductor_spans[0].junction_hint,
            base.conductor_spans[0].junction_hint
        );
        let attachments =
            HitIndex::new(&scene).pick(&camera, camera.project(Point::xy(20., 30.)), &shown, 1.);
        assert!(attachments.iter().any(|c| c.entity_id == "pole\0phase"));
    }
}
