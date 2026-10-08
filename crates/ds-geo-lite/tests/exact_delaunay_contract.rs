#![cfg(feature = "tin")]

use ds_geo_lite::exact_delaunay::{
    ExactDelaunay, ExactDelaunayError, MAX_EXACT_COORDINATE, in_circle_exact, orient,
};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 17
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn build(points: &[(i64, i64, u32)]) -> ExactDelaunay<u32> {
    let t = ExactDelaunay::build(points.iter().copied()).unwrap();
    t.verify().unwrap();
    t
}

/// Independent 4x4 lifted determinant: the Leibniz sum over all 24
/// permutations, each signed by its inversion count.
fn det4(m: [[i128; 4]; 4]) -> i128 {
    let mut total = 0;
    for p0 in 0..4 {
        for p1 in 0..4 {
            for p2 in 0..4 {
                for p3 in 0..4 {
                    let p = [p0, p1, p2, p3];
                    if (0..4).any(|i| (i + 1..4).any(|j| p[i] == p[j])) {
                        continue;
                    }
                    let inversions = (0..4)
                        .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
                        .filter(|(i, j)| p[*i] > p[*j])
                        .count();
                    let sign = if inversions.is_multiple_of(2) { 1 } else { -1 };
                    total += sign * (0..4).map(|r| m[r][p[r]]).product::<i128>();
                }
            }
        }
    }
    total
}

#[test]
fn exact_in_circle_matches_the_lifted_determinant() {
    let mut random = Lcg(3);
    for _ in 0..2_000 {
        let p: Vec<(i64, i64)> = (0..4)
            .map(|_| (random.below(41) as i64 - 20, random.below(41) as i64 - 20))
            .collect();
        let row = |q: (i64, i64)| [q.0 as i128, q.1 as i128, (q.0 * q.0 + q.1 * q.1) as i128, 1];
        let lifted = det4([row(p[0]), row(p[1]), row(p[2]), row(p[3])]);
        assert_eq!(in_circle_exact(p[0], p[1], p[2], p[3]), lifted);
    }
}

#[test]
fn cocircular_diagonal_matches_an_explicit_lifted_perturbation() {
    // Integer points on x^2 + y^2 = 25. With lifted coordinates scaled by B^5
    // and key rank r perturbed by B^(4 - r), the explicit determinant's sign is
    // the infinitesimal perturbation's sign; it decides the Delaunay diagonal.
    let circle = [
        (5, 0),
        (4, 3),
        (3, 4),
        (0, 5),
        (-3, 4),
        (-4, 3),
        (-5, 0),
        (-4, -3),
        (-3, -4),
        (0, -5),
        (3, -4),
        (4, -3),
    ];
    let b: i128 = 100_000;
    let mut random = Lcg(17);
    for _ in 0..300 {
        let mut chosen: Vec<usize> = Vec::new();
        while chosen.len() < 4 {
            let i = random.below(circle.len() as u64) as usize;
            if !chosen.contains(&i) {
                chosen.push(i);
            }
        }
        chosen.sort();
        let quad: Vec<(i64, i64)> = chosen.iter().map(|i| circle[*i]).collect();
        let keys: Vec<u32> = (0..4).map(|_| random.below(1_000) as u32).collect();
        if keys.iter().collect::<std::collections::BTreeSet<_>>().len() < 4 {
            continue;
        }
        let rank = |k: u32| keys.iter().filter(|other| **other < k).count() as u32;
        let row = |i: usize| {
            let (x, y) = quad[i];
            [
                x as i128,
                y as i128,
                (x * x + y * y) as i128 * b.pow(5) + b.pow(4 - rank(keys[i])),
                1,
            ]
        };
        // (a, b, c) counterclockwise around the circle; d the fourth point.
        let perturbed = det4([row(0), row(1), row(2), row(3)]);
        let expected_diagonal_ac = perturbed < 0;
        let triangulation = build(
            &(0..4)
                .map(|i| (quad[i].0, quad[i].1, keys[i]))
                .collect::<Vec<_>>(),
        );
        let triangles = triangulation.canonical_triangles();
        assert_eq!(triangles.len(), 2);
        let has_edge = |p: u32, q: u32| {
            triangles
                .iter()
                .filter(|t| t.contains(&p) && t.contains(&q))
                .count()
                == 2
        };
        assert_eq!(
            has_edge(keys[0], keys[2]),
            expected_diagonal_ac,
            "{quad:?} {keys:?}"
        );
        assert_eq!(has_edge(keys[1], keys[3]), !expected_diagonal_ac);
    }
}

