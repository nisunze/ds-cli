//! Neutral parameterized geographic projection mathematics.
//! Exact projected model CRS vocabulary and admission live in sibling `model_crs`.

use serde::{Deserialize, Serialize};

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;
const WEB_MERCATOR_RADIUS_M: f64 = 6_378_137.0;
const WGS84_A: f64 = 6_378_137.0;
const WGS84_INV_F: f64 = 298.257_223_563;
const NIX_ITRF_A: f64 = 6_378_137.0;
const NIX_ITRF_INV_F: f64 = 298.257_222_101;
const NIX_ITRF_FALSE_EASTING: f64 = 500_000.0;
const NIX_ITRF_FALSE_NORTHING: f64 = 5_000_000.0;
const NIX_ITRF_LON0_DEG: f64 = 30.0;
const NIX_ITRF_K0: f64 = 0.9999;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ProjectionMode {
    NixItrf2005,
    Utm,
    LocalTangent,
    WebMercator,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ProjectionSelection {
    Auto,
    NixItrf2005,
    Utm,
    LocalTangent,
    WebMercator,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
pub struct ProjectionConfig {
    pub mode: ProjectionSelection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_zone: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_southern: Option<bool>,
}

impl Default for ProjectionConfig {
    fn default() -> Self {
        Self {
            mode: ProjectionSelection::Auto,
            utm_zone: None,
            utm_southern: None,
        }
    }
}

/// Temporary metric editing/build plane. Public feature payloads remain
/// lon/lat; this describes the Rust-owned x/y representation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
pub struct ProjectionMetadata {
    pub mode: ProjectionMode,
    pub origin_lng: f64,
    pub origin_lat: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_zone: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_southern: Option<bool>,
}

impl Default for ProjectionMetadata {
    fn default() -> Self {
        Self {
            mode: ProjectionMode::LocalTangent,
            origin_lng: 0.0,
            origin_lat: 0.0,
            utm_zone: None,
            utm_southern: None,
        }
    }
}

impl ProjectionMetadata {
    pub fn from_lonlat_coords<I>(coords: I) -> Self
    where
        I: IntoIterator<Item = [f64; 2]>,
    {
        Self::from_lonlat_coords_with_config(coords, ProjectionConfig::default())
    }

    pub fn from_lonlat_coords_with_config<I>(coords: I, config: ProjectionConfig) -> Self
    where
        I: IntoIterator<Item = [f64; 2]>,
    {
        let mut bounds = LonLatBounds::default();
        for coord in coords {
            bounds.include_coord(coord);
        }
        match bounds.center() {
            Some([origin_lng, origin_lat]) => {
                Self::for_origin_with_config(origin_lng, origin_lat, config)
            }
            None => Self::default(),
        }
    }

    pub fn auto_for_origin(origin_lng: f64, origin_lat: f64) -> Self {
        if is_rwanda_lonlat(origin_lng, origin_lat) {
            return Self::nix_itrf2005(origin_lng, origin_lat);
        }
        Self::utm_for_origin(origin_lng, origin_lat)
    }

    pub fn for_origin_with_config(
        origin_lng: f64,
        origin_lat: f64,
        config: ProjectionConfig,
    ) -> Self {
        match config.mode {
            ProjectionSelection::Auto => Self::auto_for_origin(origin_lng, origin_lat),
            ProjectionSelection::NixItrf2005 => Self::nix_itrf2005(origin_lng, origin_lat),
            ProjectionSelection::Utm => Self::utm(
                origin_lng,
                origin_lat,
                config
                    .utm_zone
                    .unwrap_or_else(|| utm_zone_for_lng(origin_lng)),
                config.utm_southern.unwrap_or(origin_lat < 0.0),
            ),
            ProjectionSelection::LocalTangent => Self::local_tangent(origin_lng, origin_lat),
            ProjectionSelection::WebMercator => Self::web_mercator(origin_lng, origin_lat),
        }
    }

    pub fn nix_itrf2005(origin_lng: f64, origin_lat: f64) -> Self {
        Self {
            mode: ProjectionMode::NixItrf2005,
            origin_lng,
            origin_lat,
            utm_zone: None,
            utm_southern: None,
        }
    }

    pub fn utm_for_origin(origin_lng: f64, origin_lat: f64) -> Self {
        Self::utm(
            origin_lng,
            origin_lat,
            utm_zone_for_lng(origin_lng),
            origin_lat < 0.0,
        )
    }

    pub fn utm(origin_lng: f64, origin_lat: f64, zone: u8, southern: bool) -> Self {
        Self {
            mode: ProjectionMode::Utm,
            origin_lng,
            origin_lat,
            utm_zone: Some(zone.clamp(1, 60)),
            utm_southern: Some(southern),
        }
    }

    pub fn local_tangent(origin_lng: f64, origin_lat: f64) -> Self {
        Self {
            mode: ProjectionMode::LocalTangent,
            origin_lng,
            origin_lat,
            utm_zone: None,
            utm_southern: None,
        }
    }

    pub fn web_mercator(origin_lng: f64, origin_lat: f64) -> Self {
        Self {
            mode: ProjectionMode::WebMercator,
            origin_lng,
            origin_lat,
            utm_zone: None,
            utm_southern: None,
        }
    }

    pub fn project_coord(&self, coord: [f64; 2]) -> [f64; 2] {
        match self.mode {
            ProjectionMode::NixItrf2005 => {
                let (x, y) = wgs84_to_nix_itrf2005(coord[0], coord[1]);
                [x, y]
            }
            ProjectionMode::Utm => {
                let params = self.utm_params();
                let (x, y) = transverse_mercator_forward(coord[0], coord[1], params);
                [x, y]
            }
            ProjectionMode::WebMercator => web_mercator_project(coord),
            ProjectionMode::LocalTangent => {
                let cos_lat = self.origin_lat.to_radians().cos().abs().max(1e-9);
                [
                    (coord[0] - self.origin_lng).to_radians() * EARTH_RADIUS_M * cos_lat,
                    (coord[1] - self.origin_lat).to_radians() * EARTH_RADIUS_M,
                ]
            }
        }
    }

    pub fn unproject_coord(&self, point: [f64; 2]) -> [f64; 2] {
        match self.mode {
            ProjectionMode::NixItrf2005 => {
                let (lng, lat) = nix_itrf2005_to_wgs84(point[0], point[1]);
                [lng, lat]
            }
            ProjectionMode::Utm => {
                let params = self.utm_params();
                let (lng, lat) = transverse_mercator_inverse(point[0], point[1], params);
                [lng, lat]
            }
            ProjectionMode::WebMercator => web_mercator_unproject(point),
            ProjectionMode::LocalTangent => {
                let cos_lat = self.origin_lat.to_radians().cos().abs().max(1e-9);
                [
                    self.origin_lng + (point[0] / (EARTH_RADIUS_M * cos_lat)).to_degrees(),
                    self.origin_lat + (point[1] / EARTH_RADIUS_M).to_degrees(),
                ]
            }
        }
    }

    pub fn distance_m(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        projected_distance(self.project_coord(a), self.project_coord(b))
    }

    fn utm_params(&self) -> TransverseMercatorParams {
        let zone = self
            .utm_zone
            .unwrap_or_else(|| utm_zone_for_lng(self.origin_lng));
        let central_meridian_deg = f64::from(zone) * 6.0 - 183.0;
        TransverseMercatorParams {
            semi_major: WGS84_A,
            inv_f: WGS84_INV_F,
            false_easting: 500_000.0,
            false_northing: if self.utm_southern.unwrap_or(self.origin_lat < 0.0) {
                10_000_000.0
            } else {
                0.0
            },
            central_meridian_deg,
            scale_factor: 0.9996,
        }
    }
}

fn is_rwanda_lonlat(lng: f64, lat: f64) -> bool {
    lng.is_finite()
        && lat.is_finite()
        && (28.75..=31.1).contains(&lng)
        && (-2.95..=-1.0).contains(&lat)
}

fn utm_zone_for_lng(lng: f64) -> u8 {
    ((lng + 180.0) / 6.0).floor().clamp(0.0, 59.0) as u8 + 1
}

#[derive(Clone, Copy)]
struct TransverseMercatorParams {
    semi_major: f64,
    inv_f: f64,
    false_easting: f64,
    false_northing: f64,
    central_meridian_deg: f64,
    scale_factor: f64,
}

fn nix_params() -> TransverseMercatorParams {
    TransverseMercatorParams {
        semi_major: NIX_ITRF_A,
        inv_f: NIX_ITRF_INV_F,
        false_easting: NIX_ITRF_FALSE_EASTING,
        false_northing: NIX_ITRF_FALSE_NORTHING,
        central_meridian_deg: NIX_ITRF_LON0_DEG,
        scale_factor: NIX_ITRF_K0,
    }
}

pub fn wgs84_to_nix_itrf2005(lon_deg: f64, lat_deg: f64) -> (f64, f64) {
    transverse_mercator_forward(lon_deg, lat_deg, nix_params())
}

pub fn nix_itrf2005_to_wgs84(easting: f64, northing: f64) -> (f64, f64) {
    transverse_mercator_inverse(easting, northing, nix_params())
}

fn transverse_mercator_forward(
    lon_deg: f64,
    lat_deg: f64,
    params: TransverseMercatorParams,
) -> (f64, f64) {
    let f = 1.0 / params.inv_f;
    let e2 = f * (2.0 - f);
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    let ep2 = e2 / (1.0 - e2);

    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let lon0 = params.central_meridian_deg.to_radians();
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let tan_lat = lat.tan();

    let n = params.semi_major / (1.0 - e2 * sin_lat * sin_lat).sqrt();
    let t = tan_lat * tan_lat;
    let c = ep2 * cos_lat * cos_lat;
    let a = (lon - lon0) * cos_lat;

    let m = params.semi_major
        * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * lat
            - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * lat).sin()
            + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * lat).sin()
            - (35.0 * e6 / 3072.0) * (6.0 * lat).sin());

    let a2 = a * a;
    let a3 = a2 * a;
    let a4 = a2 * a2;
    let a5 = a4 * a;
    let a6 = a4 * a2;

    let easting = params.false_easting
        + params.scale_factor
            * n
            * (a + (1.0 - t + c) * a3 / 6.0
                + (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * ep2) * a5 / 120.0);
    let northing = params.false_northing
        + params.scale_factor
            * (m + n
                * tan_lat
                * (a2 / 2.0
                    + (5.0 - t + 9.0 * c + 4.0 * c * c) * a4 / 24.0
                    + (61.0 - 58.0 * t + t * t + 600.0 * c - 330.0 * ep2) * a6 / 720.0));

    (easting, northing)
}

