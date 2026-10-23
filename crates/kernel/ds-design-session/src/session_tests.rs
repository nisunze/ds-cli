use super::*;
use crate::projection::ProjectionSelection;
use serde_json::json;

const METERS_PER_LAT_DEG: f64 = 111_320.0;

fn pt(id: &str, lng: f64, lat: f64) -> Feature {
    Feature::new(id, Geometry::Point { coords: [lng, lat] })
}

#[cfg(feature = "enable_dataframes")]
fn pt_with_props(id: &str, lng: f64, lat: f64, props: serde_json::Value) -> Feature {
    Feature::with_properties(
        id,
        Geometry::Point { coords: [lng, lat] },
        props.as_object().cloned().unwrap_or_default(),
    )
}

fn line(id: &str, coords: &[[f64; 2]]) -> Feature {
    Feature::new(
        id,
        Geometry::LineString {
            coords: coords.to_vec(),
        },
    )
}

fn stamp_input(current: Value, base: Value, version: u64, first_create: bool) -> VersionStampInput {
    VersionStampInput {
        layers: Map::from_iter([("lv_poles".to_string(), current)]),
        base_layers: Map::from_iter([("lv_poles".to_string(), base)]),
        version,
        first_create,
        force_changed: false,
        assume_existing: false,
    }
}

fn feature(id: &str, x: f64, name: &str) -> Value {
    json!({
        "type": "Feature",
        "id": id,
        "geometry": { "type": "Point", "coordinates": [x, -2.0] },
        "properties": { "id": id, "name": name }
    })
}

#[test]
fn version_stamp_marks_content_change_and_preserves_first_version() {
    let mut before = feature("pole-1", 30.0, "before");
    before["properties"]["v_first"] = json!(2);
    before["properties"]["v_last"] = json!(3);
    let mut after = feature("pole-1", 30.0, "after");
    after["properties"]["v_first"] = json!(2);
    after["properties"]["v_last"] = json!(3);
    let result = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [after] }),
        json!({ "type": "FeatureCollection", "features": [before] }),
        4,
        false,
    ))
    .unwrap();
    let stamped = &result.layers["lv_poles"]["features"][0]["properties"];
    assert_eq!(stamped["v_first"], 2);
    assert_eq!(stamped["v_last"], 4);
    assert_eq!(result.stamped_feature_count, 1);
}

#[test]
fn version_stamp_starts_legacy_untouched_at_zero_and_marks_new_feature() {
    let untouched = feature("pole-1", 30.0, "same");
    let created = feature("pole-2", 30.1, "new");
    let result = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [untouched.clone(), created] }),
        json!({ "type": "FeatureCollection", "features": [untouched] }),
        7,
        false,
    ))
    .unwrap();
    let features = result.layers["lv_poles"]["features"].as_array().unwrap();
    assert_eq!(features[0]["properties"]["v_first"], 0);
    assert_eq!(features[0]["properties"]["v_last"], 0);
    assert_eq!(features[1]["properties"]["v_first"], 7);
    assert_eq!(features[1]["properties"]["v_last"], 7);
    assert_eq!(result.stamped_feature_count, 2);
}

#[test]
fn version_stamp_changed_legacy_feature_gets_complete_provenance() {
    let before = feature("pole-1", 30.0, "before");
    let after = feature("pole-1", 30.0, "after");
    let result = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [after] }),
        json!({ "type": "FeatureCollection", "features": [before] }),
        4,
        false,
    ))
    .unwrap();
    let stamped = &result.layers["lv_poles"]["features"][0]["properties"];
    assert_eq!(stamped["v_first"], 0);
    assert_eq!(stamped["v_last"], 4);
}

#[test]
fn version_stamp_reconciles_regenerated_top_level_id_by_persisted_property_id() {
    let mut before = feature("persisted-pole-1", 30.0, "same");
    before["id"] = json!("generated-before");
    before["properties"]["v_first"] = json!(1);
    before["properties"]["v_last"] = json!(2);
    let mut after = before.clone();
    after["id"] = json!("generated-after");

    let result = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [after] }),
        json!({ "type": "FeatureCollection", "features": [before] }),
        3,
        false,
    ))
    .unwrap();

    let stamped = &result.layers["lv_poles"]["features"][0]["properties"];
    assert_eq!(stamped["v_first"], 1);
    assert_eq!(stamped["v_last"], 2);
    assert_eq!(result.stamped_feature_count, 0);
}

#[test]
fn version_stamp_tracks_content_change_after_property_id_reconciliation() {
    let mut before = feature("persisted-pole-1", 30.0, "before");
    before["id"] = json!("generated-before");
    before["properties"]["v_first"] = json!(1);
    before["properties"]["v_last"] = json!(2);
    let mut after = feature("persisted-pole-1", 30.0, "after");
    after["id"] = json!("generated-after");

    let result = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [after] }),
        json!({ "type": "FeatureCollection", "features": [before] }),
        3,
        false,
    ))
    .unwrap();

    let stamped = &result.layers["lv_poles"]["features"][0]["properties"];
    assert_eq!(stamped["v_first"], 1);
    assert_eq!(stamped["v_last"], 3);
}

#[test]
fn version_stamp_rejects_duplicate_base_identity_aliases() {
    let first = feature("shared", 30.0, "one");
    let mut second = feature("other", 30.1, "two");
    second["properties"]["doc_id"] = json!("shared");

    let error = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [] }),
        json!({ "type": "FeatureCollection", "features": [first, second] }),
        3,
        false,
    ))
    .unwrap_err();

    assert!(error.contains("stable identity \"shared\" is shared"));
}

#[test]
fn version_stamp_rejects_current_aliases_that_name_different_base_rows() {
    let first = feature("first", 30.0, "one");
    let second = feature("second", 30.1, "two");
    let mut current = feature("first", 30.0, "one");
    current["properties"]["id"] = json!("second");

    let error = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [current] }),
        json!({ "type": "FeatureCollection", "features": [first, second] }),
        3,
        false,
    ))
    .unwrap_err();

    assert!(error.contains("stable identities resolve to multiple base features"));
}

#[test]
fn version_stamp_recovered_existing_feature_preserves_first_revision() {
    let mut current = feature("pole-1", 30.0, "edited offline");
    current["properties"]["v_first"] = json!(2);
    current["properties"]["v_last"] = json!(3);
    let mut input = stamp_input(
        json!({ "type": "FeatureCollection", "features": [current] }),
        json!({ "type": "FeatureCollection", "features": [] }),
        5,
        false,
    );
    input.force_changed = true;
    input.assume_existing = true;
    let result = stamp_geojson_layers_version(input).unwrap();
    let stamped = &result.layers["lv_poles"]["features"][0]["properties"];
    assert_eq!(stamped["v_first"], 2);
    assert_eq!(stamped["v_last"], 5);
}

#[test]
fn version_stamp_rejects_impossible_existing_provenance() {
    let mut before = feature("pole-1", 30.0, "before");
    before["properties"]["v_first"] = json!(5);
    before["properties"]["v_last"] = json!(2);
    let after = feature("pole-1", 30.0, "after");
    let error = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [after] }),
        json!({ "type": "FeatureCollection", "features": [before] }),
        6,
        false,
    ))
    .unwrap_err();
    assert!(error.contains("invalid design-version provenance"));
}

#[test]
fn version_stamp_v0_normalizes_retired_save_revisions_without_inventing_history() {
    let mut before = feature("pole-1", 30.0, "same");
    before["properties"]["v_first"] = json!(5);
    before["properties"]["v_last"] = json!(6);
    let after = before.clone();
    let result = stamp_geojson_layers_version(stamp_input(
        json!({ "type": "FeatureCollection", "features": [after] }),
        json!({ "type": "FeatureCollection", "features": [before] }),
        0,
        false,
    ))
    .unwrap();
    let stamped = &result.layers["lv_poles"]["features"][0]["properties"];
    assert_eq!(stamped["v_first"], 0);
    assert_eq!(stamped["v_last"], 0);
}

#[test]
fn version_stamp_first_create_starts_at_deliberate_v0() {
    let result = stamp_geojson_layers_version(stamp_input(
        json!({
            "type": "FeatureCollection",
            "features": [feature("pole-1", 30.0, "one"), feature("pole-2", 30.1, "two")]
        }),
        json!({ "type": "FeatureCollection", "features": [] }),
        0,
        true,
    ))
    .unwrap();
    for stamped in result.layers["lv_poles"]["features"].as_array().unwrap() {
        assert_eq!(stamped["properties"]["v_first"], 0);
        assert_eq!(stamped["properties"]["v_last"], 0);
    }
    assert_eq!(result.stamped_feature_count, 2);
}

#[test]
fn empty_session_picks_nothing() {
    let h = SessionHandle::open(vec![]);
    assert!(h.pick(0.0, 0.0, 5.0).is_none());
    assert_eq!(
        h.projection_metadata(),
        ProjectionMetadata {
            mode: ProjectionMode::LocalTangent,
            origin_lng: 0.0,
            origin_lat: 0.0,
            utm_zone: None,
            utm_southern: None,
        }
    );
}

#[test]
fn single_point_pick_hits() {
    let h = SessionHandle::open(vec![pt("a", 30.0608, -1.9536)]); // Kigali-ish
    let id = h.pick(30.0608, -1.9536, 1.0);
    assert_eq!(id, Some(FeatureId::new("a")));
}

#[test]
fn session_projection_origin_comes_from_feature_extent() {
    let h = SessionHandle::open(vec![pt("a", 30.0, -2.0), pt("b", 30.2, -1.8)]);

    let projection = h.projection_metadata();
    assert_eq!(projection.mode, ProjectionMode::NixItrf2005);
    assert!((projection.origin_lng - 30.1).abs() < 1e-12);
    assert!((projection.origin_lat + 1.9).abs() < 1e-12);
}

#[test]
fn mount_scope_with_projection_refreshes_existing_scopes() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope_with_projection(
        "design",
        vec![pt("lv_poles/p1", 30.0, -2.0)],
        ProjectionConfig {
            mode: ProjectionSelection::WebMercator,
            ..ProjectionConfig::default()
        },
    )
    .unwrap();
    assert_eq!(h.projection_metadata().mode, ProjectionMode::WebMercator);
    let web_point = h.project_coord(30.0, -2.0).unwrap();

    h.mount_scope_with_projection(
        "local",
        vec![pt("lv_poles/p2", 30.1, -1.9)],
        ProjectionConfig {
            mode: ProjectionSelection::NixItrf2005,
            ..ProjectionConfig::default()
        },
    )
    .unwrap();

    let projection = h.projection_metadata();
    assert_eq!(projection.mode, ProjectionMode::NixItrf2005);
    let nix_point = h.project_coord(30.0, -2.0).unwrap();
    assert!((nix_point[0] - 500_000.0).abs() < 1.0);
    assert!((web_point[0] - nix_point[0]).abs() > 1_000_000.0);
    assert_eq!(
        h.pick(30.0, -2.0, 1.0),
        Some(FeatureId::new("design:lv_poles/p1"))
    );
}

#[test]
fn same_scope_remount_preserves_only_selection_slots_that_still_exist() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope(
        "design",
        vec![
            pt("poles/a", 30.0, -2.0),
            pt("poles/b", 30.1, -2.0),
            pt("poles/c", 30.2, -2.0),
        ],
    )
    .unwrap();
    h.select(
        vec![FeatureId::new("design:poles/a")],
        SelectionMode::Replace,
    );
    h.hover(Some(FeatureId::new("design:poles/b")));
    h.mark_edited(FeatureId::new("design:poles/c"));

    h.mount_scope(
        "design",
        vec![
            pt("poles/a", 30.0, -2.0),
            pt("poles/b", 30.1, -2.0),
            pt("poles/c", 30.2, -2.0),
            pt("poles/d", 30.3, -2.0),
        ],
    )
    .unwrap();
    assert_eq!(
        h.selection(),
        &SelectionState {
            selected: vec![FeatureId::new("design:poles/a")],
            hovered: Some(FeatureId::new("design:poles/b")),
            last_edited: Some(FeatureId::new("design:poles/c")),
        }
    );

    h.mount_scope(
        "design",
        vec![pt("poles/a", 30.0, -2.0), pt("poles/d", 30.3, -2.0)],
    )
    .unwrap();
    assert_eq!(
        h.selection(),
        &SelectionState {
            selected: vec![FeatureId::new("design:poles/a")],
            hovered: None,
            last_edited: None,
        },
        "removed slots must clear and the final mount input must not become an edit"
    );
}

