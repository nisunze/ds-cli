use super::*;
use std::io::Read;

#[test]
fn a_geometry_source_names_its_reader_by_extension() {
    assert_eq!(geometry_reader("a.KMZ"), SourceReaderV1::Kmz);
    assert_eq!(geometry_reader("poles.shp.zip"), SourceReaderV1::Archive);
    assert_eq!(geometry_reader("a.kml"), SourceReaderV1::Kml);
    assert_eq!(geometry_reader("a.geojson"), SourceReaderV1::Geojson);
    assert_eq!(geometry_reader("a.json"), SourceReaderV1::Geojson);
}

#[test]
fn a_bare_shapefile_packs_its_sidecars_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("Poles.shp", "shp"),
        ("poles.DBF", "dbf"),
        ("poles.shx", "shx"),
        ("poles.prj", "prj"),
        ("poles.txt", "txt"),
        ("other.dbf", "other"),
    ] {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    let shp = dir.path().join("Poles.shp");
    let first = shapefile_with_sidecars_to_zip(&shp).unwrap();
    assert_eq!(first, shapefile_with_sidecars_to_zip(&shp).unwrap());
    let mut archive = zip::ZipArchive::new(Cursor::new(first)).unwrap();
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut body = String::new();
        member.read_to_string(&mut body).unwrap();
        names.push((member.name().to_string(), body));
    }
    assert_eq!(
        names,
        [
            ("Poles.shp".to_string(), "shp".to_string()),
            ("Poles.dbf".to_string(), "dbf".to_string()),
            ("Poles.prj".to_string(), "prj".to_string()),
            ("Poles.shx".to_string(), "shx".to_string()),
        ]
    );
    assert!(shapefile_with_sidecars_to_zip(&dir.path().join("absent.shp")).is_err());
}

#[test]
fn engine_failures_are_typed_refusals() {
    let missing = engine_failure(NetworkFailure {
        code: "network_engine_missing",
        message: "absent".into(),
        owner_error: None,
        owner_code: None,
        owner_detail: None,
        owner_remedy: None,
        owner_invalid_input: None,
    });
    assert_eq!(missing.code(), "network_engine_missing");
}
