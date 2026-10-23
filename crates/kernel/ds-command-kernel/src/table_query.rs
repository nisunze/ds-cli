//! Read-only table queries over held rows. Filtering, sorting and column
//! profiles run in the Network unit (`tableFilter`, `tableProfiles` through
//! the grid unit envelope); the queries, answers and their checks are the
//! Network contract, and this module admits the unit's answers against the
//! chunks the page holds.
pub use ds_network_contracts::table_query::{
    validate_filter_result, validate_profiles_result, TableFilterMode, TableFilterQuery,
    TableFilterResult, TableProfilesQuery, TableProfilesResult, TableQueryError,
};

/// The Network unit operations that filter and profile held rows.
pub const FILTER_METHOD: &str = "tableFilter";
pub const PROFILES_METHOD: &str = "tableProfiles";

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::{prelude::*, JsCast};

    fn request_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&TableQueryError::InvalidRequest(error.to_string()).to_string())
    }
    fn response_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&TableQueryError::InvalidResponse(error.to_string()).to_string())
    }
    fn row_count(chunks: &[js_sys::Uint8Array]) -> Result<usize, JsError> {
        let mut count = 0usize;
        for (index, chunk) in chunks.iter().enumerate() {
            if !chunk.is_instance_of::<js_sys::Uint8Array>() {
                return Err(request_error(format!("chunk {index} must be Uint8Array")));
            }
            for batch in ds_columnar::ipc_to_batches(&chunk.to_vec()).map_err(request_error)? {
                count = count
                    .checked_add(batch.num_rows())
                    .ok_or_else(|| request_error("row count overflow"))?;
            }
        }
        Ok(count)
    }

    #[wasm_bindgen(js_name = decodeTableFilterResult)]
    pub fn decode_filter(
        chunks: Vec<js_sys::Uint8Array>,
        result: tsify::Ts<TableFilterResult>,
    ) -> Result<tsify::Ts<TableFilterResult>, JsError> {
        let result = result.to_rust().map_err(response_error)?;
        validate_filter_result(row_count(&chunks)?, result)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(response_error)
    }
    #[wasm_bindgen(js_name = decodeTableProfilesResult)]
    pub fn decode_profiles(
        chunks: Vec<js_sys::Uint8Array>,
        query: tsify::Ts<TableProfilesQuery>,
        result: tsify::Ts<TableProfilesResult>,
    ) -> Result<tsify::Ts<TableProfilesResult>, JsError> {
        let query = query.to_rust().map_err(request_error)?;
        let result = result.to_rust().map_err(response_error)?;
        validate_profiles_result(&query, row_count(&chunks)?, result)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(response_error)
    }
}
