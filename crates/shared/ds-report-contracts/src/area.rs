//! The area a print or a print context covers, admitted once for every host.

use serde_json::Value;

/// Admit an area a request names: a GeoJSON Polygon or MultiPolygon (bare or
/// as a Feature's geometry) that DS admits as a geometry — valid where BigQuery
/// would refuse it, and not an inverted or full-globe polygon. The refusal
/// names the rule and the ring (`ds_geo_lite::admission`).
pub fn validate_area(area: &Value) -> Result<(), String> {
    ds_geo_lite::spatial_predicate::validate_area(area)?;
    let geometry = if area["type"] == "Feature" {
        &area["geometry"]
    } else {
        area
    };
    ds_geo_lite::admission::admit_geometry(geometry).map_err(|refusal| refusal.to_string())
}
