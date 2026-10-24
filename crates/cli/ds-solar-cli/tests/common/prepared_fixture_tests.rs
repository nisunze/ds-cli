#[path = "../../../../../tests/common/solar_opened_fixture_tests.rs"]
mod solar_opened_fixtures;

// Frozen ADERM boundary values; provenance lives beside the repository fixtures.
// Fixture construction reads local JSON without an engine or decoder.
use ds_solar_contracts::{
    CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision, SolarCityInput, WeatherKey,
};

pub fn prepared(city: &str) -> PreparedSolarCityInput {
    assert_eq!(city, "aderm_bere");
    let snapshot = solar_opened_fixtures::snapshot(city);
    let input = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "ds-solar-fixtures".into(),
            root: "solar".into(),
            city_id: city.into(),
            display_name: snapshot["_root"]["display_name"]
                .as_str()
                .unwrap_or(city)
                .into(),
        },
        SnapshotRevision {
            source: "frozen-fixture".into(),
            captured_at: "2026-08-08T00:00:00Z".into(),
            record_updated_at_ms: None,
            revision: None,
        },
        RunOptions::default(),
        &snapshot,
    )
    .unwrap();
    let (weather, reference) = solar_opened_fixtures::opened(city);
    let key = WeatherKey::of_dataset(&weather).unwrap();
    PreparedSolarCityInput::commit(input, key, weather, reference, "2026-08-20T00:00:00Z").unwrap()
}
