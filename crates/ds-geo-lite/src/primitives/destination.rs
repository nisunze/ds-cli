use crate::types::{Coord, EARTH_RADIUS_M};

/// Geodesic destination point — travel `distance_m` meters from `origin`
/// along the initial `bearing_deg` (0 = north, clockwise). Spherical
/// forward (direct) formula; the inverse of `bearing::initial`.
///
/// Accurate to a few cm at LV/MV span scales; the same spherical model the
/// rest of geo-ops uses (haversine, bearing).
pub fn destination(origin: Coord, bearing_deg: f64, distance_m: f64) -> Coord {
    let angular = distance_m / EARTH_RADIUS_M;
    let brng = bearing_deg.to_radians();
    let lat1 = origin.lat.to_radians();
    let lng1 = origin.lng.to_radians();

    let sin_lat1 = lat1.sin();
    let cos_lat1 = lat1.cos();
    let sin_ang = angular.sin();
    let cos_ang = angular.cos();

    let sin_lat2 = sin_lat1 * cos_ang + cos_lat1 * sin_ang * brng.cos();
    let lat2 = sin_lat2.asin();
    let y = brng.sin() * sin_ang * cos_lat1;
    let x = cos_ang - sin_lat1 * sin_lat2;
    let lng2 = lng1 + y.atan2(x);

    Coord::new(lng2.to_degrees(), lat2.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::haversine::distance;

    #[test]
    fn destination_is_inverse_of_haversine() {
        let origin = Coord::new(30.0, -2.0);
        for bearing in [0.0, 45.0, 90.0, 137.0, 270.0, 359.0] {
            let p = destination(origin, bearing, 250.0);
            assert!(
                (distance(origin, p) - 250.0).abs() < 0.05,
                "bearing {bearing}"
            );
        }
    }

    #[test]
    fn due_east_increases_longitude() {
        let p = destination(Coord::new(30.0, 0.0), 90.0, 1000.0);
        assert!(p.lng > 30.0);
        assert!(p.lat.abs() < 1e-6);
    }

    #[test]
    fn bearing_then_destination_round_trips() {
        // The kernel behind ortho/polar tracking + dynamic length/bearing input:
        // destination(a, bearing(a, b), dist(a, b)) ≈ b.
        use crate::primitives::bearing::initial;
        let a = Coord::new(30.0, -2.0);
        let b = Coord::new(30.012, -1.994);
        let back = destination(a, initial(a, b), distance(a, b));
        assert!(
            distance(back, b) < 0.1,
            "round-trip drifted {} m",
            distance(back, b)
        );
    }
}