fn transverse_mercator_inverse(
    easting: f64,
    northing: f64,
    params: TransverseMercatorParams,
) -> (f64, f64) {
    let f = 1.0 / params.inv_f;
    let e2 = f * (2.0 - f);
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());

    let x = easting - params.false_easting;
    let y = northing - params.false_northing;
    let m = y / params.scale_factor;
    let mu = m / (params.semi_major * (1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0));

    let e1_2 = e1 * e1;
    let e1_3 = e1_2 * e1;
    let e1_4 = e1_2 * e1_2;
    let phi1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1_3 / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1_2 / 16.0 - 55.0 * e1_4 / 32.0) * (4.0 * mu).sin()
        + (151.0 * e1_3 / 96.0) * (6.0 * mu).sin()
        + (1097.0 * e1_4 / 512.0) * (8.0 * mu).sin();

    let ep2 = e2 / (1.0 - e2);
    let sin_phi1 = phi1.sin();
    let cos_phi1 = phi1.cos();
    let tan_phi1 = phi1.tan();
    let n1 = params.semi_major / (1.0 - e2 * sin_phi1 * sin_phi1).sqrt();
    let r1 = params.semi_major * (1.0 - e2) / (1.0 - e2 * sin_phi1 * sin_phi1).powf(1.5);
    let c1 = ep2 * cos_phi1 * cos_phi1;
    let t1 = tan_phi1 * tan_phi1;
    let d = x / (n1 * params.scale_factor);

    let d2 = d * d;
    let d3 = d2 * d;
    let d4 = d2 * d2;
    let d5 = d4 * d;
    let d6 = d4 * d2;

    let lat = phi1
        - (n1 * tan_phi1 / r1)
            * (d2 / 2.0 - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * ep2) * d4 / 24.0
                + (61.0 + 90.0 * t1 + 298.0 * c1 + 45.0 * t1 * t1 - 252.0 * ep2 - 3.0 * c1 * c1)
                    * d6
                    / 720.0);
    let lon0 = params.central_meridian_deg.to_radians();
    let lon = lon0
        + (d - (1.0 + 2.0 * t1 + c1) * d3 / 6.0
            + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1 * c1 + 8.0 * ep2 + 24.0 * t1 * t1) * d5
                / 120.0)
            / cos_phi1;

    (lon.to_degrees(), lat.to_degrees())
}

