//! GeoParquet 1.1.0 writer — §1 of the columnar format contract.
//!
//! Every decision the contract pins is encoded here and nowhere else, so a
//! change is one edit and every file the platform has written agrees by
//! construction (§0.8c).

use crate::batch::FeatureBatch;
pub use crate::contract::{GEOPARQUET_VERSION, LAYER_NAME_KEY, SKIPPED_ROWS_KEY, crs84_projjson};
use crate::schema::PARQUET_GEOMETRY_COLUMN;
use crate::wkb::bbox_of;
use arrow_array::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use parquet::format::KeyValue;
use serde_json::{Value, json};

/// Row group ceiling in rows. Paired with the byte target below: whichever
/// bound is reached first closes the group.
pub const MAX_ROW_GROUP_ROWS: usize = 1_000_000;
/// Uncompressed bytes per row group. Small enough that a range read over a
/// bucket fetches a useful fraction; large enough that footer metadata is cheap.
pub const ROW_GROUP_TARGET_BYTES: usize = 64 * 1024 * 1024;

/// Provenance recorded alongside the data, so a converted artifact can be
/// matched back to its source without a side-channel manifest.
///
/// This is what lets an artifact be reused by identity alone. A caller that had
/// to keep a pointer file beside the artifact would be keeping a second record
/// that can disagree with the bytes; everything needed to describe a conversion
/// is written into the file that conversion produced.
#[derive(Clone, Debug, Default)]
pub struct Provenance {
    pub style_document: Option<Value>,
    pub source_digest: Option<String>,
    pub conversion: Option<String>,
    /// The layer this artifact holds. One GeoParquet file is one layer, and a
    /// reader must not have to re-derive its name from a file path.
    pub layer_name: Option<String>,
    /// Rows the inspection rejected for unusable coordinates. A fact about the
    /// conversion, so it travels with the conversion rather than being lost the
    /// moment the run that produced it ends.
    pub skipped_coordinate_rows: Option<usize>,
}

/// The `geo` metadata document, from what a file actually holds. Shared by the
/// whole-file writer and the streaming one so a streamed artifact and a
/// single-pass artifact cannot describe themselves differently.
pub fn geo_metadata(geometry_types: &[&'static str], bbox: Option<[f64; 4]>) -> Value {
    let mut geometry_meta = json!({
        "encoding": "WKB",
        "geometry_types": geometry_types,
        "crs": crs84_projjson(),
    });
    if let Some(bbox) = bbox {
        geometry_meta["bbox"] = json!(bbox);
    }
    json!({
        "version": GEOPARQUET_VERSION,
        "primary_column": PARQUET_GEOMETRY_COLUMN,
        "columns": { PARQUET_GEOMETRY_COLUMN: geometry_meta },
    })
}

/// The `ds:*` provenance entries, in a fixed order.
pub fn provenance_metadata(provenance: &Provenance) -> Vec<KeyValue> {
    let mut metadata = Vec::new();
    if let Some(style) = &provenance.style_document {
        metadata.push(KeyValue::new(
            "ds:style_document".to_string(),
            style.to_string(),
        ));
    }
    if let Some(digest) = &provenance.source_digest {
        metadata.push(KeyValue::new(
            "ds:source_digest".to_string(),
            digest.clone(),
        ));
    }
    if let Some(conversion) = &provenance.conversion {
        metadata.push(KeyValue::new(
            "ds:conversion".to_string(),
            conversion.clone(),
        ));
    }
    if let Some(layer) = &provenance.layer_name {
        metadata.push(KeyValue::new(LAYER_NAME_KEY.to_string(), layer.clone()));
    }
    if let Some(skipped) = provenance.skipped_coordinate_rows {
        metadata.push(KeyValue::new(
            SKIPPED_ROWS_KEY.to_string(),
            skipped.to_string(),
        ));
    }
    metadata
}

pub fn write(built: &FeatureBatch, provenance: &Provenance) -> Result<Vec<u8>, String> {
    let geometry_types = geometry_type_names(&built.geometries);
    let geo = geo_metadata(&geometry_types, bbox_of(&built.geometries));

    let mut metadata = vec![KeyValue::new("geo".to_string(), geo.to_string())];
    metadata.extend(provenance_metadata(provenance));

    let properties = writer_properties(row_group_rows(&built.batch), metadata);

    let mut bytes = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut bytes, built.batch.schema(), Some(properties))
        .map_err(|error| format!("could not open the GeoParquet writer: {error}"))?;
    writer
        .write(&built.batch)
        .map_err(|error| format!("could not write the GeoParquet batch: {error}"))?;
    writer
        .close()
        .map_err(|error| format!("could not finish the GeoParquet file: {error}"))?;
    Ok(bytes)
}

/// The one set of writer decisions §1 of the contract pins. Both writers take
/// it from here, so a streamed artifact and a single-pass artifact are written
/// with the same compression, dictionary and statistics settings.
pub(crate) fn writer_properties(
    row_group_rows: usize,
    metadata: Vec<KeyValue>,
) -> WriterProperties {
    WriterProperties::builder()
        // SNAPPY, not ZSTD: supported by every Parquet reader that exists.
        // Dictionary encoding already carries most of the win on attribute
        // data. Revisit only behind a reader-support survey.
        .set_compression(Compression::SNAPPY)
        .set_dictionary_enabled(true)
        // Statistics are what make predicate pushdown work. Without them the
        // format buys nothing over a CSV.
        .set_statistics_enabled(parquet::file::properties::EnabledStatistics::Chunk)
        .set_max_row_group_size(row_group_rows)
        .set_key_value_metadata(Some(metadata))
        .build()
}

