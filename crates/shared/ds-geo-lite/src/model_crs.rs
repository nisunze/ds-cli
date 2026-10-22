//! Exact projected model CRS vocabulary, admission and scalar transforms.
//! Shared by native engineering and application adapters; no model state or IO.

use crate::projection::*;
use serde::{Deserialize, Serialize};
use std::fmt;
const WGS84_A: f64 = 6_378_137.0;
const WGS84_INV_F: f64 = 298.257_223_563;

/// Canonical, parameter-complete declaration for Rwanda's characterized
/// NIX/ITRF 2005 Transverse Mercator grid. The grid has no EPSG alias;
/// EPSG:4083 is REGCAN95 / UTM zone 28N.
pub const RWANDA_TM_CRS: &str = "+proj=tmerc +lat_0=0 +lon_0=30 +k=0.9999 \
+x_0=500000 +y_0=5000000 +ellps=GRS80 +units=m +no_defs";

/// A projected metric model CRS that the platform can invert to WGS84.
///
/// This is the shared support authority for model creation, exchange, and the
/// browser adapter. Adding a CRS anywhere else without adding it here would
/// make a model that another platform surface cannot place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridModelCrs {
    RwandaTm,
    Utm { zone: u8, southern: bool },
    WorldMercator,
    WebMercator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridModelCrsError {
    detail: String,
}

impl GridModelCrsError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for GridModelCrsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for GridModelCrsError {}

/// The fixed scalar model-CRS operation surface. Unknown additive fields are
/// compatible; the operation and its required field types remain authoritative.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum GridModelCrsRequest {
    Supports { crs: String },
    Normalize { crs: String },
    ToMap { crs: String, point: [f64; 2] },
    FromMap { crs: String, point: [f64; 2] },
}

/// The existing JSON shapes, also used directly by the typed application ABI.
/// Point precedes Normalized so decoding retains the supplied coordinate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(untagged)]
pub enum GridModelCrsResponse {
    Point {
        crs: String,
        point: [f64; 2],
    },
    Support {
        supported: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        crs: Option<String>,
    },
    Normalized {
        crs: String,
    },
}

/// Evaluate only the declared operation through the exact shared CRS authority.
pub fn evaluate(request: GridModelCrsRequest) -> Result<GridModelCrsResponse, GridModelCrsError> {
    Ok(match request {
        GridModelCrsRequest::Supports { crs } => match GridModelCrs::parse(&crs) {
            Ok(parsed) => GridModelCrsResponse::Support {
                supported: true,
                crs: Some(parsed.canonical_definition()),
            },
            Err(_) => GridModelCrsResponse::Support {
                supported: false,
                crs: None,
            },
        },
        GridModelCrsRequest::Normalize { crs } => GridModelCrsResponse::Normalized {
            crs: GridModelCrs::parse(&crs)?.canonical_definition(),
        },
        GridModelCrsRequest::ToMap { crs, point } => {
            let parsed = GridModelCrs::parse(&crs)?;
            GridModelCrsResponse::Point {
                crs: parsed.canonical_definition(),
                point: parsed.to_map(point)?,
            }
        }
        GridModelCrsRequest::FromMap { crs, point } => {
            let parsed = GridModelCrs::parse(&crs)?;
            GridModelCrsResponse::Point {
                crs: parsed.canonical_definition(),
                point: parsed.from_map(point)?,
            }
        }
    })
}

