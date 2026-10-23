//! Pure Solar reference request/key admission and preparation of held bundle bytes.
//! No provider, cache or acquisition capability can be attached to this type.
use crate::reference::{self, OpenedBundle};
use ds_solar_contracts::calculation_input::prepare_calculation_inputs;
use ds_solar_contracts::reference_request::{
    CapacityOverride, DcLossRequest, LossRequest, ReferenceEquipmentRequest, ReferenceRequest,
};
use ds_solar_contracts::{
    BundleBytes, PreparedSolarCityInput, ReferenceKey, ReferencePin, SolarCityInput, SolarError,
    SolarResult, WeatherKey, WeatherPin,
};

#[derive(Default)]
pub struct ReferencePreparer {
    pin: Option<ReferencePin>,
}
impl ReferencePreparer {
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn with_pin(mut self, pin: ReferencePin) -> Self {
        self.pin = Some(pin);
        self
    }

    pub fn pin(&self) -> ReferencePin {
        self.pin.clone().unwrap_or_else(ReferencePin::current)
    }

    pub fn reference_key(&self, city: &SolarCityInput) -> SolarResult<ReferenceKey> {
        self.reference_key_for_request(&self.request_for(city)?)
    }

    pub fn reference_key_for_request(
        &self,
        request: &ReferenceRequest,
    ) -> SolarResult<ReferenceKey> {
        ReferenceKey::with_request_digest(
            request.latitude,
            request.longitude,
            self.pin(),
            request.cache_request_digest()?,
        )
    }

    pub fn request_for(&self, city: &SolarCityInput) -> SolarResult<ReferenceRequest> {
        use ds_solar_contracts::calculation_input::site_pv::ORACLE_DC_AC_RATIO;

        let configured = ds_solar_contracts::calculation_input::site_pv::resolve_inputs(city)?;
        let [
            soiling,
            shading,
            snow,
            mismatch,
            wiring,
            connections,
            lid,
            nameplate,
        ] = configured.dc_losses;
        Ok(ReferenceRequest::new(
            city.site.city.clone(),
            city.site.country.clone(),
            city.site.latitude,
            city.site.longitude,
        )
        .with_equipment(ReferenceEquipmentRequest {
            dc_ac_ratio: ORACLE_DC_AC_RATIO,
        })
        .with_losses(LossRequest {
            dc: DcLossRequest {
                soiling_percent: soiling,
                shading_percent: shading,
                snow_percent: snow,
                mismatch_percent: mismatch,
                wiring_percent: wiring,
                connections_percent: connections,
                lid_percent: lid,
                nameplate_percent: nameplate,
            },
            ac_inverter_percent: configured.ac_inverter_percent,
        })
        .with_capacity_override(CapacityOverride {
            target_inverter_capacity_kw: configured.configured_inverter_kw,
            target_module_capacity_w: configured.configured_module_w,
            // This is an engineering basis for capacity scaling, not a city
            // label.  Keeping it stable lets two project records for the
            // same site and PV configuration share one sealed reference
            // bundle; the city identity remains in the prepared input and
            // downstream result provenance.
            reason: "configured plant equipment capacity".to_string(),
        }))
    }

    pub fn commit(
        &self,
        city: SolarCityInput,
        opened: OpenedBundle,
        prepared_at: impl Into<String>,
    ) -> SolarResult<PreparedSolarCityInput> {
        // The weather key is derived from the *sealed dataset*, not from the
        // seeded record's idea of the site's timezone: the zone is derived by
        // the service that owns the numbers, and the tz database periodically
        // renames zones without moving an instant.
        let weather_key = WeatherKey::new(
            city.site.latitude,
            city.site.longitude,
            opened.weather.provenance.timezone.clone(),
            WeatherPin {
                provider: opened.weather.provenance.provider.clone(),
                provider_dataset: opened.weather.provenance.provider_dataset.clone(),
                schema_version: opened.weather.schema_version.clone(),
                dataset_kind: opened.weather.dataset_kind,
            },
        )?;
        PreparedSolarCityInput::commit(
            city,
            weather_key,
            opened.weather,
            opened.reference_unit,
            prepared_at,
        )
    }