pub fn web_mercator_project([lng, lat]: [f64; 2]) -> [f64; 2] {
    let lat = lat.clamp(-85.051_128_78, 85.051_128_78).to_radians();
    [
        WEB_MERCATOR_RADIUS_M * lng.to_radians(),
        WEB_MERCATOR_RADIUS_M * ((std::f64::consts::FRAC_PI_4 + lat / 2.0).tan()).ln(),
    ]
}

pub fn web_mercator_unproject([x, y]: [f64; 2]) -> [f64; 2] {
    [
        (x / WEB_MERCATOR_RADIUS_M).to_degrees(),
        (2.0 * (y / WEB_MERCATOR_RADIUS_M).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    ]
}

#[derive(Default)]
pub struct LonLatBounds {
    min_lng: f64,
    min_lat: f64,
    max_lng: f64,
    max_lat: f64,
    any: bool,
}

impl LonLatBounds {
    pub fn include_coord(&mut self, [lng, lat]: [f64; 2]) {
        if !lng.is_finite() || !lat.is_finite() {
            return;
        }
        if !self.any {
            self.min_lng = lng;
            self.max_lng = lng;
            self.min_lat = lat;
            self.max_lat = lat;
            self.any = true;
            return;
        }
        self.min_lng = self.min_lng.min(lng);
        self.max_lng = self.max_lng.max(lng);
        self.min_lat = self.min_lat.min(lat);
        self.max_lat = self.max_lat.max(lat);
    }

    pub fn center(&self) -> Option<[f64; 2]> {
        self.any.then_some([
            (self.min_lng + self.max_lng) / 2.0,
            (self.min_lat + self.max_lat) / 2.0,
        ])
    }
}

pub fn projected_distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    (dx * dx + dy * dy).sqrt()
}

