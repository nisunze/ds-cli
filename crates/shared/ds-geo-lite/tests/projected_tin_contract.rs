#![cfg(feature = "tin")]

use ds_geo_lite::projected_tin::{
    MAX_PROJECTED_TIN_POINTS, MAX_PROJECTED_TIN_QUERIES, ProjectedTerrainBreakline,
    ProjectedTerrainPoint as Point, ProjectedTerrainTin as Tin, ProjectedTerrainTinError as Error,
    ProjectedTerrainTinOptions as Options,
};

fn point(x_m: f64, y_m: f64, z_m: f64) -> Point {
    Point { x_m, y_m, z_m }
}
fn build(points: &[Point], max_triangle_edge_m: f64) -> Tin {
    Tin::build(
        points,
        Options {
            max_triangle_edge_m,
        },
        &[],
    )
    .unwrap()
}
fn close(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("query lies inside supported terrain");
    assert!(
        (actual - expected).abs() <= 1e-8,
        "actual={actual}, expected={expected}"
    );
}

#[test]
fn projected_affine_plane_removes_alternating_side_bias_at_actual_xy() {
    let origin = [816_000.0, 9_708_000.0];
    let observations = [
        (0.0, -12.0),
        (0.0, 12.0),
        (10.0, 12.0),
        (20.0, -12.0),
        (30.0, 12.0),
        (40.0, -12.0),
        (50.0, -12.0),
        (50.0, 12.0),
    ]
    .map(|(x, y)| point(origin[0] + x, origin[1] + y, 1_600.0 + 0.1 * x + 0.8 * y));
    let tin = build(&observations, 100.0);
    assert_eq!(tin.points(), observations);
    let queries: Vec<_> = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0]
        .into_iter()
        .flat_map(|x| [-6.0, 0.0, 6.0].map(|y| [origin[0] + x, origin[1] + y]))
        .collect();
    let elevations = tin.sample_many(&queries).unwrap();
    for (&[x, y], elevation) in queries.iter().zip(elevations) {
        close(
            elevation,
            1_600.0 + 0.1 * (x - origin[0]) + 0.8 * (y - origin[1]),
        );
    }
    // Connecting the original elevations in station order would oscillate
    // +/-9.6 m about this plane. The surface queries reproduce the plane.
    assert!((observations[2].z_m - 1_601.0).abs() > 9.0);
    close(tin.sample(origin[0] + 10.0, origin[1]).unwrap(), 1_601.0);
}

#[test]
fn real_sampled_ridge_and_valley_are_preserved() {
    for amplitude in [20.0, -20.0] {
        let mut points = Vec::new();
        for x in [-10.0_f64, 0.0, 10.0] {
            for y in [-10.0, 0.0, 10.0] {
                points.push(point(x, y, 100.0 + amplitude * (1.0 - x.abs() / 10.0)));
            }
        }
        let tin = build(&points, 30.0);
        close(tin.sample(0.0, 0.0).unwrap(), 100.0 + amplitude);
        close(tin.sample(-5.0, 0.0).unwrap(), 100.0 + amplitude * 0.5);
        close(tin.sample(5.0, 0.0).unwrap(), 100.0 + amplitude * 0.5);
        close(tin.sample(10.0, 0.0).unwrap(), 100.0);
        assert_eq!(
            tin.points(),
            points,
            "interpolation never edits observations"
        );
    }
}

#[test]
fn hull_boundary_is_supported_but_outside_is_never_extrapolated() {
    let tin = build(
        &[
            point(0.0, 0.0, 100.0),
            point(1.0, 0.0, 110.0),
            point(0.0, 1.0, 120.0),
        ],
        2.0,
    );
    close(tin.sample(0.5, 0.5).unwrap(), 115.0);
    assert_eq!(
        tin.sample(0.8, 0.8).unwrap(),
        None,
        "inside bounds but outside hull"
    );
    assert_eq!(
        tin.sample(0.5, 0.5 + 1e-11).unwrap(),
        None,
        "even tiny negative weights cannot extrapolate"
    );
    assert_eq!(tin.sample(-1.0, 0.0).unwrap(), None);
}

