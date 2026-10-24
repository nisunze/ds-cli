//! Explicit sealed-bundle verification and cache commit shared by native hosts.
//! Callers retain project/revision admission before reads and before commit.
use anyhow::{Result, ensure};
use ds_solar_contracts::{BundleBytes, ReferenceKey, ReferencePin, SolarCityInput};
use ds_solar_host::{
    cache::{DirectoryReferenceCache, ReferenceCache},
    preparation::{AddonPreparation, PreparationTasks},
};
use std::{path::Path, sync::Arc};

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
/// Verification has no durable effects. Hold this until the caller rechecks source authority.
pub struct VerifiedBundle {
    key: ReferenceKey,
    bundle: BundleBytes,
    prepared_input_digest: String,
}
pub struct CommitReceipt {
    pub reused: bool,
    pub bytes: usize,
    pub prepared_input_digest: String,
}
impl VerifiedBundle {
    pub fn verify(input: SolarCityInput, bundle: BundleBytes, prepared_at: &str) -> Result<Self> {
        Self::verify_with_tasks(
            input,
            bundle,
            prepared_at,
            Arc::new(AddonPreparation::installed()),
        )
    }

    pub fn verify_with_tasks(
        input: SolarCityInput,
        bundle: BundleBytes,
        prepared_at: &str,
        tasks: Arc<dyn PreparationTasks>,
    ) -> Result<Self> {
        let pin = ReferencePin::current();
        let plan = tasks.plan(&input, pin.clone())?;
        let prepared = tasks.prepare(plan.city, pin, &bundle, prepared_at)?;
        Ok(Self {
            key: plan.key,
            bundle,
            prepared_input_digest: prepared.input_digest,
        })
    }
    pub fn commit(self, cache: &Path) -> Result<CommitReceipt> {
        let cache = DirectoryReferenceCache::new(cache);
        let retained = cache.get(&self.key)?;
        let reused = retained
            .as_ref()
            .is_some_and(|held| same_bytes(held, &self.bundle));
        if !reused {
            cache.put(&self.key, &self.bundle)?;
        }
        // Admission already completed before the caller's authority recheck.
        // Commit proves exact four-file retention, with no new addon selection,
        // semantic calculation or second decode at this filesystem boundary.
        let retained = cache.get(&self.key)?;
        ensure!(
            retained
                .as_ref()
                .is_some_and(|held| same_bytes(held, &self.bundle)),
            "reference cache readback mismatch"
        );
        Ok(CommitReceipt {
            reused,
            bytes: self.bundle.weather_json.len()
                + self.bundle.reference_unit_json.len()
                + self.bundle.reference_unit_parquet.len()
                + self.bundle.manifest_json.len(),
            prepared_input_digest: self.prepared_input_digest,
        })
    }
}

fn same_bytes(left: &BundleBytes, right: &BundleBytes) -> bool {
    left.weather_json == right.weather_json
        && left.reference_unit_json == right.reference_unit_json
        && left.reference_unit_parquet == right.reference_unit_parquet
        && left.manifest_json == right.manifest_json
}
