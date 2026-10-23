//! Attach names AWS Terrarium only by its own token; extract keeps none only.
use super::*;

#[test]
fn attach_dispatches_terrarium_by_name_and_extract_keeps_none_only() {
    let mut tokens = [
        "--source",
        "/data/poles.csv",
        "--out",
        "/data/poles.geojson",
    ]
    .map(str::to_string)
    .to_vec();
    tokens.extend(["--fallback", "aws_terrarium_30m"].map(str::to_string));
    let parsed = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
    let request = arguments(&parsed).unwrap();
    assert_eq!(
        request["fallback"],
        ds_native_data::bridge::TERRARIUM_FALLBACK
    );
    // The bridge turns exactly that token into the Terrarium provider.
    let native = ds_native_data::bridge::elevation_request(Value::Object(request)).unwrap();
    let expected: ds_native_data::elevation::NativeElevationRequest =
        serde_json::from_value(json!({"sourcePath":"/data/poles.csv",
            "outputPath":"/data/poles.geojson", "useTerrariumFallback": true}))
        .unwrap();
    assert_eq!(native, expected);
    // Point-cloud extract shares the old argument and stays none-only.
    assert_eq!(FALLBACK_ARG.choices, &["none"]);
}
