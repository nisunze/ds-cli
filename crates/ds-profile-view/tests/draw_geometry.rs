use ds_profile_view::{draw_geometry::*, model::*};
use serde_json::json;

fn point(x: f64, y: f64) -> Point {
    Point {
        scene_x: x,
        scene_y: y,
        station_m: x,
        chain_station_m: x,
        elevation_m: 100. - y,
        ..Point::default()
    }
}

fn span(section: &str, points: Vec<Point>) -> Span {
    Span {
        section_id: section.into(),
        points,
        ..Span::default()
    }
}

fn scene(spans: Vec<Span>) -> Scene {
    Scene {
        conductor_spans: spans,
        scale: SceneScale {
            horizontal_scene_units_per_m: 1.,
            vertical_scene_units_per_m: 2.,
        },
        ..Scene::default()
    }
}

fn coordinates(points: &[Point]) -> Vec<(f64, f64, f64)> {
    points
        .iter()
        .map(|p| (p.scene_x, p.scene_y, p.elevation_m))
        .collect()
}

#[test]
fn adjacent_native_spans_share_the_lowest_offset_without_a_cross_alignment_chord() {
    let mut crossing = span("handoff", vec![point(0., 99.), point(20., 99.)]);
    crossing.cross_alignment = true;
    let scene = scene(vec![
        span("section-a", vec![point(0., 20.), point(10., 30.)]),
        span("section-a", vec![point(10., 30.), point(20., 25.)]),
        crossing,
    ]);
    let original = scene.conductor_spans.clone();

    let paths = lowest_wire_drop(&scene, 3.);

    assert_eq!(paths.len(), 1);
    assert_eq!(
        coordinates(&paths[0]),
        [(0., 26., 77.), (10., 36., 67.), (20., 31., 72.)]
    );
    assert_eq!(scene.conductor_spans, original);
}

#[test]
fn the_lowest_sample_wins_where_multiple_wires_share_a_station() {
    let scene = scene(vec![
        span("upper", vec![point(0., 10.), point(10., 20.)]),
        span("lower", vec![point(0., 30.), point(10., 40.)]),
    ]);

    let paths = lowest_wire_drop(&scene, 2.);

    assert_eq!(paths.len(), 1);
    assert_eq!(coordinates(&paths[0]), [(0., 34., 68.), (10., 44., 58.)]);
}

#[test]
fn gaps_stay_open_even_when_two_runs_have_the_same_section_id() {
    let scene = scene(vec![
        span("section", vec![point(0., 20.), point(10., 30.)]),
        span("section", vec![point(30., 40.), point(40., 50.)]),
        span("isolated", vec![point(100., 10.)]),
    ]);

    let paths = lowest_wire_drop(&scene, 1.);

    assert_eq!(paths.len(), 2);
    assert_eq!(coordinates(&paths[0]), [(0., 22., 79.), (10., 32., 69.)]);
    assert_eq!(coordinates(&paths[1]), [(30., 42., 59.), (40., 52., 49.)]);
}

#[test]
fn repeated_alignment_local_stations_remain_distinct_on_the_virtual_chain() {
    let mut a = point(0., 20.);
    let mut b = point(10., 30.);
    a.station_m = 0.;
    b.station_m = 0.;
    let scene = scene(vec![span("section", vec![a, b])]);

    let paths = lowest_wire_drop(&scene, 1.);

    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0]
            .iter()
            .map(|p| p.chain_station_m)
            .collect::<Vec<_>>(),
        [0., 10.]
    );
}

#[test]
fn disabled_and_invalid_offsets_create_no_display_geometry() {
    let scene = scene(vec![span("section", vec![point(0., 20.), point(10., 30.)])]);
    for metres in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(lowest_wire_drop(&scene, metres).is_empty());
    }
    assert!(lowest_wire_drop(&Scene::default(), 3.).is_empty());
}

#[test]
fn six_decimal_station_identity_retains_the_former_ties_away_from_zero_boundary() {
    let mut upper = point(0., 10.);
    upper.chain_station_m = 0.0078125;
    let mut lower = point(0., 30.);
    lower.chain_station_m = 0.007813;
    let scene = scene(vec![
        span("through", vec![upper, point(10., 20.)]),
        span("meets", vec![lower]),
    ]);

    let paths = lowest_wire_drop(&scene, 1.);

    assert_eq!(paths.len(), 1);
    assert_eq!(coordinates(&paths[0]), [(0., 32., 69.), (10., 22., 79.)]);
}

#[test]
fn parent_and_child_handoff_anchors_resolve_through_explicit_support_identity() {
    let scene: Scene = serde_json::from_value(json!({"bands": [
        {"alignment_id": "parent", "structures": [{"entity_id": "shared", "ground": {"scene_x": 140., "scene_y": 60.}}]},
        {"alignment_id": "child", "structures": [{"entity_id": "wrong", "ground": {"scene_x": 290.}}, {"entity_id": "shared", "ground": {"scene_x": 300., "scene_y": 100.}}]}
    ]})).unwrap();
    let hint: Junction = serde_json::from_value(json!({
        "child_alignment_id": "child", "junction_structure_id": "shared",
        "direction_arrow": {"shaft_start": {"scene_x": 145., "scene_y": 80.}, "shaft_end": {"scene_x": 160., "scene_y": 80.}}
    })).unwrap();

    let anchors = junction_scene_starts(&scene, &hint);

    assert_eq!(
        anchors
            .iter()
            .map(|p| (p.scene_x, p.scene_y))
            .collect::<Vec<_>>(),
        [(145., 80.), (300., 80.)]
    );
    let mut missing = hint.clone();
    missing.junction_structure_id = "absent".into();
    assert_eq!(junction_scene_starts(&scene, &missing).len(), 1);
    let same_scene: Scene = serde_json::from_value(json!({"bands": [{"alignment_id": "child", "structures": [{"entity_id": "shared", "ground": {"scene_x": 145.}}]}]})).unwrap();
    assert_eq!(junction_scene_starts(&same_scene, &hint).len(), 1);
}

#[test]
fn arrows_remain_twelve_css_pixels_at_every_authored_length() {
    let start = Screen { x: 100., y: 80. };
    for length in [0.01, 15., 10000.] {
        let arrow = junction_screen_arrow(
            start,
            Screen {
                x: start.x + length,
                y: start.y,
            },
        );
        assert_eq!((arrow.start.x, arrow.start.y), (100., 80.));
        assert_eq!((arrow.end.x, arrow.end.y), (112., 80.));
        assert_eq!(
            arrow.head.map(|p| (p.x, p.y)),
            [(112., 80.), (109., 83.), (109., 77.)]
        );
    }
}

#[test]
fn arrow_rotation_and_zero_length_fallback_do_not_depend_on_camera_scale() {
    let start = Screen { x: 5., y: 8. };
    let down = junction_screen_arrow(start, Screen { x: 5., y: 9. });
    assert_eq!((down.end.x, down.end.y), (5., 20.));
    assert_eq!(
        down.head.map(|p| (p.x, p.y)),
        [(5., 20.), (2., 17.), (8., 17.)]
    );
    let fallback = junction_screen_arrow(start, start);
    assert_eq!((fallback.end.x, fallback.end.y), (17., 8.));
}