#[test]
fn explicit_edge_limit_preserves_missing_survey_gap() {
    let points = [
        point(0.0, 0.0, 100.0),
        point(0.0, 1.0, 100.0),
        point(1.0, 0.0, 100.0),
        point(10.0, 0.0, 200.0),
        point(10.0, 1.0, 200.0),
        point(11.0, 0.0, 200.0),
    ];
    let tin = build(&points, 2.0);
    assert!(tin.discarded_long_edge_triangle_count() > 0);
    close(tin.sample(0.2, 0.2).unwrap(), 100.0);
    close(tin.sample(10.2, 0.2).unwrap(), 200.0);
    assert_eq!(tin.sample(5.0, 0.2).unwrap(), None);
    let no_coverage = build(&points, 0.5);
    assert!(no_coverage.triangles().is_empty());
    assert_eq!(no_coverage.sample(0.2, 0.2).unwrap(), None);
    assert_eq!(
        no_coverage.sample(0.0, 0.0).unwrap(),
        None,
        "raw vertices of discarded triangles cannot bypass coverage filtering"
    );
    let isolated = build(
        &[
            point(0.0, 0.0, 100.0),
            point(0.0, 1.0, 100.0),
            point(1.0, 0.0, 100.0),
            point(20.0, 20.0, 200.0),
        ],
        2.0,
    );
    close(isolated.sample(0.2, 0.2).unwrap(), 100.0);
    assert_eq!(
        isolated.sample(20.0, 20.0).unwrap(),
        None,
        "isolated raw vertex cannot create support beside an otherwise valid surface"
    );
}

#[test]
fn identical_duplicates_retain_raw_coordinates_and_conflicting_z_refuses() {
    let points = [
        point(0.0, 0.0, 100.0),
        point(1.0, 0.0, 110.0),
        point(0.0, 1.0, 120.0),
        point(0.0, 0.0, 100.0),
    ];
    let tin = build(&points, 2.0);
    assert_eq!(tin.points(), points);
    assert_eq!(tin.unique_point_count(), 3);
    assert_eq!(tin.triangles().len(), 1);
    assert!(!tin.triangles()[0].contains(&3));
    let conflict = [
        point(0.0, 0.0, 100.0),
        point(0.0, 10.0, 200.0),
        point(0.5e-6, 0.0, 101.0),
    ];
    assert!(matches!(
        Tin::build(
            &conflict,
            Options {
                max_triangle_edge_m: 20.0
            },
            &[]
        ),
        Err(Error::ConflictingDuplicate {
            first_index: 0,
            second_index: 2
        })
    ));
    let chained_conflict = [
        point(0.0, 0.0, 100.0),
        point(0.8e-6, 0.0, 100.0),
        point(1.6e-6, 0.0, 101.0),
        point(0.0, 10.0, 100.0),
        point(10.0, 0.0, 100.0),
    ];
    assert!(matches!(
        Tin::build(
            &chained_conflict,
            Options {
                max_triangle_edge_m: 20.0
            },
            &[]
        ),
        Err(Error::ConflictingDuplicate {
            first_index: 1,
            second_index: 2
        })
    ));
}

#[test]
fn order_reversal_does_not_change_surface_or_raw_input() {
    let points = [
        point(10.0, 10.0, 120.0),
        point(0.0, 0.0, 100.0),
        point(0.0, 10.0, 110.0),
        point(10.0, 0.0, 110.0),
    ];
    let mut reversed = points;
    reversed.reverse();
    let first = build(&points, 20.0);
    let second = build(&reversed, 20.0);
    assert_eq!(first.points(), points);
    assert_eq!(second.points(), reversed);
    let queries = [[2.0, 7.0], [5.0, 5.0], [8.0, 1.0]];
    assert_eq!(
        first.sample_many(&queries).unwrap(),
        second.sample_many(&queries).unwrap()
    );
}

