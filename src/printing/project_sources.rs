//! Bounded overview projection. Source geometry and engineering identities stay intact.
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Sources {
    layers: BTreeMap<String, Vec<Value>>,
    count: usize,
}
impl Sources {
    /// LV overview deliberately excludes schedules, customers and individual poles.
    pub fn transformer(&mut self, name: &str, layers: &Value) -> Result<(), String> {
        for id in ["tr", "lv_lines"] {
            if let Some(features) = layers[id]["features"].as_array() {
                let mut collection = Vec::with_capacity(features.len());
                for feature in features {
                    let mut feature = feature.clone();
                    let props = feature["properties"]
                        .as_object_mut()
                        .ok_or("source properties missing")?;
                    props.insert("print_transformer".into(), json!(name));
                    // Stable owner prefix prevents different rooms' local IDs colliding.
                    if !feature["id"].is_null() {
                        feature["id"] = json!(format!("{name}:{}", feature["id"]));
                    }
                    collection.push(feature);
                }
                self.add(id, collection)?;
            }
        }
        Ok(())
    }
    pub fn collections(&mut self, layers: &Value) -> Result<(), String> {
        for (id, layer) in layers.as_object().ok_or("map layers must be an object")? {
            self.add(
                id,
                layer["features"]
                    .as_array()
                    .ok_or("map layer must contain features")?
                    .clone(),
            )?;
        }
        Ok(())
    }
    pub fn mv(&mut self, layers: &Value) -> Result<(), String> {
        for id in super::map::MV_PRINT_LAYER_IDS {
            self.add(
                id,
                layers[id]["features"]
                    .as_array()
                    .ok_or("MV overview has no features")?
                    .clone(),
            )?;
        }
        Ok(())
    }
    fn add(&mut self, id: &str, features: Vec<Value>) -> Result<(), String> {
        self.count += features.len();
        if self.count > 200_000 || self.layers.len() >= 100 {
            return Err("overview exceeds 200,000 features or 100 layers".into());
        }
        self.layers.entry(id.into()).or_default().extend(features);
        Ok(())
    }
    pub fn layers(&self) -> Value {
        self.layers
            .iter()
            .map(|(id, features)| {
                (
                    id.clone(),
                    json!({"type":"FeatureCollection","features":features}),
                )
            })
            .collect()
    }
    pub fn vectors(&self) -> Vec<Value> {
        self.layers.iter().map(|(id, features)| json!({"id":id,"label":id,"geojson":{"type":"FeatureCollection","features":features}})).collect()
    }
    /// A district overview fits its complete authority polygon as well as
    /// the project network; national routes outside that scope do not widen it.
    pub fn overview_extent(&self) -> Result<[f64; 4], String> {
        let layers: Value = self
            .layers()
            .as_object()
            .ok_or("overview layers missing")?
            .iter()
            .filter(|(id, _)| {
                matches!(
                    id.as_str(),
                    "tr" | "lv_lines" | "dsgrid_mv_lines" | "district_boundaries"
                )
            })
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        crate::project_design_extent::design_extent("mv_data", &layers, 0.00001)?
            .bounds
            .ok_or_else(|| "overview has no geographic geometry".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mv_overview_preserves_routes_structures_and_angle_points() {
        let feature = json!({"type":"Feature","id":"S-7","properties":{"source_model_id":"model","source_revision":3},"geometry":{"type":"Point","coordinates":[29.7,-2.5]}});
        let layers: Value = super::super::map::MV_PRINT_LAYER_IDS
            .into_iter()
            .map(|id| (id.to_owned(), json!({"features":[feature.clone()]})))
            .collect();
        let mut sources = Sources::default();
        sources.mv(&layers).unwrap();
        for id in super::super::map::MV_PRINT_LAYER_IDS {
            assert_eq!(sources.layers()[id]["features"][0], feature);
        }
    }
    #[test]
    fn overview_includes_complete_district_but_not_unrelated_national_line_tails() {
        let mut sources = Sources::default();
        sources.collections(&json!({
            "tr":{"features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30.,-2.]}}]},
            "district_boundaries":{"features":[{"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]}}]},
            "mv_line":{"features":[{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[40.,10.]]}}]}
        })).unwrap();
        assert_eq!(sources.overview_extent().unwrap(), [29., -3., 31., -1.]);
    }
    #[test]
    fn overview_preserves_geometry_and_namespaces_room_ids_without_detail_clutter() {
        let feature = json!({"id":1,"type":"Feature","properties":{"name":"T1"},"geometry":{"type":"Point","coordinates":[30.,-2.]}});
        let source =
            json!({"tr":{"features":[feature.clone()]},"customers":{"features":[feature.clone()]}});
        let mut out = Sources::default();
        out.transformer("first", &source).unwrap();
        out.transformer("second", &source).unwrap();
        let layers = out.layers();
        assert!(layers.get("customers").is_none());
        assert_eq!(layers["tr"]["features"][0]["geometry"], feature["geometry"]);
        assert_ne!(
            layers["tr"]["features"][0]["id"],
            layers["tr"]["features"][1]["id"]
        );
        assert_eq!(source["tr"]["features"][0], feature);
    }
}
