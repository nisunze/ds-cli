//! Loading the frozen fixtures that live at the workspace root.
//!
//! These are sanitized captures of already-seeded per-city records plus the
//! weather datasets frozen alongside them. The CLI reads them so that a
//! developer, a parity run and a deployed revision can all be pointed at the
//! same bytes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ds_solar_contracts::{
    BundleBytes, CityIdentity, RunOptions, SnapshotRevision, SolarCityInput, WeatherDataset,
};

/// A fixture directory, as passed on the command line.
#[derive(Debug, Clone)]
pub struct FixtureSet {
    root: PathBuf,
}

impl FixtureSet {
    /// Use `root` as the fixture directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory holding captured city snapshots.
    pub fn city_dir(&self) -> PathBuf {
        self.root.join("city")
    }

    /// Every city in the fixture set, in stable order.
    pub fn city_ids(&self) -> Result<Vec<String>> {
        let directory = self.city_dir();
        let mut names: Vec<String> = std::fs::read_dir(&directory)
            .with_context(|| format!("reading {}", directory.display()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".snapshot.json"))
                    .map(str::to_owned)
            })
            .collect();
        names.sort();
        Ok(names)
    }

    /// The captured snapshot for one city, normalised into the contract.
    pub fn city_input(&self, city: &str, project_id: &str, root: &str) -> Result<SolarCityInput> {
        let path = self.root.join(format!("city/{city}.snapshot.json"));
        let snapshot: serde_json::Value = read_json(&path)?;
        let display_name = snapshot
            .get("_root")
            .and_then(|node| node.get("display_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(city)
            .to_string();
        let captured_at = snapshot
            .get("_provenance")
            .and_then(|node| node.get("manifest_saved_at"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("1970-01-01T00:00:00Z")
            .to_string();
        SolarCityInput::from_city_snapshot(
            CityIdentity {
                project_id: project_id.to_string(),
                root: root.to_string(),
                city_id: city.to_string(),
                display_name,
            },
            SnapshotRevision {
                source: "frozen-fixture".to_string(),
                captured_at,
                record_updated_at_ms: None,
                revision: None,
            },
            RunOptions::default(),
            &snapshot,
        )
        .with_context(|| format!("normalising the {city} snapshot at {}", path.display()))
    }

    /// The canonical weather dataset for one city.
    pub fn weather(&self, city: &str) -> Result<WeatherDataset> {
        read_json(&self.root.join(format!("weather/{city}.weather.json")))
    }

    /// The frozen Python summary for one city.
    pub fn golden(&self, city: &str) -> Result<serde_json::Value> {
        read_json(
            &self
                .root
                .join(format!("golden/{city}.site-weather-summary.golden.json")),
        )
    }

    /// One city's sealed reference bundle, as the four artifacts.
    ///
    /// The bundle's own `weather.json` is the encoding its manifest hashed;
    /// `fixtures/weather` holds the same dataset in the corpus's encoding, and
    /// `reference_bundles.rs` asserts the two carry the same content digest.
    pub fn reference_bundle(&self, city: &str) -> Result<BundleBytes> {
        let directory = self.root.join("reference").join(city);
        let read = |name: &str| -> Result<Vec<u8>> {
            let path = directory.join(name);
            std::fs::read(&path).with_context(|| format!("reading fixture {}", path.display()))
        };
        Ok(BundleBytes {
            weather_json: read("weather.json")?,
            reference_unit_json: read("reference-unit.json")?,
            reference_unit_parquet: read("reference-unit.parquet")?,
            manifest_json: read("manifest.json")?,
        })
    }

    /// Every sealed reference bundle in the fixture set, for use as a provider.
    pub fn all_reference_bundles(&self) -> Result<Vec<BundleBytes>> {
        self.city_ids()?
            .iter()
            .map(|city| self.reference_bundle(city))
            .collect()
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading fixture {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing fixture {}", path.display()))
}