#[test]
fn same_scope_remount_keeps_cleared_state_empty_and_other_scope_selected() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope("design", vec![pt("poles/a", 30.0, -2.0)])
        .unwrap();
    h.mount_scope("local", vec![pt("notes/z", 30.4, -2.0)])
        .unwrap();
    h.clear_selection();

    h.mount_scope(
        "design",
        vec![pt("poles/a", 30.0, -2.0), pt("poles/b", 30.1, -2.0)],
    )
    .unwrap();
    assert_eq!(h.selection(), &SelectionState::default());

    h.select(
        vec![FeatureId::new("local:notes/z")],
        SelectionMode::Replace,
    );
    h.mount_scope(
        "design",
        vec![
            pt("poles/a", 30.0, -2.0),
            pt("poles/b", 30.1, -2.0),
            pt("poles/c", 30.2, -2.0),
        ],
    )
    .unwrap();
    assert_eq!(
        h.selection(),
        &SelectionState {
            selected: vec![FeatureId::new("local:notes/z")],
            hovered: None,
            last_edited: None,
        }
    );
}

#[test]
fn pick_far_away_misses() {
    let h = SessionHandle::open(vec![pt("a", 30.0608, -1.9536)]);
    assert!(h.pick(0.0, 0.0, 10.0).is_none());
}

#[test]
fn closer_of_two_points_wins() {
    let h = SessionHandle::open(vec![
        pt("near", 30.0000, 0.0000),
        pt("far", 30.0010, 0.0000), // ~111 m east
    ]);
    // Click 10 m east of "near"
    let click_lng = 30.0000 + 10.0 / METERS_PER_LAT_DEG; // 1 deg lng ≈ 111320 m at the equator
    let id = h.pick(click_lng, 0.0, 200.0);
    assert_eq!(id, Some(FeatureId::new("near")));
}

#[test]
fn pick_outside_tolerance_returns_none() {
    let h = SessionHandle::open(vec![pt("a", 30.0, 0.0)]);
    // 200 m east, tolerance 100 m → miss
    let click_lng = 30.0 + 200.0 / METERS_PER_LAT_DEG;
    assert!(h.pick(click_lng, 0.0, 100.0).is_none());
}

#[test]
fn linestring_pick_near_midpoint() {
    let h = SessionHandle::open(vec![line(
        "edge",
        &[[30.0000, 0.0000], [30.0020, 0.0000]], // ~222 m east-west
    )]);
    // Click 1 m north of the midpoint
    let click_lng = 30.0010;
    let click_lat = 1.0 / METERS_PER_LAT_DEG;
    let id = h.pick(click_lng, click_lat, 5.0);
    assert_eq!(id, Some(FeatureId::new("edge")));
}

#[test]
fn linestring_pick_far_from_segment_misses() {
    let h = SessionHandle::open(vec![line("edge", &[[30.0000, 0.0000], [30.0020, 0.0000]])]);
    // Click 50 m north of the midpoint, tolerance 5 m → miss
    let click_lng = 30.0010;
    let click_lat = 50.0 / METERS_PER_LAT_DEG;
    assert!(h.pick(click_lng, click_lat, 5.0).is_none());
}

#[test]
fn snap_vertex_uses_projected_metric_distance() {
    let h = SessionHandle::open(vec![pt("lv_poles/p1", 30.0, 0.0)]);
    let hit = h.snap(SnapQuery {
        lng: 30.0 + 0.5 / METERS_PER_LAT_DEG,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["lv_poles".to_string()],
        snap_to_vertices: true,
        snap_to_segments: false,
        snap_to_midpoints: false,
        snap_to_self: false,
        self_vertices: vec![],
        exclude_feature_ids: vec![],
        max_candidates: 100,
    });

    let hit = hit.expect("snap hit");
    assert_eq!(hit.target_kind, SnapTargetKind::Vertex);
    assert_eq!(hit.feature_id, Some(FeatureId::new("lv_poles/p1")));
    assert_eq!(hit.layer_id.as_deref(), Some("lv_poles"));
    assert_eq!(hit.coord, [30.0, 0.0]);
    assert!(hit.distance_m <= 1.0);
}

#[test]
fn snap_edge_returns_perpendicular_projection_metadata() {
    let h = SessionHandle::open(vec![line("lv_lines/l1", &[[30.0, 0.0], [30.001, 0.0]])]);
    let hit = h.snap(SnapQuery {
        lng: 30.0003,
        lat: 1.0 / METERS_PER_LAT_DEG,
        tolerance_m: 2.0,
        target_layers: vec!["lv_lines".to_string()],
        snap_to_vertices: false,
        snap_to_segments: true,
        snap_to_midpoints: false,
        snap_to_self: false,
        self_vertices: vec![],
        exclude_feature_ids: vec![],
        max_candidates: 100,
    });

    let hit = hit.expect("edge snap hit");
    assert_eq!(hit.target_kind, SnapTargetKind::Edge);
    assert_eq!(hit.feature_id, Some(FeatureId::new("lv_lines/l1")));
    assert_eq!(hit.ring_index, Some(0));
    assert_eq!(hit.segment_index, Some(0));
    assert!(hit.t.expect("segment t") > 0.25);
    assert!(hit.t.expect("segment t") < 0.35);
    assert!((hit.coord[0] - 30.0003).abs() < 1e-7);
    assert!(hit.coord[1].abs() < 1e-7);
}

#[test]
fn snap_midpoint_and_self_are_ranked_by_distance_then_priority() {
    let h = SessionHandle::open(vec![line("lv_lines/l1", &[[30.0, 0.0], [30.001, 0.0]])]);
    let hit = h.snap(SnapQuery {
        lng: 30.0005,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["lv_lines".to_string()],
        snap_to_vertices: false,
        snap_to_segments: false,
        snap_to_midpoints: true,
        snap_to_self: true,
        self_vertices: vec![[30.0005, 0.0]],
        exclude_feature_ids: vec![],
        max_candidates: 100,
    });

    let hit = hit.expect("snap hit");
    assert_eq!(hit.target_kind, SnapTargetKind::SelfSnap);
    assert_eq!(hit.feature_id, None);
}

#[test]
fn snap_layer_filter_accepts_scoped_layers() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope("design", vec![pt("lv_poles/p1", 30.0, 0.0)])
        .unwrap();
    h.mount_scope("local", vec![pt("lv_poles/p2", 30.0, 0.0)])
        .unwrap();

    let hit = h.snap(SnapQuery {
        lng: 30.0,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["local:lv_poles".to_string()],
        snap_to_vertices: true,
        snap_to_segments: false,
        snap_to_midpoints: false,
        snap_to_self: false,
        self_vertices: vec![],
        exclude_feature_ids: vec![],
        max_candidates: 100,
    });

    assert_eq!(
        hit.map(|snap| snap.feature_id),
        Some(Some(FeatureId::new("local:lv_poles/p2")))
    );
}

#[test]
fn snap_excludes_current_feature_from_index_candidates() {
    let h = SessionHandle::open(vec![
        line("lv_lines/editing", &[[30.0, 0.0], [30.001, 0.0]]),
        pt("lv_poles/target", 30.0005, 0.0),
    ]);

    let hit = h.snap(SnapQuery {
        lng: 30.0005,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["lv_lines".to_string(), "lv_poles".to_string()],
        snap_to_vertices: true,
        snap_to_segments: true,
        snap_to_midpoints: true,
        snap_to_self: false,
        self_vertices: vec![],
        exclude_feature_ids: vec![FeatureId::new("lv_lines/editing")],
        max_candidates: 100,
    });

    let hit = hit.expect("remaining target should snap");
    assert_eq!(hit.target_kind, SnapTargetKind::Vertex);
    assert_eq!(hit.feature_id, Some(FeatureId::new("lv_poles/target")));
}

#[test]
fn hover_prefers_vertex_then_segment_then_feature_on_ties() {
    let h = SessionHandle::open(vec![line("lv_lines/l1", &[[30.0, 0.0], [30.001, 0.0]])]);

    let vertex = h.hover_at(HoverQuery {
        lng: 30.0,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["lv_lines".to_string()],
        include_vertices: true,
        include_segments: true,
        include_features: true,
        max_candidates: 100,
    });
    let vertex = vertex.expect("vertex hover");
    assert_eq!(vertex.target_kind, HoverTargetKind::Vertex);
    assert_eq!(vertex.feature_id, FeatureId::new("lv_lines/l1"));
    assert_eq!(vertex.vertex_index, Some(0));

    let segment = h.hover_at(HoverQuery {
        lng: 30.0005,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["lv_lines".to_string()],
        include_vertices: true,
        include_segments: true,
        include_features: true,
        max_candidates: 100,
    });
    let segment = segment.expect("segment hover");
    assert_eq!(segment.target_kind, HoverTargetKind::Segment);
    assert_eq!(segment.segment_index, Some(0));

    let feature = h.hover_at(HoverQuery {
        lng: 30.0005,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["lv_lines".to_string()],
        include_vertices: false,
        include_segments: false,
        include_features: true,
        max_candidates: 100,
    });
    assert_eq!(
        feature.map(|result| result.target_kind),
        Some(HoverTargetKind::Feature)
    );
}

#[test]
fn hover_respects_scoped_layer_filter() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope("design", vec![pt("lv_poles/p1", 30.0, 0.0)])
        .unwrap();
    h.mount_scope("local", vec![pt("lv_poles/p2", 30.0, 0.0)])
        .unwrap();

    let hit = h.hover_at(HoverQuery {
        lng: 30.0,
        lat: 0.0,
        tolerance_m: 2.0,
        target_layers: vec!["local:lv_poles".to_string()],
        include_vertices: true,
        include_segments: true,
        include_features: true,
        max_candidates: 100,
    });

    let hit = hit.expect("scoped hover");
    assert_eq!(hit.target_kind, HoverTargetKind::Vertex);
    assert_eq!(hit.feature_id, FeatureId::new("local:lv_poles/p2"));
}

#[test]
fn point_beats_line_when_closer() {
    let h = SessionHandle::open(vec![
        pt("p", 30.0000, 0.0000),
        line("l", &[[30.0010, 0.0000], [30.0020, 0.0000]]),
    ]);
    // Click on the point exactly
    let id = h.pick(30.0000, 0.0000, 200.0);
    assert_eq!(id, Some(FeatureId::new("p")));
}

#[test]
fn point_layers_beat_lines_for_pick_and_hover() {
    let h = SessionHandle::open(vec![
        line("lv_lines/line_1", &[[30.0, 0.0], [30.001, 0.0]]),
        pt("lv_poles/pole_1", 30.0, 3.0 / METERS_PER_LAT_DEG),
    ]);

    assert_eq!(
        h.pick(30.0, 0.0, 5.0),
        Some(FeatureId::new("lv_poles/pole_1"))
    );
    assert_eq!(
        h.pick_radius(30.0, 0.0, 5.0).first(),
        Some(&FeatureId::new("lv_poles/pole_1"))
    );

    let hit = h
        .hover_at(HoverQuery {
            lng: 30.0,
            lat: 0.0,
            tolerance_m: 5.0,
            target_layers: vec!["lv_lines".to_string(), "lv_poles".to_string()],
            include_vertices: true,
            include_segments: true,
            include_features: true,
            max_candidates: 100,
        })
        .expect("stacked pole hover");
    assert_eq!(hit.target_kind, HoverTargetKind::Vertex);
    assert_eq!(hit.feature_id, FeatureId::new("lv_poles/pole_1"));
}

