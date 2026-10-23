//! Layer and dashboard file export: GeoJSON, delimited text, XLSX, KMZ and
//! shapefile ZIP from GeoJSON layers, and the styled dashboard workbook. The
//! `ds-io` writers own every byte; this module owns the typed request, the
//! refusal vocabulary and the reply decoder. Hosts own file names and saving.
use ds_io::{DashboardExportInput, GeoLayerExportInput, GeoLayerInput};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

fn comma() -> String {
    ",".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
#[cfg_attr(feature = "wasm", tsify(hashmap_as_object))]
pub enum LayerExportRequest {
    Geojson {
        layers: Vec<GeoLayerInput>,
    },
    Xlsx {
        layers: Vec<GeoLayerInput>,
    },
    Delimited {
        layers: Vec<GeoLayerInput>,
        /// Cell separator; a comma when empty or absent.
        #[serde(default = "comma")]
        separator: String,
    },
    Kmz {
        layers: Vec<GeoLayerInput>,
    },
    ShpZip {
        layers: Vec<GeoLayerInput>,
    },
    Dashboard(DashboardExportInput),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub enum LayerExportFormat {
    Geojson,
    Xlsx,
    Delimited,
    Kmz,
    ShpZip,
    Dashboard,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify::Tsify))]
pub struct LayerExportReceipt {
    pub format: LayerExportFormat,
    pub byte_length: usize,
    /// UTF-8 text (GeoJSON, delimited) rather than a binary archive.
    pub text: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerExportError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for LayerExportError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(out, "export_request_invalid: {message}"),
            Self::InvalidResponse(message) => write!(out, "export_response_invalid: {message}"),
        }
    }
}
impl std::error::Error for LayerExportError {}

impl LayerExportRequest {
    pub fn format(&self) -> LayerExportFormat {
        match self {
            Self::Geojson { .. } => LayerExportFormat::Geojson,
            Self::Xlsx { .. } => LayerExportFormat::Xlsx,
            Self::Delimited { .. } => LayerExportFormat::Delimited,
            Self::Kmz { .. } => LayerExportFormat::Kmz,
            Self::ShpZip { .. } => LayerExportFormat::ShpZip,
            Self::Dashboard(_) => LayerExportFormat::Dashboard,
        }
    }
}

/// Decode with every JSON object key-sorted, as the Network engine parsed it:
/// property order decides column order in every tabular format.
pub fn decode_request(bytes: &[u8]) -> Result<LayerExportRequest, LayerExportError> {
    crate::network_json_order::decode_sorted(bytes)
        .map_err(|error| LayerExportError::InvalidRequest(error.to_string()))
}

/// The file bytes, exactly as the `ds-io` writer produced them.
pub fn execute(request: LayerExportRequest) -> Result<Vec<u8>, LayerExportError> {
    let request = crate::network_json_order::sorted(request)
        .map_err(|error| LayerExportError::InvalidRequest(error.to_string()))?;
    execute_sorted(request)
}

fn execute_sorted(request: LayerExportRequest) -> Result<Vec<u8>, LayerExportError> {
    let layers = |layers| GeoLayerExportInput { layers };
    match request {
        LayerExportRequest::Geojson { layers: input } => {
            ds_io::layers_to_geojson_string(layers(input)).and_then(|text| {
                // The writer adds objects of its own; emit them key-sorted too.
                crate::network_json_order::sorted_pretty_text(&text)
                    .map(String::into_bytes)
                    .map_err(|error| error.to_string())
            })
        }
        LayerExportRequest::Xlsx { layers: input } => ds_io::layers_to_xlsx(layers(input)),
        LayerExportRequest::Delimited {
            layers: input,
            separator,
        } => ds_io::layers_to_delimited(layers(input), &separator).map(String::into_bytes),
        LayerExportRequest::Kmz { layers: input } => ds_io::layers_to_kmz(layers(input)),
        LayerExportRequest::ShpZip { layers: input } => ds_io::layers_to_shp_zip(layers(input)),
        LayerExportRequest::Dashboard(input) => ds_io::dashboard_to_xlsx(input),
    }
    .map_err(LayerExportError::InvalidRequest)
}