#[test]
fn cocircular_ties_follow_one_symbolic_perturbation_independent_of_order() {
    // A regular grid is all cocircular squares: every insertion order and every
    // removal sequence must still produce the one perturbed triangulation.
    let mut points = Vec::new();
    for x in 0..12i64 {
        for y in 0..9i64 {
            points.push((x * 10_000, y * 10_000, (x * 100 + y) as u32));
        }
    }
    let reference = build(&points).canonical_triangles();
    let mut random = Lcg(11);
    for _ in 0..6 {
        let mut shuffled = points.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, random.below(i as u64 + 1) as usize);
        }
        assert_eq!(build(&shuffled).canonical_triangles(), reference);
    }
    // Each square chooses exactly one diagonal.
    assert_eq!(reference.len(), 2 * 11 * 8);
}

#[test]
fn incremental_inserts_and_removals_equal_fresh_builds() {
    for seed in [1u64, 2, 3, 4, 5] {
        let mut random = Lcg(seed);
        let mut live: Vec<(i64, i64, u32)> = Vec::new();
        let mut triangulation: ExactDelaunay<u32> = ExactDelaunay::new();
        let mut handles = std::collections::BTreeMap::new();
        let mut key = 0u32;
        // Mix scattered points, a coarse grid (cocircular) and collinear rows.
        let candidate = |random: &mut Lcg| -> (i64, i64) {
            match random.below(3) {
                0 => (
                    random.below(400) as i64 * 250,
                    random.below(400) as i64 * 250,
                ),
                1 => (
                    random.below(8) as i64 * 5_000,
                    random.below(8) as i64 * 5_000,
                ),
                _ => (random.below(60) as i64 * 1_000, 7_000),
            }
        };
        for step in 0..400 {
            let remove = !live.is_empty() && random.below(3) == 0;
            if remove {
                let at = random.below(live.len() as u64) as usize;
                let (x, y, k) = live.swap_remove(at);
                let handle = handles.remove(&k).unwrap();
                assert_eq!(triangulation.remove(handle).unwrap(), k);
                assert!(triangulation.vertex_at(x, y).is_none());
            } else {
                let (x, y) = candidate(&mut random);
                if triangulation.vertex_at(x, y).is_some() {
                    continue;
                }
                key += 1;
                let handle = triangulation.insert(x, y, key).unwrap();
                handles.insert(key, handle);
                live.push((x, y, key));
            }
            if step % 7 == 0 || step > 390 {
                triangulation
                    .verify()
                    .unwrap_or_else(|e| panic!("seed {seed} step {step}: {e}"));
                assert_eq!(
                    triangulation.canonical_triangles(),
                    build(&live).canonical_triangles(),
                    "seed {seed} step {step}"
                );
            }
        }
    }
}

#[test]
fn insertion_and_removal_touch_only_their_cavity() {
    let mut random = Lcg(42);
    let mut points = Vec::new();
    for key in 0..3_000u32 {
        points.push((
            random.below(2_000_000) as i64,
            random.below(2_000_000) as i64,
            key,
        ));
    }
    points.sort();
    points.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let mut triangulation = build(&points);
    let total = triangulation.finite_triangle_count();
    let handle = triangulation.insert(1_000_123, 999_877, 900_000).unwrap();
    let work = triangulation.last_work();
    assert!(
        work.removed_triangles < 30 && work.created_triangles < 32,
        "{work:?}"
    );
    triangulation.remove(handle).unwrap();
    let work = triangulation.last_work();
    assert!(work.removed_triangles < 32, "{work:?}");
    assert_eq!(triangulation.finite_triangle_count(), total);
    triangulation.verify().unwrap();
}

