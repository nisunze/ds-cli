//! Preparation: the only place that may touch the network, and its fixed order.
//!
//! An offline desktop run depends on every step of this, so each one gets its
//! own assertion — including the two that matter most: a cache hit performs no
//! provider call at all, and the request that leaves carries nothing but a
//! place.

mod common;

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ds_solar_contracts::{
    BundleBytes, MATERIAL_DERIVED_FROM, MATERIAL_DERIVED_FROM_PROPOSED, MATERIAL_DETAIL_FIELDS,
    MaterialEvidence, ReferencePin, SolarErrorCode, SolarResult,
};
use ds_solar_host::cache::{DirectoryReferenceCache, InMemoryReferenceCache, ReferenceCache};
use ds_solar_reference_host::{
    preparation::{BundleResolution, ReferencePreparer, Refresh},
    provider::{ReferenceBundleProvider, ReferenceRequest},
};

/// A provider that records how often it was asked, and with what.
struct CountingProvider {
    bundle: BundleBytes,
    calls: AtomicUsize,
    requests: Mutex<Vec<ReferenceRequest>>,
}

impl CountingProvider {
    fn new(bundle: BundleBytes) -> Self {
        Self {
            bundle,
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_request(&self) -> ReferenceRequest {
        self.requests
            .lock()
            .expect("requests")
            .last()
            .cloned()
            .expect("a request was made")
    }
}

impl ReferenceBundleProvider for CountingProvider {
    fn fetch(&self, request: &ReferenceRequest) -> SolarResult<BundleBytes> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .expect("requests")
            .push(request.clone());
        Ok(self.bundle.clone())
    }
}

/// A provider that fails the test if it is ever consulted.
struct ForbiddenProvider;

impl ReferenceBundleProvider for ForbiddenProvider {
    fn fetch(&self, request: &ReferenceRequest) -> SolarResult<BundleBytes> {
        panic!("a provider call was made for {request:?} when none was permitted");
    }
}

#[test]
fn explicit_held_bundle_admission_matches_application_preparation_and_refuses_another_site() {
    let input = common::city_input("aderm_bere");
    let bytes = common::reference_bytes("aderm_bere");
    let provider = CountingProvider::new(bytes.clone());
    let application = ReferencePreparer::new()
        .with_provider(&provider)
        .prepare(input.clone(), common::PREPARED_AT)
        .unwrap();
    let pure = ds_solar_host::preparation::ReferencePreparer::new()
        .prepare(input.clone(), &bytes, common::PREPARED_AT)
        .unwrap();
    assert_eq!(provider.calls(), 1);
    assert_eq!(
        serde_json::to_value(application.input).unwrap(),
        serde_json::to_value(pure).unwrap()
    );
    let wrong_site = ds_solar_host::preparation::ReferencePreparer::new()
        .prepare(
            input,
            &common::reference_bytes("aderm_gagal"),
            common::PREPARED_AT,
        )
        .unwrap_err();
    assert_eq!(wrong_site.code, SolarErrorCode::ReferenceUnitInvalid);
}

// ---------------------------------------------------------------------------
// Cache-first
// ---------------------------------------------------------------------------

#[test]
fn a_cache_hit_never_reaches_the_provider() {
    let key = common::reference_key("aderm_bere");
    let cache = InMemoryReferenceCache::new();
    cache
        .put(&key, &common::reference_bytes("aderm_bere"))
        .unwrap();

    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&ForbiddenProvider)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("a cached city prepares");
    assert_eq!(prepared.resolution, BundleResolution::Cache);
}

#[test]
fn a_cache_miss_fetches_once_and_the_next_preparation_does_not() {
    let cache = InMemoryReferenceCache::new();
    let provider = CountingProvider::new(common::reference_bytes("aderm_bere"));
    let preparer = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider);

    let first = preparer
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("the first preparation fetches");
    assert_eq!(first.resolution, BundleResolution::Provider);
    assert_eq!(provider.calls(), 1);

    let second = preparer
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("the second preparation reads the cache");
    assert_eq!(second.resolution, BundleResolution::Cache);
    assert_eq!(provider.calls(), 1, "the provider was consulted twice");
    assert_eq!(first.input.input_digest, second.input.input_digest);
}

/// A cache whose entry decayed after it was validated on `put` — damaged
/// bytes on disk, or a bundle sealed under a pin this deployment no longer
/// runs. `DirectoryReferenceCache` already reads byte damage as a miss; this
/// double reaches the classes the preparer itself must survive: an entry
/// that fails `reference::open`/`check_bundle`, and a cache whose read errs.
struct PoisonedCache {
    bundle: Option<BundleBytes>,
}