    pub fn check_bundle(
        &self,
        opened: &OpenedBundle,
        key: &ReferenceKey,
        request: &ReferenceRequest,
    ) -> SolarResult<()> {
        if !self.pin().matches(&opened.reference_unit.receipt) {
            let receipt = &opened.reference_unit.receipt;
            return Err(SolarError::reference_unit_invalid(format!(
                "reference bundle {} was produced under {}/{}/{} but this deployment is pinned to \
                 {}/{}/{}",
                key.cache_entry(),
                receipt.weather.provider,
                receipt.weather.provider_dataset,
                receipt.schema_version,
                self.pin().provider,
                self.pin().provider_dataset,
                self.pin().reference_schema_version,
            )));
        }
        let answered = ReferenceKey::of_unit(&opened.reference_unit.receipt, self.pin())?;
        if !answered.same_site_and_pin(key) {
            return Err(SolarError::reference_unit_invalid(format!(
                "reference bundle {} was sealed for {} rather than the requested site",
                key.cache_entry(),
                answered.cache_entry(),
            )));
        }
        receipt_matches_request(&opened.reference_unit.receipt, request)
    }
    /// Validate explicit acquired bytes and commit the immutable calculation input.
    pub fn prepare(
        &self,
        city: SolarCityInput,
        artifacts: &BundleBytes,
        prepared_at: impl Into<String>,
    ) -> SolarResult<PreparedSolarCityInput> {
        let city = prepare_calculation_inputs(city)?;
        let request = self.request_for(&city)?;
        let key = self.reference_key_for_request(&request)?;
        let opened = reference::open(artifacts)?;
        self.check_bundle(&opened, &key, &request)?;
        self.commit(city, opened, prepared_at)
    }
}

/// Defence in depth for the request-aware cache. A cache key prevents normal
/// aliases; this check makes a misplaced or maliciously copied entry a typed
/// refusal rather than an arithmetically plausible calculation.
fn receipt_matches_request(
    receipt: &ds_solar_contracts::ReferenceUnit,
    request: &ReferenceRequest,
) -> SolarResult<()> {
    if let Some(equipment) = &request.equipment
        && !same_number(receipt.equipment.dc_ac_ratio, equipment.dc_ac_ratio)
    {
        return Err(SolarError::reference_unit_invalid(
            "reference bundle DC/AC ratio does not match the requested configuration",
        ));
    }

    if let Some(losses) = &request.losses {
        let actual = &receipt.losses;
        let expected = &losses.dc;
        let matches = [
            same_number(actual.dc.soiling_percent, expected.soiling_percent),
            same_number(actual.dc.shading_percent, expected.shading_percent),
            same_number(actual.dc.snow_percent, expected.snow_percent),
            same_number(actual.dc.mismatch_percent, expected.mismatch_percent),
            same_number(actual.dc.wiring_percent, expected.wiring_percent),
            same_number(actual.dc.connections_percent, expected.connections_percent),
            same_number(actual.dc.lid_percent, expected.lid_percent),
            same_number(actual.dc.nameplate_percent, expected.nameplate_percent),
            same_number(actual.ac_inverter_percent, losses.ac_inverter_percent),
        ];
        if matches.iter().any(|matches| !matches) {
            return Err(SolarError::reference_unit_invalid(
                "reference bundle losses do not match the requested configuration",
            ));
        }
    }

    match &request.capacity_override {
        Some(override_) => {
            let scaling = &receipt.capacity_scaling;
            if scaling.mode != "compatibility_override"
                || !same_number(
                    scaling.target_inverter_capacity_kw,
                    override_.target_inverter_capacity_kw,
                )
                || !same_number(
                    scaling.target_module_capacity_w,
                    override_.target_module_capacity_w,
                )
            {
                return Err(SolarError::reference_unit_invalid(
                    "reference bundle capacity scaling does not match the requested configuration",
                ));
            }
        }
        None if receipt.capacity_scaling.mode != "reference_equipment" => {
            return Err(SolarError::reference_unit_invalid(
                "reference bundle has capacity scaling that the request did not ask for",
            ));
        }
        None => {}
    }

    Ok(())
}

fn same_number(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
}
