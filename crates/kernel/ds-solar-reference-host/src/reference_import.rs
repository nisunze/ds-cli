//! Explicit sealed-bundle verification and cache commit shared by native hosts.
//! Callers retain project/revision admission before reads and before commit.
use crate::{preparation::ReferencePreparer, provider::ReferenceBundleProvider};
use anyhow::{Result, ensure};
use ds_solar_contracts::{BundleBytes, SolarCityInput, SolarResult};
use ds_solar_host::cache::{DirectoryReferenceCache, ReferenceCache};
use std::path::Path;

pub fn read_bundle(directory: &Path) -> Result<BundleBytes> {
    let metadata = std::fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "reference bundle directory must be regular, not a link"
    );
    let read = |name: &str| {
        ds_solar_host::files::read(
            &directory.join(name),
            ds_solar_host::MAX_REFERENCE_ARTIFACT_BYTES,
        )
    };
    Ok(BundleBytes {
        weather_json: read("weather.json")?,
        reference_unit_json: read("reference-unit.json")?,
        reference_unit_parquet: read("reference-unit.parquet")?,
        manifest_json: read("manifest.json")?,
    })
}
struct OneBundle(BundleBytes);
impl ReferenceBundleProvider for OneBundle {
    fn fetch(&self, _: &crate::provider::ReferenceRequest) -> SolarResult<BundleBytes> {
        Ok(self.0.clone())
    }
}
/// Verification has no durable effects. Hold this until the caller rechecks source authority.
pub struct VerifiedBundle {
    input: SolarCityInput,
    bundle: BundleBytes,
    prepared_at: String,
    prepared_input_digest: String,
}
pub struct CommitReceipt {
    pub reused: bool,
    pub bytes: usize,
    pub prepared_input_digest: String,
}
impl VerifiedBundle {
    pub fn verify(input: SolarCityInput, bundle: BundleBytes, prepared_at: &str) -> Result<Self> {
        let provider = OneBundle(bundle);
        let prepared = ReferencePreparer::new()
            .with_provider(&provider)
            .prepare(input.clone(), prepared_at)?;
        Ok(Self {
            input,
            bundle: provider.0,
            prepared_at: prepared_at.into(),
            prepared_input_digest: prepared.input.input_digest,
        })
    }
    pub fn commit(self, cache: &Path) -> Result<CommitReceipt> {
        let key = ReferencePreparer::new().reference_key(&self.input)?;
        let cache = DirectoryReferenceCache::new(cache);
        let retained = cache.get(&key)?;
        let reused = retained.as_ref().is_some_and(|held| {
            held.weather_json == self.bundle.weather_json
                && held.reference_unit_json == self.bundle.reference_unit_json
                && held.reference_unit_parquet == self.bundle.reference_unit_parquet
                && held.manifest_json == self.bundle.manifest_json
        });
        if !reused {
            cache.put(&key, &self.bundle)?;
        }
        let verified = ReferencePreparer::new()
            .with_cache(&cache)
            .prepare(self.input, &self.prepared_at)?;
        ensure!(
            self.prepared_input_digest == verified.input.input_digest,
            "reference cache readback mismatch"
        );
        Ok(CommitReceipt {
            reused,
            bytes: self.bundle.weather_json.len()
                + self.bundle.reference_unit_json.len()
                + self.bundle.reference_unit_parquet.len()
                + self.bundle.manifest_json.len(),
            prepared_input_digest: verified.input.input_digest,
        })
    }
}
