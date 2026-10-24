use super::*;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

fn workspace(tag: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ds-web-native-columnar-{tag}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn source(root: &Path, bytes: &[u8]) -> PathBuf {
    let path = root.join("points.txt");
    std::fs::File::create(&path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    path
}

fn input() -> SourceReadingV1 {
    SourceReadingV1 {
        sheet_key: "text".to_string(),
        separator: Some("whitespace".to_string()),
        header_row: None,
        x_column: Some("x".to_string()),
        y_column: Some("y".to_string()),
        crs: "wgs84_lonlat".to_string(),
        utm_zone: None,
        utm_southern: None,
        attributes_only: false,
        layer_name: Some("points".to_string()),
        column_renames: vec![],
        drop_columns: vec![],
    }
}

#[test]
fn an_artifact_is_addressed_by_the_converters_own_identity() {
    let root = workspace("address");
    let path = source(&root, b"x y\n30 -2\n");
    let store = root.join("columnar");

    let plan = plan(
        &store,
        &path,
        "points.txt",
        &input(),
        Some("points"),
        &ReadingParameters::default(),
    )
    .unwrap();

    assert_eq!(plan.source_digest.len(), 64);
    assert_eq!(plan.conversion_id.len(), 64);
    assert_eq!(
        plan.artifact_path,
        store
            .join(&plan.source_digest)
            .join(format!("{}.parquet", plan.conversion_id)),
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_different_post_parse_reading_of_the_same_bytes_is_a_different_artifact() {
    let root = workspace("reading");
    let path = source(&root, b"x y z\n30 -2 100\n");
    let store = root.join("columnar");
    let plain = ReadingParameters {
        data_start: 1,
        columns: vec!["x".into(), "y".into(), "z".into()],
        z_column: None,
    };
    let elevated = ReadingParameters {
        z_column: Some("z".into()),
        ..plain.clone()
    };

    let a = plan(&store, &path, "points.txt", &input(), None, &plain).unwrap();
    let b = plan(&store, &path, "points.txt", &input(), None, &elevated).unwrap();

    assert_eq!(a.source_digest, b.source_digest, "the same bytes");
    assert_ne!(
        a.conversion_id, b.conversion_id,
        "promoting elevation changes what is written"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unchanged_reading_addresses_the_same_artifact_twice() {
    let root = workspace("stable");
    let path = source(&root, b"x y\n30 -2\n");
    let store = root.join("columnar");
    let reading = ReadingParameters {
        data_start: 1,
        columns: vec!["x".into(), "y".into()],
        z_column: None,
    };

    let a = plan(&store, &path, "points.txt", &input(), None, &reading).unwrap();
    let b = plan(&store, &path, "points.txt", &input(), None, &reading).unwrap();

    assert_eq!(a.artifact_path, b.artifact_path);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_layer_name_falls_back_to_the_file_stem_only_when_nothing_named_it() {
    let mut named = input();
    assert_eq!(
        fallback_layer_name(&named, "rwanda_sub1_orthocut.txt"),
        "points"
    );
    named.layer_name = Some("   ".to_string());
    assert_eq!(
        fallback_layer_name(&named, "rwanda_sub1_orthocut.txt"),
        "rwanda_sub1_orthocut",
    );
}