#[test]
fn authored_breaklines_are_an_explicit_refusal() {
    let points = [
        point(0.0, 0.0, 100.0),
        point(1.0, 0.0, 110.0),
        point(0.0, 1.0, 120.0),
    ];
    let breaklines = [ProjectedTerrainBreakline {
        vertices: points[..2].to_vec(),
    }];
    assert!(matches!(
        Tin::build(
            &points,
            Options {
                max_triangle_edge_m: 2.0
            },
            &breaklines
        ),
        Err(Error::UnsupportedBreaklines { count: 1 })
    ));
}

#[test]
fn finite_and_count_admission_refuses_without_partial_results() {
    let points = [
        point(0.0, 0.0, 100.0),
        point(1.0, 0.0, 110.0),
        point(0.0, 1.0, 120.0),
    ];
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for ordinate in 0..3 {
            let mut invalid = points;
            match ordinate {
                0 => invalid[1].x_m = bad,
                1 => invalid[1].y_m = bad,
                _ => invalid[1].z_m = bad,
            }
            assert!(matches!(
                Tin::build(
                    &invalid,
                    Options {
                        max_triangle_edge_m: 2.0
                    },
                    &[]
                ),
                Err(Error::NonFinitePoint { index: 1 })
            ));
        }
    }
    for edge in [0.0, -1.0, 1e-6, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            Tin::build(
                &points,
                Options {
                    max_triangle_edge_m: edge
                },
                &[]
            ),
            Err(Error::InvalidMaximumEdge)
        ));
    }
    let oversized = vec![points[0]; MAX_PROJECTED_TIN_POINTS + 1];
    assert!(matches!(
        Tin::build(
            &oversized,
            Options {
                max_triangle_edge_m: 2.0
            },
            &[]
        ),
        Err(Error::TooManyPoints { .. })
    ));
    let tin = build(&points, 2.0);
    assert!(matches!(
        tin.sample_many(&[[0.1, 0.1], [f64::NAN, 0.1]]),
        Err(Error::NonFiniteQuery { index: 1 })
    ));
    assert!(matches!(
        tin.sample_many(&vec![[0.1, 0.1]; MAX_PROJECTED_TIN_QUERIES + 1]),
        Err(Error::TooManyQueries { .. })
    ));
    assert!(matches!(
        Tin::build(
            &[
                point(0.0, 0.0, 1.0),
                point(1.0, 0.0, 2.0),
                point(2.0, 0.0, 3.0)
            ],
            Options {
                max_triangle_edge_m: 3.0
            },
            &[]
        ),
        Err(Error::DegenerateSurface)
    ));
    assert!(matches!(
        Tin::build(
            &points[..2],
            Options {
                max_triangle_edge_m: 2.0
            },
            &[]
        ),
        Err(Error::TooFewUniquePoints { count: 2 })
    ));
    assert!(matches!(
        Tin::build(
            &[
                point(0.0, 0.0, 1.0),
                point(1_000_001.0, 0.0, 2.0),
                point(0.0, 1.0, 3.0)
            ],
            Options {
                max_triangle_edge_m: 2_000_000.0
            },
            &[]
        ),
        Err(Error::ExtentTooLarge)
    ));
}

#[test]
fn dense_query_batches_have_a_work_budget_in_addition_to_count_limit() {
    let points: Vec<_> = (0..40)
        .flat_map(|x| (0..40).map(move |y| point(x as f64, y as f64, 0.0)))
        .collect();
    let tin = build(&points, 1_000_000.0);
    close(tin.sample(20.0, 20.0).unwrap(), 0.0);
    assert!(matches!(
        tin.sample_many(&vec![[20.0, 20.0]; MAX_PROJECTED_TIN_QUERIES]),
        Err(Error::QueryWorkLimit { .. })
    ));
    close(tin.sample_many(&[[20.0, 20.0]]).unwrap()[0], 0.0);
}
