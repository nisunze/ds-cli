#[path = "../../../../../tests/common/solar_opened_fixture_tests.rs"]
mod solar_opened_fixtures;

use ds_solar_contracts::{
    CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision, SolarCityInput, WeatherKey,
    WeatherPin,
};
use serde_json::json;
const SOLAR_PROJECT: &str = "durable-server-test";
pub fn prepared_solar_request() -> Vec<u8> {
    let city = "aderm_bere";
    let snapshot = solar_opened_fixtures::snapshot(city);
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
    let (weather, reference) = solar_opened_fixtures::opened(city);
    let weather_key = WeatherKey::new(
        input.site.latitude,
        input.site.longitude,
        weather.provenance.timezone.clone(),
        WeatherPin::pvgis_tmy(),
    )
    .unwrap();
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
