//! Frozen Solar boundary values; fixture construction runs no decoder or addon.
use ds_solar_contracts::{SealedReferenceUnit, WeatherDataset};
use std::path::{Path, PathBuf};

fn fixture(directory: &str, city: &str, suffix: &str) -> PathBuf {
    assert!(
        ds_solar_contracts::is_solar_id(city),
        "invalid fixture city"
    );
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/solar")
        .join(directory)
        .join(format!("{city}{suffix}"))
}

pub fn snapshot(city: &str) -> serde_json::Value {
    let path = fixture("city", city, ".snapshot.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

pub fn opened(city: &str) -> (WeatherDataset, SealedReferenceUnit) {
    let path = fixture("opened", city, ".json");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}