#[test]
fn line_pick_beats_containing_diagnostic_polygon_on_exact_hit() {
    let d = 100.0 / METERS_PER_LAT_DEG;
    let h = SessionHandle::open(vec![
        poly(
            "drafting_errors/cycle_1",
            &[&[
                [30.0 - d, -d],
                [30.0 + d, -d],
                [30.0 + d, d],
                [30.0 - d, d],
                [30.0 - d, -d],
            ]],
        ),
        line("lv_lines/line_1", &[[30.0 - d, 0.0], [30.0 + d, 0.0]]),
    ]);

    assert_eq!(
        h.pick(30.0, 0.0, 5.0),
        Some(FeatureId::new("lv_lines/line_1")),
        "diagnostic polygons may be mounted, but an exact line hit must remain selectable"
    );
}

#[test]
fn negative_tolerance_returns_none() {
    let h = SessionHandle::open(vec![pt("a", 30.0, 0.0)]);
    assert!(h.pick(30.0, 0.0, -1.0).is_none());
}

#[test]
fn deterministic_pick_on_tied_distance() {
    // Two points equidistant from the click — the result must be stable
    // across runs. (Determinism contract: same input → same output.)
    let click = [30.0, 0.0];
    let offset_deg = 5.0 / METERS_PER_LAT_DEG;
    let h = SessionHandle::open(vec![
        pt("a", click[0] - offset_deg, click[1]),
        pt("b", click[0] + offset_deg, click[1]),
    ]);
    let first = h.pick(click[0], click[1], 100.0);
    for _ in 0..10 {
        let again = h.pick(click[0], click[1], 100.0);
        assert_eq!(first, again);
    }
    assert_eq!(first, Some(FeatureId::new("a")));
}

#[test]
fn scoped_mount_prefixes_ids_and_tie_breaks_by_scope_id() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope("local", vec![pt("poles/same", 30.0, 0.0)])
        .unwrap();
    h.mount_scope("design", vec![pt("poles/same", 30.0, 0.0)])
        .unwrap();

    assert_eq!(h.feature_count(), 2);
    assert_eq!(
        h.pick(30.0, 0.0, 1.0),
        Some(FeatureId::new("design:poles/same"))
    );
}

#[test]
fn scope_status_is_wasm_owned_readiness_signal() {
    let mut h = SessionHandle::open(vec![]);

    let before = h.scope_status("design").unwrap();
    assert_eq!(before.phase, "idle");
    assert!(!before.mounted);
    assert_eq!(before.feature_count, 0);

    h.mount_scope("design", vec![]).unwrap();
    let empty_ready = h.scope_status("design").unwrap();
    assert_eq!(empty_ready.phase, "ready");
    assert!(empty_ready.mounted);
    assert_eq!(empty_ready.feature_count, 0);
    assert_eq!(empty_ready.total_feature_count, 0);

    h.mount_scope("design", vec![pt("poles/a", 30.0, 0.0)])
        .unwrap();
    let populated_ready = h.scope_status("design").unwrap();
    assert_eq!(populated_ready.phase, "ready");
    assert!(populated_ready.mounted);
    assert_eq!(populated_ready.feature_count, 1);
    assert_eq!(populated_ready.total_feature_count, 1);
    assert_eq!(
        populated_ready.layers,
        vec![ScopeLayerStatus {
            name: "poles".to_string(),
            geometry_type: Some("Point".to_string()),
            input_feature_count: 1,
            feature_count: 1,
            skipped_feature_count: 0,
            skipped_missing_id_count: 0,
            skipped_unsupported_geometry_count: 0,
        }]
    );

    h.unmount_scope("design").unwrap();
    let after = h.scope_status("design").unwrap();
    assert_eq!(after.phase, "idle");
    assert!(!after.mounted);
    assert_eq!(after.feature_count, 0);
}

#[test]
fn mount_scope_layers_accepts_geojson_and_owns_geometry_parsing() {
    let mut h = SessionHandle::open(vec![]);
    let count = h
        .mount_scope_layers(
            "design",
            vec![
                SessionLayerInput {
                    name: "lv_poles".to_string(),
                    geojson: json!({
                        "type": "FeatureCollection",
                        "features": [{
                            "type": "Feature",
                            "id": "pole-a",
                            "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                            "properties": { "name": "A" }
                        }]
                    }),
                },
                SessionLayerInput {
                    name: "lv_lines".to_string(),
                    geojson: json!({
                        "type": "FeatureCollection",
                        "features": [{
                            "type": "Feature",
                            "properties": { "id": "line-a" },
                            "geometry": { "type": "LineString", "coordinates": [[30.01, -2.0], [30.011, -2.0]] }
                        }, {
                            "type": "Feature",
                            "properties": { "id": "multi-a" },
                            "geometry": { "type": "MultiLineString", "coordinates": [] }
                        }]
                    }),
                },
            ],
        )
        .unwrap();

    assert_eq!(count, 2);
    assert_eq!(
        h.pick(30.0, -2.0, 1.0),
        Some(FeatureId::new("design:lv_poles/pole-a"))
    );
    let status = h.scope_status("design").unwrap();
    assert_eq!(status.feature_count, 2);
    assert_eq!(
        status.layers,
        vec![
            ScopeLayerStatus {
                name: "lv_lines".to_string(),
                geometry_type: Some("LineString".to_string()),
                input_feature_count: 2,
                feature_count: 1,
                skipped_feature_count: 1,
                skipped_missing_id_count: 0,
                skipped_unsupported_geometry_count: 1,
            },
            ScopeLayerStatus {
                name: "lv_poles".to_string(),
                geometry_type: Some("Point".to_string()),
                input_feature_count: 1,
                feature_count: 1,
                skipped_feature_count: 0,
                skipped_missing_id_count: 0,
                skipped_unsupported_geometry_count: 0,
            },
        ]
    );
    assert_eq!(status.input_feature_count, 3);
    assert_eq!(status.skipped_feature_count, 1);
    assert_eq!(status.skipped_unsupported_geometry_count, 1);
}

#[test]
fn mount_scope_layers_explodes_multilinestring_into_independent_lines() {
    let mut h = SessionHandle::open(vec![]);
    let count = h
        .mount_scope_layers(
            "design",
            vec![SessionLayerInput {
                name: "lv_lines".to_string(),
                geojson: json!({
                    "type": "FeatureCollection",
                    "features": [{
                        "type": "Feature",
                        "properties": { "id": "line-a", "cable_size": "ABC 50" },
                        "geometry": {
                            "type": "MultiLineString",
                            "coordinates": [
                                [[30.0, -2.0], [30.001, -2.0]],
                                [[30.002, -2.0], [30.003, -2.0]]
                            ]
                        }
                    }]
                }),
            }],
        )
        .unwrap();

    assert_eq!(count, 2);
    assert_eq!(
        h.pick(30.0005, -2.0, 100.0),
        Some(FeatureId::new("design:lv_lines/line-a"))
    );
    assert_eq!(
        h.pick(30.0025, -2.0, 100.0),
        Some(FeatureId::new("design:lv_lines/line-a__part_2"))
    );
    let status = h.scope_status("design").unwrap();
    assert_eq!(status.input_feature_count, 1);
    assert_eq!(status.feature_count, 2);
    assert_eq!(status.skipped_feature_count, 0);
    assert_eq!(
        status.layers[0].geometry_type,
        Some("LineString".to_string())
    );
}

#[test]
fn mount_scope_layers_resolves_doc_id_and_feature_id_for_uploaded_features() {
    // Regression: uploaded MV / shapefile / PLS-CADD features arrive
    // carrying `properties.doc_id` or `properties.feature_id` rather
    // than a top-level `id` or `properties.id`. Before the fix to
    // `geojson_feature_id`, every such feature was silently dropped
    // during mount and the later vertex / move / selection lookups
    // missed because the WASM session had never indexed them.
    let mut h = SessionHandle::open(vec![]);
    let count = h
        .mount_scope_layers(
            "design",
            vec![SessionLayerInput {
                name: "mv_lines".to_string(),
                geojson: json!({
                    "type": "FeatureCollection",
                    "features": [
                        { "type": "Feature",
                          "properties": { "doc_id": "doc-abc" },
                          "geometry": { "type": "LineString",
                            "coordinates": [[30.0, -2.0], [30.001, -2.0]] } },
                        { "type": "Feature",
                          "properties": { "feature_id": "feat-xyz" },
                          "geometry": { "type": "LineString",
                            "coordinates": [[30.002, -2.0], [30.003, -2.0]] } },
                        { "type": "Feature",
                          "id": "top-level-id",
                          "properties": { "doc_id": "should-be-ignored-when-top-level-wins" },
                          "geometry": { "type": "LineString",
                            "coordinates": [[30.004, -2.0], [30.005, -2.0]] } },
                    ],
                }),
            }],
        )
        .unwrap();

    assert_eq!(count, 3, "all three identity shapes must be indexed");
    let status = h.scope_status("design").unwrap();
    assert_eq!(status.feature_count, 3);
    // The selectable feature ids encode under the canonical
    // `scope:layer/feature_id` shape so getRow / selection / vertex
    // lookups land on the right rows.
    let mut keys: Vec<String> = h.features.keys().map(|fid| fid.0.clone()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "design:mv_lines/doc-abc",
            "design:mv_lines/feat-xyz",
            "design:mv_lines/top-level-id",
        ]
    );
}

#[test]
fn unmount_scope_removes_only_that_scope_and_prunes_selection() {
    let mut h = SessionHandle::open(vec![]);
    h.mount_scope("design", vec![pt("poles/a", 30.0, 0.0)])
        .unwrap();
    h.mount_scope("local", vec![pt("sketches/a", 30.1, 0.0)])
        .unwrap();
    h.select(
        vec![
            FeatureId::new("design:poles/a"),
            FeatureId::new("local:sketches/a"),
        ],
        SelectionMode::Replace,
    );
    // Point hover + last_edited at the scope being torn down so the bulk
    // prune has to clear all three selection slots for the removed id.
    h.hover(Some(FeatureId::new("design:poles/a")));
    h.mark_edited(FeatureId::new("design:poles/a"));

    assert_eq!(h.unmount_scope("design").unwrap(), 1);

    assert_eq!(h.feature_count(), 1);
    // Only the surviving scope's feature remains indexed.
    let survivors: Vec<String> = h.features.keys().map(|id| id.0.clone()).collect();
    assert_eq!(survivors, vec!["local:sketches/a".to_string()]);
    // Every selection slot pointing into the removed scope is cleared; the
    // other scope's selection is untouched.
    assert_eq!(
        h.selection().selected,
        vec![FeatureId::new("local:sketches/a")]
    );
    assert!(h.selection().hovered.is_none());
    assert!(h.selection().last_edited.is_none());
    assert!(h.pick(30.0, 0.0, 1.0).is_none());
    assert_eq!(
        h.pick(30.1, 0.0, 1.0),
        Some(FeatureId::new("local:sketches/a"))
    );
    // The surviving scope keeps its DataFrame table; the torn-down scope's
    // tables are dropped wholesale by the bulk `{scope}:` prefix sweep.
    #[cfg(feature = "enable_dataframes")]
    {
        assert!(h.tables.contains_key("local:sketches"));
        assert!(!h.tables.keys().any(|key| key.starts_with("design:")));
    }
}

