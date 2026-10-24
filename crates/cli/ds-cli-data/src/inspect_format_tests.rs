//! Feedback 8ed4a478: `ds data convert --out x.geoparquet` wrote valid
//! GeoParquet, and `ds data inspect --source x.geoparquet --rows 100` refused
//! it as `source_unsupported` "run data convert first". Inspection recognises
//! its converter's output by content, whatever the suffix.

use super::*;

fn scratch(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "ds-inspect-format-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    folder.join(name)
}

const POINTS: &[u8] = br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"a"},"geometry":{"type":"Point","coordinates":[30.06,-1.95]}}]}"#;

#[test]
fn converted_output_is_geoparquet_whatever_its_suffix() {
    let converted = crate::source_engine::fixture_geoparquet("points.geojson", POINTS);
    for name in [
        "points.geoparquet",
        "points.GEOPARQUET",
        "points",
        "points.parquet",
    ] {
        let path = scratch(name);
        std::fs::write(&path, &converted.parquet).unwrap();
        assert!(
            is_converted_geoparquet(&path, name),
            "{name} holds GeoParquet bytes"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn other_sources_are_not_taken_for_geoparquet() {
    // A GeoJSON, a truncated artifact without its footer magic, an empty
    // file and a missing path keep their own readers and refusals.
    let converted = crate::source_engine::fixture_geoparquet("points.geojson", POINTS);
    let truncated = &converted.parquet[..converted.parquet.len() - 1];
    for (name, bytes) in [
        ("points.geoparquet", POINTS),
        ("cut.geoparquet", truncated),
        ("empty.geoparquet", &[][..]),
    ] {
        let path = scratch(name);
        std::fs::write(&path, bytes).unwrap();
        assert!(!is_converted_geoparquet(&path, name), "{name}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    assert!(!is_converted_geoparquet(
        std::path::Path::new("/nonexistent/ds-inspect/points.geoparquet"),
        "points.geoparquet"
    ));
}

#[test]
fn rows_on_a_source_that_is_not_geoparquet_name_the_conversion_remedy() {
    let path = scratch("points.geojson");
    std::fs::write(&path, POINTS).unwrap();
    let tokens = [
        "--source".to_string(),
        path.display().to_string(),
        "--rows".to_string(),
        "10".to_string(),
    ];
    let inputs = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
    let context = ds_cli_contract::Context {
        confirmed: false,
        output: ds_cli_contract::Output {
            format: ds_cli_contract::Format::Json,
            pretty: false,
            color: false,
        },
    };
    let refused = run(&inputs, &context).unwrap_err();
    assert_eq!(refused.code(), "source_unsupported");
    assert!(
        refused
            .message()
            .contains("points.geojson is not GeoParquet")
    );
    assert!(refused.remedy_text().unwrap().contains("ds data convert"));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