#[test]
fn collinear_sets_hold_no_triangle_until_a_point_leaves_the_line() {
    let mut triangulation: ExactDelaunay<u32> = ExactDelaunay::new();
    for k in 0..6 {
        triangulation
            .insert(k * 1_000, 2 * k * 1_000, k as u32)
            .unwrap();
    }
    assert_eq!(triangulation.finite_triangle_count(), 0);
    triangulation.verify().unwrap();
    let apex = triangulation.insert(-5_000, 9_000, 99).unwrap();
    // A fan from the apex to five collinear intervals.
    assert_eq!(triangulation.finite_triangle_count(), 5);
    triangulation.verify().unwrap();
    triangulation.remove(apex).unwrap();
    assert_eq!(triangulation.finite_triangle_count(), 0);
    triangulation.verify().unwrap();
    // Points exactly on a hull edge split that edge.
    let mut hull = build(&[(0, 0, 1), (10_000, 0, 2), (0, 10_000, 3)]);
    hull.insert(5_000, 0, 4).unwrap();
    hull.verify().unwrap();
    assert_eq!(hull.finite_triangle_count(), 2);
    let removed = hull.vertex_at(5_000, 0).unwrap();
    hull.remove(removed).unwrap();
    hull.verify().unwrap();
    assert_eq!(hull.finite_triangle_count(), 1);
}

#[test]
fn extreme_admitted_coordinates_stay_exact_and_others_refuse() {
    let m = MAX_EXACT_COORDINATE;
    let points = [
        (-m, -m, 1u32),
        (m, -m, 2),
        (m, m, 3),
        (-m, m, 4),
        (0, 0, 5),
        (m - 1, m - 2, 6),
        (-m + 3, m - 1, 7),
    ];
    let triangulation = build(&points);
    assert!(triangulation.finite_triangle_count() >= 6);
    let mut refused: ExactDelaunay<u32> = ExactDelaunay::new();
    assert_eq!(
        refused.insert(m + 1, 0, 1),
        Err(ExactDelaunayError::CoordinateOutOfRange)
    );
    refused.insert(0, 0, 1).unwrap();
    assert!(matches!(
        refused.insert(0, 0, 2),
        Err(ExactDelaunayError::DuplicatePosition { .. })
    ));
    assert!(orient((-m, -m), (m, -m), (m, m)) > 0);
}

#[test]
fn neighbors_follow_finite_edges() {
    let triangulation = build(&[(0, 0, 1), (10, 0, 2), (10, 10, 3), (0, 10, 4), (5, 5, 5)]);
    let center = triangulation.vertex_at(5, 5).unwrap();
    let mut keys: Vec<u32> = triangulation
        .neighbors(center)
        .into_iter()
        .map(|v| *triangulation.key(v).unwrap())
        .collect();
    keys.sort();
    assert_eq!(keys, [1, 2, 3, 4]);
}

/// Opt-in release timing: scattered and cocircular-grid builds plus local
/// insert/remove on the built triangulation.
#[test]
#[ignore = "timing evidence; run with --release --ignored --nocapture"]
fn release_build_and_local_update_costs() {
    let mut random = Lcg(99);
    let scattered: Vec<(i64, i64, u32)> = (0..20_000u32)
        .map(|k| {
            (
                random.below(8_000_000) as i64,
                random.below(8_000_000) as i64,
                k,
            )
        })
        .collect();
    let grid: Vec<(i64, i64, u32)> = (0..44_100u32)
        .map(|k| (((k % 210) * 10_000) as i64, ((k / 210) * 10_000) as i64, k))
        .collect();
    for (name, mut points) in [("scattered_20k", scattered), ("grid_44k", grid)] {
        points.sort_by_key(|p| (p.0 / 64_000, p.1));
        points.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        let started = std::time::Instant::now();
        let mut triangulation = ExactDelaunay::build(points.iter().copied()).unwrap();
        let build_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        let handle = triangulation
            .insert(1_000_017, 1_000_013, u32::MAX)
            .unwrap();
        let insert_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        triangulation.remove(handle).unwrap();
        let remove_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{{\"case\":\"{name}\",\"points\":{},\"triangles\":{},\"build_ms\":{build_ms:.3},\"insert_ms\":{insert_ms:.4},\"remove_ms\":{remove_ms:.4}}}",
            points.len(),
            triangulation.finite_triangle_count()
        );
    }
}