/// Perf-regression guard for edit-session exit teardown, mirroring
/// `ds-graph::build_graph_stays_subquadratic_*`. Scope unmount used to
/// remove features one at a time, each rebuilding the whole layer
/// DataFrame (`to_features` + `from_features`), so tearing down a scope of
/// H features was O(H^2) — the multi-second "page unresponsive" freeze on
/// exiting a large MV/LV transformer (PAIN_HISTORY.md Tombstone #2 at the
/// unmount path). The fix drops the scope's tables wholesale and rebuilds
/// the R-tree once, making teardown linear.
///
/// This is a **shape** check, not an absolute-time bound: an unoptimized
/// debug test build is far too slow for a wall-clock threshold to mean
/// anything, but the ratio between two sizes exposes a reintroduced
/// per-feature DataFrame rebuild regardless of machine speed. 4x the
/// features costs ~4-5x linear and ~16x quadratic; the threshold sits
/// comfortably between. Gated on `enable_dataframes` because the regressed
/// cost lived entirely in the per-feature table round-trip.
#[cfg(feature = "enable_dataframes")]
#[test]
fn unmount_scope_stays_subquadratic_on_large_scopes() {
    // Spread features across several layers so the (still per-feature) mount
    // setup stays affordable; the teardown shape is what we measure.
    const LAYER_COUNT: usize = 10;

    fn scoped_layer_features(count: usize, base_lng: f64) -> Vec<Feature> {
        (0..count)
            .map(|i| {
                let layer = i % LAYER_COUNT;
                let lng = base_lng + (i as f64) * 1e-4;
                pt(&format!("layer{layer}/feat_{i}"), lng, 0.0)
            })
            .collect()
    }

    // Keep a same-size sibling scope mounted so teardown does real survivor
    // work (`rebuild_projected_index`) rather than short-circuiting to
    // `clear_projection` on an empty session — a stabler timing signal.
    fn mounted(count: usize) -> SessionHandle {
        let mut h = SessionHandle::open(vec![]);
        h.mount_scope("keep", scoped_layer_features(count, 30.0))
            .unwrap();
        h.mount_scope("design", scoped_layer_features(count, 45.0))
            .unwrap();
        h
    }

    fn best_unmount_time(count: usize) -> std::time::Duration {
        let mut best = std::time::Duration::MAX;
        for _ in 0..3 {
            let mut h = mounted(count);
            let start = std::time::Instant::now();
            let removed = h.unmount_scope("design").unwrap();
            best = best.min(start.elapsed());
            assert_eq!(removed, count);
            std::hint::black_box(&h);
        }
        best
    }

    let small = 500;
    let large = 2_000; // 4x small

    let ratio =
        best_unmount_time(large).as_secs_f64() / best_unmount_time(small).as_secs_f64().max(1e-6);
    assert!(
        ratio < 10.0,
        "unmount_scope scaling ratio {ratio:.1} for 4x features — per-feature \
         DataFrame-rebuild regression on teardown? (linear ~= 4-5x, quadratic ~= 16x)"
    );
}

/// Perf-regression guard for edit-session ENTER (scope mount), the mirror
/// of `unmount_scope_stays_subquadratic_on_large_scopes`. Mount used to
/// insert incoming features one at a time, each rebuilding the whole layer
/// DataFrame (`to_features` + `from_features` via `put_feature_no_log`), so
/// mounting M features was O(M^2) — the "first geometry create freezes the
/// tab" freeze on a large `terrain_pls` layer (PAIN_HISTORY.md §1). The fix
/// builds each layer table once and rebuilds the R-tree once, making mount
/// linear. Same shape (ratio, not wall-clock) check as the unmount guard.
#[cfg(feature = "enable_dataframes")]
#[test]
fn mount_scope_stays_subquadratic_on_large_scopes() {
    const LAYER_COUNT: usize = 10;

    fn scoped_layer_features(count: usize, base_lng: f64) -> Vec<Feature> {
        (0..count)
            .map(|i| {
                let layer = i % LAYER_COUNT;
                let lng = base_lng + (i as f64) * 1e-4;
                pt(&format!("layer{layer}/feat_{i}"), lng, 0.0)
            })
            .collect()
    }

    // Keep a same-size sibling scope mounted so the mount does real survivor
    // re-projection work instead of short-circuiting on an empty session.
    fn best_mount_time(count: usize) -> std::time::Duration {
        let mut best = std::time::Duration::MAX;
        for _ in 0..3 {
            let mut h = SessionHandle::open(vec![]);
            h.mount_scope("keep", scoped_layer_features(count, 30.0))
                .unwrap();
            let incoming = scoped_layer_features(count, 45.0);
            let start = std::time::Instant::now();
            let mounted = h.mount_scope("design", incoming).unwrap();
            best = best.min(start.elapsed());
            assert_eq!(mounted, count);
            std::hint::black_box(&h);
        }
        best
    }

    let small = 500;
    let large = 2_000; // 4x small

    let ratio =
        best_mount_time(large).as_secs_f64() / best_mount_time(small).as_secs_f64().max(1e-6);
    assert!(
        ratio < 10.0,
        "mount_scope scaling ratio {ratio:.1} for 4x features — per-feature \
         DataFrame-rebuild regression on mount? (linear ~= 4-5x, quadratic ~= 16x)"
    );
}

/// Behavioral equivalence for the bulk mount: a duplicate scoped id inside
/// one mount is deduped last-wins (the old per-feature `put_feature_no_log`
/// replaced on repeat), the count reflects the deduped total, the layer
/// table carries exactly the surviving row, and `last_edited` points at the
/// final input feature.
#[cfg(feature = "enable_dataframes")]
#[test]
fn mount_scope_dedupes_duplicate_ids_last_wins() {
    let mut h = SessionHandle::open(vec![]);
    let count = h
        .mount_scope(
            "design",
            vec![
                pt_with_props("lv_poles/p1", 30.0, -2.0, json!({ "v": "first" })),
                pt_with_props("lv_lines/l1", 30.01, -2.0, json!({ "v": "line" })),
                pt_with_props("lv_poles/p1", 30.02, -2.0, json!({ "v": "second" })),
            ],
        )
        .unwrap();

    // Two distinct scoped ids after last-wins dedupe (raw input was 3).
    assert_eq!(count, 2);
    assert_eq!(h.feature_count(), 2);

    // The surviving `p1` row is the last-written one, on the scoped table key.
    let row = h.get_row("design:lv_poles", "p1").unwrap().unwrap();
    assert_eq!(row.properties.get("v"), Some(&json!("second")));
    assert_eq!(h.tables.get("design:lv_poles").unwrap().height(), 1);
    assert_eq!(h.tables.get("design:lv_lines").unwrap().height(), 1);

    // R-tree resolves the surviving geometry (last-wins coord), not the first.
    assert_eq!(
        h.pick(30.02, -2.0, 1.0),
        Some(FeatureId::new("design:lv_poles/p1"))
    );
    assert!(h.pick(30.0, -2.0, 1.0).is_none());

    // `last_edited` is the final input feature (the second `p1`).
    assert_eq!(
        h.selection().last_edited,
        Some(FeatureId::new("design:lv_poles/p1"))
    );
}

#[test]
fn session_reports_feature_count() {
    let h = SessionHandle::open(vec![pt("a", 0.0, 0.0), pt("b", 1.0, 1.0)]);
    assert_eq!(h.feature_count(), 2);
}

fn poly(id: &str, rings: &[&[[f64; 2]]]) -> Feature {
    Feature::new(
        id,
        Geometry::Polygon {
            rings: rings.iter().map(|r| r.to_vec()).collect(),
        },
    )
}

#[test]
fn polygon_pick_inside_returns_hit() {
    // 200m x 200m square centered around (30, 0)
    let d = 100.0 / METERS_PER_LAT_DEG;
    let h = SessionHandle::open(vec![poly(
        "p",
        &[&[
            [30.0 - d, -d],
            [30.0 + d, -d],
            [30.0 + d, d],
            [30.0 - d, d],
            [30.0 - d, -d],
        ]],
    )]);
    // Click at the centroid — strictly inside.
    let id = h.pick(30.0, 0.0, 1.0);
    assert_eq!(id, Some(FeatureId::new("p")));
}

#[test]
fn polygon_pick_outside_within_tolerance_hits_via_edge_distance() {
    let d = 100.0 / METERS_PER_LAT_DEG;
    let h = SessionHandle::open(vec![poly(
        "p",
        &[&[
            [30.0 - d, -d],
            [30.0 + d, -d],
            [30.0 + d, d],
            [30.0 - d, d],
            [30.0 - d, -d],
        ]],
    )]);
    // 5 m east of the right edge — within a 10 m tolerance.
    let outside_lng = 30.0 + d + 5.0 / METERS_PER_LAT_DEG;
    let id = h.pick(outside_lng, 0.0, 10.0);
    assert_eq!(id, Some(FeatureId::new("p")));
}

#[test]
fn polygon_pick_in_hole_treats_hole_boundary_as_distance() {
    // Outer 200x200, hole 40x40 centered on (30, 0).
    let outer = 100.0 / METERS_PER_LAT_DEG;
    let hole = 20.0 / METERS_PER_LAT_DEG;
    let h = SessionHandle::open(vec![poly(
        "p",
        &[
            &[
                [30.0 - outer, -outer],
                [30.0 + outer, -outer],
                [30.0 + outer, outer],
                [30.0 - outer, outer],
                [30.0 - outer, -outer],
            ],
            &[
                [30.0 - hole, -hole],
                [30.0 + hole, -hole],
                [30.0 + hole, hole],
                [30.0 - hole, hole],
                [30.0 - hole, -hole],
            ],
        ],
    )]);
    // Click in the hole's centre — inside the outer ring but inside
    // the hole, so distance is to the hole boundary (~20 m).
    // Tolerance 10 m → miss.
    assert!(h.pick(30.0, 0.0, 10.0).is_none());
    // Tolerance 30 m → hit.
    assert_eq!(h.pick(30.0, 0.0, 30.0), Some(FeatureId::new("p")));
}

#[test]
fn insert_then_pick_returns_inserted_feature() {
    let mut h = SessionHandle::open(vec![pt("existing", 0.0, 0.0)]);
    h.insert(pt("fresh", 30.0, -2.0)).unwrap();
    let id = h.pick(30.0, -2.0, 1.0);
    assert_eq!(id, Some(FeatureId::new("fresh")));
    assert_eq!(h.feature_count(), 2);
}

#[test]
fn insert_collision_errors_and_does_not_alter_state() {
    let mut h = SessionHandle::open(vec![pt("a", 30.0, -2.0)]);
    let err = h.insert(pt("a", 30.001, -2.0)).unwrap_err();
    assert!(err.contains("`a`"));
    // Original geometry untouched.
    assert_eq!(h.pick(30.0, -2.0, 1.0), Some(FeatureId::new("a")));
    assert_eq!(h.feature_count(), 1);
}

#[test]
fn create_geojson_feature_mints_local_id_and_sanitizes_properties() {
    let mut h = SessionHandle::open(vec![]);

    let diff = h
        .create_geojson_feature(
            "design:tr",
            json!({ "type": "Point", "coordinates": [30.0, -2.0] }),
            json!({
                "pole_number": "P1",
                "geometry": "reserved",
                "type": "reserved",
                "bbox": [1, 2, 3, 4]
            })
            .as_object()
            .cloned()
            .unwrap(),
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "create_feature");
    assert_eq!(diff.changes.len(), 1);
    let created = diff.changes[0].after.as_ref().unwrap();
    assert_eq!(created.id, FeatureId::new("design:tr/tr_00000001"));
    assert_eq!(created.properties.get("id"), Some(&json!("tr_00000001")));
    assert_eq!(created.properties.get("pole_number"), Some(&json!("P1")));
    assert!(!created.properties.contains_key("geometry"));
    assert!(!created.properties.contains_key("type"));
    assert!(!created.properties.contains_key("bbox"));
    assert_eq!(
        h.pick(30.0, -2.0, 1.0),
        Some(FeatureId::new("design:tr/tr_00000001"))
    );

    let undo = h.undo().unwrap().unwrap();
    assert_eq!(undo.kind, "undo");
    assert!(h.pick(30.0, -2.0, 1.0).is_none());
    assert_eq!(h.feature_count(), 0);
}

