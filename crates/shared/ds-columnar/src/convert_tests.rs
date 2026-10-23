use super::*;
use serde_json::json;

const CSV: &[u8] = b"lon,lat,name\n30.06,-1.94,Kigali\n29.74,-2.60,Huye\n";

fn point(name: &str, x: f64, y: f64) -> Value {
    json!({"type":"Feature","geometry":{"type":"Point","coordinates":[x, y]},
        "properties":{"name": name}})
}

fn layer(name: &str) -> SourceLayer {
    SourceLayer {
        name: name.into(),
        features: vec![point("Kigali", 30.06, -1.94), point("Huye", 29.74, -2.60)],
        style_document: None,
    }
}

fn reading() -> Value {
    json!({"sheetKey":"s0","separator":null,"headerRow":null,"xColumn":"lon",
        "yColumn":"lat","crs":"wgs84_lonlat","utmZone":null,"utmSouthern":null,
        "attributesOnly":false,"layerName":null,"columnRenames":[],"dropColumns":[]})
}

#[test]
fn a_layer_style_document_survives_in_the_geoparquet_document() {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let style = json!({"type":"line","metadata":{"kml_styles":{"doc.kml#select@inline-0":{}}}});
    let mut styled = layer("Lines");
    styled.style_document = Some(style.clone());
    let out = layers_to_geoparquet("earth.kml", b"<kml/>", None, vec![styled], None, 0).unwrap();
    assert_eq!(out.receipt.feature_count, 2);
    let reader = SerializedFileReader::new(bytes::Bytes::from(out.parquet)).unwrap();
    let metadata = reader
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .unwrap();
    let document = metadata
        .iter()
        .find(|entry| entry.key == "ds:style_document")
        .unwrap();
    let doc: Value = serde_json::from_str(document.value.as_ref().unwrap()).unwrap();
    assert_eq!(doc, style);
}

#[test]
fn a_source_carrying_its_own_geometry_needs_no_reading() {
    assert!(carries_geometry("places.GeoJSON"));
    assert!(carries_geometry("poles.shp.zip"));
    assert!(!carries_geometry("points.csv"));
    let out =
        layers_to_geoparquet("places.geojson", b"{}", None, vec![layer("p")], None, 0).unwrap();
    assert_eq!(out.receipt.feature_count, 2);
    assert!(out.parquet.starts_with(b"PAR1"), "parquet magic");
    assert!(out.parquet.ends_with(b"PAR1"), "parquet footer magic");
    assert_eq!(out.receipt.source_digest, digest(b"{}"));
    assert_eq!(out.receipt.conversion_id.len(), 64);
}

#[test]
fn one_artifact_holds_one_named_layer() {
    let two = || vec![layer("a"), layer("b")];
    let error = layers_to_geoparquet("x.zip", b"z", None, two(), None, 0).unwrap_err();
    assert!(error.contains("holds 2 layers (a, b)"), "got: {error}");
    let error = layers_to_geoparquet("x.zip", b"z", None, two(), Some("c"), 0).unwrap_err();
    assert!(error.contains("no layer named `c`"), "got: {error}");
    let named = layers_to_geoparquet("x.zip", b"z", None, two(), Some("b"), 0).unwrap();
    assert_eq!(named.receipt.layer_name, "b");
    let error = layers_to_geoparquet("x.zip", b"z", None, Vec::new(), None, 0).unwrap_err();
    assert!(error.contains("produced no layer"), "got: {error}");
}

#[test]
fn an_unchanged_conversion_is_the_same_artifact_and_rows_are_recorded() {
    let run =
        || layers_to_geoparquet("p.csv", CSV, Some(&reading()), vec![layer("p")], None, 3).unwrap();
    let (a, b) = (run(), run());
    assert_eq!(a.receipt.conversion_id, b.receipt.conversion_id);
    assert_eq!(a.parquet, b.parquet);
    assert_eq!(a.receipt.skipped_coordinate_rows, 3);
    let other =
        layers_to_geoparquet("p.csv", b"x", Some(&reading()), vec![layer("p")], None, 3).unwrap();
    assert_ne!(a.receipt.conversion_id, other.receipt.conversion_id);
}
