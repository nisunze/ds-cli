use ds_solar_contracts::{
    BundleBytes, CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision,
    SolarCityInput, WeatherDataset, WeatherKey, WeatherPin,
};
use serde_json::json;
const SOLAR_PROJECT: &str = "durable-server-test";
pub fn prepared_solar_request() -> Vec<u8> {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-solar/fixtures");
    let city = "aderm_bere";
    let snapshot: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(format!("city/{city}.snapshot.json"))).unwrap(),
    )
    .unwrap();
    let input = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: SOLAR_PROJECT.to_string(),
            root: "solar".to_string(),
            city_id: city.to_string(),
            display_name: snapshot["_root"]["display_name"]
                .as_str()
                .unwrap()
                .to_string(),
        },
        SnapshotRevision {
            source: "ds-brain".to_string(),
            captured_at: "2026-08-08T00:00:00Z".to_string(),
            record_updated_at_ms: None,
            revision: Some("a".repeat(64)),
        },
        RunOptions::default(),
        &snapshot,
    )
    .unwrap();
    let weather: WeatherDataset = serde_json::from_slice(
        &std::fs::read(root.join(format!("weather/{city}.weather.json"))).unwrap(),
    )
    .unwrap();
    let weather_key = WeatherKey::new(
        input.site.latitude,
        input.site.longitude,
        weather.provenance.timezone.clone(),
        WeatherPin::pvgis_tmy(),
    )
    .unwrap();
    let reference_root = root.join("reference").join(city);
    let reference = ds_solar_contracts::reference_unit::open(&BundleBytes {
        weather_json: std::fs::read(reference_root.join("weather.json")).unwrap(),
        reference_unit_json: std::fs::read(reference_root.join("reference-unit.json")).unwrap(),
        reference_unit_parquet: std::fs::read(reference_root.join("reference-unit.parquet"))
            .unwrap(),
        manifest_json: std::fs::read(reference_root.join("manifest.json")).unwrap(),
    })
    .unwrap()
    .reference_unit;
    let prepared = PreparedSolarCityInput::commit(
        input,
        weather_key,
        weather,
        reference,
        "2026-08-20T00:00:00Z",
    )
    .unwrap();
    serde_json::to_vec(&json!({
        "prepared": prepared,
        "render_charts": false,
        "run_id": "durable-solar-run",
    }))
    .unwrap()
}
