//! Pure geographic projection of a held, verified model for printing.
//! No acquisition, credentials, model mutation or storage is performed here.
use ds_command_kernel::project_dataset_cache::Extent;
use ds_geo::projection::GridModelCrs;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

/// One exact local draft projected once for both map capture and geographic
/// acquisition. The caller supplies and authorizes the DS project separately.
pub struct LocalMvProjection {
    pub model_id: String,
    pub revision_id: String,
    pub sha256: String,
    pub projection: Value,
    pub route_extents: Vec<Extent>,
}

pub fn load_local(path: &Path) -> Result<LocalMvProjection, String> {
    if !path.is_absolute() || path.extension().and_then(|v| v.to_str()) != Some("dsgrid") {
        return Err("local MV model must be an absolute .dsgrid file".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 256_000_000 {
        return Err("local MV model must be a regular file of at most 256 MB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let model_id = format!("local-{}", &sha256[..16]);
    let revision_id = format!("local:sha256:{sha256}");
    let projection = project(&bytes, &revision_id)?;
    let route_extents = route_extents_in(&projection, &model_id)?;
    Ok(LocalMvProjection {
        model_id,
        revision_id,
        sha256,
        projection,
        route_extents,
    })
}

pub fn project(bytes: &[u8], revision: &str) -> Result<Value, String> {
    let package = ds_grid_exchange::package::unpack(bytes).map_err(|e| e.to_string())?;
    let declared = package.manifest.model.coordinate_system.as_str();
    let crs = GridModelCrs::parse(declared).map_err(|e| e.to_string())?;
    let mut options = ds_grid_exchange::gis::GisProjectionOptions::new(declared);
    options.model_revision = Some(revision.to_string());
    let projected = ds_grid_exchange::gis::project_model(&package.snapshot, &options);
    let mut projection = projected.presentation_geojson();
    let features = projection["features"]
        .as_array_mut()
        .ok_or_else(|| "Model projection has no features".to_string())?;
    features.retain(|f| {
        matches!(
            f["properties"]["_layer"].as_str(),
            Some("alignments" | "structures" | "angle_points" | "prohibited_zones")
        ) && !f["geometry"].is_null()
    });
    for feature in features {
        reproject(&mut feature["geometry"]["coordinates"], crs)?;
    }
    Ok(projection)
}

/// Small route-segment footprints for project data acquisition. One long MV
/// line must not turn into a district-sized rectangular provider request.
pub fn route_extents(bytes: &[u8], revision: &str, model_id: &str) -> Result<Vec<Extent>, String> {
    let projection = project(bytes, revision)?;
    route_extents_in(&projection, model_id)
}

fn route_extents_in(projection: &Value, model_id: &str) -> Result<Vec<Extent>, String> {
    let features = projection["features"]
        .as_array()
        .ok_or("MV projection has no features")?;
    let mut extents = Vec::new();
    for (feature_index, feature) in features.iter().enumerate() {
        if feature["properties"]["_layer"] != "alignments" {
            continue;
        }
        let lines = match feature["geometry"]["type"].as_str() {
            Some("LineString") => vec![&feature["geometry"]["coordinates"]],
            Some("MultiLineString") => feature["geometry"]["coordinates"]
                .as_array()
                .ok_or("MV route has no coordinate array")?
                .iter()
                .collect(),
            _ => continue,
        };
        for (line_index, line) in lines.into_iter().enumerate() {
            let points = line.as_array().ok_or("MV route has invalid coordinates")?;
            for (segment_index, segment) in points.windows(2).enumerate() {
                let point = |value: &Value| -> Result<[f64; 2], String> {
                    let coords = value.as_array().ok_or("MV route point is not an array")?;
                    let lon = coords
                        .first()
                        .and_then(Value::as_f64)
                        .ok_or("MV longitude is invalid")?;
                    let lat = coords
                        .get(1)
                        .and_then(Value::as_f64)
                        .ok_or("MV latitude is invalid")?;
                    if !lon.is_finite()
                        || !lat.is_finite()
                        || !(-180.0..=180.0).contains(&lon)
                        || !(-85.0..=85.0).contains(&lat)
                    {
                        return Err("MV route coordinate is outside geographic bounds".into());
                    }
                    Ok([lon, lat])
                };
                let a = point(&segment[0])?;
                let b = point(&segment[1])?;
                if a == b {
                    continue;
                }
                extents.push(Extent {
                    name: format!("mv:{model_id}:{feature_index}:{line_index}:{segment_index}"),
                    bounds: Some([
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[0].max(b[0]),
                        a[1].max(b[1]),
                    ]),
                });
                if extents.len() > ds_command_kernel::project_dataset_cache::MAX_EXTENTS {
                    return Err("MV route has more than 5,000 acquisition segments; split the model into governed heads".into());
                }
            }
        }
    }
    Ok(extents)
}

fn reproject(value: &mut Value, crs: GridModelCrs) -> Result<(), String> {
    let coords = value
        .as_array_mut()
        .ok_or_else(|| "MV geometry has no coordinate array".to_string())?;
    if coords.first().is_some_and(Value::is_number) {
        if coords.len() < 2 {
            return Err("MV geometry contains an incomplete coordinate".to_string());
        }
        let xy = crs
            .to_map([
                coords[0]
                    .as_f64()
                    .ok_or_else(|| "Invalid MV easting".to_string())?,
                coords[1]
                    .as_f64()
                    .ok_or_else(|| "Invalid MV northing".to_string())?,
            ])
            .map_err(|e| e.to_string())?;
        coords[0] = json!(xy[0]);
        coords[1] = json!(xy[1]);
    } else {
        for coord in coords {
            reproject(coord, crs)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metric_coordinates_preserve_elevation() {
        let mut coordinates = json!([[[500000., 10000000., 1200.], [500100., 10000000., 1250.]]]);
        reproject(&mut coordinates, GridModelCrs::parse("EPSG:32735").unwrap()).unwrap();
        assert!((coordinates[0][0][0].as_f64().unwrap() - 27.).abs() < 1e-6);
        assert!(coordinates[0][0][1].as_f64().unwrap().abs() < 1e-6);
        assert_eq!(coordinates[0][0][2], 1200.);
        assert!(coordinates[0][1][0].as_f64().unwrap() > 27.);
        assert_eq!(coordinates[0][1][2], 1250.);
        assert!(reproject(&mut json!([1]), GridModelCrs::parse("EPSG:32735").unwrap()).is_err());
    }

    #[test]
    fn mv_route_coverage_uses_segments_instead_of_one_district_envelope() {
        let projection = json!({"features":[
            {"properties":{"_layer":"alignments"},"geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-2.0],[30.001,-2.001]]}},
            {"properties":{"_layer":"structures"},"geometry":{"type":"Point","coordinates":[30.0,-2.0]}}
        ]});
        let extents = route_extents_in(&projection, "model-1").unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].bounds, Some([30.0, -2.0, 30.001, -2.0]));
        assert_eq!(extents[1].bounds, Some([30.001, -2.001, 30.001, -2.0]));
        assert!(extents
            .iter()
            .all(|extent| extent.name.starts_with("mv:model-1:")));
    }
}