impl ReferenceCache for PoisonedCache {
    fn get(&self, _key: &ds_solar_contracts::ReferenceKey) -> SolarResult<Option<BundleBytes>> {
        match &self.bundle {
            Some(bundle) => Ok(Some(bundle.clone())),
            None => Err(ds_solar_contracts::SolarError::reference_unit_invalid(
                "the cache read failed",
            )),
        }
    }

    fn put(
        &self,
        _key: &ds_solar_contracts::ReferenceKey,
        _artifacts: &BundleBytes,
    ) -> SolarResult<()> {
        Ok(())
    }
}

#[test]
fn an_unusable_cache_entry_falls_through_to_the_provider() {
    let mut corrupt = common::reference_bytes("aderm_bere");
    corrupt.weather_json = b"{not json".to_vec();
    let cache = PoisonedCache {
        bundle: Some(corrupt),
    };

    let provider = CountingProvider::new(common::reference_bytes("aderm_bere"));
    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("an unusable cache entry is a miss, not a preparation failure");
    assert_eq!(prepared.resolution, BundleResolution::Provider);
    assert_eq!(provider.calls(), 1);
}

#[test]
fn a_failing_cache_read_falls_through_to_the_provider() {
    let cache = PoisonedCache { bundle: None };
    let provider = CountingProvider::new(common::reference_bytes("aderm_bere"));
    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("a cache read failure is a miss when the provider can answer");
    assert_eq!(prepared.resolution, BundleResolution::Provider);
}

#[test]
fn an_unusable_cache_entry_with_no_provider_names_the_real_cause() {
    let mut corrupt = common::reference_bytes("aderm_bere");
    corrupt.weather_json = b"{not json".to_vec();
    let cache = PoisonedCache {
        bundle: Some(corrupt),
    };

    let error = ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect_err("no provider means the unusable entry is a refusal");
    assert!(
        error.message.contains("cached but unusable"),
        "the refusal names the cache state: {}",
        error.message
    );
}

#[test]
fn preparation_time_is_metadata_not_input_identity() {
    let cache = InMemoryReferenceCache::new();
    cache
        .put(
            &common::reference_key("aderm_bere"),
            &common::reference_bytes("aderm_bere"),
        )
        .unwrap();
    let preparer = ReferencePreparer::new().with_cache(&cache);
    let first = preparer
        .prepare(common::city_input("aderm_bere"), "2026-08-01T00:00:00Z")
        .unwrap();
    let second = preparer
        .prepare(common::city_input("aderm_bere"), "2031-02-03T04:05:06Z")
        .unwrap();
    assert_ne!(first.input.prepared_at, second.input.prepared_at);
    assert_eq!(first.input.input_digest, second.input.input_digest);
}

#[test]
fn network_material_is_digest_bound_during_preparation_before_calculation() {
    let cache = InMemoryReferenceCache::new();
    cache
        .put(
            &common::reference_key("aderm_bere"),
            &common::reference_bytes("aderm_bere"),
        )
        .unwrap();

    let mut city = common::city_input("aderm_bere");
    let published = city.seeded.city_inputs["network_data"]["material_quantities"].clone();
    city.seeded.city_inputs["network_data"]
        .as_object_mut()
        .expect("fixture network data")
        .remove("material_quantities")
        .expect("fixture material quantities");
    city = city.sealed();
    let captured_digest = city.content_digest.clone();

    // This call is preparation only. No SolarRuntime or calculation stage has
    // run when the assertions below inspect the committed input.
    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(city, common::PREPARED_AT)
        .expect("the network city prepares");
    let material = prepared.input.city.seeded.city_inputs["network_data"]["material_quantities"]
        .as_object()
        .expect("preparation seals material quantities");

    assert_eq!(MaterialEvidence::of(material), MaterialEvidence::Derived);
    assert_eq!(
        material[MATERIAL_DERIVED_FROM],
        MATERIAL_DERIVED_FROM_PROPOSED
    );
    assert_eq!(
        material["transformers_by_kva"],
        published["transformers_by_kva"]
    );
    let prepared_lv_m = material["total_lv_length_m"].as_f64().unwrap();
    let published_lv_m = published["total_lv_length_m"].as_f64().unwrap();
    assert!((prepared_lv_m - published_lv_m).abs() <= 1.0);
    for field in MATERIAL_DETAIL_FIELDS {
        assert!(
            !material.contains_key(field),
            "preparation must not invent non-derivable `{field}` detail"
        );
    }

    assert_ne!(prepared.input.city.content_digest, captured_digest);
    assert_eq!(
        prepared.input.city.content_digest,
        prepared.input.city.compute_content_digest().unwrap()
    );
    assert_eq!(
        prepared.input.input_digest,
        prepared.input.compute_input_digest().unwrap()
    );
}