#[test]
fn remove_drops_from_pick_results() {
    let mut h = SessionHandle::open(vec![pt("a", 30.0, -2.0), pt("b", 30.001, -2.0)]);
    let removed = h.remove(&FeatureId::new("a")).unwrap().unwrap();
    assert_eq!(removed.id, FeatureId::new("a"));
    assert!(h.pick(30.0, -2.0, 1.0).is_none());
    assert_eq!(h.feature_count(), 1);
}

#[test]
fn remove_missing_id_is_none_and_idempotent() {
    let mut h = SessionHandle::open(vec![pt("a", 30.0, -2.0)]);
    assert!(h.remove(&FeatureId::new("ghost")).unwrap().is_none());
    assert!(h.remove(&FeatureId::new("ghost")).unwrap().is_none());
    assert_eq!(h.feature_count(), 1);
}

#[test]
fn remove_features_and_layer_return_diffs_and_are_undoable() {
    let mut h = SessionHandle::open(vec![
        pt("design:tr/a", 30.0, -2.0),
        pt("design:tr/b", 30.001, -2.0),
        pt("design:other/c", 30.002, -2.0),
    ]);
    h.select(
        vec![FeatureId::new("design:tr/a"), FeatureId::new("design:tr/b")],
        SelectionMode::Replace,
    );
    h.hover(Some(FeatureId::new("design:tr/a")));

    let diff = h
        .remove_features(
            "design:tr",
            vec!["a".to_string(), "b".to_string(), "b".to_string()],
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "remove_features");
    assert_eq!(diff.changes.len(), 2);
    assert_eq!(h.feature_count(), 1);
    assert_eq!(
        h.pick(30.002, -2.0, 1.0),
        Some(FeatureId::new("design:other/c"))
    );
    let selection = h.selection();
    assert!(selection.selected.is_empty());
    assert!(selection.hovered.is_none());

    let undo = h.undo().unwrap().unwrap();
    assert_eq!(undo.kind, "undo");
    assert_eq!(h.feature_count(), 3);

    let layer_diff = h.remove_layer("design:tr").unwrap().unwrap();
    assert_eq!(layer_diff.kind, "remove_layer");
    assert_eq!(layer_diff.changes.len(), 2);
    assert_eq!(h.feature_count(), 1);
    assert_eq!(
        h.pick(30.002, -2.0, 1.0),
        Some(FeatureId::new("design:other/c"))
    );
}

#[test]
fn replace_layer_geojson_features_mints_missing_ids_and_rebuilds_layer() {
    let mut h = SessionHandle::open(vec![
        pt("design:tr/old", 30.0, -2.0),
        pt("design:other/keep", 30.002, -2.0),
    ]);

    let diff = h
        .replace_layer_geojson_features(
            "design:tr",
            vec![
                json!({
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [31.0, -3.0] },
                    "properties": { "pole_number": "P1" }
                }),
                json!({
                    "type": "Feature",
                    "id": "provided",
                    "geometry": { "type": "Point", "coordinates": [31.1, -3.1] },
                    "properties": { "id": "provided", "pole_number": "P2" }
                }),
                json!({
                    "type": "Feature",
                    "id": "provided",
                    "geometry": { "type": "Point", "coordinates": [31.2, -3.2] },
                    "properties": { "id": "provided", "pole_number": "P3" }
                }),
            ],
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "replace_layer");
    assert_eq!(diff.changes.len(), 4);
    assert!(h.pick(30.0, -2.0, 1.0).is_none());
    assert_eq!(
        h.pick(31.0, -3.0, 1.0),
        Some(FeatureId::new("design:tr/tr_00000001"))
    );
    assert_eq!(
        h.pick(31.1, -3.1, 1.0),
        Some(FeatureId::new("design:tr/provided"))
    );
    assert_eq!(
        h.pick(31.2, -3.2, 1.0),
        Some(FeatureId::new("design:tr/tr_00000002"))
    );
    assert_eq!(
        h.pick(30.002, -2.0, 1.0),
        Some(FeatureId::new("design:other/keep"))
    );

    h.undo().unwrap().unwrap();
    assert_eq!(
        h.pick(30.0, -2.0, 1.0),
        Some(FeatureId::new("design:tr/old"))
    );
    assert!(h.pick(31.0, -3.0, 1.0).is_none());
}

#[test]
fn replace_swaps_geometry_under_same_id() {
    let mut h = SessionHandle::open(vec![pt("a", 30.0, -2.0)]);
    let old = h.replace(pt("a", 30.5, -2.5)).unwrap().unwrap();
    match old.geometry {
        Geometry::Point { coords } => assert_eq!(coords, [30.0, -2.0]),
        _ => panic!("expected Point"),
    }
    assert!(h.pick(30.0, -2.0, 100.0).is_none());
    assert_eq!(h.pick(30.5, -2.5, 1.0), Some(FeatureId::new("a")));
    assert_eq!(h.feature_count(), 1);
}

#[test]
fn replace_on_missing_id_inserts_and_returns_none() {
    let mut h = SessionHandle::open(vec![pt("existing", 0.0, 0.0)]);
    assert!(h.replace(pt("fresh", 30.0, -2.0)).unwrap().is_none());
    assert_eq!(h.feature_count(), 2);
    assert_eq!(h.pick(30.0, -2.0, 1.0), Some(FeatureId::new("fresh")));
}

#[test]
fn replace_existing_geojson_feature_returns_diff_without_inserting_missing_rows() {
    let mut h = SessionHandle::open(vec![pt("design:tr/a", 30.0, -2.0)]);

    let diff = h
        .replace_existing_geojson_feature(
            "design:tr",
            json!({
                "type": "Feature",
                "id": "a",
                "geometry": { "type": "Point", "coordinates": [31.0, -3.0] },
                "properties": { "id": "a", "pole_number": "P1" }
            }),
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "replace_feature");
    assert_eq!(diff.changes[0].feature_id, FeatureId::new("design:tr/a"));
    assert_eq!(h.pick(31.0, -3.0, 1.0), Some(FeatureId::new("design:tr/a")));

    let missing = h
        .replace_existing_geojson_feature(
            "design:tr",
            json!({
                "type": "Feature",
                "id": "missing",
                "geometry": { "type": "Point", "coordinates": [32.0, -4.0] },
                "properties": { "id": "missing" }
            }),
        )
        .unwrap();

    assert!(missing.is_none());
    assert_eq!(h.feature_count(), 1);
    assert!(h.pick(32.0, -4.0, 1.0).is_none());
}

#[test]
fn pick_radius_returns_features_inside_radius_only() {
    let h = SessionHandle::open(vec![
        pt("a", 30.0, 0.0),
        pt("b", 30.0001, 0.0), // ~11 m east
        pt("c", 30.0010, 0.0), // ~111 m east
    ]);
    // Click at "a" with 50 m radius → hits a + b but not c.
    let hits = h.pick_radius(30.0, 0.0, 50.0);
    let ids: Vec<&str> = hits.iter().map(|id| id.0.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

#[test]
fn pick_radius_orders_nearest_first() {
    let h = SessionHandle::open(vec![
        pt("far", 30.0010, 0.0),  // ~111 m
        pt("near", 30.0001, 0.0), // ~11 m
        pt("mid", 30.0005, 0.0),  // ~55 m
    ]);
    let hits = h.pick_radius(30.0, 0.0, 200.0);
    let ids: Vec<&str> = hits.iter().map(|id| id.0.as_str()).collect();
    assert_eq!(ids, vec!["near", "mid", "far"]);
}

#[test]
fn pick_radius_empty_for_negative_radius() {
    let h = SessionHandle::open(vec![pt("a", 30.0, 0.0)]);
    assert!(h.pick_radius(30.0, 0.0, -1.0).is_empty());
}

#[test]
fn pick_radius_includes_polygons_when_inside() {
    let d = 100.0 / METERS_PER_LAT_DEG;
    let h = SessionHandle::open(vec![
        Feature::new(
            "square",
            Geometry::Polygon {
                rings: vec![vec![
                    [30.0 - d, -d],
                    [30.0 + d, -d],
                    [30.0 + d, d],
                    [30.0 - d, d],
                    [30.0 - d, -d],
                ]],
            },
        ),
        pt("near", 30.0001, 0.0),
    ]);
    // Click at centre — polygon distance is 0 (inside), point ~11 m east.
    let hits = h.pick_radius(30.0, 0.0, 50.0);
    let ids: Vec<&str> = hits.iter().map(|id| id.0.as_str()).collect();
    // Polygon (distance 0) wins the tiebreaker.
    assert_eq!(ids, vec!["square", "near"]);
}

#[test]
fn fresh_session_has_empty_selection() {
    let h = SessionHandle::open(vec![pt("a", 0.0, 0.0)]);
    let sel = h.selection();
    assert!(sel.selected.is_empty());
    assert!(sel.hovered.is_none());
    assert!(sel.last_edited.is_none());
}

#[test]
fn select_replace_overwrites_existing() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0), pt("b", 1.0, 1.0)]);
    h.select(vec![FeatureId::new("a")], SelectionMode::Replace);
    h.select(vec![FeatureId::new("b")], SelectionMode::Replace);
    assert_eq!(h.selection().selected, vec![FeatureId::new("b")]);
}

#[test]
fn select_add_dedupes_and_appends() {
    let mut h = SessionHandle::open(vec![
        pt("a", 0.0, 0.0),
        pt("b", 1.0, 1.0),
        pt("c", 2.0, 2.0),
    ]);
    h.select(
        vec![FeatureId::new("a"), FeatureId::new("b")],
        SelectionMode::Replace,
    );
    h.select(
        vec![FeatureId::new("b"), FeatureId::new("c")],
        SelectionMode::Add,
    );
    assert_eq!(
        h.selection().selected,
        vec![
            FeatureId::new("a"),
            FeatureId::new("b"),
            FeatureId::new("c")
        ],
    );
}

#[test]
fn select_toggle_removes_present_adds_absent() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0), pt("b", 1.0, 1.0)]);
    h.select(vec![FeatureId::new("a")], SelectionMode::Replace);
    h.select(
        vec![FeatureId::new("a"), FeatureId::new("b")],
        SelectionMode::Toggle,
    );
    // a was present → removed; b was absent → added.
    assert_eq!(h.selection().selected, vec![FeatureId::new("b")]);
}

#[test]
fn select_ignores_ids_that_are_not_in_the_session() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0)]);
    h.select(
        vec![FeatureId::new("ghost"), FeatureId::new("a")],
        SelectionMode::Replace,
    );
    assert_eq!(h.selection().selected, vec![FeatureId::new("a")]);

    h.select(vec![FeatureId::new("ghost")], SelectionMode::Add);
    assert_eq!(h.selection().selected, vec![FeatureId::new("a")]);

    h.select(vec![FeatureId::new("ghost")], SelectionMode::Toggle);
    assert_eq!(h.selection().selected, vec![FeatureId::new("a")]);
}

fn selector_square(min: [f64; 2], max: [f64; 2]) -> Value {
    json!({
        "type": "Polygon",
        "coordinates": [[
            [min[0], min[1]],
            [max[0], min[1]],
            [max[0], max[1]],
            [min[0], max[1]],
            [min[0], min[1]],
        ]]
    })
}

