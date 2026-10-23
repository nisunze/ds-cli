//! The centralized conversion entry point (§0.8c).
//!
//! Every caller — Desktop import, `ds`, the estate migration, any future batch
//! job — comes through here. None of them re-derives the format decisions.
//! Reading the source is not here: the Network engine's reader turns the file
//! into canonical layers behind a typed task, and this crate encodes the one
//! artifact those layers become.

use crate::batch;
use crate::batch::Layout;
use crate::geoparquet::{self, Provenance};
use crate::identity::{conversion_identity, digest};
use serde::Serialize;
use serde_json::Value;

/// What a conversion produced, for the caller to record and the Desktop to
/// later reclaim by identity (§0.8a.3).
#[derive(Clone, Debug, Serialize)]
pub struct ConversionReceipt {
    pub layer_name: String,
    pub feature_count: usize,
    pub column_count: usize,
    /// sha256 of the source bytes.
    pub source_digest: String,
    /// sha256 of source digest + canonical conversion parameters. Re-running an
    /// unchanged conversion produces the same value, so it can be skipped.
    pub conversion_id: String,
    pub byte_count: usize,
    /// Rows the inspection rejected for unusable coordinates. Reported rather
    /// than silently dropped.
    pub skipped_coordinate_rows: usize,
}

#[derive(Debug)]
pub struct Conversion {
    pub parquet: Vec<u8>,
    pub receipt: ConversionReceipt,
}

/// One canonical layer a reader produced from the source, in source order.
#[derive(Clone, Debug, Default)]
pub struct SourceLayer {
    pub name: String,
    pub features: Vec<Value>,
    pub style_document: Option<Value>,
}

/// Whether a source carries geometry of its own, or is a table whose
/// coordinates live in named columns.
///
/// A shapefile already knows its geometry; a CSV does not, and someone has to
/// say which columns hold it. Conflating the two would mean guessing for one of
/// them, which is precisely what the contract forbids.
pub fn carries_geometry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".geojson", ".json", ".kml", ".kmz", ".zip", ".shp"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// Convert one source, already read into its layers, to GeoParquet.
///
/// `name` and `bytes` are the source exactly as read; `reading` is the table
/// reading the caller confirmed (`None` for a source carrying its own
/// geometry). They address the artifact; the layers become it. One GeoParquet
/// file holds one layer, so a multi-layer source must name the one it means.
pub fn layers_to_geoparquet(
    name: &str,
    bytes: &[u8],
    reading: Option<&Value>,
    layers: Vec<SourceLayer>,
    layer_name: Option<&str>,
    skipped_coordinate_rows: usize,
) -> Result<Conversion, String> {
    let source_digest = digest(bytes);
    let conversion_id = conversion_identity(&source_digest, name, reading, layer_name, None)?;

    let available: Vec<String> = layers.iter().map(|l| l.name.clone()).collect();
    let layer = match layer_name {
        Some(wanted) => layers
            .into_iter()
            .find(|layer| layer.name == wanted)
            .ok_or_else(|| {
                format!(
                    "{name}: no layer named `{wanted}`; it holds {}",
                    available.join(", ")
                )
            })?,
        None if available.len() > 1 => {
            return Err(format!(
                "{name}: holds {} layers ({}); name one to convert",
                available.len(),
                available.join(", ")
            ));
        }
        None => layers
            .into_iter()
            .next()
            .ok_or_else(|| format!("{name}: the source produced no layer to convert"))?,
    };

    let layer_name = layer.name.clone();
    let built = batch::build(&layer.features, &[], Layout::GeoParquet)?;
    let parquet = geoparquet::write(
        &built,
        &Provenance {
            style_document: layer.style_document.clone(),
            source_digest: Some(source_digest.clone()),
            conversion: Some(conversion_id.clone()),
            layer_name: Some(layer_name.clone()),
            skipped_coordinate_rows: Some(skipped_coordinate_rows),
        },
    )?;

    Ok(Conversion {
        receipt: ConversionReceipt {
            layer_name,
            feature_count: layer.features.len(),
            column_count: built.columns.len(),
            source_digest,
            conversion_id,
            byte_count: parquet.len(),
            skipped_coordinate_rows,
        },
        parquet,
    })
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
