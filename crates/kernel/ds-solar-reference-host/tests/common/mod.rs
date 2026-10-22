#![allow(dead_code)] // Shared by several test targets, each consuming a subset.
use ds_solar_contracts::{CityIdentity, RunOptions, SnapshotRevision, SolarCityInput};
use std::path::PathBuf;

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures")
}

pub fn city_input(city: &str) -> SolarCityInput {
    let path = fixtures_dir().join(format!("city/{city}.snapshot.json"));
    let snapshot: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("city fixture"))
            .expect("city fixture is valid JSON");
    SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "ds-solar-fixtures".to_string(),
            root: "solar".to_string(),
            city_id: city.to_string(),
            display_name: snapshot["_root"]["display_name"]
                .as_str()
                .unwrap_or(city)
                .to_string(),
        },
        SnapshotRevision {
            source: "frozen-fixture".to_string(),
            captured_at: "2026-08-08T00:00:00Z".to_string(),
            record_updated_at_ms: None,
            revision: None,
        },
        RunOptions::default(),
        &snapshot,
    )
    .expect("fixture snapshot normalises")
}

/// A fixed preparation timestamp, so a test never depends on the clock.
pub const PREPARED_AT: &str = "2026-08-20T00:00:00Z";

/// The reference key one city resolves to under the shipped pin.
pub fn reference_key(city: &str) -> ds_solar_contracts::ReferenceKey {
    let input = city_input(city);
    ds_solar_reference_host::ReferencePreparer::new()
        .reference_key(&input)
        .expect("the fixture city has a usable reference request")
}

/// One city's sealed reference bundle, as the four artifacts a real
/// preparation would have downloaded and verified.
///
/// The bundle's own `weather.json` is the exact encoding its manifest hashed.
/// `fixtures/weather` holds the same dataset in the corpus's encoding, and
/// `reference_bundles.rs` asserts the two carry the same content digest.
pub fn reference_bytes(city: &str) -> ds_solar_contracts::BundleBytes {
    let directory = fixtures_dir().join("reference").join(city);
    let read = |path: std::path::PathBuf| -> Vec<u8> {
        std::fs::read(&path).unwrap_or_else(|err| panic!("{path:?}: {err}"))
    };
    ds_solar_contracts::BundleBytes {
        weather_json: read(directory.join("weather.json")),
        reference_unit_json: read(directory.join("reference-unit.json")),
        reference_unit_parquet: read(directory.join("reference-unit.parquet")),
        manifest_json: read(directory.join("manifest.json")),
    }
}
