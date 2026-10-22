//! Preparation effects stop at the application boundary; Run receives held inputs.
mod common;
use ds_solar_contracts::{BundleBytes, SolarErrorCode, SolarResult};
use ds_solar_io::cache::{InMemoryReferenceCache, ReferenceCache};
use ds_solar_reference_host::{ReferenceBundleProvider, ReferencePreparer, ReferenceRequest};
use ds_solar_runtime::{CalculationOptions, SolarRuntime};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A provider that fails the test if it is ever consulted.
struct ForbiddenProvider(AtomicUsize);

impl ReferenceBundleProvider for ForbiddenProvider {
    fn fetch(&self, request: &ReferenceRequest) -> SolarResult<BundleBytes> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("Run reached the provider for {request:?}");
    }
}

#[test]
fn a_full_run_completes_from_the_cache_with_no_provider_in_reach() {
    // The whole offline claim in one test: fill the cache, prepare with a
    // provider that panics if touched, then run. `SolarRuntime` holds no
    // cache, no provider and no client, so the run half cannot reach the
    // network even if it wanted to — but preparing against a forbidden
    // provider is what proves the *cache* answered.
    let cache = InMemoryReferenceCache::new();
    let provider = ForbiddenProvider(AtomicUsize::new(0));
    let city = "aderm_bere";
    cache
        .put(&common::reference_key(city), &common::reference_bytes(city))
        .expect("the cache accepts a sealed bundle");

    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(common::city_input(city), common::PREPARED_AT)
        .expect("a cached city prepares offline");

    let outcome = SolarRuntime::new()
        .calculate(&prepared.input, &CalculationOptions::default())
        .expect("a prepared city runs offline");
    assert_eq!(outcome.result.identity.city_id, city);
    assert_eq!(provider.0.load(Ordering::SeqCst), 0);
}

#[test]
fn a_missing_reference_unit_refuses_before_run_and_never_fetches_during_it() {
    // Preparation is where the network is allowed to be, and it is what
    // refuses. There is no code path from Run back to a provider, so the
    // refusal has to happen here or the city simply never becomes runnable.
    let cache = InMemoryReferenceCache::new();
    let error = ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect_err("an unprepared city cannot be prepared from nothing");
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
    assert!(cache.is_empty(), "a refusal wrote to the cache");
}
