//! Kernel-owned reference acquisition, cache-miss and refresh orchestration.
//!
//! The input page owns this. For each *selected* city it derives the exact
//! reference key from that city's coordinates, local PV request and deployment
//! pin, reads the local cache, makes at most one bounded authenticated call on a miss,
//! caches the four artifacts atomically, and commits one immutable
//! [`PreparedSolarCityInput`].
//!
//! Three consequences are deliberate:
//!
//! * Nothing is downloaded for a city the operator may never run.
//! * The object Run receives is complete — weather *and* the sealed reference
//!   unit — so Run can be, and is, built from types that hold no cache, no
//!   provider and no client.
//! * A cache hit is a **complete skip**. A reference unit is a constant of its
//!   coordinates, local PV request and pins; re-fetching one because a clock moved would be
//!   paying for a network round trip to receive bytes already on disk.
//!   Refreshing is [`Refresh::Overwrite`], an explicit act.

use ds_solar_contracts::calculation_input::prepare_calculation_inputs;
use ds_solar_contracts::{
    BundleBytes, CityInputReadiness, PreparedSolarCityInput, ReferenceKey, ReferencePin,
    SolarCityInput, SolarError, SolarResult,
};

use crate::provider::{ReferenceBundleProvider, ReferenceRequest};
use ds_solar_io::cache::ReferenceCache;
use ds_solar_io::reference::{self, OpenedBundle};

/// Where a resolved bundle came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleResolution {
    /// The versioned local cache answered; no network request was made.
    Cache,
    /// The provider was called and the result was cached.
    Provider,
}

impl BundleResolution {
    /// The wire spelling, for receipts and log lines.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Provider => "provider",
        }
    }
}

/// Whether a cached bundle may answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Refresh {
    /// Use the cache when it holds a verifiable entry. The default, and the
    /// only mode that can run with no network at all.
    #[default]
    UseCache,
    /// Fetch and rewrite the entry even when the cache holds one.
    ///
    /// This is the *only* way to pick up a producer-side pin change — a pvlib
    /// upgrade, a revised catalog row, a different loss profile. None of those
    /// are knowable from the client's side of the boundary, so the refresh is
    /// a deliberate act rather than an inference.
    Overwrite,
}

/// What one preparation produced.
#[derive(Debug, Clone)]
pub struct Preparation {
    /// The committed input.
    pub input: PreparedSolarCityInput,
    /// Where its reference bundle came from.
    pub resolution: BundleResolution,
}

/// Prepares city inputs for the input page.
#[derive(Default)]
pub struct ReferencePreparer<'a> {
    cache: Option<&'a dyn ReferenceCache>,
    provider: Option<&'a dyn ReferenceBundleProvider>,
    pin: Option<ReferencePin>,
    refresh: Refresh,
}

impl<'a> ReferencePreparer<'a> {
    fn policy(&self) -> ds_solar_io::preparation::ReferencePreparer {
        let mut policy = ds_solar_io::preparation::ReferencePreparer::new();
        if let Some(pin) = &self.pin {
            policy = policy.with_pin(pin.clone());
        }
        policy
    }

