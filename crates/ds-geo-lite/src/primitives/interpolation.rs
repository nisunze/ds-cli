use crate::types::Coord;

/// Linear interpolation between two coordinates.
///
/// `t` in [0, 1] — 0 returns `a`, 1 returns `b`.
/// Accurate for segments under ~50 km.
pub fn lerp(a: Coord, b: Coord, t: f64) -> Coord {
    Coord::new(a.lng + t * (b.lng - a.lng), a.lat + t * (b.lat - a.lat))
}
