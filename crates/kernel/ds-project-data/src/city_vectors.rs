//! Local public vector acquisition. Hosts provide bounded public byte transport;
//! the kernel owns tile selection, coverage and feature projection.
use ds_command_kernel::city_vectors::{self as policy, Bounds, MicrosoftTile};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read},
    path::Path,
};

pub trait Provider {
    fn microsoft_index(&mut self) -> Result<Vec<u8>, String>;
    fn microsoft_tile(&mut self, tile: &MicrosoftTile) -> Result<Vec<u8>, String>;
    fn osm(&mut self, query: &str) -> Result<Vec<u8>, String>;
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn acquire(
    provider: &mut dyn Provider,
    bounds: Bounds,
    country: &str,
    source: &str,
    out: &Path,
    write: bool,
) -> Result<Value, String> {
    bounds.validate()?;
    if !matches!(source, "both" | "microsoft" | "osm") {
        return Err("source must be both, microsoft or osm".into());
    }
    if country.trim().is_empty() || country.len() > 128 {
        return Err("one bounded country name is required".into());
    }
    if write && std::fs::symlink_metadata(out).is_ok() {
        return Err("output directory already exists; choose a fresh path".into());
    }
    let mut tiles = Vec::new();
    let mut index_digest = Value::Null;
    if source != "osm" {
        let index = provider.microsoft_index()?;
        index_digest = json!(sha(&index));
        tiles = policy::microsoft_tiles(&index, bounds, country)?;
    }
    let mut receipt = json!({"schema":"ds.city-vectors/v1","bounds":bounds,"country":country,"source":source,
        "microsoft":{"index":policy::MICROSOFT_INDEX,"index_sha256":index_digest,"tiles":tiles,"license":"CDLA-Permissive-2.0","attribution":"Microsoft Global ML Building Footprints"},
        "osm":{"provider":"OpenStreetMap via Overpass","license":"ODbL-1.0","attribution":"© OpenStreetMap contributors","scope":"roads, building ways, waterways, water areas, landuse ways and place nodes; relations are not requested"},
        "provider_fee":0,"cost_unit":"provider download fee; bandwidth/storage are local","per_source_download_limit_bytes":policy::SOURCE_MAX_BYTES,"out":out,"written":false});
    if !write {
        return Ok(receipt);
    }
    let mut layers: BTreeMap<String, Value> = BTreeMap::new();
    let mut sources = Vec::new();
    if source != "osm" {
        let mut features = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for tile in &tiles {
            let bytes = provider.microsoft_tile(tile)?;
            if bytes.len() > policy::SOURCE_MAX_BYTES {
                return Err("Microsoft compressed tile exceeds bound".into());
            }
            sources.push(json!({"source":"microsoft","quadkey":tile.quadkey,"sha256":sha(&bytes),"bytes":bytes.len()}));
            let decoder = flate2::read::GzDecoder::new(bytes.as_slice());
            let mut reader = std::io::BufReader::new(decoder.take(512 * 1024 * 1024 + 1));
            let mut total = 0;
            let mut line = String::new();
            loop {
                line.clear();
                let read = reader
                    .by_ref()
                    .take(2 * 1024 * 1024 + 1)
                    .read_line(&mut line)
                    .map_err(|_| "Microsoft gzip/GeoJSON stream unreadable")?;
                if read == 0 {
                    break;
                }
                total += read;
                if read > 2 * 1024 * 1024 || total > 512 * 1024 * 1024 {
                    return Err("Microsoft expanded data exceeds bound".into());
                }
                let mut f: Value = serde_json::from_str(&line)
                    .map_err(|_| "Microsoft source is not GeoJSON lines")?;
                if f["type"] != "Feature"
                    || !matches!(
                        f["geometry"]["type"].as_str(),
                        Some("Polygon" | "MultiPolygon")
                    )
                {
                    return Err("Microsoft footprint is not a polygon feature".into());
                }
                if !bounds.intersects(&f["geometry"]) {
                    continue;
                }
                let id = sha(&serde_json::to_vec(&f["geometry"]).map_err(|e| e.to_string())?);
                if !seen.insert(id.clone()) {
                    continue;
                }
                if !f["properties"].is_object() {
                    f["properties"] = json!({})
                }
                f["properties"]["source"] = json!("Microsoft Global ML Building Footprints");
                f["id"] = json!(format!("microsoft:{id}"));
                features.push(f);
                if features.len() > policy::MAX_FEATURES {
                    return Err("city building count exceeds bound".into());
                }
            }
        }
        layers.insert(
            "microsoft_buildings".into(),
            json!({"type":"FeatureCollection","features":features}),
        );
    }
    if source != "microsoft" {
        let raw = provider.osm(&policy::osm_query(bounds)?)?;
        if raw.len() > policy::SOURCE_MAX_BYTES {
            return Err("OSM response exceeds bound".into());
        }
        sources.push(json!({"source":"osm","sha256":sha(&raw),"bytes":raw.len()}));
        let parsed = serde_json::from_slice(&raw).map_err(|_| "OSM response is not JSON")?;
        layers.extend(policy::osm_layers(&parsed, bounds)?);
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix(".city-vectors-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    let mut artifacts = Vec::new();
    for (name, fc) in layers {
        let bytes = serde_json::to_vec(&fc).map_err(|e| e.to_string())?;
        let filename = format!("{name}.geojson");
        std::fs::write(stage.path().join(&filename), &bytes).map_err(|e| e.to_string())?;
        artifacts.push(json!({"layer":name,"file":filename,"features":fc["features"].as_array().map_or(0,Vec::len),"bytes":bytes.len(),"sha256":sha(&bytes)}));
    }
    receipt["written"] = json!(true);
    receipt["artifacts"] = json!(artifacts);
    receipt["source_receipts"] = json!(sources);
    std::fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    // Reserve the absent destination atomically and never replace a file.
    std::fs::create_dir(out).map_err(|e| e.to_string())?;
    for artifact in receipt["artifacts"]
        .as_array()
        .ok_or("artifact inventory missing")?
    {
        let file = artifact["file"].as_str().ok_or("artifact name missing")?;
        std::fs::hard_link(stage.path().join(file), out.join(file)).map_err(|e| e.to_string())?;
    }
    // Receipt appears last, so an interrupted installation cannot claim ready.
    std::fs::hard_link(stage.path().join("receipt.json"), out.join("receipt.json"))
        .map_err(|e| e.to_string())?;
    Ok(receipt)
}

/// Verify a city acquisition receipt and materialize its held map context.
/// The caller chooses this local presentation context explicitly; it does not
/// replace any saved engineering layer or claim project-owned survey provenance.
pub fn print_context(dir: &Path) -> Result<crate::print_context::PrintContext, String> {
    fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
        let mut data = Vec::new();
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("city context source is not a file".into());
        }
        file.take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > limit {
            return Err("city context file exceeds its bound".into());
        }
        Ok(data)
    }
    let receipt: Value = serde_json::from_slice(&read(&dir.join("receipt.json"), 1024 * 1024)?)
        .map_err(|e| e.to_string())?;
    if receipt["schema"] != "ds.city-vectors/v1" || receipt["written"] != true {
        return Err("city context requires a completed acquisition receipt".into());
    }
    let b: Bounds = serde_json::from_value(receipt["bounds"].clone()).map_err(|e| e.to_string())?;
    b.validate()?;
    let artifacts = receipt["artifacts"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 16)
        .ok_or("city context has no bounded artifact inventory")?;
    let mut layers = BTreeMap::new();
    let mut sources = Vec::new();
    for artifact in artifacts {
        let id = artifact["layer"]
            .as_str()
            .ok_or("city context artifact has no layer")?;
        if !matches!(
            id,
            "microsoft_buildings"
                | "osm_buildings"
                | "osm_roads"
                | "osm_water"
                | "osm_waterways"
                | "osm_landuse"
                | "osm_places"
        ) {
            return Err("unknown city context layer".into());
        }
        let name = format!("{id}.geojson");
        if artifact["file"] != name {
            return Err("city context filename is not its owned layer name".into());
        }
        let bytes = read(&dir.join(&name), 32 * 1024 * 1024)?;
        if artifact["bytes"].as_u64() != Some(bytes.len() as u64)
            || artifact["sha256"] != sha(&bytes)
        {
            return Err(format!("city context {id} bytes changed after acquisition"));
        }
        let fc: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let features = fc["features"]
            .as_array()
            .ok_or("city context has no feature collection")?;
        if artifact["features"].as_u64() != Some(features.len() as u64) {
            return Err("city context feature count mismatch".into());
        }
        if features.is_empty() {
            continue;
        }
        if layers.insert(id.to_owned(), fc).is_some() {
            return Err("city context repeats a layer".into());
        }
        sources.push(json!({"layer":id,"kind":"public_city_vectors","sha256":artifact["sha256"],"attribution":if id=="microsoft_buildings"{"Microsoft Global ML Building Footprints (CDLA-Permissive-2.0)"}else{"© OpenStreetMap contributors (ODbL-1.0)"}}));
    }
    let coverage = json!({"type":"Polygon","coordinates":[[[b.west,b.south],[b.east,b.south],[b.east,b.north],[b.west,b.north],[b.west,b.south]]]});
    let document = ds_command_kernel::report_export::print_context_document(
        &coverage,
        &json!(sources),
        &layers,
    )?;
    Ok(crate::print_context::PrintContext {
        sha256: Some(sha(&document)),
        document: Some(document),
        layers: layers.into_keys().collect(),
        omitted: Vec::new(),
        warnings: Vec::new(),
        reused: true,
    })
}

/// Refuse a different city's basemap instead of printing a plausible empty context.
/// Coverage is checked against the design owner's geographic extent, not names.
pub fn require_design_coverage(
    context: &crate::print_context::PrintContext,
    name: &str,
    layers: &Value,
) -> Result<(), String> {
    let document: Value = serde_json::from_slice(
        context
            .document
            .as_deref()
            .ok_or("city context has no document")?,
    )
    .map_err(|e| e.to_string())?;
    let extent = crate::extents::extent_of(name, layers)
        .map_err(|e| e.to_string())?
        .bounds
        .ok_or("design has no geographic extent")?;
    let ring = document["coverage"]["coordinates"][0]
        .as_array()
        .filter(|ring| ring.len() == 5)
        .ok_or("city context has no rectangular coverage")?;
    let bounds = [
        ring[0][0].as_f64(),
        ring[0][1].as_f64(),
        ring[2][0].as_f64(),
        ring[2][1].as_f64(),
    ];
    let [Some(w), Some(s), Some(e), Some(n)] = bounds else {
        return Err("city context coverage is invalid".into());
    };
    if extent[0] < w || extent[1] < s || extent[2] > e || extent[3] > n {
        return Err(format!("city vectors do not cover the complete design of {name}; acquire vectors for its full geographic extent"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture;
    impl Provider for Fixture {
        fn microsoft_index(&mut self) -> Result<Vec<u8>, String> {
            Err("not requested".into())
        }
        fn microsoft_tile(&mut self, _: &MicrosoftTile) -> Result<Vec<u8>, String> {
            Err("not requested".into())
        }
        fn osm(&mut self, _: &str) -> Result<Vec<u8>, String> {
            Ok(serde_json::to_vec(&json!({"elements":[{"type":"way","id":1,"tags":{"highway":"residential"},"geometry":[{"lon":15.0,"lat":8.0},{"lon":15.01,"lat":8.01}]}]})).unwrap())
        }
    }
    #[test]
    fn fresh_parent_context_readback_and_tamper_refusal() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("fresh/city");
        let b = Bounds {
            west: 14.9,
            south: 7.9,
            east: 15.1,
            north: 8.1,
        };
        let r = acquire(&mut Fixture, b, "Chad", "osm", &out, true).unwrap();
        assert_eq!(r["written"], true);
        let held = print_context(&out).unwrap();
        assert_eq!(held.layers, vec!["osm_roads"]);
        let room = |x| json!({"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[x,8.0],[x+0.01,8.01]]}}]}});
        require_design_coverage(&held, "inside", &room(15.0)).unwrap();
        assert!(require_design_coverage(&held, "outside", &room(16.0))
            .unwrap_err()
            .contains("complete design"));
        assert!(acquire(&mut Fixture, b, "Chad", "osm", &out, true).is_err());
        std::fs::write(out.join("osm_roads.geojson"), b"{}").unwrap();
        assert!(print_context(&out).unwrap_err().contains("changed"));
    }
}
