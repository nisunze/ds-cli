//! Stateless table edit proposals. The application owns applying a proposal;
//! algorithms, value types and wire DTOs retain their existing ds-geo owner.
use ds_geo::table::{
    self, AddColumnInput, CalculateFieldInput, CalculateGeometryPropertiesInput, ReplaceCount,
    ReplaceInput, ReplaceUpdateList, TableGeometryPropertiesPlan, TableMutationPlan,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "input", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum TableEditRequest {
    CountReplace(ReplaceInput),
    CollectReplace(ReplaceInput),
    AddColumn(AddColumnInput),
    CalculateField(CalculateFieldInput),
    GeometryProperties(CalculateGeometryPropertiesInput),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct AddColumnEditResult {
    pub plan: TableMutationPlan,
    pub default_value: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub struct TableEditSnapshot {
    #[cfg_attr(feature = "wasm", tsify(type = "Record<string, Value>[]"))]
    pub rows: Vec<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub geometries: Vec<serde_json::Value>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "result", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object, missing_as_null))]
pub enum TableEditResponse {
    CountReplace(ReplaceCount),
    CollectReplace(ReplaceUpdateList),
    AddColumn(AddColumnEditResult),
    CalculateField(TableMutationPlan),
    GeometryProperties(TableGeometryPropertiesPlan),
}
#[derive(Clone, Debug, PartialEq)]
pub enum TableEditError {
    InvalidRequest(String),
    InvalidResponse(String),
    StaleInput,
}
impl std::fmt::Display for TableEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "table_edit_request_invalid: {message}"),
            Self::InvalidResponse(message) => write!(f, "table_edit_response_invalid: {message}"),
            Self::StaleInput => write!(f, "table_edit_stale: The table data changed while this edit was calculated. No changes were applied. Run it again."),
        }
    }
}
impl std::error::Error for TableEditError {}

