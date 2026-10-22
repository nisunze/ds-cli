//! Reference-bundle providers.
//!
//! Provider contracts live here; HTTP acquisition belongs to the application. The
//! execution runtime holds no provider at all, which is what makes "Run
//! performs no network call" a property of the types rather than a rule
//! someone has to remember.
//!
//! There is no weather provider any more, because there is no weather
//! endpoint. `ds-solar-weather` publishes one thing — a sealed reference
//! bundle — and weather leaves that service *inside* it, digest-bound to the
//! simulation computed from it. A client that could fetch weather separately
//! could pair it with a reference unit computed from different weather, which
//! is the failure the bundle exists to make impossible.

use ds_solar_contracts::{BundleBytes, ReferenceKey, ReferenceUnit, SolarError, SolarResult};

pub use ds_solar_contracts::reference_request::{
    CapacityOverride, DcLossRequest, LossRequest, ReferenceEquipmentRequest, ReferenceRequest,
};

/// Something that can retrieve a sealed reference bundle.
pub trait ReferenceBundleProvider: Send + Sync {
    /// Retrieve one bundle's four artifacts. The caller verifies them.
    fn fetch(&self, request: &ReferenceRequest) -> SolarResult<BundleBytes>;
}

/// A provider backed by bundles held in memory. Used by tests and by the CLI
/// when it is pointed at a fixture directory.
#[derive(Debug, Default)]
pub struct FixtureReferenceProvider {
    bundles: Vec<(ReferenceKey, BundleBytes)>,
}

impl FixtureReferenceProvider {
    /// A provider serving exactly these bundles, keyed by the coordinates each
    /// one was sealed for.
    pub fn new(
        bundles: Vec<BundleBytes>,
        pin: ds_solar_contracts::ReferencePin,
    ) -> SolarResult<Self> {
        let mut keyed = Vec::with_capacity(bundles.len());
        for artifacts in bundles {
            let receipt: ReferenceUnit = serde_json::from_slice(&artifacts.reference_unit_json)
                .map_err(|err| {
                    SolarError::reference_unit_invalid(format!(
                        "a fixture bundle carries no readable receipt: {err}"
                    ))
                })?;
            keyed.push((ReferenceKey::of_unit(&receipt, pin.clone())?, artifacts));
        }
        Ok(Self { bundles: keyed })
    }
}

impl ReferenceBundleProvider for FixtureReferenceProvider {
    fn fetch(&self, request: &ReferenceRequest) -> SolarResult<BundleBytes> {
        let pin = self
            .bundles
            .first()
            .map(|(key, _)| key.pin.clone())
            .unwrap_or_else(ds_solar_contracts::ReferencePin::current);
        let wanted = ReferenceKey::new(request.latitude, request.longitude, pin)?;
        self.bundles
            .iter()
            .find(|(key, _)| {
                key.latitude_micro_deg == wanted.latitude_micro_deg
                    && key.longitude_micro_deg == wanted.longitude_micro_deg
            })
            .map(|(_, artifacts)| artifacts.clone())
            .ok_or_else(|| {
                SolarError::weather_unavailable(format!(
                    "no fixture reference bundle for {}",
                    wanted.cache_entry()
                ))
            })
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
