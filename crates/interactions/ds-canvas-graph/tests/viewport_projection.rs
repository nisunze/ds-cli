//! The world-to-window projection.

mod common;

use common::*;
use ds_canvas_graph::viewport::{FIT_PADDING_PX, MAX_ZOOM, MIN_ZOOM};
use ds_canvas_graph::{GeometrySession, Orientation, Point};

fn session() -> GeometrySession {
    let mut session = GeometrySession::open(chapter(Orientation::Horizontal)).unwrap();
    session.set_host(800.0, 600.0, 2.0).unwrap();
    session
}

#[test]
fn a_fitted_camera_shows_the_whole_scene_inside_the_window() {
    let session = session();
    let projection = session.projection();
    let bounds = session.scene().bounds;

    let top_left = projection.project(Point::new(bounds.min_x, bounds.min_y));
    let bottom_right = projection.project(Point::new(bounds.max_x, bounds.max_y));
    assert!(top_left.x >= -1e-9 && top_left.y >= -1e-9, "{top_left:?}");
    assert!(
        bottom_right.x <= 800.0 + 1e-9 && bottom_right.y <= 600.0 + 1e-9,
        "{bottom_right:?}"
    );
    // Fit leaves the padding band free on the tighter axis.
    let used = bottom_right.x - top_left.x;
    close(used, 800.0 - FIT_PADDING_PX);
}

#[test]
fn projection_and_unprojection_round_trip() {
    let mut session = session();
    session.set_viewport(2.5, 37.0, -14.0).unwrap();
    let projection = session.projection();
    for world in [
        Point::new(0.0, 0.0),
        Point::new(212.0, 64.0),
        Point::new(-52.0, 194.0),
    ] {
        let back = projection.unproject(projection.project(world));
        close(back.x, world.x);
        close(back.y, world.y);
    }
}

#[test]
fn the_device_pixel_ratio_travels_with_the_projection_and_changes_nothing_else() {
    let mut session = session();
    let one = session.projection();
    session.set_host(800.0, 600.0, 3.0).unwrap();
    let three = session.projection();

    close(one.scale, three.scale);
    close(one.offset_x, three.offset_x);
    close(one.device_pixel_ratio, 2.0);
    close(three.device_pixel_ratio, 3.0);
}

#[test]
fn zoom_is_clamped_to_the_supported_band_and_never_refused() {
    let mut session = session();
    session.set_viewport(1e12, 0.0, 0.0).unwrap();
    close(session.viewport().zoom, MAX_ZOOM);
    session.set_viewport(1e-12, 0.0, 0.0).unwrap();
    close(session.viewport().zoom, MIN_ZOOM);
}

#[test]
fn panning_translates_the_camera_by_exactly_the_screen_delta() {
    let mut session = session();
    let before = session.projection();
    session.pan(25.0, -40.0).unwrap();
    let after = session.projection();

    close(after.offset_x - before.offset_x, 25.0);
    close(after.offset_y - before.offset_y, -40.0);
    close(after.scale, before.scale);
}

#[test]
fn zooming_at_an_anchor_keeps_the_world_point_under_it_still() {
    let mut session = session();
    let anchor = Point::new(310.0, 220.0);
    let world_before = session.projection().unproject(anchor);

    for factor in [1.25, 1.25, 0.8, 4.0, 0.1] {
        session.zoom_at(anchor.x, anchor.y, factor).unwrap();
        let world_after = session.projection().unproject(anchor);
        close(world_after.x, world_before.x);
        close(world_after.y, world_before.y);
    }
}

#[test]
fn zooming_at_the_clamp_boundary_is_a_no_op_rather_than_a_jump() {
    let mut session = session();
    session.set_viewport(MAX_ZOOM, 11.0, 12.0).unwrap();
    session.zoom_at(400.0, 300.0, 2.0).unwrap();
    close(session.viewport().zoom, MAX_ZOOM);
    close(session.viewport().pan_x, 11.0);
    close(session.viewport().pan_y, 12.0);
}

#[test]
fn centering_puts_a_world_point_in_the_middle_of_the_window() {
    let mut session = session();
    session.set_viewport(3.0, 0.0, 0.0).unwrap();
    let target = Point::new(212.0, 90.0);
    session.center_on(target.x, target.y).unwrap();

    let screen = session.projection().project(target);
    close(screen.x, 400.0);
    close(screen.y, 300.0);
}

#[test]
fn a_centred_zoom_keeps_the_window_centre_stable() {
    let mut session = session();
    session.set_viewport(2.0, 30.0, -20.0).unwrap();
    let centre_before = session.projection().unproject(Point::new(400.0, 300.0));
    session.zoom_centered(1.5).unwrap();
    let centre_after = session.projection().unproject(Point::new(400.0, 300.0));

    // The fitted camera is centred on the scene, so a centred zoom moves the
    // window centre by less than a pixel of world at the new scale.
    let drift = centre_before.distance_to(centre_after) * session.projection().scale;
    assert!(drift < 1.0, "centre drifted {drift} screen pixels");
}

#[test]
fn fit_returns_the_camera_the_session_started_with() {
    let mut session = session();
    session.set_viewport(7.0, 100.0, 200.0).unwrap();
    session.fit();
    close(session.viewport().zoom, 1.0);
    close(session.viewport().pan_x, 0.0);
    close(session.viewport().pan_y, 0.0);
}

#[test]
fn a_tiny_window_still_projects_a_drawable_scene() {
    let mut session = session();
    session.set_host(1.0, 1.0, 1.0).unwrap();
    let projection = session.projection();
    assert!(projection.scale > 0.0 && projection.scale.is_finite());
    assert!(projection.offset_x.is_finite() && projection.offset_y.is_finite());
}

#[test]
fn the_projection_buffer_carries_the_transform_and_the_camera() {
    let mut session = session();
    session.set_viewport(2.0, 15.0, -5.0).unwrap();
    let buffer = session.projection_buffer();
    let projection = session.projection();

    assert_eq!(buffer.len(), 7);
    close(buffer[0], projection.scale);
    close(buffer[1], projection.offset_x);
    close(buffer[2], projection.offset_y);
    close(buffer[3], 2.0);
    close(buffer[4], 2.0);
    close(buffer[5], 15.0);
    close(buffer[6], -5.0);
}

#[test]
fn the_camera_is_not_graph_state_so_it_never_bumps_the_revision() {
    let mut session = session();
    let revision = session.revision();
    session.set_viewport(3.0, 10.0, 10.0).unwrap();
    session.pan(5.0, 5.0).unwrap();
    session.zoom_at(100.0, 100.0, 2.0).unwrap();
    session.set_host(1024.0, 768.0, 1.0).unwrap();
    session.fit();
    assert_eq!(session.revision(), revision);
}