pub fn validate_request(request: &TableEditRequest) -> Result<(), TableEditError> {
    let (indices, length) = match request {
        TableEditRequest::CountReplace(input) | TableEditRequest::CollectReplace(input) => {
            (&input.scope.indices, input.scope.rows.len())
        }
        TableEditRequest::CalculateField(input) => (&input.indices, input.rows.len()),
        TableEditRequest::GeometryProperties(input) => {
            (&input.indices, input.rows.len().max(input.geometries.len()))
        }
        TableEditRequest::AddColumn(_) => return Ok(()),
    };
    if let Some(index) = indices.iter().find(|index| **index >= length) {
        return Err(TableEditError::InvalidRequest(format!(
            "row index {index} is outside the supplied {length} rows"
        )));
    }
    Ok(())
}
pub fn decode_request(bytes: &[u8]) -> Result<TableEditRequest, TableEditError> {
    let request = serde_json::from_slice(bytes)
        .map_err(|error| TableEditError::InvalidRequest(error.to_string()))?;
    validate_request(&request)?;
    Ok(request)
}
/// A data revision check, independent of releases, schema digests or byte order.
pub fn validate_snapshot(
    request: &TableEditRequest,
    current: &TableEditSnapshot,
) -> Result<(), TableEditError> {
    let same = match request {
        TableEditRequest::CountReplace(input) | TableEditRequest::CollectReplace(input) => {
            input.scope.rows == current.rows
        }
        TableEditRequest::AddColumn(input) => input.rows == current.rows,
        TableEditRequest::CalculateField(input) => {
            input.rows == current.rows && input.geometries == current.geometries
        }
        TableEditRequest::GeometryProperties(input) => {
            input.rows == current.rows && input.geometries == current.geometries
        }
    };
    if same {
        Ok(())
    } else {
        Err(TableEditError::StaleInput)
    }
}
pub fn execute(request: &TableEditRequest) -> Result<TableEditResponse, TableEditError> {
    use TableEditRequest as Request;
    use TableEditResponse as Response;
    validate_request(request)?;
    match request {
        Request::CountReplace(input) => {
            table::count_replace_matches(input.clone()).map(Response::CountReplace)
        }
        Request::CollectReplace(input) => {
            table::collect_replace_updates(input.clone()).map(Response::CollectReplace)
        }
        Request::AddColumn(input) => table::add_column(input.clone()).and_then(|plan| {
            Ok(Response::AddColumn(AddColumnEditResult {
                plan,
                default_value: table::coerce_value(&input.default_value, input.data_type)?,
            }))
        }),
        Request::CalculateField(input) => {
            table::calculate_field(input.clone()).map(Response::CalculateField)
        }
        Request::GeometryProperties(input) => {
            table::calculate_geometry_properties(input.clone()).map(Response::GeometryProperties)
        }
    }
    .map_err(TableEditError::InvalidRequest)
}
pub fn validate_response(
    request: &TableEditRequest,
    response: TableEditResponse,
) -> Result<TableEditResponse, TableEditError> {
    use TableEditRequest as Request;
    use TableEditResponse as Response;
    validate_request(request)?;
    match (request, &response) {
        (Request::CountReplace(input), Response::CountReplace(reply)) => {
            table::validate_replace_count(input, reply)
        }
        (Request::CollectReplace(input), Response::CollectReplace(reply)) => {
            table::validate_replace_updates(input, reply)
        }
        (Request::AddColumn(input), Response::AddColumn(reply)) => {
            table::validate_add_column_plan(input, &reply.plan).and_then(|()| {
                if reply.default_value
                    != table::coerce_value(&input.default_value, input.data_type)?
                {
                    return Err("default value does not match the requested type".into());
                }
                Ok(())
            })
        }
        (Request::CalculateField(input), Response::CalculateField(reply)) => {
            table::validate_calculate_field_plan(input, reply)
        }
        (Request::GeometryProperties(input), Response::GeometryProperties(reply)) => {
            table::validate_geometry_properties_plan(input, reply)
        }
        _ => Err("reply operation does not match the request".into()),
    }
    .map_err(TableEditError::InvalidResponse)?;
    Ok(response)
}
pub fn decode_response(
    request: &TableEditRequest,
    bytes: &[u8],
) -> Result<TableEditResponse, TableEditError> {
    validate_response(
        request,
        serde_json::from_slice(bytes)
            .map_err(|error| TableEditError::InvalidResponse(error.to_string()))?,
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = validateTableEditSnapshot)]
    pub fn current(input: &[u8], snapshot: tsify::Ts<TableEditSnapshot>) -> Result<(), JsError> {
        let request = decode_request(input).map_err(|error| JsError::new(&error.to_string()))?;
        let snapshot = snapshot.to_rust().map_err(|error| {
            JsError::new(&TableEditError::InvalidRequest(error.to_string()).to_string())
        })?;
        validate_snapshot(&request, &snapshot).map_err(|error| JsError::new(&error.to_string()))
    }
    #[wasm_bindgen(js_name=encodeTableEdit)]
    pub fn encode(input: tsify::Ts<TableEditRequest>) -> Result<Vec<u8>, JsError> {
        let request = input.to_rust().map_err(|error| {
            JsError::new(&TableEditError::InvalidRequest(error.to_string()).to_string())
        })?;
        validate_request(&request).map_err(|error| JsError::new(&error.to_string()))?;
        serde_json::to_vec(&request).map_err(|error| {
            JsError::new(&TableEditError::InvalidRequest(error.to_string()).to_string())
        })
    }
    #[wasm_bindgen(js_name=tableEdit)]
    pub fn run(input: &[u8]) -> Result<tsify::Ts<TableEditResponse>, JsError> {
        let request = decode_request(input).map_err(|error| JsError::new(&error.to_string()))?;
        execute(&request)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
    #[wasm_bindgen(js_name=decodeTableEdit)]
    pub fn decode(
        input: &[u8],
        reply: tsify::Ts<TableEditResponse>,
    ) -> Result<tsify::Ts<TableEditResponse>, JsError> {
        let request = decode_request(input).map_err(|error| JsError::new(&error.to_string()))?;
        let reply = reply.to_rust().map_err(|error| {
            JsError::new(&TableEditError::InvalidResponse(error.to_string()).to_string())
        })?;
        validate_response(&request, reply)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
