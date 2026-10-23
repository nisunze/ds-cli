//! The attach fallback is closed: none by default, AWS Terrarium only by its
//! own name, anything else refused before a file is read.
use super::*;

fn request(fallback: &str) -> Result<elevation::NativeElevationRequest, CommandFailure> {
    elevation_request(
        json!({"source":"/data/a.csv", "out":"/data/b.geojson", "fallback": fallback}),
    )
}

fn expected(terrarium: bool) -> elevation::NativeElevationRequest {
    serde_json::from_value(
        json!({"sourcePath":"/data/a.csv", "outputPath":"/data/b.geojson",
        "useTerrariumFallback": terrarium}),
    )
    .unwrap()
}

#[test]
fn terrarium_fills_rwanda_holes_only_when_named() {
    assert_eq!(request("none").unwrap(), expected(false));
    let defaulted =
        elevation_request(json!({"source":"/data/a.csv", "out":"/data/b.geojson"})).unwrap();
    assert_eq!(defaulted, expected(false));
    assert_eq!(request(TERRARIUM_FALLBACK).unwrap(), expected(true));
    for token in ["terrarium", "aws", "AWS_TERRARIUM_30M", ""] {
        assert!(request(token).is_err(), "{token}");
    }
}
