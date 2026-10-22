//! Application access to the same vocabulary consumed by Network cell authority.
pub use ds_network_contracts::cell_authority::{
    design_cell_lock_vocabulary, validate_vocabulary, DesignCellLockVocabulary, VocabularyError,
};

/// Owning decoder for serialized native/remote exchanges.
pub fn decode_vocabulary(bytes: &[u8]) -> Result<DesignCellLockVocabulary, VocabularyError> {
    validate_vocabulary(
        serde_json::from_slice(bytes).map_err(|error| VocabularyError(error.to_string()))?,
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = designCellLockVocabulary)]
    pub fn vocabulary() -> Result<tsify::Ts<DesignCellLockVocabulary>, JsError> {
        design_cell_lock_vocabulary()
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decodeDesignCellLockVocabulary)]
    pub fn decode(
        value: tsify::Ts<DesignCellLockVocabulary>,
    ) -> Result<tsify::Ts<DesignCellLockVocabulary>, JsError> {
        let value = value
            .to_rust()
            .map_err(|error| JsError::new(&VocabularyError(error.to_string()).to_string()))?;
        validate_vocabulary(value)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
