use crate::types::Coord;

/// Initial bearing from `a` to `b` (degrees, 0 = north, clockwise).
pub fn initial(a: Coord, b: Coord) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let dlng = (b.lng - a.lng).to_radians();

    let y = dlng.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlng.cos();

    (y.atan2(x).to_degrees() + 360.0) % 360.0
}
