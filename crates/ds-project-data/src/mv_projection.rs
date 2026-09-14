//! Pure geographic projection of a held, verified model for printing.
//! No acquisition, credentials, model mutation or storage is performed here.
use ds_geo::projection::GridModelCrs;
use serde_json::{json, Value};

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
            Some("alignments" | "structures" | "angle_points")
        ) && !f["geometry"].is_null()
    });
    for feature in features {
        reproject(&mut feature["geometry"]["coordinates"], crs)?;
    }
    Ok(projection)
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
}