pub fn execute_bytes(bytes: &[u8]) -> Result<Vec<u8>, LayerExportError> {
    execute_sorted(decode_request(bytes)?)
}

fn invalid(message: impl Into<String>) -> LayerExportError {
    LayerExportError::InvalidResponse(message.into())
}

fn archive_members(bytes: &[u8]) -> Result<Vec<String>, LayerExportError> {
    let archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| invalid(format!("the reply is not a ZIP archive: {error}")))?;
    Ok(archive.file_names().map(str::to_owned).collect())
}

/// Bounded structural check of a writer's reply: the container a format
/// promises, never a second parse of every feature.
pub fn decode_reply(
    format: LayerExportFormat,
    bytes: &[u8],
) -> Result<LayerExportReceipt, LayerExportError> {
    let text = matches!(
        format,
        LayerExportFormat::Geojson | LayerExportFormat::Delimited
    );
    if text {
        let body =
            std::str::from_utf8(bytes).map_err(|_| invalid("the reply is not UTF-8 text"))?;
        if format == LayerExportFormat::Geojson {
            // Objects are key-sorted, so the top-level `type` follows the
            // features: look for it at either end, never parse every feature.
            let trimmed = body.trim();
            let mut head = trimmed.len().min(256);
            while !trimmed.is_char_boundary(head) {
                head -= 1;
            }
            let mut tail = trimmed.len().saturating_sub(256);
            while !trimmed.is_char_boundary(tail) {
                tail += 1;
            }
            let typed = |part: &str| part.contains("\"type\": \"FeatureCollection\"");
            if !trimmed.starts_with('{')
                || !trimmed.ends_with('}')
                || !(typed(&trimmed[..head]) || typed(&trimmed[tail..]))
            {
                return Err(invalid("the reply is not a GeoJSON FeatureCollection"));
            }
        } else if body.is_empty() {
            return Err(invalid("the delimited reply has no header row"));
        }
    } else {
        let members = archive_members(bytes)?;
        let has = |name: &str| members.iter().any(|member| member == name);
        let suffixed = |suffix: &str| {
            members
                .iter()
                .filter(|member| member.to_ascii_lowercase().ends_with(suffix))
                .count()
        };
        let complete = match format {
            LayerExportFormat::Xlsx | LayerExportFormat::Dashboard => {
                has("[Content_Types].xml") && has("xl/workbook.xml")
            }
            LayerExportFormat::Kmz => has("doc.kml"),
            LayerExportFormat::ShpZip => {
                let shapes = suffixed(".shp");
                shapes > 0 && suffixed(".shx") == shapes && suffixed(".dbf") == shapes
            }
            LayerExportFormat::Geojson | LayerExportFormat::Delimited => true,
        };
        if !complete {
            return Err(invalid(
                "the archive is missing a member its format requires",
            ));
        }
    }
    Ok(LayerExportReceipt {
        format,
        byte_length: bytes.len(),
        text,
    })
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refuse(error: LayerExportError) -> JsError {
        JsError::new(&error.to_string())
    }

    /// The file bytes for one typed export request (owning JSON bytes).
    #[wasm_bindgen(js_name = layerExport)]
    pub fn run(input: &[u8]) -> Result<Vec<u8>, JsError> {
        execute_bytes(input).map_err(refuse)
    }

    #[wasm_bindgen(js_name = decodeLayerExport)]
    pub fn decode(
        format: tsify::Ts<LayerExportFormat>,
        output: &[u8],
    ) -> Result<tsify::Ts<LayerExportReceipt>, JsError> {
        let format = format
            .to_rust()
            .map_err(|error| refuse(LayerExportError::InvalidRequest(error.to_string())))?;
        decode_reply(format, output)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(invalid(error.to_string())))
    }
}

#[cfg(test)]
#[path = "layer_export_tests.rs"]
mod tests;
