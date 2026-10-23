use super::*;

fn file(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

const POINT: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[29.7,-2.6]},"properties":{"name":"c1"}}]}"#;

/// A GeoJSON file named for an LV layer becomes that layer, read by the shared
/// reader with its features intact.
#[test]
fn a_named_geojson_file_is_its_lv_layer() {
    let dir = tempfile::tempdir().unwrap();
    let (layers, ignored) = read_layers(&file(dir.path(), "customers.geojson", POINT)).unwrap();
    assert_eq!(layers.keys().collect::<Vec<_>>(), ["customers"]);
    assert_eq!(layers["customers"]["type"], "FeatureCollection");
    assert_eq!(layers["customers"]["features"].as_array().unwrap().len(), 1);
    assert!(ignored.is_empty());
}

/// Nothing is guessed: a file with no LV layer refuses by name, an unknown
/// format or an empty file refuses before any read, and a broken GeoPackage
/// is the reader's refusal.
#[test]
fn unrecognized_or_unreadable_files_refuse_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let survey = read_layers(&file(dir.path(), "survey_points.geojson", POINT)).unwrap_err();
    assert_eq!(survey.code(), "status_upload_layers_unrecognized");
    assert!(
        survey.message().contains("survey_points"),
        "{}",
        survey.message()
    );
    let text = read_layers(&file(dir.path(), "design.txt", "x")).unwrap_err();
    assert_eq!(text.code(), "status_upload_invalid");
    let empty = read_layers(&file(dir.path(), "tr.geojson", "")).unwrap_err();
    assert_eq!(empty.code(), "status_upload_invalid");
    let broken = read_layers(&file(dir.path(), "t001.gpkg", "not a geopackage")).unwrap_err();
    assert_eq!(broken.code(), "status_upload_layers_unreadable");
    let not_json = read_layers(&file(dir.path(), "tr.geojson", "{")).unwrap_err();
    assert_eq!(not_json.code(), "status_upload_layers_unreadable");
}

#[test]
fn layer_names_map_once_and_presets_follow_the_mode() {
    assert_eq!(lv_layer("LV_Poles.shp"), Some("lv_poles"));
    assert_eq!(lv_layer("archive/spans"), Some("spans"));
    assert_eq!(lv_layer("poles"), None);
    assert_eq!(preset("lv-drafting"), "drafting");
    assert_eq!(preset("lv-process"), "drafting");
    assert_eq!(preset("sketch-lv"), "sketch");
}
