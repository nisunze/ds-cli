//! Application entry for the shared single-cell domain rules. The page's
//! editors answer these synchronously while tables render, so the rules are
//! the neutral ds-upload-rules crate (no engine, project acquisition or
//! application state is needed to interpret a value).
pub use ds_upload_rules::domain_value::{
    evaluate, validate_response, DomainValueError, DomainValueInput, DomainValueResponse,
};
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = designDomainValue)]
    pub fn domain_value(
        request: tsify::Ts<DomainValueInput>,
    ) -> Result<tsify::Ts<DomainValueResponse>, JsError> {
        let request = request.to_rust().map_err(|error| {
            JsError::new(&DomainValueError::InvalidRequest(error.to_string()).to_string())
        })?;
        evaluate(request)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decodeDesignDomainValue)]
    pub fn decode_domain_value(
        request: tsify::Ts<DomainValueInput>,
        response: tsify::Ts<DomainValueResponse>,
    ) -> Result<tsify::Ts<DomainValueResponse>, JsError> {
        let request = request.to_rust().map_err(|error| {
            JsError::new(&DomainValueError::InvalidRequest(error.to_string()).to_string())
        })?;
        let response = response.to_rust().map_err(|error| {
            JsError::new(&DomainValueError::InvalidResponse(error.to_string()).to_string())
        })?;
        validate_response(&request, response)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