    /// A preparer with nothing attached. Useless until given at least a cache
    /// or a provider, which is the point: preparation is explicit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the versioned local cache.
    #[must_use]
    pub fn with_cache(mut self, cache: &'a dyn ReferenceCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Attach the provider used on a cache miss.
    #[must_use]
    pub fn with_provider(mut self, provider: &'a dyn ReferenceBundleProvider) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Pin the provider and contract release. Defaults to [`ReferencePin::current`].
    #[must_use]
    pub fn with_pin(mut self, pin: ReferencePin) -> Self {
        self.pin = Some(pin);
        self
    }

    /// Fetch even on a cache hit.
    #[must_use]
    pub fn with_refresh(mut self, refresh: Refresh) -> Self {
        self.refresh = refresh;
        self
    }

    /// The pin in force.
    pub fn pin(&self) -> ReferencePin {
        self.policy().pin()
    }

    /// The exact bundle identity one city needs.
    pub fn reference_key(&self, city: &SolarCityInput) -> SolarResult<ReferenceKey> {
        self.policy().reference_key(city)
    }

    fn reference_key_for_request(&self, request: &ReferenceRequest) -> SolarResult<ReferenceKey> {
        self.policy().reference_key_for_request(request)
    }

    /// Whether this city could be run right now without any network access.
    ///
    /// The input page calls this to paint readiness without committing to work.
    pub fn readiness(&self, city: &SolarCityInput) -> CityInputReadiness {
        // Readiness must apply the same local, deterministic preparation as
        // `prepare`: a cached reference unit is not enough when the governed
        // network cannot be expanded into the material quantities a portfolio
        // needs. Report that input refusal now, before painting the city as a
        // bundle away from ready.
        let city = match prepare_calculation_inputs(city.clone()) {
            Ok(city) => city,
            Err(error) => return CityInputReadiness::NotReady { error },
        };
        let request = match self.request_for(&city) {
            Ok(request) => request,
            Err(error) => return CityInputReadiness::NotReady { error },
        };
        let key = match self.reference_key_for_request(&request) {
            Ok(key) => key,
            Err(error) => return CityInputReadiness::NotReady { error },
        };
        let cached = self
            .cache
            .and_then(|cache| cache.get(&key).ok().flatten())
            .and_then(|artifacts| reference::open(&artifacts).ok());
        match cached {
            Some(opened) => match self.check_bundle(&opened, &key, &request) {
                Ok(()) => CityInputReadiness::Preparing {
                    detail: format!(
                        "reference bundle {} is cached; committing input",
                        key.cache_entry()
                    ),
                },
                Err(error) => CityInputReadiness::NotReady { error },
            },
            None => CityInputReadiness::NotReady {
                error: SolarError::reference_unit_invalid(format!(
                    "reference bundle {} is not cached for {}",
                    key.cache_entry(),
                    city.identity.city_id
                )),
            },
        }
    }

    /// The request this city sends.
    ///
    /// Four fields that name a place, plus the **PV half of
    /// `technical_params`**: the DC/AC ratio the strings are sized against,
    /// the eight DC loss factors, the AC inverter derate, and the explicit
    /// scaling the configured capacities require.
    ///
    /// This is the whole of what leaves for `ds-solar-weather`. The seeded
    /// record next to it holds demand profiles, availability targets, battery
    /// chemistry, SOC limits, generator behaviour, degradation, O&M rates and
    /// a project lifetime, and not one of them has a field to travel in —
    /// they are this crate's, downstream of the unit.
    ///
    /// The DC/AC ratio sent is [`ORACLE_DC_AC_RATIO`], not the configured
    /// `pv_system.inverter.dc_ac_ratio`; read that constant before changing
    /// it, because the two have never agreed and the difference re-sizes every
    /// plant.
    ///
    /// [`ORACLE_DC_AC_RATIO`]: ds_solar_contracts::calculation_input::site_pv::ORACLE_DC_AC_RATIO
    pub fn request_for(&self, city: &SolarCityInput) -> SolarResult<ReferenceRequest> {
        self.policy().request_for(city)
    }

    /// Prepare one city.
    ///
    /// Resolution order is fixed and observable:
    ///
    /// 1. read the versioned local cache (unless [`Refresh::Overwrite`]);
    /// 2. a present, verifiable bundle ends it — no network request at all;
    /// 3. otherwise fetch once, verify, cache atomically;
    /// 4. with no provider and no cache entry, fail `reference_unit_invalid`.
    pub fn prepare(
        &self,
        city: SolarCityInput,
        prepared_at: impl Into<String>,
    ) -> SolarResult<Preparation> {
        // This is the last mutable boundary before the immutable prepared
        // input is committed. Material quantities derived from the governed
        // proposed network therefore belong here, not in report assembly
        // after the calculation already ran.
        let city = prepare_calculation_inputs(city)?;
        let request = self.request_for(&city)?;
        let key = self.reference_key_for_request(&request)?;

        // A cache entry that cannot be read, opened, or verified is a MISS,
        // not a preparation failure: the cache is a copy, and the provider can
        // replace it. Failing here made every city read as "weather not
        // prepared" after a pin change or a damaged cache file, even though
        // the bundle was one fetch away. The failure is kept only for the
        // no-provider refusal, so the operator learns the real cause.
        let mut cache_failure: Option<SolarError> = None;
        if self.refresh == Refresh::UseCache
            && let Some(cache) = self.cache
        {
            match self.open_cached(cache, &key, &request) {
                Ok(Some(opened)) => {
                    return Ok(Preparation {
                        input: self.commit(city, opened, prepared_at)?,
                        resolution: BundleResolution::Cache,
                    });
                }
                Ok(None) => {}
                Err(error) => cache_failure = Some(error),
            }
        }

        let Some(provider) = self.provider else {
            return Err(match cache_failure {
                Some(error) => SolarError::reference_unit_invalid(format!(
                    "reference bundle {} is cached but unusable ({}) and no provider is available to replace it; use solar.reference.plan and solar.reference.import with an explicitly sealed local bundle",
                    key.cache_entry(),
                    error.message
                )),
                None => SolarError::reference_unit_invalid(format!(
                    "reference bundle {} is not cached and no provider is available; use solar.reference.plan and solar.reference.import with an explicitly sealed local bundle",
                    key.cache_entry()
                )),
            });
        };

        let artifacts: BundleBytes = provider.fetch(&request)?;
        let opened = reference::open(&artifacts)?;
        self.check_bundle(&opened, &key, &request)?;
        if let Some(cache) = self.cache {
            cache.put(&key, &artifacts)?;
        }
        Ok(Preparation {
            input: self.commit(city, opened, prepared_at)?,
            resolution: BundleResolution::Provider,
        })
    }

    /// Open and verify one cached bundle. `Ok(None)` is an ordinary miss;
    /// `Err` is a present-but-unusable entry, which the caller treats as a
    /// miss whenever a provider can replace it.
    fn open_cached(
        &self,
        cache: &dyn ReferenceCache,
        key: &ReferenceKey,
        request: &ReferenceRequest,
    ) -> SolarResult<Option<OpenedBundle>> {
        let Some(artifacts) = cache.get(key)? else {
            return Ok(None);
        };
        let opened = reference::open(&artifacts)?;
        self.check_bundle(&opened, key, request)?;
        Ok(Some(opened))
    }

    fn commit(
        &self,
        city: SolarCityInput,
        opened: OpenedBundle,
        prepared_at: impl Into<String>,
    ) -> SolarResult<PreparedSolarCityInput> {
        self.policy().commit(city, opened, prepared_at)
    }

    fn check_bundle(
        &self,
        opened: &OpenedBundle,
        key: &ReferenceKey,
        request: &ReferenceRequest,
    ) -> SolarResult<()> {
        self.policy().check_bundle(opened, key, request)
    }
}
