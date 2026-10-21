use ds_canvas_graph::connector::{curve_markers, distance_buffer, markers_buffer, route_buffer};
use ds_canvas_graph::{Curve, CurveSegment, Point};

fn input(shape: f64) -> Vec<f64> {
    vec![
        0.0, 0.0, 200.0, 100.0, 400.0, 180.0, 200.0, 100.0, shape, 0.0, -1.0, -1.0, 0.5, 0.5, 0.0,
        38.0,
    ]
}

#[test]
fn every_shape_keeps_the_same_card_anchors_and_stable_lane() {
    let mut anchors = None;
    for shape in 0..4 {
        let request = input(f64::from(shape));
        let result = route_buffer(&request).unwrap();
        assert_eq!(route_buffer(&request).unwrap(), result);
        assert_eq!(*anchors.get_or_insert(result[..4].to_vec()), result[..4]);
        assert_eq!(result.len(), 10 + result[9] as usize * 7);
        assert!(result.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn cubic_arrival_is_perpendicular_to_every_named_face_in_both_directions() {
    for source in 0..4 {
        for target in 0..4 {
            let mut request = input(3.0);
            request[10] = f64::from(source);
            request[11] = f64::from(target);
            let result = route_buffer(&request).unwrap();
            let normals = [
                std::f64::consts::PI,
                0.0,
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ];
            let expected = normals[source as usize];
            assert!((result[4].cos() - expected.cos()).abs() < 1e-12);
            assert!((result[4].sin() - expected.sin()).abs() < 1e-12);
            let expected = normals[target as usize] + std::f64::consts::PI;
            assert!((result[5].cos() - expected.cos()).abs() < 1e-12);
            assert!((result[5].sin() - expected.sin()).abs() < 1e-12);
        }
    }
}

#[test]
fn vertically_stacked_cards_use_a_stable_external_side_and_preserve_positions() {
    let mut request = input(3.0);
    request[4] = 0.0;
    request[5] = 240.0;
    let right = route_buffer(&request).unwrap();
    assert_eq!(&right[..4], &[200.0, 50.0, 200.0, 290.0]);
    assert!(right[11] > right[0]);
    assert_eq!(right[5].abs(), std::f64::consts::PI);
    request[9] = 1.0;
    let left = route_buffer(&request).unwrap();
    assert_eq!(&left[..4], &[0.0, 50.0, 0.0, 290.0]);
    assert!(left[11] < left[0]);
    assert_eq!(left[5], 0.0);
}

#[test]
fn midpoint_marker_follows_arc_length_instead_of_the_node_to_node_chord() {
    let curve = Curve::polyline_curve(&[
        Point::new(0.0, 0.0),
        Point::new(100.0, 0.0),
        Point::new(100.0, 300.0),
    ]);
    let result = curve_markers(&curve);
    assert_eq!(&result[6..8], &[100.0, 100.0]);
    assert_eq!(result[8], std::f64::consts::FRAC_PI_2);
    let packed = markers_buffer(&[
        0.0, 0.0, 2.0, 1.0, 100.0, 0.0, 100.0, 0.0, 100.0, 0.0, 1.0, 100.0, 300.0, 100.0, 300.0,
        100.0, 300.0,
    ])
    .unwrap();
    assert_eq!(packed, result);
}

#[test]
fn endpoint_tangent_skips_repeated_control_points() {
    let curve = Curve::new(
        Point::new(0.0, 0.0),
        vec![CurveSegment::Cubic {
            control_a: Point::new(0.0, 0.0),
            control_b: Point::new(100.0, 200.0),
            to: Point::new(100.0, 200.0),
        }],
    );
    let result = curve_markers(&curve);
    assert_eq!(result[4], 200.0f64.atan2(100.0));
    assert_eq!(result[5], result[4]);
}

#[test]
fn malformed_or_nonfinite_connector_operands_are_refused() {
    for (at, value) in [
        (0, f64::NAN),
        (7, 0.0),
        (8, 4.0),
        (9, 0.5),
        (10, 6.0),
        (12, 1.1),
        (14, 2.0),
        (15, -1.0),
    ] {
        let mut request = input(3.0);
        request[at] = value;
        assert!(
            route_buffer(&request).is_err(),
            "accepted operand {at}={value}"
        );
    }
    assert!(route_buffer(&[]).is_err());
    assert!(markers_buffer(&[0.0, 0.0, 1.0]).is_err());
}

#[test]
fn native_hit_geometry_uses_the_chosen_shape_and_refuses_bad_pointers() {
    for shape in 0..4 {
        let result = route_buffer(&input(f64::from(shape))).unwrap();
        let curve: Vec<_> = [result[0], result[1], result[9]]
            .into_iter()
            .chain(result[10..].iter().copied())
            .collect();
        assert!(distance_buffer(&curve, result[6], result[7]).unwrap() < 1e-9);
        assert!(distance_buffer(&curve, -100.0, -200.0).unwrap() > 100.0);
        assert!(distance_buffer(&curve, f64::NAN, 0.0).is_err());
    }
}

#[test]
fn container_start_and_finish_keep_authored_direction_and_prune_distant_stubs() {
    let note = [20.0, 40.0, 200.0, 100.0];
    let container = [0.0, 0.0, 260.0, 240.0];
    let mut request = input(3.0);
    request[..4].copy_from_slice(&note);
    request[4..8].copy_from_slice(&container);
    request.extend_from_slice(&[1.0, 14.0]);
    let end = route_buffer(&request).unwrap();
    assert_eq!(&end[..4], &[220.0, 90.0, 246.0, 90.0]);
    assert_eq!(end[5], 0.0);
    request[..4].copy_from_slice(&container);
    request[4..8].copy_from_slice(&note);
    request[16] = 2.0;
    let start = route_buffer(&request).unwrap();
    assert_eq!(&start[..4], &[14.0, 90.0, 20.0, 90.0]);
    assert_eq!(start[5], 0.0);
    request[4] = 1000.0;
    assert!(route_buffer(&request).unwrap().is_empty());
}

#[test]
fn native_projection_changes_only_coordinates_and_preserves_marker_directions() {
    let request = input(3.0);
    let base = route_buffer(&request).unwrap();
    let projected = route_buffer(&[request, vec![0.0, 0.0, 12.0, -40.0, 2.5]].concat()).unwrap();
    assert_eq!(projected[0], 12.0 + base[0] * 2.5);
    assert_eq!(projected[1], -40.0 + base[1] * 2.5);
    assert_eq!(projected[5], base[5]);
    assert_eq!(projected[6], 12.0 + base[6] * 2.5);
}
