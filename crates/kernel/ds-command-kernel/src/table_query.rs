//! Read-only table queries over held rows. Filtering, sorting, column
//! profiles and the held row count run in the Network unit (`tableFilter`,
//! `tableProfiles`, `tableRowCount` through the grid unit envelope); the
//! queries, answers and their checks are the Network contract. This module
//! admits each answer against the row scope the unit counted over the same
//! chunks, so the Kernel reads no column (it links no Arrow reader).
pub use ds_network_contracts::table_query::{
    validate_filter_result, validate_profiles_result, TableFilterMode, TableFilterQuery,
    TableFilterResult, TableProfilesQuery, TableProfilesResult, TableQueryError, TableRowCount,
};

/// The Network unit operations that filter, profile and count held rows.
pub const FILTER_METHOD: &str = "tableFilter";
pub const PROFILES_METHOD: &str = "tableProfiles";
pub const ROW_COUNT_METHOD: &str = "tableRowCount";

/// Admit a filter answer against the counted row scope.
pub fn admit_filter(
    rows: TableRowCount,
    result: TableFilterResult,
) -> Result<TableFilterResult, TableQueryError> {
    validate_filter_result(rows.total_rows, result)
}

/// Admit a profile answer against the query and the counted row scope.
pub fn admit_profiles(
    rows: TableRowCount,
    query: &TableProfilesQuery,
    result: TableProfilesResult,
) -> Result<TableProfilesResult, TableQueryError> {
    validate_profiles_result(query, rows.total_rows, result)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn request_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&TableQueryError::InvalidRequest(error.to_string()).to_string())
    }
    fn response_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&TableQueryError::InvalidResponse(error.to_string()).to_string())
    }

    /// Admit the unit's `tableFilter` answer against its `tableRowCount`
    /// answer over the same chunks.
    #[wasm_bindgen(js_name = decodeTableFilterAnswer)]
    pub fn decode_filter(
        rows: tsify::Ts<TableRowCount>,
        result: tsify::Ts<TableFilterResult>,
    ) -> Result<tsify::Ts<TableFilterResult>, JsError> {
        let rows = rows.to_rust().map_err(response_error)?;
        let result = result.to_rust().map_err(response_error)?;
        admit_filter(rows, result)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(response_error)
    }
    /// Admit the unit's `tableProfiles` answer against the query and its
    /// `tableRowCount` answer over the same chunks.
    #[wasm_bindgen(js_name = decodeTableProfilesAnswer)]
    pub fn decode_profiles(
        rows: tsify::Ts<TableRowCount>,
        query: tsify::Ts<TableProfilesQuery>,
        result: tsify::Ts<TableProfilesResult>,
    ) -> Result<tsify::Ts<TableProfilesResult>, JsError> {
        let rows = rows.to_rust().map_err(response_error)?;
        let query = query.to_rust().map_err(request_error)?;
        let result = result.to_rust().map_err(response_error)?;
        admit_profiles(rows, &query, result)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(response_error)
    }
}
