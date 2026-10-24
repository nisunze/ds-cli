use super::*;
use ds_solar_contracts::{
    CityInputReadiness, PreparedSolarCityInput, ReferenceBundleManifest, ReferenceUnit,
    SolarCityInput, SolarError,
};
use ds_solar_host::preparation::{PreparationTasks, ReferencePlan};
use std::sync::Arc;

fn invalid_parquet_with_valid_source_digests() -> BundleBytes {
    let mut bundle = common::reference_bytes("aderm_bere");
    bundle.reference_unit_parquet = b"a fully sealed candidate with no Parquet profile".to_vec();
    let mut receipt: ReferenceUnit = serde_json::from_slice(&bundle.reference_unit_json).unwrap();
    receipt.profile.parquet_bytes = bundle.reference_unit_parquet.len();
    receipt.profile.parquet_sha256 =
        ds_solar_contracts::digest::hex_sha256(&bundle.reference_unit_parquet);
    bundle.reference_unit_json = serde_json::to_vec(&receipt).unwrap();
    let mut manifest: ReferenceBundleManifest =
        serde_json::from_slice(&bundle.manifest_json).unwrap();
    for entry in &mut manifest.entries {
        let bytes = match entry.name.as_str() {
            "weather.json" => &bundle.weather_json,
            "reference-unit.json" => &bundle.reference_unit_json,
            "reference-unit.parquet" => &bundle.reference_unit_parquet,
            name => panic!("unexpected sealed manifest member {name}"),
        };
        entry.bytes = bytes.len();
        entry.sha256 = ds_solar_contracts::digest::hex_sha256(bytes);
    }
    manifest.bundle_digest = manifest.recompute_bundle_digest();
    bundle.manifest_json = serde_json::to_vec(&manifest).unwrap();
    manifest.verify(&bundle).unwrap();
    bundle
}

#[test]
fn a_byte_valid_candidate_cannot_bypass_addon_admission_or_import() {
    let directory = tempdir();
    let cache = DirectoryReferenceCache::new(directory.path());
    let city = common::city_input("aderm_bere");
    let key = common::reference_key("aderm_bere");
    let bundle = invalid_parquet_with_valid_source_digests();
    cache.put(&key, &bundle).unwrap();
    assert_eq!(cache.get(&key).unwrap().unwrap(), bundle);

    let preparer = ReferencePreparer::new().with_cache(&cache);
    let error = preparer
        .prepare(city.clone(), common::PREPARED_AT)
        .unwrap_err();
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
    assert!(matches!(
        preparer.readiness(&city),
        CityInputReadiness::NotReady { error }
            if error.code == SolarErrorCode::ReferenceUnitInvalid
    ));

    let error = ds_solar_reference_host::reference_import::VerifiedBundle::verify(
        city.clone(),
        bundle.clone(),
        common::PREPARED_AT,
    )
    .err()
    .expect("malformed Parquet must refuse before an import can commit");
    assert_eq!(
        error.downcast_ref::<SolarError>().unwrap().code,
        SolarErrorCode::ReferenceUnitInvalid
    );
    assert_eq!(cache.get(&key).unwrap().unwrap(), bundle);

    let replacement = common::reference_bytes("aderm_bere");
    let provider = CountingProvider::new(replacement.clone());
    let repaired = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(city, common::PREPARED_AT)
        .expect("a semantic candidate failure can be replaced once");
    assert_eq!(repaired.resolution, BundleResolution::Provider);
    assert_eq!(provider.calls(), 1);
    assert_eq!(cache.get(&key).unwrap().unwrap(), replacement);
}

/// Only the outcome is controlled; the actual policy supplies the fixture plan.
struct RefusingTasks {
    plan: ReferencePlan,
    refusal: SolarError,
    prepares: AtomicUsize,
}

impl PreparationTasks for RefusingTasks {
    fn plan(&self, _: &SolarCityInput, _: ReferencePin) -> SolarResult<ReferencePlan> {
        Ok(self.plan.clone())
    }

    fn prepare(
        &self,
        _: SolarCityInput,
        _: ReferencePin,
        _: &BundleBytes,
        _: &str,
    ) -> SolarResult<PreparedSolarCityInput> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Err(self.refusal.clone())
    }
}

#[test]
fn missing_capability_and_cancellation_never_fetch_or_become_cache_misses() {
    for code in [
        SolarErrorCode::SolarEngineUpdateRequired,
        SolarErrorCode::Cancelled,
        SolarErrorCode::ArtifactFailed,
    ] {
        let input = common::city_input("aderm_bere");
        let policy = ds_solar_runtime::ReferencePreparer::new();
        let key = policy.reference_key(&input).unwrap();
        let plan = ReferencePlan {
            request: policy.request_for(&input).unwrap(),
            key: key.clone(),
            city: input.clone(),
        };
        let bundle = common::reference_bytes("aderm_bere");
        let cache = InMemoryReferenceCache::new();
        cache.put(&key, &bundle).unwrap();
        let provider = CountingProvider::new(bundle.clone());
        let tasks = Arc::new(RefusingTasks {
            plan,
            refusal: SolarError::new(code, "retained exact typed refusal"),
            prepares: AtomicUsize::new(0),
        });
        let preparer = ReferencePreparer::new()
            .with_tasks(tasks.clone())
            .with_cache(&cache)
            .with_provider(&provider);
        let error = preparer
            .prepare(input.clone(), common::PREPARED_AT)
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.message, "retained exact typed refusal");
        assert_eq!(tasks.prepares.load(Ordering::SeqCst), 1);
        assert_eq!(provider.calls(), 0);
        assert!(matches!(
            preparer.readiness(&input),
            CityInputReadiness::NotReady { error } if error.code == code
        ));
        assert_eq!(provider.calls(), 0);
        assert_eq!(cache.get(&key).unwrap().unwrap(), bundle);
    }
}
