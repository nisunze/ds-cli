use super::*;

const METERS_PER_LAT_DEG: f64 = 111_320.0;

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
fn non_positive_or_non_finite_tolerance_leaves_coords_unchanged() {
    let projection = projection_at(30.0, 0.0);
    let original = vec![[30.0, 0.0], [30.0, 0.0], [30.0, 0.0], [30.001, 0.0]];
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut coords = original.clone();
        dedupe_line_coords(&mut coords, projection, tolerance);
        assert_eq!(coords, original);
        let mut ring = original.clone();
        ring.push(original[0]);
        let expected = ring.clone();
        dedupe_ring_coords(&mut ring, projection, tolerance);
        assert_eq!(ring, expected);
    }
}