#[test]
fn preparation_preserves_governed_detailed_material_exactly() {
    let cache = InMemoryReferenceCache::new();
    cache
        .put(
            &common::reference_key("aderm_bere"),
            &common::reference_bytes("aderm_bere"),
        )
        .unwrap();
    let city = common::city_input("aderm_bere");
    let governed = city.seeded.city_inputs["network_data"]["material_quantities"].clone();
    let captured_digest = city.content_digest.clone();

    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(city, common::PREPARED_AT)
        .expect("the governed network city prepares");

    assert_eq!(
        prepared.input.city.seeded.city_inputs["network_data"]["material_quantities"],
        governed
    );
    assert_eq!(prepared.input.city.content_digest, captured_digest);
}

#[test]
fn underivable_network_material_refuses_readiness_and_prepare() {
    let cache = InMemoryReferenceCache::new();
    cache
        .put(
            &common::reference_key("aderm_bere"),
            &common::reference_bytes("aderm_bere"),
        )
        .unwrap();
    let mut city = common::city_input("aderm_bere");
    let network = city.seeded.city_inputs["network_data"]
        .as_object_mut()
        .expect("fixture network data");
    network.remove("material_quantities");
    network["proposed"]["transformers"][0]
        .as_object_mut()
        .expect("fixture transformer")
        .remove("kva");
    city = city.sealed();

    let preparer = ReferencePreparer::new().with_cache(&cache);
    let readiness = preparer.readiness(&city);
    match readiness {
        ds_solar_contracts::CityInputReadiness::NotReady { error } => {
            assert_eq!(error.code, SolarErrorCode::CityInputIncomplete);
            assert!(error.fields.iter().any(|field| field.contains("kva")));
        }
        other => panic!("underivable network material reported ready: {other:?}"),
    }
    let error = preparer
        .prepare(city, common::PREPARED_AT)
        .expect_err("underivable material must refuse before calculation");
    assert_eq!(error.code, SolarErrorCode::CityInputIncomplete);
    assert!(error.fields.iter().any(|field| field.contains("kva")));
}

#[test]
fn changed_pv_configuration_never_reuses_a_same_coordinate_bundle() {
    let cache = InMemoryReferenceCache::new();
    let base = common::city_input("aderm_bere");
    let mut changed = base.clone();
    *changed
        .seeded
        .templates
        .pointer_mut("/technical_params/losses/ac/inverter_percent")
        .expect("fixture AC loss") = serde_json::json!(3.0);
    changed = changed.sealed();

    let preparer = ReferencePreparer::new().with_cache(&cache);
    let base_key = preparer.reference_key(&base).expect("base key");
    let changed_key = preparer.reference_key(&changed).expect("changed key");
    assert_ne!(
        base_key, changed_key,
        "loss change needs its own cache entry"
    );
    cache
        .put(&base_key, &common::reference_bytes("aderm_bere"))
        .expect("cache baseline bundle");

    let cached = preparer
        .prepare(base.clone(), common::PREPARED_AT)
        .expect("baseline remains locally runnable");
    assert_eq!(cached.resolution, BundleResolution::Cache);

    let error = preparer
        .prepare(changed.clone(), common::PREPARED_AT)
        .expect_err("changed PV inputs cannot borrow the baseline unit");
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
    assert!(
        error.message.contains("not cached"),
        "the old same-coordinate cache entry must be invisible: {error:?}"
    );

    // A cache implementation or disk repair must not be able to circumvent
    // the distinct key by copying the old bytes into the new entry.  The
    // sealed receipt is re-checked against every PV input before committing.
    cache
        .put(&changed_key, &common::reference_bytes("aderm_bere"))
        .expect("place a deliberately mismatched bundle under the changed key");
    let error = preparer
        .prepare(changed, common::PREPARED_AT)
        .expect_err("a copied bundle must not be accepted for changed losses");
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
    assert!(
        error.message.contains("losses do not match"),
        "receipt verification must reject a mis-keyed cache entry: {error:?}"
    );
}

