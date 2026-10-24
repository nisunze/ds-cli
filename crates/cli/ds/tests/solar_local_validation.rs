//! Public pure-owner routes must work with every Solar executable unavailable.
use ds_solar_contracts::{WeatherDataset, WeatherKey, result_comparison::comparison_receipt};
use serde_json::{Value, json};
use std::{path::Path, process::Command};
#[path = "common/solar_result_tests.rs"]
mod fixture;

fn invoke(args: &[&str], missing: &Path) -> (Value, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .env("NO_COLOR", "1")
        .env("DS_SOLAR_BIN", missing)
        .env("DS_SOLAR_ENGINE_BIN", missing)
        .env("PATH", missing)
        .output()
        .unwrap();
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        output.status.success(),
    )
}

#[test]
fn local_result_comparison_needs_no_engine_and_preserves_equal_false() {
    let dir = tempfile::tempdir().unwrap();
    let left_path = dir.path().join("left.json");
    let right_path = dir.path().join("right.json");
    let missing = dir.path().join("missing-solar");
    let left = fixture::result();
    let mut right = left.clone();
    right.run_id = Some("another-run".into());
    std::fs::write(&left_path, serde_json::to_vec(&left).unwrap()).unwrap();
    for equal in [true, false] {
        if !equal {
            right.demand_noise_seed += 1;
            right = right.sealed();
        }
        std::fs::write(&right_path, serde_json::to_vec(&right).unwrap()).unwrap();
        let (value, success) = invoke(
            &[
                "solar",
                "result",
                "compare",
                "--left",
                left_path.to_str().unwrap(),
                "--right",
                right_path.to_str().unwrap(),
            ],
            &missing,
        );
        assert!(success, "{value}");
        assert_eq!(value["data"], comparison_receipt(&left, &right).unwrap());
        assert_eq!(value["data"]["equal"], equal);
    }
    right.demand_noise_seed += 1;
    std::fs::write(&right_path, serde_json::to_vec(&right).unwrap()).unwrap();
    let (value, success) = invoke(
        &[
            "solar",
            "result",
            "compare",
            "--left",
            left_path.to_str().unwrap(),
            "--right",
            right_path.to_str().unwrap(),
        ],
        &missing,
    );
    assert!(!success);
    assert_eq!(value["error"]["code"], "engine_refused");
    assert_eq!(value["error"]["detail"]["owner"], "ds-solar-contracts");
}

#[test]
fn local_weather_validation_needs_no_engine_and_preserves_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weather.json");
    let missing = dir.path().join("missing-solar");
    // Frozen dataset copied from ds-solar 3adcc6d fixtures/weather.
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/solar/aderm_bere/weather.json");
    let mut weather: WeatherDataset =
        serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&weather).unwrap()).unwrap();
    let (value, success) = invoke(
        &[
            "solar",
            "verify-weather",
            "--dataset",
            path.to_str().unwrap(),
        ],
        &missing,
    );
    assert!(success, "{value}");
    assert_eq!(
        value["data"],
        json!({"dataset":path,"verified":true,"engine":format!(
        "schema   {}\nkey      {}\nrecords  {}\ndigest   {}\nvalid    yes",
        weather.schema_version,WeatherKey::of_dataset(&weather).unwrap().cache_entry(),
        weather.record_count(),weather.content_digest)})
    );
    weather.series[0].values[4000] += 0.5;
    std::fs::write(&path, serde_json::to_vec(&weather).unwrap()).unwrap();
    let (value, success) = invoke(
        &[
            "solar",
            "verify-weather",
            "--dataset",
            path.to_str().unwrap(),
        ],
        &missing,
    );
    assert!(!success);
    assert_eq!(value["error"]["code"], "engine_refused");
}

#[test]
fn missing_artifacts_keep_their_declared_refusals_without_an_engine() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let text = missing.to_str().unwrap();
    for (args, code) in [
        (
            vec![
                "solar", "result", "compare", "--left", text, "--right", text,
            ],
            "left_result_not_found",
        ),
        (
            vec!["solar", "verify-weather", "--dataset", text],
            "dataset_not_found",
        ),
    ] {
        let (value, success) = invoke(&args, &missing);
        assert!(!success);
        assert_eq!(value["error"]["code"], code);
    }
}
