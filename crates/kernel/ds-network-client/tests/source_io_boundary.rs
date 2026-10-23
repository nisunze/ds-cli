//! Real consumer of the local GIS source calls against actual engines: the
//! insertion engine reads, inspects and writes; a sorted engine and an absent
//! engine refuse with typed codes before any source is staged.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::source_io::{
    GeojsonLayerV1, LayersGeojsonInputV1, SourceInspectV1, SourceReaderV1,
};
use serde_json::json;
use std::{env, path::PathBuf, process::Command};

const GEOJSON: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature",
"geometry":{"type":"Point","coordinates":[30.06,-1.94]},"properties":{"zeta":1,"alpha":"a"}}]}"#;

fn engine(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} must name a built Network engine"))
}

fn isolated(test: &str, engine: Option<&std::path::Path>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command.args(["--exact", test, "--ignored", "--nocapture"]);
    match engine {
        Some(path) => command.env("DS_NETWORK_ENGINE_BIN", path),
        None => command.env("DS_NETWORK_ENGINE_BIN", "/nonexistent/ds-network"),
    };
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{test}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires DS_NETWORK_SOURCE_IO_ENGINE (insertion) and DS_NETWORK_SOURCE_IO_SORTED_ENGINE"]
fn source_calls_through_actual_engines() {
    isolated(
        "isolated_insertion_engine_reads_and_writes",
        Some(&engine("DS_NETWORK_SOURCE_IO_ENGINE")),
    );
    isolated(
        "isolated_sorted_engine_refuses_source_calls",
        Some(&engine("DS_NETWORK_SOURCE_IO_SORTED_ENGINE")),
    );
    isolated("isolated_missing_engine_refuses", None);
}

#[test]
#[ignore = "isolated executable selection"]
fn isolated_insertion_engine_reads_and_writes() {
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let layers = client
        .source_layers(
            "places.geojson",
            GEOJSON.as_bytes().to_vec(),
            SourceReaderV1::Geojson,
            &|| false,
        )
        .unwrap();
    assert_eq!(layers.layers[0].name, "places");
    let properties = serde_json::to_string(&layers.layers[0].features[0]["properties"]).unwrap();
    assert!(properties.find("zeta").unwrap() < properties.find("alpha").unwrap());
    let summary = client
        .source_inspect(
            "places.geojson",
            GEOJSON.as_bytes().to_vec(),
            SourceInspectV1::Layers(SourceReaderV1::Geojson),
            &|| false,
        )
        .unwrap();
    assert_eq!(summary.layers[0].feature_count, 1);
    let refused = client
        .source_layers(
            "broken.geojson",
            b"{".to_vec(),
            SourceReaderV1::Geojson,
            &|| false,
        )
        .unwrap_err();
    assert!(refused.is_source_refusal(), "{refused:?}");
    let text = client
        .layers_geojson(
            &LayersGeojsonInputV1 {
                layers: vec![GeojsonLayerV1 {
                    name: "places".into(),
                    geojson: serde_json::from_str(GEOJSON).unwrap(),
                }],
            },
            &|| false,
        )
        .unwrap();
    let written: serde_json::Value = serde_json::from_slice(&text).unwrap();
    assert_eq!(
        written["features"][0]["properties"]["_layer"],
        json!("places")
    );
}

#[test]
#[ignore = "isolated executable selection"]
fn isolated_sorted_engine_refuses_source_calls() {
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let error = client
        .source_layers(
            "places.geojson",
            GEOJSON.as_bytes().to_vec(),
            SourceReaderV1::Geojson,
            &|| false,
        )
        .unwrap_err();
    assert_eq!(error.code(), "network_serialization_incompatible");
}

#[test]
#[ignore = "isolated executable selection"]
fn isolated_missing_engine_refuses() {
    let error = CapturedNetworkClient::capture(1, &|| false)
        .err()
        .expect("an absent engine is refused");
    assert_eq!(error.code(), "network_engine_missing");
}