#[test]
fn city_identity_does_not_fragment_an_equivalent_reference_cache_entry() {
    let base = common::city_input("aderm_bere");
    let mut same_site = base.clone();
    same_site.identity.city_id = "another-record-for-the-same-site".to_string();
    same_site = same_site.sealed();

    let preparer = ReferencePreparer::new();
    assert_eq!(
        preparer.reference_key(&base).expect("base key"),
        preparer.reference_key(&same_site).expect("same-site key"),
        "city identity belongs to the downstream calculation, not the reference bundle cache"
    );
}

#[test]
fn overwrite_re_fetches_even_when_the_cache_holds_a_verifiable_entry() {
    // The only way to pick up a producer-side pin change. Nothing about a
    // pvlib upgrade or a revised catalog row is visible from this side of the
    // boundary, so the refresh is deliberate rather than inferred.
    let cache = InMemoryReferenceCache::new();
    let provider = CountingProvider::new(common::reference_bytes("aderm_bere"));
    let key = common::reference_key("aderm_bere");
    cache
        .put(&key, &common::reference_bytes("aderm_bere"))
        .unwrap();

    let prepared = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .with_refresh(Refresh::Overwrite)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("an overwritten city prepares");
    assert_eq!(prepared.resolution, BundleResolution::Provider);
    assert_eq!(provider.calls(), 1);
}

#[test]
fn with_no_cache_entry_and_no_provider_preparation_refuses() {
    let cache = InMemoryReferenceCache::new();
    let error = ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect_err("nothing can prepare this city");
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
    assert!(error.message.contains("solar.reference.plan"));
    assert!(error.message.contains("solar.reference.import"));
}

#[test]
fn readiness_reports_a_city_that_could_run_offline_right_now() {
    let cache = InMemoryReferenceCache::new();
    let input = common::city_input("aderm_bere");

    let preparer = ReferencePreparer::new().with_cache(&cache);
    assert!(matches!(
        preparer.readiness(&input),
        ds_solar_contracts::CityInputReadiness::NotReady { .. }
    ));

    cache
        .put(
            &common::reference_key("aderm_bere"),
            &common::reference_bytes("aderm_bere"),
        )
        .unwrap();
    assert!(matches!(
        preparer.readiness(&input),
        ds_solar_contracts::CityInputReadiness::Preparing { .. }
    ));
}

// ---------------------------------------------------------------------------
// What leaves
// ---------------------------------------------------------------------------

#[test]
fn the_request_that_leaves_carries_a_place_and_its_equipment_and_nothing_else() {
    // The seeded record beside this request holds a demand profile, an
    // availability target, battery chemistry, SOC limits, a diesel price and a
    // project lifetime. None of them has a field to travel in.
    let cache = InMemoryReferenceCache::new();
    let provider = CountingProvider::new(common::reference_bytes("aderm_bere"));
    ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect("the city prepares");

    let request = provider.last_request();
    let encoded = serde_json::to_value(&request).expect("the request serialises");
    let fields: Vec<&String> = encoded
        .as_object()
        .expect("the request is an object")
        .keys()
        .collect();
    assert_eq!(
        fields,
        vec![
            "city",
            "country",
            "latitude",
            "longitude",
            "equipment",
            "losses",
            "capacity_override"
        ]
    );

    // The PV half of technical_params travels — that is the point of the
    // boundary, not an exception to it. The eight DC factors and the AC
    // derate are inputs to the model that owns them.
    let losses = &encoded["losses"];
    assert_eq!(losses["dc"]["soiling_percent"], 5.0);
    assert_eq!(losses["dc"]["wiring_percent"], 2.0);
    assert_eq!(losses["ac_inverter_percent"], 2.0);
    assert_eq!(
        encoded["equipment"]["dc_ac_ratio"],
        ds_solar_contracts::calculation_input::site_pv::ORACLE_DC_AC_RATIO
    );
    assert_eq!(
        encoded["capacity_override"]["target_inverter_capacity_kw"],
        100.0
    );
    assert_eq!(
        encoded["capacity_override"]["target_module_capacity_w"],
        509.0
    );
    let text = serde_json::to_string(&encoded).expect("the request encodes");
    // The other half of technical_params — everything downstream of one unit —
    // has no field to travel in.
    for forbidden in [
        "demand",
        "availability",
        "battery",
        "storage",
        "generator",
        "diesel",
        "soc",
        "chemistry",
        "customers",
        "peak_load",
        "lifetime",
        "degradation",
        "om_cost",
        "noise",
        "safety_margin",
        "technical_params",
    ] {
        assert!(
            !text.to_lowercase().contains(forbidden),
            "the reference request carries '{forbidden}': {text}"
        );
    }
}