impl GridModelCrs {
    /// Parse the exact characterized model-CRS vocabulary.
    pub fn parse(raw: &str) -> Result<Self, GridModelCrsError> {
        let declared = raw.trim();
        if declared.is_empty() {
            return Err(GridModelCrsError::new(
                "a model coordinate system declaration is required",
            ));
        }
        if canonical_crs_words(declared) == canonical_crs_words(RWANDA_TM_CRS) {
            return Ok(Self::RwandaTm);
        }

        let normalized = declared.to_ascii_uppercase();
        match normalized.as_str() {
            "EPSG:0" => {
                return Err(GridModelCrsError::new(
                    "EPSG:0 is a placeholder, not a model coordinate system",
                ));
            }
            "EPSG:4083" => {
                return Err(GridModelCrsError::new(
                    "EPSG:4083 is REGCAN95 / UTM zone 28N, not the Rwanda Transverse Mercator grid; Rwanda TM has no EPSG code — declare its PROJ definition instead",
                ));
            }
            "EPSG:3395" => return Ok(Self::WorldMercator),
            "EPSG:3857" => return Ok(Self::WebMercator),
            _ => {}
        }

        for (prefix, southern) in [("EPSG:326", false), ("EPSG:327", true)] {
            if let Some(zone) = normalized.strip_prefix(prefix)
                && zone.len() == 2
                && let Ok(zone) = zone.parse::<u8>()
                && (1..=60).contains(&zone)
            {
                return Ok(Self::Utm { zone, southern });
            }
        }

        Err(GridModelCrsError::new(format!(
            "{declared} is not a characterized model coordinate system; declare WGS84 UTM (EPSG:32601..32660 / EPSG:32701..32760), EPSG:3395, EPSG:3857, or the parameterized Rwanda TM definition"
        )))
    }

    pub fn canonical_definition(self) -> String {
        match self {
            Self::RwandaTm => RWANDA_TM_CRS.to_string(),
            Self::Utm { zone, southern } => {
                format!("EPSG:{}{zone:02}", if southern { 327 } else { 326 })
            }
            Self::WorldMercator => "EPSG:3395".to_string(),
            Self::WebMercator => "EPSG:3857".to_string(),
        }
    }

    /// Convert projected model metres to WGS84 `[longitude, latitude]`.
    pub fn to_map(self, point: [f64; 2]) -> Result<[f64; 2], GridModelCrsError> {
        require_finite(point, "model")?;
        let result = match self {
            Self::RwandaTm => {
                let (lng, lat) = nix_itrf2005_to_wgs84(point[0], point[1]);
                [lng, lat]
            }
            Self::Utm { zone, southern } => {
                ProjectionMetadata::utm(0.0, 0.0, zone, southern).unproject_coord(point)
            }
            Self::WorldMercator => world_mercator_unproject(point),
            Self::WebMercator => web_mercator_unproject(point),
        };
        require_finite(result, "map")?;
        if result[1].abs() >= 90.0 {
            return Err(GridModelCrsError::new(
                "the projected coordinate resolves at or beyond a geographic pole",
            ));
        }
        Ok(result)
    }

    /// Convert WGS84 `[longitude, latitude]` to projected model metres.
    pub fn from_map(self, point: [f64; 2]) -> Result<[f64; 2], GridModelCrsError> {
        require_finite(point, "map")?;
        if point[1].abs() >= 90.0 {
            return Err(GridModelCrsError::new(
                "Mercator and Transverse Mercator are undefined at the geographic poles",
            ));
        }
        if matches!(self, Self::Utm { .. }) && !(-80.0..=84.0).contains(&point[1]) {
            return Err(GridModelCrsError::new(
                "WGS84 UTM supports latitudes from 80°S to 84°N",
            ));
        }
        let result = match self {
            Self::RwandaTm => {
                let (x, y) = wgs84_to_nix_itrf2005(point[0], point[1]);
                [x, y]
            }
            Self::Utm { zone, southern } => {
                ProjectionMetadata::utm(0.0, 0.0, zone, southern).project_coord(point)
            }
            Self::WorldMercator => world_mercator_project(point),
            Self::WebMercator => web_mercator_project(point),
        };
        require_finite(result, "model")?;
        Ok(result)
    }
}

