//! Public city context: bounded source selection and GeoJSON projection.
//! No HTTP, credential, filesystem, or engineering design mutation.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MICROSOFT_INDEX: &str =
    "https://bfppub.blob.core.windows.net/%24web/2026-08-13/dataset-links.csv";
pub const OSM_ENDPOINT: &str = "https://overpass-api.de/api/interpreter";
pub const INDEX_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const SOURCE_MAX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_FEATURES: usize = 200_000;
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}
impl Bounds {
    pub fn validate(&self) -> Result<(), String> {
        if [self.west, self.south, self.east, self.north]
            .iter()
            .any(|v| !v.is_finite())
            || self.west < -180.
            || self.east > 180.
            || self.south < -85.
            || self.north > 85.
            || self.west >= self.east
            || self.south >= self.north
            || self.east - self.west > 2.
            || self.north - self.south > 2.
        {
            return Err(
                "city bounds must be an ordered WGS84 box no wider/taller than two degrees".into(),
            );
        }
        Ok(())
    }
    pub fn quadkeys(&self) -> Result<BTreeSet<String>, String> {
        self.validate()?;
        let tile = |lon: f64, lat: f64| -> (u32, u32) {
            let x = ((lon + 180.) / 360. * 512.).floor().clamp(0., 511.) as u32;
            let r = lat.to_radians();
            let y = ((1. - (r.tan() + 1. / r.cos()).ln() / std::f64::consts::PI) / 2. * 512.)
                .floor()
                .clamp(0., 511.) as u32;
            (x, y)
        };
        let (x0, y1) = tile(self.west, self.south);
        let (x1, y0) = tile(self.east, self.north);
        let mut keys = BTreeSet::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                let key = (0..9)
                    .rev()
                    .map(|i| char::from(b'0' + ((x >> i) & 1) as u8 + 2 * ((y >> i) & 1) as u8))
                    .collect();
                keys.insert(key);
            }
        }
        if keys.len() > 16 {
            return Err("city covers more than sixteen Microsoft tiles".into());
        }
        Ok(keys)
    }
    pub fn intersects(&self, geometry: &Value) -> bool {
        fn points(v: &Value, extent: &mut [f64; 4]) {
            if let Some(a) = v.as_array() {
                if a.len() >= 2 && a[0].is_number() && a[1].is_number() {
                    if let (Some(x), Some(y)) = (a[0].as_f64(), a[1].as_f64()) {
                        extent[0] = extent[0].min(x);
                        extent[1] = extent[1].min(y);
                        extent[2] = extent[2].max(x);
                        extent[3] = extent[3].max(y);
                    }
                } else {
                    for c in a {
                        points(c, extent)
                    }
                }
            }
        }
        let mut e = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        points(&geometry["coordinates"], &mut e);
        e[0] <= self.east && e[2] >= self.west && e[1] <= self.north && e[3] >= self.south
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrosoftTile {
    pub quadkey: String,
    pub country: String,
    pub url: String,
    pub published_size: String,
}
fn country(s: &str) -> String {
    match s.trim().to_lowercase().as_str() {
        "tchad" => "chad".into(),
        other => other.to_owned(),
    }
}
pub fn microsoft_tiles(
    index: &[u8],
    bounds: Bounds,
    region: &str,
) -> Result<Vec<MicrosoftTile>, String> {
    if index.len() > INDEX_MAX_BYTES {
        return Err("Microsoft index exceeds bound".into());
    }
    let keys = bounds.quadkeys()?;
    let mut csv = csv::Reader::from_reader(index);
    let headers = csv.headers().map_err(|e| e.to_string())?.clone();
    let col = |name: &str| {
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("Microsoft index lacks {name}"))
    };
    let (q, l, u) = (col("QuadKey")?, col("Location")?, col("Url")?);
    let size = col("Size").ok();
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for r in csv.records() {
        let r = r.map_err(|e| e.to_string())?;
        let raw = r.get(q).unwrap_or("");
        let key = format!("{raw:0>9}");
        if country(r.get(l).unwrap_or("")) != country(region) || !keys.contains(&key) {
            continue;
        }
        let url = r.get(u).unwrap_or("");
        let parsed = url::Url::parse(url).map_err(|_| "Microsoft tile URL invalid")?;
        if parsed.scheme() != "https"
            || !matches!(
                parsed.host_str(),
                Some("bfppub.blob.core.windows.net" | "bfppub.z5.web.core.windows.net")
            )
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
            || !parsed.path().ends_with(".csv.gz")
        {
            return Err(format!("Microsoft tile is outside the published source: scheme={}, host={}, path={}, query={}", parsed.scheme(), parsed.host_str().unwrap_or(""), parsed.path(), parsed.query().is_some()));
        }
        if !seen.insert(key.clone()) {
            return Err("duplicate Microsoft tile in selected region".into());
        }
        rows.push(MicrosoftTile {
            quadkey: key,
            country: r.get(l).unwrap_or("").into(),
            url: url.into(),
            published_size: size.and_then(|i| r.get(i)).unwrap_or("unknown").into(),
        });
    }
    if rows.is_empty() {
        return Err("Microsoft publishes no selected tile for this country/bounds".into());
    }
    rows.sort_by(|a, b| a.quadkey.cmp(&b.quadkey));
    Ok(rows)
}
pub fn osm_query(b: Bounds) -> Result<String, String> {
    b.validate()?;
    Ok(format!("[out:json][timeout:90][maxsize:67108864][bbox:{},{},{},{}];(way[highway];way[building];way[waterway];way[natural=water];way[landuse];node[place];);out geom;",b.south,b.west,b.north,b.east))
}
pub fn osm_layers(raw: &Value, b: Bounds) -> Result<BTreeMap<String, Value>, String> {
    b.validate()?;
    if raw.get("remark").is_some() {
        return Err("OSM provider reported a partial/failed query".into());
    }
    let rows = raw["elements"]
        .as_array()
        .ok_or("OSM response lacks elements")?;
    if rows.len() > MAX_FEATURES {
        return Err("OSM feature count exceeds bound".into());
    }
    let mut layers: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in rows {
        let tags = &r["tags"];
        let kind = r["type"].as_str().unwrap_or("");
        let layer = if kind == "node" && tags.get("place").is_some() {
            "osm_places"
        } else if tags.get("highway").is_some() {
            "osm_roads"
        } else if tags.get("building").is_some() {
            "osm_buildings"
        } else if tags.get("waterway").is_some() || tags["natural"] == "water" {
            "osm_water"
        } else if tags.get("landuse").is_some() {
            "osm_landuse"
        } else {
            continue;
        };
        let geometry = if kind == "node" {
            json!({"type":"Point","coordinates":[r["lon"],r["lat"]]})
        } else {
            let coords = r["geometry"]
                .as_array()
                .ok_or("OSM way lacks full geometry")?
                .iter()
                .map(|p| {
                    let (x, y) = (
                        p["lon"].as_f64().ok_or("OSM longitude missing")?,
                        p["lat"].as_f64().ok_or("OSM latitude missing")?,
                    );
                    Ok(json!([x, y]))
                })
                .collect::<Result<Vec<_>, String>>()?;
            if coords.len() < 2 {
                return Err("OSM way has insufficient geometry".into());
            }
            let area =
                matches!(layer, "osm_buildings" | "osm_landuse") || tags["natural"] == "water";
            if area && coords.len() >= 4 && coords.first() == coords.last() {
                json!({"type":"Polygon","coordinates":[coords]})
            } else {
                json!({"type":"LineString","coordinates":coords})
            }
        };
        if !b.intersects(&geometry) {
            continue;
        }
        let mut properties = tags.as_object().cloned().unwrap_or_default();
        properties.insert("source".into(), json!("OpenStreetMap"));
        properties.insert("osm_id".into(), r["id"].clone());
        layers.entry(layer.into()).or_default().push(json!({"type":"Feature","id":format!("osm:{kind}:{}",r["id"]),"geometry":geometry,"properties":properties}));
    }
    Ok(layers
        .into_iter()
        .map(|(name, features)| {
            (
                name,
                json!({"type":"FeatureCollection","features":features}),
            )
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn b() -> Bounds {
        Bounds {
            west: 15.36,
            south: 8.66,
            east: 15.40,
            north: 8.70,
        }
    }
    #[test]
    fn microsoft_selection_is_spatial_country_bound_and_refuses_external_hosts() {
        let key = b().quadkeys().unwrap().into_iter().next().unwrap();
        let csv=format!("QuadKey,Location,Url,Size\n{key},Chad,https://bfppub.blob.core.windows.net/a.csv.gz,1 MB\n{key},Libya,https://bfppub.blob.core.windows.net/b.csv.gz,2 MB\n");
        assert_eq!(
            microsoft_tiles(csv.as_bytes(), b(), "Tchad").unwrap().len(),
            1
        );
        assert!(microsoft_tiles(
            csv.replace("bfppub.blob.core.windows.net", "localhost")
                .as_bytes(),
            b(),
            "Chad"
        )
        .is_err());
    }
    #[test]
    fn rejects_unbounded_or_inverted_boxes() {
        assert!(Bounds { east: 20., ..b() }.validate().is_err());
        assert!(Bounds { south: 9., ..b() }.validate().is_err());
        assert_eq!(b().quadkeys().unwrap().iter().next().unwrap().len(), 9);
    }
    #[test]
    fn osm_retains_polygons_and_refuses_partial_results() {
        let r = json!({"elements":[{"type":"way","id":1,"tags":{"building":"yes"},"geometry":[{"lon":15.37,"lat":8.67},{"lon":15.38,"lat":8.67},{"lon":15.38,"lat":8.68},{"lon":15.37,"lat":8.67}]}]});
        assert_eq!(
            osm_layers(&r, b()).unwrap()["osm_buildings"]["features"][0]["geometry"]["type"],
            "Polygon"
        );
        assert!(osm_layers(&json!({"elements":[],"remark":"timeout"}), b()).is_err());
    }
}
