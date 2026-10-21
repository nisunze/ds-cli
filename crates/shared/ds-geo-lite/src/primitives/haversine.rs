use crate::types::{Coord, EARTH_RADIUS_M};

/// Haversine distance between two WGS-84 points (meters).
pub fn distance(a: Coord, b: Coord) -> f64 {
    let dlat = (b.lat - a.lat).to_radians();
    let dlng = (b.lng - a.lng).to_radians();
    let h = (dlat / 2.0).sin().powi(2)
        + a.lat.to_radians().cos() * b.lat.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    EARTH_RADIUS_M * 2.0 * h.sqrt().atan2((1.0 - h).sqrt())
}