fn canonical_crs_words(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn require_finite(point: [f64; 2], label: &str) -> Result<(), GridModelCrsError> {
    if point.into_iter().all(f64::is_finite) {
        Ok(())
    } else {
        Err(GridModelCrsError::new(format!(
            "{label} coordinates must be finite"
        )))
    }
}

fn world_mercator_project([lng, lat]: [f64; 2]) -> [f64; 2] {
    let flattening = 1.0 / WGS84_INV_F;
    let eccentricity = (flattening * (2.0 - flattening)).sqrt();
    let phi = lat.to_radians();
    let sin_phi = phi.sin();
    let eccentricity_term =
        ((1.0 - eccentricity * sin_phi) / (1.0 + eccentricity * sin_phi)).powf(eccentricity / 2.0);
    [
        WGS84_A * lng.to_radians(),
        WGS84_A * ((std::f64::consts::FRAC_PI_4 + phi / 2.0).tan() * eccentricity_term).ln(),
    ]
}

fn world_mercator_unproject([x, y]: [f64; 2]) -> [f64; 2] {
    let flattening = 1.0 / WGS84_INV_F;
    let eccentricity = (flattening * (2.0 - flattening)).sqrt();
    let t = (-y / WGS84_A).exp();
    let mut phi = std::f64::consts::FRAC_PI_2 - 2.0 * t.atan();
    for _ in 0..12 {
        let sin_phi = phi.sin();
        let next = std::f64::consts::FRAC_PI_2
            - 2.0
                * (t * ((1.0 - eccentricity * sin_phi) / (1.0 + eccentricity * sin_phi))
                    .powf(eccentricity / 2.0))
                .atan();
        if (next - phi).abs() < 1e-13 {
            phi = next;
            break;
        }
        phi = next;
    }
    [(x / WGS84_A).to_degrees(), phi.to_degrees()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_operations_preserve_the_support_and_coordinate_results() {
        assert_eq!(
            evaluate(GridModelCrsRequest::Supports {
                crs: "epsg:32736".into()
            })
            .unwrap(),
            GridModelCrsResponse::Support {
                supported: true,
                crs: Some("EPSG:32736".into())
            }
        );
        assert_eq!(
            evaluate(GridModelCrsRequest::Supports {
                crs: "EPSG:4326".into()
            })
            .unwrap(),
            GridModelCrsResponse::Support {
                supported: false,
                crs: None
            }
        );
        assert_eq!(
            evaluate(GridModelCrsRequest::FromMap {
                crs: RWANDA_TM_CRS.into(),
                point: [30.0, 0.0]
            })
            .unwrap(),
            GridModelCrsResponse::Point {
                crs: RWANDA_TM_CRS.into(),
                point: [500_000.0, 5_000_000.0]
            }
        );
        assert!(
            evaluate(GridModelCrsRequest::Normalize {
                crs: "EPSG:4083".into()
            })
            .is_err()
        );
        assert!(
            evaluate(GridModelCrsRequest::FromMap {
                crs: "EPSG:3395".into(),
                point: [0.0, 90.0]
            })
            .is_err()
        );
        assert!(
            evaluate(GridModelCrsRequest::ToMap {
                crs: "EPSG:3857".into(),
                point: [0.0, f64::NAN]
            })
            .is_err()
        );
    }

    #[cfg(feature = "geojson")]
    #[test]
    fn owning_serde_preserves_legacy_wire_and_accepts_compatible_additions() {
        use serde_json::{Value, json};
        let request: GridModelCrsRequest = serde_json::from_value(json!({
            "operation":"from_map", "crs":RWANDA_TM_CRS, "point":[30.0,0.0],
            "v":2, "contract":7, "future_field":{"retained":true}
        }))
        .unwrap();
        let output = evaluate(request).unwrap();
        let wire = serde_json::to_value(&output).unwrap();
        assert_eq!(
            wire,
            json!({"crs":RWANDA_TM_CRS,"point":[500000.0,5000000.0]})
        );
        let mut additive = wire;
        additive["contract"] = json!(19);
        let decoded: GridModelCrsResponse = serde_json::from_value(additive).unwrap();
        assert_eq!(
            decoded, output,
            "point must survive untagged response decoding"
        );
        let unsupported = evaluate(GridModelCrsRequest::Supports {
            crs: "EPSG:4326".into(),
        })
        .unwrap();
        assert_eq!(
            serde_json::to_value(unsupported).unwrap(),
            json!({"supported":false})
        );
        for malformed in [
            Value::Null,
            json!({"operation":"invented","crs":"EPSG:3857"}),
            json!({"operation":"supports","crs":2}),
            json!({"operation":"from_map","crs":"EPSG:3857"}),
            json!({"operation":"to_map","crs":"EPSG:3857","point":[0.0]}),
            json!({"operation":"to_map","crs":"EPSG:3857","point":[0.0,"bad"]}),
        ] {
            assert!(
                serde_json::from_value::<GridModelCrsRequest>(malformed.clone()).is_err(),
                "accepted {malformed}"
            );
        }
    }

    #[test]
    fn grid_model_crs_parser_is_the_exact_projected_support_authority() {
        assert_eq!(
            GridModelCrs::parse(RWANDA_TM_CRS),
            Ok(GridModelCrs::RwandaTm)
        );
        assert_eq!(
            GridModelCrs::parse("epsg:32601"),
            Ok(GridModelCrs::Utm {
                zone: 1,
                southern: false
            })
        );
        assert_eq!(
            GridModelCrs::parse("EPSG:32760"),
            Ok(GridModelCrs::Utm {
                zone: 60,
                southern: true
            })
        );
        assert_eq!(
            GridModelCrs::parse("EPSG:3395"),
            Ok(GridModelCrs::WorldMercator)
        );
        assert_eq!(
            GridModelCrs::parse("EPSG:3857"),
            Ok(GridModelCrs::WebMercator)
        );
        for refused in [
            "",
            "EPSG:0",
            "EPSG:32600",
            "EPSG:32761",
            "EPSG:4083",
            "EPSG:4326",
        ] {
            assert!(GridModelCrs::parse(refused).is_err(), "accepted {refused}");
        }
    }

    #[test]
    fn characterized_model_crs_transforms_round_trip_known_points() {
        let kigali = [30.0619, -1.9441];
        let rwanda = GridModelCrs::RwandaTm;
        assert_eq!(
            rwanda.from_map([30.0, 0.0]).unwrap(),
            [500_000.0, 5_000_000.0]
        );

        for crs in [
            rwanda,
            GridModelCrs::Utm {
                zone: 36,
                southern: true,
            },
            GridModelCrs::WorldMercator,
            GridModelCrs::WebMercator,
        ] {
            let restored = crs.to_map(crs.from_map(kigali).unwrap()).unwrap();
            assert!((restored[0] - kigali[0]).abs() < 1e-7, "{crs:?}");
            assert!((restored[1] - kigali[1]).abs() < 1e-7, "{crs:?}");
        }
    }

    #[test]
    fn world_and_web_mercator_are_not_conflated() {
        let point = [30.0619, -1.9441];
        let world = GridModelCrs::WorldMercator.from_map(point).unwrap();
        let web = GridModelCrs::WebMercator.from_map(point).unwrap();
        assert_eq!(world[0], web[0]);
        assert!((world[1] - web[1]).abs() > 1_000.0);
    }

    #[test]
    fn model_crs_transform_refuses_non_finite_poles_and_utm_outside_domain() {
        assert!(GridModelCrs::WorldMercator.from_map([0.0, 90.0]).is_err());
        assert!(GridModelCrs::WebMercator.to_map([0.0, f64::NAN]).is_err());
        assert!(
            GridModelCrs::Utm {
                zone: 36,
                southern: false
            }
            .from_map([33.0, 85.0])
            .is_err()
        );
    }
}