pub fn interpolate_projected(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub fn projected_segment_t(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let denom = dx * dx + dy * dy;
    if denom <= 0.0 {
        0.0
    } else {
        ((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / denom
    }
}

pub fn projected_point_segment_distance(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let t = projected_segment_t(point, a, b).clamp(0.0, 1.0);
    projected_distance(point, interpolate_projected(a, b, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rwanda_auto_projection_uses_nix_itrf2005() {
        let projection = ProjectionMetadata::from_lonlat_coords([[29.430_251, -1.946_812]]);
        assert_eq!(projection.mode, ProjectionMode::NixItrf2005);
    }

    #[test]
    fn outside_rwanda_auto_projection_uses_utm() {
        let projection = ProjectionMetadata::from_lonlat_coords([[2.3522, 48.8566]]);
        assert_eq!(projection.mode, ProjectionMode::Utm);
        assert_eq!(projection.utm_zone, Some(31));
        assert_eq!(projection.utm_southern, Some(false));
    }

    #[test]
    fn nix_itrf2005_origin_matches_legacy_grid() {
        let [x, y] = ProjectionMetadata::nix_itrf2005(30.0, 0.0).project_coord([30.0, 0.0]);
        assert!((x - 500_000.0).abs() < 1e-6);
        assert!((y - 5_000_000.0).abs() < 1e-6);
    }

    #[test]
    fn projection_round_trips_rwanda_sample() {
        let projection = ProjectionMetadata::from_lonlat_coords([[29.430_251, -1.946_812]]);
        let original = [29.430_251, -1.946_812];
        let projected = projection.project_coord(original);
        let restored = projection.unproject_coord(projected);
        assert!((restored[0] - original[0]).abs() < 1e-8);
        assert!((restored[1] - original[1]).abs() < 1e-8);
    }

    #[test]
    fn projection_round_trips_utm_sample() {
        let projection = ProjectionMetadata::from_lonlat_coords([[2.3522, 48.8566]]);
        let original = [2.3522, 48.8566];
        let projected = projection.project_coord(original);
        let restored = projection.unproject_coord(projected);
        assert!((restored[0] - original[0]).abs() < 1e-8);
        assert!((restored[1] - original[1]).abs() < 1e-8);
    }
}