#[test]
fn select_by_polygon_reuses_selection_modes_and_layer_filter() {
    let mut h = SessionHandle::open(vec![
        pt("design:lv_poles/p1", 30.0, 0.0),
        pt("design:lv_poles/p2", 30.001, 0.001),
        line("design:lv_lines/l1", &[[29.999, 0.001], [30.002, 0.001]]),
        pt("design:customers/c1", 30.0005, 0.0005),
        pt("design:lv_poles/outside", 30.02, 0.02),
    ]);

    let selector = selector_square([29.9995, -0.0005], [30.0015, 0.0015]);
    let selected = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector.clone(),
            target_layers: vec!["design:lv_poles".to_string(), "design:lv_lines".to_string()],
            predicate: PolygonSelectionPredicate::Intersects,
            mode: SelectionMode::Replace,
            max_candidates: default_polygon_selection_max_candidates(),
        })
        .unwrap();
    assert_eq!(
        selected.selected,
        vec![
            FeatureId::new("design:lv_lines/l1"),
            FeatureId::new("design:lv_poles/p1"),
            FeatureId::new("design:lv_poles/p2"),
        ],
    );

    let selected = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector.clone(),
            target_layers: vec!["design:customers".to_string()],
            predicate: PolygonSelectionPredicate::Intersects,
            mode: SelectionMode::Add,
            max_candidates: default_polygon_selection_max_candidates(),
        })
        .unwrap();
    assert!(
        selected
            .selected
            .contains(&FeatureId::new("design:customers/c1"))
    );

    let selected = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector,
            target_layers: vec!["design:lv_poles".to_string()],
            predicate: PolygonSelectionPredicate::Intersects,
            mode: SelectionMode::Toggle,
            max_candidates: default_polygon_selection_max_candidates(),
        })
        .unwrap();
    assert!(
        !selected
            .selected
            .contains(&FeatureId::new("design:lv_poles/p1"))
    );
    assert!(
        !selected
            .selected
            .contains(&FeatureId::new("design:lv_poles/p2"))
    );
    assert!(
        selected
            .selected
            .contains(&FeatureId::new("design:lv_lines/l1"))
    );
    assert!(
        selected
            .selected
            .contains(&FeatureId::new("design:customers/c1"))
    );
}

#[test]
fn select_by_polygon_within_excludes_crossing_lines() {
    let mut h = SessionHandle::open(vec![
        line(
            "design:lv_lines/inside",
            &[[30.0001, 0.0001], [30.0008, 0.0008]],
        ),
        line(
            "design:lv_lines/crossing",
            &[[29.999, 0.0005], [30.001, 0.0005]],
        ),
        pt("design:lv_poles/p1", 30.0005, 0.0005),
    ]);
    let selector = selector_square([30.0, 0.0], [30.001, 0.001]);

    let intersects = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector.clone(),
            target_layers: vec!["design:lv_lines".to_string(), "design:lv_poles".to_string()],
            predicate: PolygonSelectionPredicate::Intersects,
            mode: SelectionMode::Replace,
            max_candidates: default_polygon_selection_max_candidates(),
        })
        .unwrap();
    assert_eq!(
        intersects.selected,
        vec![
            FeatureId::new("design:lv_lines/crossing"),
            FeatureId::new("design:lv_lines/inside"),
            FeatureId::new("design:lv_poles/p1"),
        ],
    );

    let within = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector,
            target_layers: vec!["design:lv_lines".to_string(), "design:lv_poles".to_string()],
            predicate: PolygonSelectionPredicate::Within,
            mode: SelectionMode::Replace,
            max_candidates: default_polygon_selection_max_candidates(),
        })
        .unwrap();
    assert_eq!(
        within.selected,
        vec![
            FeatureId::new("design:lv_lines/inside"),
            FeatureId::new("design:lv_poles/p1"),
        ],
    );
}

#[test]
fn select_by_polygon_outside_excludes_crossings_and_scans_with_bounds() {
    let mut h = SessionHandle::open(vec![
        pt("design:lv_poles/boundary", 30.0, 0.0),
        pt("design:lv_poles/outside", 30.02, 0.02),
        line(
            "design:lv_lines/crossing",
            &[[29.999, 0.0005], [30.002, 0.0005]],
        ),
        pt("design:customers/outside", 30.02, 0.02),
    ]);
    let mut query = PolygonSelectionQuery {
        polygon: selector_square([30.0, 0.0], [30.001, 0.001]),
        target_layers: vec!["design:lv_poles".into(), "design:lv_lines".into()],
        predicate: PolygonSelectionPredicate::Outside,
        mode: SelectionMode::Replace,
        max_candidates: default_polygon_selection_max_candidates(),
    };
    assert_eq!(
        h.select_by_polygon(query.clone()).unwrap().selected,
        vec![FeatureId::new("design:lv_poles/outside")]
    );
    query.max_candidates = 1;
    assert!(h.select_by_polygon(query).unwrap_err().contains("exceeded"));
    assert_eq!(
        h.selection().selected,
        vec![FeatureId::new("design:lv_poles/outside")]
    );
}

#[test]
fn select_by_polygon_empty_target_layers_selects_nothing() {
    // Empty `target_layers` must NOT fan out to "all mounted layers" — that
    // overload is the documented main-thread freeze footgun for lasso. A
    // selector covering every feature with no target layers selects nothing.
    let mut h = SessionHandle::open(vec![
        pt("design:lv_poles/p1", 30.0, 0.0),
        pt("design:lv_poles/p2", 30.001, 0.001),
        line("design:lv_lines/l1", &[[29.999, 0.001], [30.002, 0.001]]),
    ]);
    let selector = selector_square([29.9, -0.1], [30.1, 0.1]);
    let selected = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector,
            target_layers: vec![],
            predicate: PolygonSelectionPredicate::Intersects,
            mode: SelectionMode::Replace,
            max_candidates: default_polygon_selection_max_candidates(),
        })
        .unwrap();
    assert!(
        selected.selected.is_empty(),
        "empty target layers must select nothing, got {:?}",
        selected.selected
    );
}

#[test]
fn select_by_polygon_bounds_candidate_scan() {
    // A selector that covers the whole session with explicit target layers
    // must not run an unbounded exact-geometry scan: the candidate cap turns
    // a pathological global lasso into a diagnosable error, never a freeze.
    let mut h = SessionHandle::open(vec![
        pt("design:lv_poles/p1", 30.0, 0.0),
        pt("design:lv_poles/p2", 30.001, 0.001),
        pt("design:lv_poles/p3", 30.002, 0.002),
    ]);
    let selector = selector_square([29.9, -0.1], [30.1, 0.1]);
    let err = h
        .select_by_polygon(PolygonSelectionQuery {
            polygon: selector,
            target_layers: vec!["design:lv_poles".to_string()],
            predicate: PolygonSelectionPredicate::Intersects,
            mode: SelectionMode::Replace,
            max_candidates: 2,
        })
        .unwrap_err();
    assert!(
        err.contains("exceeded"),
        "expected a candidate-cap error, got {err:?}"
    );
}

#[test]
fn hover_is_independent_of_selected() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0), pt("b", 1.0, 1.0)]);
    h.select(vec![FeatureId::new("a")], SelectionMode::Replace);
    h.hover(Some(FeatureId::new("b")));
    assert_eq!(h.selection().selected, vec![FeatureId::new("a")]);
    assert_eq!(h.selection().hovered, Some(FeatureId::new("b")));
    h.hover(None);
    assert!(h.selection().hovered.is_none());
    // selection is intact through hover changes.
    assert_eq!(h.selection().selected, vec![FeatureId::new("a")]);
}

#[test]
fn hover_and_mark_edited_clear_on_missing_id() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0)]);
    h.hover(Some(FeatureId::new("ghost")));
    assert!(h.selection().hovered.is_none());

    h.mark_edited(FeatureId::new("ghost"));
    assert!(h.selection().last_edited.is_none());
}

#[test]
fn mark_edited_stamps_last_edited() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0)]);
    h.mark_edited(FeatureId::new("a"));
    assert_eq!(h.selection().last_edited, Some(FeatureId::new("a")));
}

#[test]
fn clear_selection_drops_all_three_slots() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0)]);
    h.select(vec![FeatureId::new("a")], SelectionMode::Replace);
    h.hover(Some(FeatureId::new("a")));
    h.mark_edited(FeatureId::new("a"));
    h.clear_selection();
    let sel = h.selection();
    assert!(sel.selected.is_empty());
    assert!(sel.hovered.is_none());
    assert!(sel.last_edited.is_none());
}

#[test]
fn replace_preserves_selection_slots_for_same_id() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0)]);
    h.select(vec![FeatureId::new("a")], SelectionMode::Replace);
    h.hover(Some(FeatureId::new("a")));
    h.mark_edited(FeatureId::new("a"));

    h.replace(pt("a", 1.0, 1.0)).unwrap();

    let sel = h.selection();
    assert_eq!(sel.selected, vec![FeatureId::new("a")]);
    assert_eq!(sel.hovered, Some(FeatureId::new("a")));
    assert_eq!(sel.last_edited, Some(FeatureId::new("a")));
}

#[test]
fn remove_auto_prunes_selection() {
    let mut h = SessionHandle::open(vec![pt("a", 0.0, 0.0), pt("b", 1.0, 1.0)]);
    h.select(
        vec![FeatureId::new("a"), FeatureId::new("b")],
        SelectionMode::Replace,
    );
    h.hover(Some(FeatureId::new("a")));
    h.mark_edited(FeatureId::new("a"));
    h.remove(&FeatureId::new("a")).unwrap();
    let sel = h.selection();
    assert_eq!(sel.selected, vec![FeatureId::new("b")]);
    assert!(sel.hovered.is_none());
    assert!(sel.last_edited.is_none());
}

#[test]
fn split_simulation_keeps_pick_consistent_for_each_segment() {
    // Drawing a long line then splitting it: replace with the first
    // segment and insert the second — what `splitLineFeatureById`
    // does. Both halves must be pickable on their own midpoints.
    let mut h = SessionHandle::open(vec![line("edge", &[[30.0000, 0.0000], [30.0040, 0.0000]])]);
    // Pre-split: midpoint click hits the full line.
    assert_eq!(h.pick(30.0020, 0.0, 5.0), Some(FeatureId::new("edge")));
    // Split.
    h.replace(line("edge", &[[30.0000, 0.0000], [30.0020, 0.0000]]))
        .unwrap();
    h.insert(line("edge_b", &[[30.0020, 0.0000], [30.0040, 0.0000]]))
        .unwrap();
    // Click 1 m north of (30.001, 0) → first half.
    let click_lat_a = 1.0 / METERS_PER_LAT_DEG;
    assert_eq!(
        h.pick(30.0010, click_lat_a, 5.0),
        Some(FeatureId::new("edge"))
    );
    // Click 1 m north of (30.003, 0) → second half.
    let click_lat_b = 1.0 / METERS_PER_LAT_DEG;
    assert_eq!(
        h.pick(30.0030, click_lat_b, 5.0),
        Some(FeatureId::new("edge_b"))
    );
}

#[test]
fn undo_redo_replays_feature_replacements() {
    let mut h = SessionHandle::open(vec![pt("a", 30.0, -2.0)]);

    h.replace(pt("a", 30.5, -2.5)).unwrap();
    assert!(h.can_undo());
    assert_eq!(h.pick(30.5, -2.5, 1.0), Some(FeatureId::new("a")));

    let undo = h.undo().unwrap().unwrap();
    assert_eq!(undo.kind, "undo");
    assert_eq!(h.pick(30.0, -2.0, 1.0), Some(FeatureId::new("a")));
    assert!(h.pick(30.5, -2.5, 1.0).is_none());
    assert!(h.can_redo());

    let redo = h.redo().unwrap().unwrap();
    assert_eq!(redo.kind, "replace_feature");
    assert_eq!(h.pick(30.5, -2.5, 1.0), Some(FeatureId::new("a")));
}

#[test]
fn commit_clears_mutation_history() {
    let mut h = SessionHandle::open(vec![pt("a", 30.0, -2.0)]);
    h.replace(pt("a", 30.5, -2.5)).unwrap();
    assert!(h.can_undo());

    h.commit();

    assert!(!h.can_undo());
    assert!(!h.can_redo());
    assert!(h.undo().unwrap().is_none());
}

