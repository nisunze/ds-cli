use super::*;

#[test]
fn a_geopackage_with_a_live_journal_is_refused_before_anything_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("t001.gpkg");
    std::fs::write(&source, b"SQLite format 3\0").unwrap();
    std::fs::write(dir.path().join("t001.gpkg-wal"), b"live").unwrap();
    let error = closed_geopackage(&source, 1024).unwrap_err();
    assert!(error.contains("live SQLite journal sidecar"), "{error}");
    assert_eq!(
        std::fs::read(dir.path().join("t001.gpkg-wal")).unwrap(),
        b"live"
    );
}

#[test]
fn a_geopackage_is_bounded_and_read_whole_once_admitted() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("t001.gpkg");
    std::fs::write(&source, vec![7u8; 64]).unwrap();
    assert!(
        closed_geopackage(&source, 63)
            .unwrap_err()
            .contains("within 63 bytes")
    );
    assert_eq!(closed_geopackage(&source, 64).unwrap(), vec![7u8; 64]);
    assert!(
        closed_geopackage(&dir.path().join("absent.gpkg"), 64)
            .unwrap_err()
            .contains("cannot open GeoPackage source")
    );
}
