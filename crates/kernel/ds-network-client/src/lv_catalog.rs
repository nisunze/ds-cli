//! The LV processor catalogue of the captured engine, read once per capture.
//! Core compiles no catalogue: presets resolve over this answer and every LV
//! process request is admitted against it before anything is staged.
use super::*;
use ds_network_contracts::{
    grid_operation::GridOperationV1,
    lv_processor_catalog::{
        LvProcessorCatalogResultV1, LvProcessorOffer, LvProcessorRefusal,
        LV_PROCESSOR_CATALOG_SCHEMA_V1, MAX_LV_PROCESSOR_CATALOG_BYTES,
    },
};

/// What the captured engine offers an LV process request.
#[derive(Debug)]
pub struct LvProcessorCatalog {
    catalog: serde_json::Value,
    offer: LvProcessorOffer,
}

impl LvProcessorCatalog {
    /// The engine's editor catalogue exactly as it answered it.
    pub fn catalog(&self) -> &serde_json::Value {
        &self.catalog
    }
    pub fn offer(&self) -> &LvProcessorOffer {
        &self.offer
    }
}

/// A refusal of the engine's offer, under its own stable code.
pub(crate) fn offer_failure(error: LvProcessorRefusal) -> NetworkFailure {
    let code = match error.code() {
        "lv_process_setting_not_offered" => "lv_process_setting_not_offered",
        "lv_process_preset_not_offered" => "lv_process_preset_not_offered",
        _ => "lv_processor_catalog_invalid",
    };
    NetworkFailure::new(code, error.to_string())
}

impl CapturedNetworkClient {
    /// The captured engine's LV processor catalogue. An engine that does not
    /// advertise `lv_processor_catalog_v1` is refused before any staging.
    pub fn lv_processor_catalog(
        &self,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<&LvProcessorCatalog, NetworkFailure> {
        if let Some(catalogue) = self.lv_catalogue.get() {
            return Ok(catalogue);
        }
        let call = GridOperationV1::LvProcessorCatalog;
        if !self.identity.capabilities.contains(&call.capability()) {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "the captured Network engine publishes no LV processor catalogue \
                 (lv_processor_catalog_v1), so no LV process request can be admitted; \
                 install a Network engine that offers it",
            ));
        }
        let outcome = self.grid_operation(&call, None, &[], scratch, cancel)?;
        let bytes = serde_json::to_vec(&outcome.result.result)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        if bytes.len() > MAX_LV_PROCESSOR_CATALOG_BYTES || !outcome.artifacts.is_empty() {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "the LV processor catalogue exceeds its bound or carries artifacts",
            ));
        }
        let answer: LvProcessorCatalogResultV1 = serde_json::from_slice(&bytes)
            .map_err(|e| NetworkFailure::new("network_result_invalid", e.to_string()))?;
        if !same_family(&answer.schema, LV_PROCESSOR_CATALOG_SCHEMA_V1) {
            return Err(NetworkFailure::new(
                "network_result_invalid",
                "the LV processor catalogue answer names another document",
            ));
        }
        let offer = LvProcessorOffer::from_catalog(&answer.catalog).map_err(offer_failure)?;
        Ok(self.lv_catalogue.get_or_init(|| LvProcessorCatalog {
            catalog: answer.catalog,
            offer,
        }))
    }
}