// ---------------------------------------------------------------------------
// What a cache will not answer with
// ---------------------------------------------------------------------------

#[test]
fn a_corrupted_cache_entry_reads_as_a_miss_rather_than_a_repair() {
    let directory = tempdir();
    let cache = DirectoryReferenceCache::new(directory.path());
    let key = common::reference_key("aderm_bere");
    cache
        .put(&key, &common::reference_bytes("aderm_bere"))
        .unwrap();
    assert!(cache.get(&key).unwrap().is_some());

    // One flipped byte in the Parquet. There is no repair path.
    let parquet = cache.entry_dir(&key).join("reference-unit.parquet");
    let mut body = std::fs::read(&parquet).expect("the cached profile");
    let middle = body.len() / 2;
    body[middle] ^= 0x01;
    std::fs::write(&parquet, &body).expect("damaging the cached profile");

    assert!(
        cache.get(&key).unwrap().is_none(),
        "a corrupted entry answered as a hit"
    );
}

#[test]
fn a_partially_written_cache_entry_reads_as_a_miss() {
    let directory = tempdir();
    let cache = DirectoryReferenceCache::new(directory.path());
    let key = common::reference_key("aderm_bere");
    cache
        .put(&key, &common::reference_bytes("aderm_bere"))
        .unwrap();

    std::fs::remove_file(cache.entry_dir(&key).join("manifest.json")).expect("removing a member");
    assert!(cache.get(&key).unwrap().is_none());
}

#[test]
fn the_dc_ac_ratio_sent_is_the_oracles_and_the_discrepancy_is_visible() {
    // The Technical Params screen shows 1.2. `_pv_generation.py` hard-codes
    // 1.3 and never reads the field. They have never agreed, and at 1.2 the
    // Béré unit takes 13 strings per inverter instead of 14 — a different
    // plant for every city.
    //
    // This test exists so that switching is a decision somebody makes rather
    // than a line somebody changes.
    let configured = ds_solar_contracts::calculation_input::site_pv::resolve_inputs(
        &common::city_input("aderm_bere"),
    )
    .expect("the fixture city resolves");
    assert_eq!(configured.configured_dc_ac_ratio, 1.2);
    assert_eq!(
        ds_solar_contracts::calculation_input::site_pv::ORACLE_DC_AC_RATIO,
        1.3
    );

    let request = ReferencePreparer::new()
        .request_for(&common::city_input("aderm_bere"))
        .expect("the city yields a request");
    assert_eq!(
        request.equipment.expect("equipment").dc_ac_ratio,
        1.3,
        "preparation must send the ratio the sealed corpus was built with"
    );
}

#[test]
fn a_bundle_produced_under_another_pin_is_refused() {
    let cache = InMemoryReferenceCache::new();
    let provider = CountingProvider::new(common::reference_bytes("aderm_bere"));
    let mut pin = ReferencePin::current();
    pin.provider_dataset = "PVGIS-TMY-2099".to_string();

    let error = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .with_pin(pin)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect_err("a differently pinned bundle is refused");
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
}

#[test]
fn a_provider_answering_for_the_wrong_place_is_refused() {
    let cache = InMemoryReferenceCache::new();
    let provider = CountingProvider::new(common::reference_bytes("aderm_gagal"));
    let error = ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(common::city_input("aderm_bere"), common::PREPARED_AT)
        .expect_err("a bundle for another site is refused");
    assert_eq!(error.code, SolarErrorCode::ReferenceUnitInvalid);
}

#[test]
fn the_directory_cache_survives_a_round_trip_and_stays_byte_identical() {
    let directory = tempdir();
    let cache = DirectoryReferenceCache::new(directory.path());
    let key = common::reference_key("aderm_bere");
    let written = common::reference_bytes("aderm_bere");
    cache.put(&key, &written).unwrap();
    let read = cache.get(&key).unwrap().expect("a cache hit");
    assert_eq!(read, written);
}

/// A scratch directory that removes itself.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> TempDir {
    // Named from the test binary and a monotonic counter rather than a random
    // number: two runs in the same second must not collide, and nothing here
    // may depend on a clock.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ds-solar-preparation-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    TempDir(path)
}

