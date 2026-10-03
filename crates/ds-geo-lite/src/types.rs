/// A WGS-84 coordinate (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coord {
    pub lng: f64,
    pub lat: f64,
}

impl Coord {
    pub fn new(lng: f64, lat: f64) -> Self {
        Self { lng, lat }
    }
}

/// A point with cumulative distance from some origin.
#[derive(Clone, Copy, Debug)]
pub struct StationedPoint {
    pub coord: Coord,
    pub distance_m: f64,
}

/// Earth mean radius (meters) — WGS-84 volumetric.
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;
