//! Selection field hints. The profile is computed by the Network unit
//! (`selectionFieldInfo` through the grid unit envelope); the query, profile
//! and their checks are the Network contract, and this module admits the
//! unit's answer in the page.
pub use ds_network_contracts::table_query::{
    decode_selection_field_profile, validate_selection_field_profile, SelectionFieldError,
    SelectionFieldProfile, SelectionFieldQuery,
};

/// The Network unit operation that profiles one selection field.
pub const UNIT_METHOD: &str = "selectionFieldInfo";

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = decodeSelectionFieldProfile)]
    pub fn decode_profile(
        query: tsify::Ts<SelectionFieldQuery>,
        profile: tsify::Ts<SelectionFieldProfile>,
    ) -> Result<tsify::Ts<SelectionFieldProfile>, JsError> {
        let query = query.to_rust().map_err(|error| {
            JsError::new(&SelectionFieldError::InvalidRequest(error.to_string()).to_string())
        })?;
        let profile = profile.to_rust().map_err(|error| {
            JsError::new(&SelectionFieldError::InvalidResponse(error.to_string()).to_string())
        })?;
        validate_selection_field_profile(&query, profile)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
