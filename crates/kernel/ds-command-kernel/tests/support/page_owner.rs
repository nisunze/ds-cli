//! The Network unit's page handlers (`selectLayersByArea`,
//! `lineExtensionDifference`), reproduced over ds-geo as the reply oracle of
//! the Kernel's admission tests. ds-geo is a dev-dependency only: the Kernel
//! links no Network algorithm, and the unit's own tests pin the same bytes.
#![allow(dead_code)]
use ds_command_kernel::area_selection::{
    self, AreaSelectionError, AreaSelectionRequest, AreaSelectionResult,
};
use ds_command_kernel::line_extension::{
    self, LineExtensionDifferenceInput, LineExtensionDifferenceOutput, LineExtensionError,
};
use serde_json::Value;

pub fn select(request: &AreaSelectionRequest) -> Result<AreaSelectionResult, AreaSelectionError> {
    area_selection::validate_request(request)?;
    let mut result = ds_geo::area_selection::filter_layers_by_polygon_intersects(request.clone())
        .map_err(AreaSelectionError::InvalidRequest)?;
    for layer in &mut result.layers {
        layer.features.iter_mut().for_each(Value::sort_all_objects);
    }
    Ok(result)
}

pub fn select_json(bytes: &[u8]) -> Result<String, AreaSelectionError> {
    Ok(serde_json::to_string(&select(&area_selection::decode_request(bytes)?)?).unwrap())
}

pub fn difference(
    input: &LineExtensionDifferenceInput,
) -> Result<LineExtensionDifferenceOutput, LineExtensionError> {
    line_extension::validate_request(input)?;
    let mut output = ds_geo::line_difference::line_extension_difference(input.clone())
        .map_err(LineExtensionError::InvalidRequest)?;
    output.layer.sort_all_objects();
    Ok(output)
}

pub fn difference_json(bytes: &[u8]) -> Result<String, LineExtensionError> {
    Ok(serde_json::to_string(&difference(&line_extension::decode_request(bytes)?)?).unwrap())
}
