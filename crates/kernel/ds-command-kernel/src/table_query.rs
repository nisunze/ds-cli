//! Read-only table queries shared by native callers and browser workers.
//! Explicit rows/chunks are the complete input; no project acquisition occurs.
use crate::selection_field::{
    SelectionFieldError, SelectionFieldProfile, SelectionFieldQuery,
    validate_selection_field_profile,
};
use ds_columnar::schema::infer_row_kinds;
use ds_geo::table::{
    ColumnFilterInput, FilterSortInput, SortInput, ValueStatsInput, ValueStatsOptions,
    filter_sort_groups, selection_field_values, value_stats,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum TableFilterMode {
    #[default]
    All,
    Any,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct TableFilterQuery {
    #[serde(default)]
    pub filters: Vec<ColumnFilterInput>,
    #[serde(default)]
    pub groups: Vec<Vec<ColumnFilterInput>>,
    #[serde(default)]
    pub mode: TableFilterMode,
    #[serde(default)]
    pub sort: Option<SortInput>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct TableProfilesQuery {
    pub columns: Vec<String>,
    #[serde(default)]
    pub options: Option<ValueStatsOptions>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct TableFilterResult {
    pub indices: Vec<usize>,
    pub total_rows: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct TableProfilesResult {
    pub stats: Vec<SelectionFieldProfile>,
    pub total_rows: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TableQueryError {
    InvalidRequest(String),
    InvalidResponse(String),
}
impl std::fmt::Display for TableQueryError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(out, "table_query_request_invalid: {message}"),
            Self::InvalidResponse(message) => {
                write!(out, "table_query_response_invalid: {message}")
            }
        }
    }
}
impl std::error::Error for TableQueryError {}
impl From<SelectionFieldError> for TableQueryError {
    fn from(error: SelectionFieldError) -> Self {
        match error {
            SelectionFieldError::InvalidRequest(message) => Self::InvalidRequest(message),
            SelectionFieldError::InvalidResponse(message) => Self::InvalidResponse(message),
        }
    }
}

pub fn filter_rows(
    query: &TableFilterQuery,
    rows: Vec<Map<String, Value>>,
) -> Result<TableFilterResult, TableQueryError> {
    let total_rows = rows.len();
    let indices = filter_sort_groups(
        FilterSortInput {
            rows,
            filters: query.filters.clone(),
            sort: query.sort.clone(),
        },
        query.groups.clone(),
        match query.mode {
            TableFilterMode::All => false,
            TableFilterMode::Any => true,
        },
    )
    .map_err(TableQueryError::InvalidRequest)?;
    Ok(TableFilterResult {
        indices,
        total_rows,
    })
}

pub fn profile_rows(
    query: &TableProfilesQuery,
    rows: Vec<Map<String, Value>>,
) -> Result<TableProfilesResult, TableQueryError> {
    for column in &query.columns {
        SelectionFieldQuery {
            column: column.clone(),
        }
        .validate()
        .map_err(TableQueryError::from)?;
    }
    let total_rows = rows.len();
    let kinds = infer_row_kinds(&rows, &query.columns);
    let tokens = query
        .columns
        .iter()
        .map(|column| selection_field_values(&rows, column, true))
        .collect::<Vec<_>>();
    let result = value_stats(ValueStatsInput {
        rows,
        columns: query.columns.clone(),
        options: query.options.clone().unwrap_or_default(),
    })
    .map_err(TableQueryError::InvalidRequest)?;
    let stats = result
        .stats
        .into_iter()
        .zip(tokens)
        .map(|(stats, token_values)| SelectionFieldProfile {
            kind: kinds[&stats.column],
            stats,
            token_values,
        })
        .collect();
    Ok(TableProfilesResult { stats, total_rows })
}

pub fn validate_filter_result(
    expected_rows: usize,
    result: TableFilterResult,
) -> Result<TableFilterResult, TableQueryError> {
    if result.total_rows != expected_rows {
        return Err(TableQueryError::InvalidResponse(
            "answer belongs to a different row scope".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    if result
        .indices
        .iter()
        .any(|index| *index >= expected_rows || !seen.insert(*index))
    {
        return Err(TableQueryError::InvalidResponse(
            "row indices must be unique and inside the captured input".into(),
        ));
    }
    Ok(result)
}

pub fn validate_profiles_result(
    query: &TableProfilesQuery,
    expected_rows: usize,
    result: TableProfilesResult,
) -> Result<TableProfilesResult, TableQueryError> {
    if result.total_rows != expected_rows || result.stats.len() != query.columns.len() {
        return Err(TableQueryError::InvalidResponse(
            "answer belongs to a different row or column scope".into(),
        ));
    }
    for (column, profile) in query.columns.iter().zip(&result.stats) {
        if profile.stats.total_rows != expected_rows {
            return Err(TableQueryError::InvalidResponse(
                "profile has a different row scope".into(),
            ));
        }
        validate_selection_field_profile(
            &SelectionFieldQuery {
                column: column.clone(),
            },
            profile.clone(),
        )
        .map_err(TableQueryError::from)?;
    }
    Ok(result)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::{JsCast, prelude::*};

    fn request_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&TableQueryError::InvalidRequest(error.to_string()).to_string())
    }
    fn response_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&TableQueryError::InvalidResponse(error.to_string()).to_string())
    }
    fn rows(chunks: &[js_sys::Uint8Array]) -> Result<Vec<Map<String, Value>>, JsError> {
        let mut rows = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            if !chunk.is_instance_of::<js_sys::Uint8Array>() {
                return Err(request_error(format!("chunk {index} must be Uint8Array")));
            }
            rows.extend(
                ds_columnar::ipc_to_rows(&chunk.to_vec())
                    .map_err(|error| request_error(format!("chunk {index}: {error}")))?,
            );
        }
        Ok(rows)
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

    #[wasm_bindgen(js_name = tableFilterSortIndicesIpc)]
    pub fn filter_ipc(
        chunks: Vec<js_sys::Uint8Array>,
        query: tsify::Ts<TableFilterQuery>,
    ) -> Result<tsify::Ts<TableFilterResult>, JsError> {
        let query = query.to_rust().map_err(request_error)?;
        filter_rows(&query, rows(&chunks)?)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(response_error)
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
    #[wasm_bindgen(js_name = tableProfilesIpc)]
    pub fn profiles_ipc(
        chunks: Vec<js_sys::Uint8Array>,
        query: tsify::Ts<TableProfilesQuery>,
    ) -> Result<tsify::Ts<TableProfilesResult>, JsError> {
        let query = query.to_rust().map_err(request_error)?;
        profile_rows(&query, rows(&chunks)?)
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