#[test]
fn insert_vertex_updates_pick_and_is_undoable() {
    let mut h = SessionHandle::open(vec![line("edge", &[[30.0000, 0.0000], [30.0040, 0.0000]])]);

    let diff = h
        .insert_vertex(&FeatureId::new("edge"), 0, 0, [30.0020, 0.0020])
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "insert_vertex");
    let feature = h.features.get(&FeatureId::new("edge")).unwrap();
    match &feature.geometry {
        Geometry::LineString { coords } => {
            assert_eq!(
                coords,
                &vec![[30.0000, 0.0000], [30.0020, 0.0020], [30.0040, 0.0000]]
            );
        }
        _ => panic!("expected LineString"),
    }

    h.undo().unwrap().unwrap();
    let feature = h.features.get(&FeatureId::new("edge")).unwrap();
    match &feature.geometry {
        Geometry::LineString { coords } => {
            assert_eq!(coords, &vec![[30.0000, 0.0000], [30.0040, 0.0000]]);
        }
        _ => panic!("expected LineString"),
    }
}

#[test]
fn polygon_endpoint_move_keeps_ring_closed() {
    let mut h = SessionHandle::open(vec![poly(
        "area",
        &[&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]],
    )]);

    h.move_vertex(&FeatureId::new("area"), 0, 0, [0.2, 0.2])
        .unwrap()
        .unwrap();

    let feature = h.features.get(&FeatureId::new("area")).unwrap();
    match &feature.geometry {
        Geometry::Polygon { rings } => {
            assert_eq!(rings[0].first(), Some(&[0.2, 0.2]));
            assert_eq!(rings[0].last(), Some(&[0.2, 0.2]));
        }
        _ => panic!("expected Polygon"),
    }
}

#[test]
fn move_feature_translates_geometry_and_is_undoable() {
    let mut h = SessionHandle::open(vec![line(
        "lv_lines/a",
        &[[30.0000, 0.0000], [30.0010, 0.0000]],
    )]);

    let diff = h
        .move_feature(
            &FeatureId::new("lv_lines/a"),
            [30.0000, 0.0000],
            [30.0000, 10.0 / METERS_PER_LAT_DEG],
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "move_feature");
    assert_eq!(
        h.pick(30.0005, 10.0 / METERS_PER_LAT_DEG, 5.0),
        Some(FeatureId::new("lv_lines/a"))
    );

    h.undo().unwrap().unwrap();
    assert_eq!(
        h.pick(30.0005, 0.0, 5.0),
        Some(FeatureId::new("lv_lines/a"))
    );
}

#[test]
fn extend_line_to_uses_indexed_target_and_keeps_old_endpoint_vertex() {
    let mut h = SessionHandle::open(vec![
        line("lv_lines/source", &[[30.0000, 0.0000], [30.0010, 0.0000]]),
        line("lv_lines/target", &[[30.0020, -0.0010], [30.0020, 0.0010]]),
    ]);

    let diff = h
        .extend_line_to(
            &FeatureId::new("lv_lines/source"),
            &FeatureId::new("lv_lines/target"),
            Some([30.0010, 0.0]),
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "extend_line");
    let feature = h.features.get(&FeatureId::new("lv_lines/source")).unwrap();
    match &feature.geometry {
        Geometry::LineString { coords } => {
            assert_eq!(coords.len(), 3);
            assert!((coords[1][0] - 30.0010).abs() < 1e-9);
            assert!((coords[2][0] - 30.0020).abs() < 1e-6);
        }
        _ => panic!("expected LineString"),
    }
}

#[test]
fn join_lines_fuses_near_endpoints_removes_target_and_undos_as_one_diff() {
    let mut h = SessionHandle::open(vec![
        line("lv_lines/a", &[[30.0000, 0.0000], [30.0010, 0.0000]]),
        line(
            "lv_lines/b",
            &[[30.0010 + 1.0 / METERS_PER_LAT_DEG, 0.0], [30.0020, 0.0]],
        ),
    ]);

    let diff = h
        .join_lines(
            &FeatureId::new("lv_lines/a"),
            &FeatureId::new("lv_lines/b"),
            2.0,
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "join_lines");
    assert_eq!(diff.changes.len(), 2);
    assert!(h.features.contains_key(&FeatureId::new("lv_lines/a")));
    assert!(!h.features.contains_key(&FeatureId::new("lv_lines/b")));
    assert_eq!(h.feature_count(), 1);
    assert_eq!(
        h.pick(30.0018, 1.0 / METERS_PER_LAT_DEG, 5.0),
        Some(FeatureId::new("lv_lines/a"))
    );

    h.undo().unwrap().unwrap();
    assert!(h.features.contains_key(&FeatureId::new("lv_lines/a")));
    assert!(h.features.contains_key(&FeatureId::new("lv_lines/b")));
    assert_eq!(h.feature_count(), 2);
}

#[test]
fn split_line_is_one_undoable_diff() {
    let mut h = SessionHandle::open(vec![line(
        "lv_lines/line-a",
        &[[30.0000, 0.0000], [30.0040, 0.0000]],
    )]);

    let diff = h
        .split_line(
            "lv_lines",
            "line-a",
            vec![
                vec![[30.0000, 0.0000], [30.0020, 0.0000]],
                vec![[30.0020, 0.0000], [30.0040, 0.0000]],
            ],
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "split_line");
    assert_eq!(diff.changes.len(), 2);
    assert_eq!(h.feature_count(), 2);
    // WASM mints the new segment's id; we don't pre-declare it. Read it
    // out of the diff and verify the geometry round-trips to that id.
    let new_segment_id = diff.changes[1]
        .after
        .as_ref()
        .expect("split_line diff carries the new segment")
        .id
        .clone();
    assert_eq!(
        h.pick(30.0030, 1.0 / METERS_PER_LAT_DEG, 5.0),
        Some(new_segment_id.clone())
    );

    let undo = h.undo().unwrap().unwrap();
    assert_eq!(undo.changes.len(), 2);
    assert_eq!(h.feature_count(), 1);
    assert!(h.features.contains_key(&FeatureId::new("lv_lines/line-a")));
    assert!(!h.features.contains_key(&new_segment_id));
}

