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

/// The refusal code of a page export that a newer Kernel UI keeps only so an
/// older page still boots: the page's own exchange left the unit.
pub const UI_UNIT_UPDATE_REQUIRED: &str = "ui_unit_update_required";

/// The typed refusal a retired page export answers. The Kernel UI linked
/// Arrow for `getRows` (a row window as IPC) and for the row counts of
/// `decodeTableFilterResult` / `decodeTableProfilesResult` until 24 Oct 2026;
/// they stay exported, Arrow-free, so a page built before the row-window
/// exchange boots and only its attribute table names the update.
pub fn retired_page_export(export: &str) -> String {
    format!(
        "{UI_UNIT_UPDATE_REQUIRED}: this page predates the Kernel UI row-window exchange \
         ({export}); update DS GridDesign"
    )
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

    // Retired page exports, kept so a page built before the row-window
    // exchange (Desktop r100 / Windows r99 and older) still boots: its start
    // check finds every export it calls. Each answers the typed update
    // refusal; the Kernel UI reads no Arrow for them.
    /// Retired: count the chunks with `tableRowCount` and admit with
    /// `decodeTableFilterAnswer`.
    #[wasm_bindgen(js_name = decodeTableFilterResult, unchecked_return_type = "TableFilterResult")]
    pub fn decode_filter_result_retired(
        #[wasm_bindgen(unchecked_param_type = "Uint8Array[]")] _chunks: JsValue,
        #[wasm_bindgen(unchecked_param_type = "TableFilterResult")] _result: JsValue,
    ) -> Result<JsValue, JsError> {
        Err(JsError::new(&retired_page_export(
            "decodeTableFilterResult",
        )))
    }
    /// Retired: count the chunks with `tableRowCount` and admit with
    /// `decodeTableProfilesAnswer`.
    #[wasm_bindgen(js_name = decodeTableProfilesResult, unchecked_return_type = "TableProfilesResult")]
    pub fn decode_profiles_result_retired(
        #[wasm_bindgen(unchecked_param_type = "Uint8Array[]")] _chunks: JsValue,
        #[wasm_bindgen(unchecked_param_type = "TableProfilesQuery")] _query: JsValue,
        #[wasm_bindgen(unchecked_param_type = "TableProfilesResult")] _result: JsValue,
    ) -> Result<JsValue, JsError> {
        Err(JsError::new(&retired_page_export(
            "decodeTableProfilesResult",
        )))
    }
}
