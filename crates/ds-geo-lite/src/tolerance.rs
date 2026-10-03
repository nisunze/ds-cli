//! Compatibility tolerances, separated by units and operation semantics.
//! See `docs/contracts/geometry-tolerance-policy.md`; these are not a universal EPS.
pub const EDIT_PARAMETER_EPS: f64 = 1e-10;
pub const REGION_DETERMINANT: f64 = 1e-12;
pub const REGION_BOUNDS: f64 = 1e-12;
pub const LV_STATION_VERTEX_M: f64 = 1e-3;
/// Borrowed-pole projection keeps the original squared-metre degeneracy rule.
pub const POLE_SOURCE_SEGMENT_MIN_SQUARED_LENGTH_M2: f64 = f64::EPSILON;