#[test]
fn concurrent_refreshes_never_expose_partial_bundles() {
    let root = std::env::temp_dir().join(format!("solar-cache-concurrent-{}", std::process::id()));
    let cache = DirectoryReferenceCache::new(&root);
    let key = common::reference_key("aderm_bere");
    let bytes = common::reference_bytes("aderm_bere");
    cache.put(&key, &bytes).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let cache = &cache;
            let key = &key;
            let bytes = &bytes;
            scope.spawn(move || {
                for _ in 0..4 {
                    cache.put(key, bytes).unwrap();
                    assert!(cache.get(key).unwrap().is_some());
                }
            });
        }
        for _ in 0..16 {
            assert!(cache.get(&key).unwrap().is_some());
        }
    });
    let generation = cache.entry_dir(&key);
    let mut invalid = bytes.clone();
    invalid.manifest_json = b"{}".to_vec();
    assert!(cache.put(&key, &invalid).is_err());
    assert_eq!(cache.entry_dir(&key), generation);
    assert!(cache.get(&key).unwrap().is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn previous_v3_entries_remain_available_during_upgrade() {
    let root = std::env::temp_dir().join(format!("solar-cache-upgrade-{}", std::process::id()));
    let cache = DirectoryReferenceCache::new(&root);
    let key = common::reference_key("aderm_bere");
    let bytes = common::reference_bytes("aderm_bere");
    let legacy = cache.entry_dir(&key);
    std::fs::create_dir_all(&legacy).unwrap();
    for (name, body) in [
        ("weather.json", &bytes.weather_json),
        ("reference-unit.json", &bytes.reference_unit_json),
        ("reference-unit.parquet", &bytes.reference_unit_parquet),
        ("manifest.json", &bytes.manifest_json),
    ] {
        std::fs::write(legacy.join(name), body).unwrap();
    }
    assert!(cache.get(&key).unwrap().is_some());
    cache.put(&key, &bytes).unwrap();
    assert_ne!(cache.entry_dir(&key), legacy);
    assert!(legacy.join("manifest.json").exists());
    assert!(cache.get(&key).unwrap().is_some());
    std::fs::write(legacy.join("CURRENT"), "x".repeat(4096)).unwrap();
    assert!(cache.get(&key).unwrap().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

/// Captured before preparation moved out of runtime, using the existing CLI
/// against every unchanged frozen fixture. This checks the committed input
/// identity, including the exact normalized network and electrical defaults.
#[test]
fn moving_preparation_preserves_every_frozen_prepared_input_digest() {
    let expected = [
        (
            "aderm_beinamar",
            "sha256:87a1dbd94424e338d598f32b5d40c8c4a330a39e4770c590f6a6c9df49a278b4",
        ),
        (
            "aderm_bere",
            "sha256:c860e7ceed2d3fbcfcd9f59cf3f8e8e3c3823466a61ac9955e914e30ee410a57",
        ),
        (
            "aderm_gagal",
            "sha256:801c62adefcd5d3eaf6dc1db6a02e9a97d612ef56b88c1522ebac9f5d068526d",
        ),
        (
            "aderm_kyabe",
            "sha256:17cb7230c783e57e37841fff22fa55c480cfbb893ef9794ca8e28bd17d8e6f08",
        ),
        (
            "aderm_massenya",
            "sha256:089856fa70c4009c306cc53ea6c4f5a02195ad3a62435dfbd5d3766e5eedce94",
        ),
        (
            "aderm_mbaye",
            "sha256:acd39228fda684e9c0eb7b7ac02c5bcfa8400c740f7bc5f544c6c3fce0872a74",
        ),
        (
            "aderm_moissaia",
            "sha256:2338c72d0a6024ede9a9ae1cb563c83d10d2909ce60c7fe439b2c395709e4f01",
        ),
    ];
    for (city, expected) in expected {
        let cache = InMemoryReferenceCache::new();
        cache
            .put(&common::reference_key(city), &common::reference_bytes(city))
            .unwrap();
        let prepared = ReferencePreparer::new()
            .with_cache(&cache)
            .with_provider(&ForbiddenProvider)
            .prepare(common::city_input(city), common::PREPARED_AT)
            .unwrap();
        assert_eq!(prepared.resolution, BundleResolution::Cache);
        assert_eq!(prepared.input.input_digest, expected, "{city}");
    }
}
