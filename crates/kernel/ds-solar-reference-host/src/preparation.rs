//! Kernel-owned reference acquisition, cache-miss and refresh orchestration.
//!
//! The independently selected Solar addon owns request planning and admission
//! of raw bundle candidates into complete prepared inputs. The host owns one
//! cache read, at most one provider read on a miss, and atomic byte storage.
//! A candidate cache entry alone never establishes readiness.

use std::sync::Arc;

use ds_solar_contracts::{
    CityInputReadiness, PreparedSolarCityInput, ReferenceKey, ReferencePin, SolarCityInput,
    SolarError, SolarErrorCode, SolarResult,
};
use ds_solar_host::{
    cache::ReferenceCache,
    preparation::{AddonPreparation, PreparationTasks, ReferencePlan},
};

use crate::provider::{ReferenceBundleProvider, ReferenceRequest};

/// Where a resolved bundle came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleResolution {
    /// The versioned local cache answered; no network request was made.
    Cache,
    /// The provider was called and the admitted candidate was cached.
    Provider,
}

impl BundleResolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Provider => "provider",
        }
    }
}

/// Whether a cached bundle may answer. Time never changes this choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Refresh {
    #[default]
    UseCache,
    /// Fetch and rewrite the candidate even when the cache holds one.
    Overwrite,
}

#[derive(Debug, Clone)]
pub struct Preparation {
    pub input: PreparedSolarCityInput,
    pub resolution: BundleResolution,
}

/// Preparation owns acquisition; the explicitly retained addon owns policy.
pub struct ReferencePreparer<'a> {
    cache: Option<&'a dyn ReferenceCache>,
    provider: Option<&'a dyn ReferenceBundleProvider>,
    pin: Option<ReferencePin>,
    refresh: Refresh,
    tasks: Arc<dyn PreparationTasks>,
}

impl Default for ReferencePreparer<'_> {
    fn default() -> Self {
        Self {
            cache: None,
            provider: None,
            pin: None,
            refresh: Refresh::UseCache,
            // Construction captures no executable and starts no identity probe.
            tasks: Arc::new(AddonPreparation::installed()),
        }
    }
}

impl<'a> ReferencePreparer<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_cache(mut self, cache: &'a dyn ReferenceCache) -> Self {
        self.cache = Some(cache);
        self
    }

    #[must_use]
    pub fn with_provider(mut self, provider: &'a dyn ReferenceBundleProvider) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Retain the caller's lane/context-scoped preparation owner through intake.
    #[must_use]
    pub fn with_tasks(mut self, tasks: Arc<dyn PreparationTasks>) -> Self {
        self.tasks = tasks;
        self
    }

    #[must_use]
    pub fn with_pin(mut self, pin: ReferencePin) -> Self {
        self.pin = Some(pin);
        self
    }

    #[must_use]
    pub fn with_refresh(mut self, refresh: Refresh) -> Self {
        self.refresh = refresh;
        self
    }

    pub fn pin(&self) -> ReferencePin {
        self.pin.clone().unwrap_or_else(ReferencePin::current)
    }

    /// The addon admits and normalizes this exact city and computes its key.
    pub fn reference_key(&self, city: &SolarCityInput) -> SolarResult<ReferenceKey> {
        self.tasks.plan(city, self.pin()).map(|plan| plan.key)
    }

    /// Whether the held candidate can be admitted without acquisition.
    pub fn readiness(&self, city: &SolarCityInput) -> CityInputReadiness {
        let pin = self.pin();
        let plan = match self.tasks.plan(city, pin.clone()) {
            Ok(plan) => plan,
            Err(error) => return CityInputReadiness::NotReady { error },
        };
        let cached = match self.cache {
            Some(cache) => cache.get(&plan.key),
            None => Ok(None),
        };
        match cached {
            Ok(Some(bundle)) => {
                // This value is discarded, never persisted. The same complete
                // admission must succeed before an input-page Preparing state.
                match self
                    .tasks
                    .prepare(plan.city, pin, &bundle, "1970-01-01T00:00:00Z")
                {
                    Ok(_) => CityInputReadiness::Preparing {
                        detail: format!(
                            "reference bundle {} is cached; committing input",
                            plan.key.cache_entry()
                        ),
                    },
                    Err(error) => CityInputReadiness::NotReady { error },
                }
            }
            Err(error) => CityInputReadiness::NotReady { error },
            Ok(None) => CityInputReadiness::NotReady {
                error: SolarError::reference_unit_invalid(format!(
                    "reference bundle {} is not cached for {}",
                    plan.key.cache_entry(),
                    plan.city.identity.city_id
                )),
            },
        }
    }

    /// The addon's exact site/equipment request; the host does not derive it.
    pub fn request_for(&self, city: &SolarCityInput) -> SolarResult<ReferenceRequest> {
        self.tasks.plan(city, self.pin()).map(|plan| plan.request)
    }

    /// Resolve one candidate, have the addon admit it, then store its bytes.
    /// A corrupt candidate is replaceable; missing capability and cancellation
    /// are terminal and must never turn into a cache miss or a provider call.
    pub fn prepare(
        &self,
        city: SolarCityInput,
        prepared_at: impl Into<String>,
    ) -> SolarResult<Preparation> {
        let pin = self.pin();
        let plan = self.tasks.plan(&city, pin.clone())?;
        let prepared_at = prepared_at.into();
        let mut cache_failure: Option<SolarError> = None;
        if self.refresh == Refresh::UseCache
            && let Some(cache) = self.cache
        {
            match self.prepare_cached(cache, &plan, &pin, &prepared_at) {
                Ok(Some(input)) => {
                    return Ok(Preparation {
                        input,
                        resolution: BundleResolution::Cache,
                    });
                }
                Ok(None) => {}
                Err(error)
                    if matches!(
                        error.code,
                        SolarErrorCode::SolarEngineUpdateRequired
                            | SolarErrorCode::Cancelled
                            | SolarErrorCode::ArtifactFailed
                    ) =>
                {
                    return Err(error);
                }
                Err(error) => cache_failure = Some(error),
            }
        }

        let Some(provider) = self.provider else {
            return Err(match cache_failure {
                Some(error) => SolarError::reference_unit_invalid(format!(
                    "reference bundle {} is cached but unusable ({}) and no provider is available to replace it; use solar.reference.plan and solar.reference.import with an explicitly sealed local bundle",
                    plan.key.cache_entry(),
                    error.message
                )),
                None => SolarError::reference_unit_invalid(format!(
                    "reference bundle {} is not cached and no provider is available; use solar.reference.plan and solar.reference.import with an explicitly sealed local bundle",
                    plan.key.cache_entry()
                )),
            });
        };

        let bundle = provider.fetch(&plan.request)?;
        let input = self.tasks.prepare(plan.city, pin, &bundle, &prepared_at)?;
        if let Some(cache) = self.cache {
            cache.put(&plan.key, &bundle)?;
        }
        Ok(Preparation {
            input,
            resolution: BundleResolution::Provider,
        })
    }

    fn prepare_cached(
        &self,
        cache: &dyn ReferenceCache,
        plan: &ReferencePlan,
        pin: &ReferencePin,
        prepared_at: &str,
    ) -> SolarResult<Option<PreparedSolarCityInput>> {
        let Some(bundle) = cache.get(&plan.key)? else {
            return Ok(None);
        };
        self.tasks
            .prepare(plan.city.clone(), pin.clone(), &bundle, prepared_at)
            .map(Some)
    }
}
