//! GeoPackage reading in a browser host: the host owns the SQLite file and
//! hands the Kernel one geometry cell or one catalog row at a time. The
//! GeoPackage header/WKB codec and the layer admission policy are the portable
//! `ds_geo_lite::gpkg` owner shared with the native reader; this module owns
//! the bounded typed application operations and their refusals.
pub use ds_geo_lite::gpkg::{
    GpkgGeometry, GpkgGeometryHeader, GpkgLayerPolicy, GpkgLayerPolicyInput,
};

/// Largest single geometry cell a host may hand over.
pub const MAX_GPKG_GEOMETRY_BYTES: usize = 16 * 1024 * 1024;
/// Largest text a catalog row may carry across its fields.
pub const MAX_GPKG_POLICY_TEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum GpkgError {
    InvalidRequest(String),
}

impl std::fmt::Display for GpkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "gpkg_request_invalid: {message}"),
        }
    }
}

impl std::error::Error for GpkgError {}

/// Decode one GeoPackage geometry cell (GeoPackage binary or bare WKB) to
/// GeoJSON, XY only, exactly as every DS reader imports it. Geometry object
/// keys are in sorted order, as the retired Network export always returned
/// them, so stored GeoJSON text keeps its bytes.
pub fn decode_geometry(bytes: &[u8]) -> Result<GpkgGeometry, GpkgError> {
    if bytes.len() > MAX_GPKG_GEOMETRY_BYTES {
        return Err(GpkgError::InvalidRequest(format!(
            "GeoPackage geometry is {} bytes; maximum is {MAX_GPKG_GEOMETRY_BYTES}",
            bytes.len()
        )));
    }
    let mut decoded =
        ds_geo_lite::gpkg::gpkg_geometry_to_geojson(bytes).map_err(GpkgError::InvalidRequest)?;
    if let Some(geometry) = decoded.geometry.as_mut() {
        geometry.sort_all_objects();
    }
    Ok(decoded)
}

fn policy_text_bytes(input: &GpkgLayerPolicyInput) -> usize {
    input.data_type.len()
        + [
            &input.geometry_column,
            &input.geometry_type,
            &input.srs_organization,
        ]
        .iter()
        .map(|value| value.as_deref().map_or(0, str::len))
        .sum::<usize>()
}

/// Classify one host-extracted catalog row: feature table, CRS, geometry
/// type and row bound decide whether the browser may read it locally.
pub fn layer_policy(input: &GpkgLayerPolicyInput) -> Result<GpkgLayerPolicy, GpkgError> {
    let bytes = policy_text_bytes(input);
    if bytes > MAX_GPKG_POLICY_TEXT_BYTES {
        return Err(GpkgError::InvalidRequest(format!(
            "GeoPackage layer policy text is {bytes} bytes; maximum is {MAX_GPKG_POLICY_TEXT_BYTES}"
        )));
    }
    Ok(ds_geo_lite::gpkg::gpkg_layer_policy(input))
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = decodeGpkgGeometry)]
    pub fn decode(bytes: &[u8]) -> Result<tsify::Ts<GpkgGeometry>, JsError> {
        decode_geometry(bytes)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = gpkgLayerPolicy)]
    pub fn policy(
        input: tsify::Ts<GpkgLayerPolicyInput>,
    ) -> Result<tsify::Ts<GpkgLayerPolicy>, JsError> {
        let input = input.to_rust().map_err(|error| {
            JsError::new(&GpkgError::InvalidRequest(error.to_string()).to_string())
        })?;
        layer_policy(&input)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
