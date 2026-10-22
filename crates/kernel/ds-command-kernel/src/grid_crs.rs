//! Application entry for exact projected model CRS admission and coordinates.
//! The neutral leaf owns the types, support policy and projection implementation.
pub use ds_geo_lite::model_crs::{GridModelCrsRequest, GridModelCrsResponse, evaluate};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = gridModelCrs)]
    pub fn grid_model_crs(
        request: tsify::Ts<GridModelCrsRequest>,
    ) -> Result<tsify::Ts<GridModelCrsResponse>, JsError> {
        let request = request
            .to_rust()
            .map_err(|error| JsError::new(&error.to_string()))?;
        let response = evaluate(request).map_err(|error| JsError::new(&error.to_string()))?;
        response
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
