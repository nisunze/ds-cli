//! Selection field hints. Native and browser adapters share this operation;
//! table statistics and stored value kinds retain their existing owners.
use ds_columnar::schema::{PropertyKind, infer_row_kinds};
use ds_geo::table::{ColumnValueStats, ValueStatsInput, selection_field_values, value_stats};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct SelectionFieldQuery {
    pub column: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct SelectionFieldProfile {
    #[serde(flatten)]
    pub stats: ColumnValueStats,
    pub kind: PropertyKind,
    pub token_values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectionFieldError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for SelectionFieldError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => {
                write!(out, "selection_field_request_invalid: {message}")
            }
            Self::InvalidResponse(message) => {
                write!(out, "selection_field_response_invalid: {message}")
            }
        }
    }
}
impl std::error::Error for SelectionFieldError {}

impl SelectionFieldQuery {
    pub fn validate(&self) -> Result<(), SelectionFieldError> {
        if self.column.trim().is_empty() {
            return Err(SelectionFieldError::InvalidRequest(
                "column must not be empty".into(),
            ));
        }
        Ok(())
    }
}

pub fn selection_field_info(
    query: &SelectionFieldQuery,
    rows: Vec<Map<String, Value>>,
) -> Result<SelectionFieldProfile, SelectionFieldError> {
    query.validate()?;
    let columns = vec![query.column.clone()];
    let kind = infer_row_kinds(&rows, &columns)[&query.column];
    let token_values = selection_field_values(&rows, &query.column, true);
    let mut result = value_stats(ValueStatsInput {
        rows,
        columns,
        options: Default::default(),
    })
    .map_err(SelectionFieldError::InvalidRequest)?;
    let stats = result.stats.remove(0); // exactly one requested column
    Ok(SelectionFieldProfile {
        stats,
        kind,
        token_values,
    })
}

/// Decode at the owning boundary, without an independent TypeScript shape.
/// Unknown additive fields are deliberately accepted by serde.
pub fn decode_selection_field_profile(
    query: &SelectionFieldQuery,
    bytes: &[u8],
) -> Result<SelectionFieldProfile, SelectionFieldError> {
    let profile = serde_json::from_slice(bytes)
        .map_err(|error| SelectionFieldError::InvalidResponse(error.to_string()))?;
    validate_selection_field_profile(query, profile)
}

pub fn validate_selection_field_profile(
    query: &SelectionFieldQuery,
    profile: SelectionFieldProfile,
) -> Result<SelectionFieldProfile, SelectionFieldError> {
    query.validate()?;
    let stats = &profile.stats;
    if stats.column != query.column {
        return Err(SelectionFieldError::InvalidResponse(
            "answer belongs to a different column".into(),
        ));
    }
    if stats.non_empty_count > stats.total_rows || stats.distinct_count > stats.non_empty_count {
        return Err(SelectionFieldError::InvalidResponse(
            "inconsistent row counts".into(),
        ));
    }
    match (stats.numeric_min, stats.numeric_max) {
        (None, None) => (),
        (Some(min), Some(max)) if min.is_finite() && max.is_finite() && min <= max => (),
        _ => {
            return Err(SelectionFieldError::InvalidResponse(
                "invalid numeric range".into(),
            ));
        }
    }
    Ok(profile)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = selectionFieldInfoIpc)]
    pub fn selection_field_info_ipc(
        input: &[u8],
        query: tsify::Ts<SelectionFieldQuery>,
    ) -> Result<tsify::Ts<SelectionFieldProfile>, JsError> {
        let query = query.to_rust().map_err(|error| {
            JsError::new(&SelectionFieldError::InvalidRequest(error.to_string()).to_string())
        })?;
        query
            .validate()
            .map_err(|error| JsError::new(&error.to_string()))?;
        let rows = ds_columnar::ipc_to_rows(input).map_err(|error| {
            JsError::new(&SelectionFieldError::InvalidRequest(error).to_string())
        })?;
        selection_field_info(&query, rows)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }

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
