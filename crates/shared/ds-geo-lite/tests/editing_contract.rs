#![cfg(feature = "editing")]
use ds_geo_lite::{
    operations::{extend, split, trim},
    types::Coord,
};

fn c(x: f64, y: f64) -> Coord {
    Coord::new(x, y)
}

#[test]
fn split_preserves_authored_vertices_and_source_direction() {
    let line = [c(0., 0.), c(1., 0.), c(2., 0.)];
    assert_eq!(
        split::split_at_point(&line, c(1., 1.)),
        Some((line[..2].to_vec(), line[1..].to_vec()))
    );
    assert_eq!(split::split_at_point(&line, c(0., 0.)), None);
    // Characterize the existing final-parameter alias during relocation.
    // Adopting a terminal-cut no-op requires a separate rule and regression.
    assert_eq!(
        split::split_at_point(&line, c(2., 0.)),
        Some((line[..2].to_vec(), line[1..].to_vec()))
    );
}

#[test]
fn cutter_split_keeps_intersections_in_source_order() {
    let line = [c(0., 0.), c(4., 0.)];
    let cutter = [c(1., -1.), c(1., 1.), c(3., 1.), c(3., -1.)];
    assert_eq!(
        split::split_with_line(&line, &cutter),
        Some(vec![
            vec![line[0], c(1., 0.)],
            vec![c(1., 0.), c(3., 0.)],
            vec![c(3., 0.), line[1]]
        ])
    );
}

#[test]
fn trim_keeps_the_requested_side_without_reordering_source_vertices() {
    let line = [c(0., 0.), c(2., 0.), c(4., 0.)];
    let cutter = [c(1., -1.), c(1., 1.)];
    assert_eq!(
        trim::trim_at(&line, &cutter, c(0., 0.)),
        Some(vec![line[0], c(1., 0.)])
    );
    assert_eq!(
        trim::trim_at(&line, &cutter, c(4., 0.)),
        Some(vec![c(1., 0.), line[1], line[2]])
    );
}

#[test]
fn extend_prefers_finite_hits_and_retains_target_extrapolation_fallback() {
    let line = [c(0., 0.), c(1., 0.)];
    let finite = [c(2., -1.), c(2., 1.)];
    assert_eq!(
        extend::extend_to(&line, &finite),
        Some(vec![line[0], c(2., 0.)])
    );
    let extrapolated = [c(2., 1.), c(2., 2.)];
    assert_eq!(
        extend::extend_to(&line, &extrapolated),
        Some(vec![line[0], c(2., 0.)])
    );
    assert_eq!(extend::extend_to(&[c(0., 0.)], &finite), None);
}
