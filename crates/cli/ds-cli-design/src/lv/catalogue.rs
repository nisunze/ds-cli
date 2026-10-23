//! The installed Network engine's LV processor catalogue: the one source of
//! the settings and presets an LV process may carry. Core compiles no copy;
//! it asks the engine through its typed `lv_processor_catalog` task.
use ds_cli_contract::Failure;
use ds_cli_contract::spec::Refusal;
use ds_network_contracts::lv_processor_catalog::LvProcessorOffer;
use serde_json::Value;

pub(crate) const PRESET_NOT_OFFERED: Refusal = Refusal {
    code: "lv_process_preset_not_offered",
    when: "the installed Network engine's LV processor catalogue does not offer the requested preset",
    remedy: "choose a preset `ds design process settings --catalog` lists, or install a Network engine that offers it",
};
pub(crate) const SETTING_NOT_OFFERED: Refusal = Refusal {
    code: "lv_process_setting_not_offered",
    when: "a job carries a setting the installed Network engine's LV processor catalogue does not offer, or a value of another kind",
    remedy: "drop or correct that setting (`ds design process settings --catalog` lists what the installed engine offers), or install a Network engine that offers it",
};
pub(crate) const UPDATE_REQUIRED: Refusal = Refusal {
    code: "network_engine_update_required",
    when: "a preset must be resolved and the installed Network engine predates its LV processor catalogue (lv_processor_catalog_v1)",
    remedy: "update the Network engine to a release that offers its LV processor catalogue",
};
pub(crate) const CATALOGUE_INVALID: Refusal = Refusal {
    code: "lv_processor_catalog_invalid",
    when: "the installed Network engine answers a malformed LV processor catalogue",
    remedy: "install an intact compatible Network engine",
};

/// The catalogue and offer of the engine of `lane`, or of the packaged
/// engine when the command names no lane.
pub(crate) fn engine_catalogue(lane: Option<&str>) -> Result<(Value, LvProcessorOffer), Failure> {
    let cancel = || false;
    let client = match lane {
        Some(lane) => ds_network_client::CapturedNetworkClient::capture_for_lane(lane, 1, &cancel),
        None => ds_network_client::CapturedNetworkClient::capture(1, &cancel),
    }
    .map_err(super::process::map_network_error)?;
    let stage =
        ds_network_client::private_disk_stage().map_err(super::process::map_network_error)?;
    let catalogue = client
        .lv_processor_catalog(stage.path(), &cancel)
        .map_err(super::process::map_network_error)?;
    Ok((catalogue.catalog().clone(), catalogue.offer().clone()))
}

/// Refuse a preset the engine does not offer, naming the ones it does.
pub(crate) fn admit_preset(offer: &LvProcessorOffer, preset: &str) -> Result<(), Failure> {
    offer.admit_preset(preset).map_err(|error| {
        Failure::invalid(
            error.code(),
            format!(
                "{error}; it offers: {}",
                offer.presets().collect::<Vec<_>>().join(", ")
            ),
        )
        .remedy(PRESET_NOT_OFFERED.remedy)
    })
}