#[test]
fn split_line_minted_ids_never_collide_with_existing_features() {
    // Seed the layer with a feature whose id matches what the minter
    // would produce on first call. The minter must retry and emit a
    // fresh, non-colliding id rather than re-using or clobbering.
    let prefix = feature_id_prefix("lv_lines");
    let preloaded = format!("{prefix}_{:08}", 1);
    let mut h = SessionHandle::open(vec![
        line(
            &format!("lv_lines/{preloaded}"),
            &[[30.10, 0.0], [30.11, 0.0]],
        ),
        line(
            "lv_lines/line-original",
            &[[30.0000, 0.0000], [30.0040, 0.0000]],
        ),
    ]);

    let diff = h
        .split_line(
            "lv_lines",
            "line-original",
            vec![
                vec![[30.0000, 0.0000], [30.0020, 0.0000]],
                vec![[30.0020, 0.0000], [30.0040, 0.0000]],
            ],
        )
        .unwrap()
        .unwrap();
    let new_id = diff.changes[1].after.as_ref().unwrap().id.clone();
    assert_ne!(
        new_id.0,
        format!("lv_lines/{preloaded}"),
        "mint_feature_id must skip the preloaded serial and never clobber it"
    );
    assert_eq!(h.feature_count(), 3);
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn open_builds_layer_tables_in_source_order() {
    let h = SessionHandle::open(vec![
        pt("lv_poles/pole-2", 30.1, -2.1),
        pt("lv_lines/line-1", 30.0, -2.0),
        pt("lv_poles/pole-1", 30.0, -2.0),
    ]);

    let window = h.get_rows("lv_poles", 0, 10).unwrap().unwrap();

    assert_eq!(window.layer_name, "lv_poles");
    assert_eq!(window.total_rows, 2);
    assert_eq!(h.tables.get("lv_poles").unwrap().height(), 2);
    assert_eq!(h.tables.get("lv_lines").unwrap().height(), 1);
    assert_eq!(window.rows[0].feature_id, "pole-2");
    assert_eq!(window.rows[0].row_index, 0);
    assert_eq!(window.rows[1].feature_id, "pole-1");
    assert_eq!(window.rows[1].row_index, 1);
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn mount_scope_builds_layer_tables_in_source_order() {
    // A permutation, so neither id order nor a hasher's order can pass for it.
    let ids: Vec<String> = (0..12).map(|n| format!("pole-{}", n * 7 % 12)).collect();
    let mut features: Vec<Value> = ids
        .iter()
        .map(|id| {
            json!({ "type": "Feature", "id": id, "properties": { "label": id },
                    "geometry": { "type": "Point", "coordinates": [30.0, -2.0] } })
        })
        .collect();
    // A repeated id keeps its first row and takes the later value.
    features.push(
        json!({ "type": "Feature", "id": "pole-7", "properties": { "label": "last" },
                          "geometry": { "type": "Point", "coordinates": [30.1, -2.1] } }),
    );
    let mut h = SessionHandle::open(Vec::new());
    let layers = vec![SessionLayerInput {
        name: "lv_poles".to_string(),
        geojson: json!({ "type": "FeatureCollection", "features": features }),
    }];
    assert_eq!(h.mount_scope_layers("design", layers).unwrap(), 12);

    let window = h.get_rows("design:lv_poles", 0, 20).unwrap().unwrap();
    let order: Vec<&str> = window
        .rows
        .iter()
        .map(|row| row.feature_id.as_str())
        .collect();
    assert_eq!(order, ids.iter().map(String::as_str).collect::<Vec<_>>());
    for (index, row) in window.rows.iter().enumerate() {
        assert_eq!(row.row_index, index);
    }
    let repeated = &window.rows[ids.iter().position(|id| id == "pole-7").unwrap()];
    assert_eq!(repeated.properties["label"], json!("last"));
    assert_eq!(
        h.get_row("design:lv_poles", "pole-0")
            .unwrap()
            .unwrap()
            .row_index,
        0
    );
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn table_rows_stay_synchronized_after_mutations() {
    let mut h = SessionHandle::open(vec![pt_with_props(
        "lv_poles/pole-1",
        30.0,
        -2.0,
        json!({ "pole_number": "P1" }),
    )]);
    h.insert(pt("lv_poles/pole-2", 30.1, -2.1)).unwrap();
    h.replace(pt_with_props(
        "lv_poles/pole-1",
        31.0,
        -3.0,
        json!({ "pole_number": "P1-updated" }),
    ))
    .unwrap();
    h.remove(&FeatureId::new("lv_poles/pole-2")).unwrap();

    let row = h.get_row("lv_poles", "pole-1").unwrap().unwrap();
    assert_eq!(row.feature_id, "pole-1");
    assert_eq!(
        row.geometry,
        Geometry::Point {
            coords: [31.0, -3.0]
        }
    );
    assert_eq!(
        row.properties.get("pole_number"),
        Some(&json!("P1-updated"))
    );
    assert!(h.get_row("lv_poles", "pole-2").unwrap().is_none());
    assert!(h.get_rows("missing", 0, 10).unwrap().is_none());
    assert_eq!(h.tables.get("lv_poles").unwrap().height(), 1);
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn table_insert_collision_does_not_alter_rows() {
    let mut h = SessionHandle::open(vec![pt("lv_poles/pole-1", 30.0, -2.0)]);

    let err = h.insert(pt("lv_poles/pole-1", 31.0, -3.0)).unwrap_err();

    assert!(err.contains("lv_poles/pole-1"));
    let row = h.get_row("lv_poles", "pole-1").unwrap().unwrap();
    assert_eq!(
        row.geometry,
        Geometry::Point {
            coords: [30.0, -2.0]
        }
    );
    assert_eq!(h.tables.get("lv_poles").unwrap().height(), 1);
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn table_tracks_create_and_remove_feature_diffs() {
    let mut h = SessionHandle::open(vec![]);

    let diff = h
        .create_geojson_feature(
            "lv_poles",
            json!({ "type": "Point", "coordinates": [30.0, -2.0] }),
            json!({ "pole_number": "P1" }).as_object().cloned().unwrap(),
        )
        .unwrap()
        .unwrap();

    let feature_id = diff.changes[0].feature_id.clone();
    let row = h.get_row("lv_poles", "lv_poles_00000001").unwrap().unwrap();
    assert_eq!(row.feature_id, "lv_poles_00000001");
    assert_eq!(row.properties.get("pole_number"), Some(&json!("P1")));
    assert_eq!(h.tables.get("lv_poles").unwrap().height(), 1);

    let removed = h.remove_diff(&feature_id).unwrap().unwrap();
    assert_eq!(removed.kind, "remove_feature");
    assert!(h.get_rows("lv_poles", 0, 10).unwrap().is_none());
    assert!(!h.tables.contains_key("lv_poles"));

    h.undo().unwrap().unwrap();
    let restored = h.get_row("lv_poles", "lv_poles_00000001").unwrap().unwrap();
    assert_eq!(restored.properties.get("pole_number"), Some(&json!("P1")));
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn table_tracks_replace_layer_geojson_features() {
    let mut h = SessionHandle::open(vec![pt_with_props(
        "lv_poles/old",
        30.0,
        -2.0,
        json!({ "pole_number": "OLD" }),
    )]);

    let diff = h
        .replace_layer_geojson_features(
            "lv_poles",
            vec![json!({
                "type": "Feature",
                "id": "new",
                "geometry": { "type": "Point", "coordinates": [31.0, -3.0] },
                "properties": { "id": "new", "pole_number": "NEW" }
            })],
        )
        .unwrap()
        .unwrap();

    assert_eq!(diff.kind, "replace_layer");
    assert!(h.get_row("lv_poles", "old").unwrap().is_none());
    let row = h.get_row("lv_poles", "new").unwrap().unwrap();
    assert_eq!(row.properties.get("pole_number"), Some(&json!("NEW")));
    assert_eq!(h.tables.get("lv_poles").unwrap().height(), 1);
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn replace_on_missing_id_creates_layer_table() {
    let mut h = SessionHandle::open(vec![pt("lv_poles/pole-1", 30.0, -2.0)]);

    let old = h.replace(pt("lv_lines/line-1", 30.1, -2.1)).unwrap();

    assert!(old.is_none());
    let window = h.get_rows("lv_lines", 0, 10).unwrap().unwrap();
    assert_eq!(window.total_rows, 1);
    assert_eq!(window.rows[0].feature_id, "line-1");
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn remove_last_layer_feature_drops_table() {
    let mut h = SessionHandle::open(vec![pt("lv_poles/pole-1", 30.0, -2.0)]);

    let removed = h.remove(&FeatureId::new("lv_poles/pole-1")).unwrap();

    assert!(removed.is_some());
    assert!(h.get_rows("lv_poles", 0, 10).unwrap().is_none());
    assert!(!h.tables.contains_key("lv_poles"));
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn edit_cell_updates_session_feature_and_table() {
    let mut h = SessionHandle::open(vec![pt_with_props(
        "lv_poles/pole-1",
        30.0,
        -2.0,
        json!({ "pole_number": "P1" }),
    )]);

    let diff = h
        .edit_cell("lv_poles", "pole-1", "pole_number", json!("P2"))
        .unwrap()
        .unwrap();

    assert_eq!(diff.before, Some(json!("P1")));
    assert_eq!(diff.after, Some(json!("P2")));
    let encoded = FeatureId::new("lv_poles/pole-1");
    assert_eq!(
        h.features
            .get(&encoded)
            .unwrap()
            .properties
            .get("pole_number"),
        Some(&json!("P2"))
    );
    assert_eq!(h.selection().last_edited, Some(encoded));
    assert_eq!(
        h.get_row("lv_poles", "pole-1")
            .unwrap()
            .unwrap()
            .properties
            .get("pole_number"),
        Some(&json!("P2"))
    );
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn edit_cells_updates_session_feature_table_and_undo() {
    let mut h = SessionHandle::open(vec![
        pt_with_props(
            "lv_poles/pole-1",
            30.0,
            -2.0,
            json!({ "struct_type": "S100", "material": "wooden" }),
        ),
        pt_with_props(
            "lv_poles/pole-2",
            30.1,
            -2.1,
            json!({ "struct_type": "S190", "material": "wooden" }),
        ),
    ]);
    let mut values = Map::new();
    values.insert("struct_type".to_string(), json!("S190"));
    values.insert("material".to_string(), json!("concrete"));

    let result = h
        .edit_cells(
            "lv_poles",
            &[
                "pole-1".to_string(),
                "pole-2".to_string(),
                "missing".to_string(),
            ],
            values,
        )
        .unwrap();

    assert_eq!(result.missing_feature_ids, vec!["missing"]);
    assert_eq!(result.changed.len(), 3);
    assert_eq!(
        h.features
            .get(&FeatureId::new("lv_poles/pole-1"))
            .unwrap()
            .properties
            .get("material"),
        Some(&json!("concrete"))
    );
    assert_eq!(
        h.get_row("lv_poles", "pole-1")
            .unwrap()
            .unwrap()
            .properties
            .get("struct_type"),
        Some(&json!("S190"))
    );
    assert_eq!(
        h.selection().last_edited,
        Some(FeatureId::new("lv_poles/pole-2"))
    );

    let undo = h.undo().unwrap().unwrap();
    assert_eq!(undo.kind, "undo");
    assert_eq!(undo.changes.len(), 2);
    assert_eq!(
        h.features
            .get(&FeatureId::new("lv_poles/pole-1"))
            .unwrap()
            .properties
            .get("struct_type"),
        Some(&json!("S100"))
    );
    assert_eq!(
        h.get_row("lv_poles", "pole-2")
            .unwrap()
            .unwrap()
            .properties
            .get("material"),
        Some(&json!("wooden"))
    );
}

#[cfg(feature = "enable_dataframes")]
#[test]
fn delete_column_updates_session_feature_and_table() {
    let mut h = SessionHandle::open(vec![
        pt_with_props("lv_poles/pole-1", 30.0, -2.0, json!({ "status": "old" })),
        pt_with_props("lv_poles/pole-2", 30.1, -2.1, json!({ "status": "old" })),
    ]);

    assert_eq!(h.delete_column("lv_poles", "status").unwrap(), 2);

    assert!(
        !h.features
            .get(&FeatureId::new("lv_poles/pole-1"))
            .unwrap()
            .properties
            .contains_key("status")
    );
    assert!(
        !h.get_row("lv_poles", "pole-1")
            .unwrap()
            .unwrap()
            .properties
            .contains_key("status")
    );
}

// --- post-mutation vertex dedupe ---
//
// Reuses network::VERTEX_DEDUPE_TOLERANCE_M (0.5 m). Every Session geometry
// mutator runs the sanitizer so the edit surface and the build kernel
// agree on "same vertex" and the join/move/insert paths can never leave
// two vertices in the same place on a line.

fn projection_at(lng: f64, lat: f64) -> ProjectionMetadata {
    ProjectionMetadata::from_lonlat_coords([[lng, lat]])
}

#[test]
fn dedupe_line_coords_drops_interior_duplicate_within_tolerance() {
    let projection = projection_at(30.0, 0.0);
    // Vertices 0 and 1 are ~0.1m apart at lat 0 (well below 0.5m).
    let near_lng = 30.0 + 0.1 / METERS_PER_LAT_DEG;
    let mut coords = vec![[30.0, 0.0], [near_lng, 0.0], [30.001, 0.0]];
    dedupe_line_coords(&mut coords, projection, VERTEX_DEDUPE_TOLERANCE_M);
    assert_eq!(coords, vec![[30.0, 0.0], [30.001, 0.0]]);
}

#[test]
fn dedupe_line_coords_preserves_open_endpoints() {
    let projection = projection_at(30.0, 0.0);
    // Penultimate and last within tolerance — drop the penultimate so the
    // endpoint (which is the connection point) survives.
    let near_end = 30.001 - 0.1 / METERS_PER_LAT_DEG;
    let mut coords = vec![[30.0, 0.0], [near_end, 0.0], [30.001, 0.0]];
    dedupe_line_coords(&mut coords, projection, VERTEX_DEDUPE_TOLERANCE_M);
    assert_eq!(coords.first(), Some(&[30.0, 0.0]));
    assert_eq!(coords.last(), Some(&[30.001, 0.0]));
    assert_eq!(coords.len(), 2);
}

#[test]
fn dedupe_line_coords_keeps_a_minimal_2_vertex_line_even_if_duplicate() {
    // Degenerate input (both vertices identical) must not collapse below 2.
    let projection = projection_at(30.0, 0.0);
    let mut coords = vec![[30.0, 0.0], [30.0, 0.0]];
    dedupe_line_coords(&mut coords, projection, VERTEX_DEDUPE_TOLERANCE_M);
    assert_eq!(coords.len(), 2);
}

#[test]
fn dedupe_ring_coords_preserves_closure_and_min_three_unique() {
    let projection = projection_at(30.0, 0.0);
    let off = 0.1 / METERS_PER_LAT_DEG; // ~0.1m, within 0.5m tolerance
    // Closed ring with one near-duplicate interior vertex.
    let mut ring = vec![
        [30.0, 0.0],
        [30.0 + off, 0.0], // dup of v0
        [30.001, 0.0],
        [30.001, 0.001],
        [30.0, 0.0],
    ];
    dedupe_ring_coords(&mut ring, projection, VERTEX_DEDUPE_TOLERANCE_M);
    assert_eq!(ring.first(), ring.last());
    assert_eq!(ring.len(), 4); // 3 unique + closure copy
}

#[test]
fn join_lines_emits_no_consecutive_near_duplicate_vertices() {
    // Reproduces the operator-visible bug: line A's penultimate vertex
    // sits ~0.1 m east of A's endpoint; line B starts exactly at A's
    // endpoint. Pre-sanitizer this left [..., near_end, fused] — two
    // vertices in essentially the same place. Post-sanitizer the near
    // duplicate is collapsed.
    let near_end_lng = 30.0010 - 0.1 / METERS_PER_LAT_DEG;
    let mut h = SessionHandle::open(vec![
        line(
            "lv_lines/a",
            &[[30.0000, 0.0], [near_end_lng, 0.0], [30.0010, 0.0]],
        ),
        line("lv_lines/b", &[[30.0010, 0.0], [30.0020, 0.0]]),
    ]);

    h.join_lines(
        &FeatureId::new("lv_lines/a"),
        &FeatureId::new("lv_lines/b"),
        1.0,
    )
    .unwrap()
    .unwrap();

    let feature = h.features.get(&FeatureId::new("lv_lines/a")).unwrap();
    let coords = match &feature.geometry {
        Geometry::LineString { coords } => coords.clone(),
        _ => panic!("expected LineString"),
    };
    let projection = h.projection_metadata();
    for window in coords.windows(2) {
        let d = projection.distance_m(window[0], window[1]);
        assert!(
            d > VERTEX_DEDUPE_TOLERANCE_M,
            "consecutive vertices within dedupe tolerance: {:?} -> {:?} ({:.4}m)",
            window[0],
            window[1],
            d
        );
    }
}

#[test]
fn move_vertex_onto_neighbor_collapses_the_duplicate() {
    // A vertex moved on top of (or within 0.5m of) its neighbor must not
    // leave two vertices in the same place. The interior duplicate is
    // dropped; the line still has its original endpoints.
    let mut h = SessionHandle::open(vec![line(
        "lv_lines/a",
        &[[30.0000, 0.0], [30.0010, 0.0], [30.0020, 0.0]],
    )]);

    // Move v1 (interior) on top of v0.
    h.move_vertex(&FeatureId::new("lv_lines/a"), 0, 1, [30.0000, 0.0])
        .unwrap()
        .unwrap();

    let feature = h.features.get(&FeatureId::new("lv_lines/a")).unwrap();
    match &feature.geometry {
        Geometry::LineString { coords } => {
            assert_eq!(coords, &vec![[30.0000, 0.0], [30.0020, 0.0]]);
        }
        _ => panic!("expected LineString"),
    }
}