/// Rows per group, from the contract's byte target and row ceiling.
pub(crate) fn row_group_rows(batch: &RecordBatch) -> usize {
    let rows = batch.num_rows();
    if rows == 0 {
        return MAX_ROW_GROUP_ROWS;
    }
    let per_row = (batch.get_array_memory_size() / rows).max(1);
    (ROW_GROUP_TARGET_BYTES / per_row).clamp(1, MAX_ROW_GROUP_ROWS)
}

/// Distinct WKB type codes present, as GeoParquet type names. An empty list
/// means the writer makes no claim, which is the honest answer for a batch
/// that carried no geometry.
pub(crate) fn geometry_type_names(geometries: &[Option<Vec<u8>>]) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for wkb in geometries.iter().flatten() {
        if wkb.len() < 5 {
            continue;
        }
        let code = u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]);
        let name = match code {
            1 => "Point",
            2 => "LineString",
            3 => "Polygon",
            4 => "MultiPoint",
            5 => "MultiLineString",
            6 => "MultiPolygon",
            _ => continue,
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.sort_unstable();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::{self, Layout};
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use serde_json::json;
    use std::sync::Arc;

    /// PROJ refuses a datum ensemble with fewer than two members — "ensemble
    /// should have at least 2 datums" — and it refuses it in the READER, so a
    /// conversion reports success and the artifact is unusable. Every consumer
    /// built on PROJ is affected, geopandas included, which is why this is
    /// asserted on the document every writer embeds rather than on one file.
    #[test]
    fn the_crs_ensemble_carries_every_realization_a_proj_reader_demands() {
        let crs = crs84_projjson();
        let members = crs["datum_ensemble"]["members"]
            .as_array()
            .expect("the ensemble declares its members");
        assert!(
            members.len() >= 2,
            "PROJ rejects an ensemble with fewer than two datums; found {}",
            members.len()
        );
        for member in members {
            let name = member["name"].as_str().expect("each member is named");
            assert!(
                name.starts_with("World Geodetic System 1984"),
                "an ensemble member outside WGS 84: {name}"
            );
        }
        assert_eq!(crs["id"]["authority"], "OGC");
        assert_eq!(crs["id"]["code"], "CRS84");
    }

    /// The whole-file writer and the streaming writer must describe themselves
    /// identically, so a fix to one cannot leave the other malformed.
    #[test]
    fn both_writers_embed_the_same_repaired_crs() {
        // `super::` is required: the test module shadows this name with its
        // own reader helper of the same name.
        let embedded = super::geo_metadata(&["Point"], None);
        assert_eq!(
            embedded["columns"][PARQUET_GEOMETRY_COLUMN]["crs"],
            crs84_projjson()
        );
    }

    fn parquet_of(features: &[Value]) -> Vec<u8> {
        let built = batch::build(features, &[], Layout::GeoParquet).unwrap();
        write(&built, &Provenance::default()).unwrap()
    }

    fn geo_metadata(bytes: &[u8]) -> Value {
        let reader = SerializedFileReader::new(bytes::Bytes::from(bytes.to_vec())).unwrap();
        let kv = reader
            .metadata()
            .file_metadata()
            .key_value_metadata()
            .unwrap()
            .iter()
            .find(|entry| entry.key == "geo")
            .unwrap()
            .value
            .clone()
            .unwrap();
        serde_json::from_str(&kv).unwrap()
    }

    #[test]
    fn the_geo_metadata_declares_version_encoding_and_crs() {
        let bytes = parquet_of(&[json!({
            "geometry": {"type": "Point", "coordinates": [30.06, -1.94]},
            "properties": {"name": "Kigali"}
        })]);
        let geo = geo_metadata(&bytes);
        assert_eq!(geo["version"], GEOPARQUET_VERSION);
        assert_eq!(geo["primary_column"], PARQUET_GEOMETRY_COLUMN);
        let column = &geo["columns"][PARQUET_GEOMETRY_COLUMN];
        assert_eq!(column["encoding"], "WKB");
        assert_eq!(column["crs"]["id"]["code"], "CRS84");
        assert_eq!(column["geometry_types"], json!(["Point"]));
        assert_eq!(column["bbox"], json!([30.06, -1.94, 30.06, -1.94]));
    }

    #[test]
    fn an_attributes_only_import_claims_no_geometry_type_and_no_bbox() {
        let bytes = parquet_of(&[json!({"properties": {"name": "no shape"}})]);
        let column = geo_metadata(&bytes)["columns"][PARQUET_GEOMETRY_COLUMN].clone();
        assert_eq!(column["geometry_types"], json!([]));
        assert!(
            column["bbox"].is_null(),
            "no geometry means no extent claim"
        );
    }

    #[test]
    fn provenance_is_written_when_supplied() {
        let built =
            batch::build(&[json!({"properties": {"a": 1}})], &[], Layout::GeoParquet).unwrap();
        let bytes = write(
            &built,
            &Provenance {
                source_digest: Some("abc123".into()),
                conversion: Some("{\"sheet\":\"s\"}".into()),
                ..Provenance::default()
            },
        )
        .unwrap();
        let reader = SerializedFileReader::new(bytes::Bytes::from(bytes)).unwrap();
        let kv = reader
            .metadata()
            .file_metadata()
            .key_value_metadata()
            .unwrap()
            .clone();
        let keys: Vec<&str> = kv.iter().map(|entry| entry.key.as_str()).collect();
        assert!(keys.contains(&"ds:source_digest"));
        assert!(keys.contains(&"ds:conversion"));
    }

    #[test]
    fn row_groups_respect_the_contract_ceiling() {
        let built =
            batch::build(&[json!({"properties": {"a": 1}})], &[], Layout::GeoParquet).unwrap();
        assert!(row_group_rows(&built.batch) <= MAX_ROW_GROUP_ROWS);
        let _ = Arc::new(0); // keep the import honest across arrow versions
    }
}
