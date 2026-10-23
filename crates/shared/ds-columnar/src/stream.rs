//! Bounded conversion: a source larger than memory, one artifact, one pass.
//!
//! Reading a source whole is the right shape for a file that fits, and the
//! wrong shape for the class this module exists for — `NATIVE_DUCKDB_ANALYTICS_
//! IMPLEMENTATION_PLAN.md` §8.4's `orthocut1` worst case is 1.15 GiB of XYZ
//! text, and holding it, its parsed features and its Arrow batches at once is
//! three copies of a file that already does not fit.
//!
//! So the caller drives it: it reads bounded records from wherever they live,
//! has its reader parse them, and hands the resulting features here batch by
//! batch. This module still owns every format decision — schema, types, row
//! groups, `geo` metadata, provenance, identity — so there is still exactly one
//! converter (§0.8c). What the caller contributes is transport, not format.
//!
//! Two properties are load-bearing:
//!
//! - **One schema, pinned once.** A Parquet file has a single schema, so the
//!   types are decided from the first batch and every later batch is checked
//!   against them. A batch that does not fit is refused BY NAME rather than
//!   written with the offending values nulled out.
//! - **Publication is a rename.** Bytes accumulate in a sibling `.partial`, and
//!   the artifact appears at its real path only after the footer is written and
//!   flushed. An interrupted conversion therefore leaves nothing that a later
//!   run could mistake for a complete artifact.

use crate::batch::{self, FeatureBatch, Layout};
use crate::geoparquet::{self, Provenance};
use crate::schema::{
    self, FEATURE_ID_COLUMN, IPC_GEOMETRY_COLUMN, PARQUET_GEOMETRY_COLUMN, PropertyKind,
};
use crate::wkb::bbox_of;
use arrow_array::{
    Array, ArrayRef, BinaryArray, LargeBinaryArray, LargeStringArray, RecordBatch, StringArray,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::format::KeyValue;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Bytes hashed per read while digesting a source. Bounded on purpose: the
/// point of streaming identity is that a 1.15 GiB source never becomes a 1.15
/// GiB allocation.
const DIGEST_CHUNK_BYTES: usize = 1024 * 1024;

/// sha256 of a file's bytes, read in bounded chunks.
///
/// The same value [`crate::identity::digest`] returns for the same bytes — it is
/// the source identity in [`crate::ConversionReceipt`], and a second definition
/// of it would mean two artifacts for one file.
pub fn digest_file(path: &Path) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut reader = BufReader::with_capacity(DIGEST_CHUNK_BYTES, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; DIGEST_CHUNK_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Identity of ordered Arrow fields, including nested types, nullability and
/// field metadata. Schema-level artifact provenance is deliberately excluded.
/// Map keys are recursively sorted so hash-map insertion order is immaterial.
pub fn schema_digest(schema: &Schema) -> Result<String, String> {
    let mut fields = serde_json::to_value(schema.fields())
        .map_err(|error| format!("could not describe columnar schema: {error}"))?;
    fields.sort_all_objects();
    let bytes = serde_json::to_vec(&fields)
        .map_err(|error| format!("could not encode columnar schema: {error}"))?;
    let mut hash = Sha256::new();
    hash.update(b"ds.columnar-schema/v1\0");
    hash.update(bytes);
    Ok(format!("{:x}", hash.finalize()))
}

/// What a completed streaming conversion produced.
#[derive(Clone, Debug, Serialize)]
pub struct StreamedConversion {
    pub artifact_path: PathBuf,
    pub layer_name: String,
    pub feature_count: usize,
    pub column_count: usize,
    pub columns: Vec<String>,
    pub byte_count: usize,
    pub skipped_coordinate_rows: usize,
    pub geometry_type: Option<String>,
}

/// A GeoParquet artifact being written from bounded batches.
pub struct ConversionStream {
    artifact_path: PathBuf,
    partial_path: PathBuf,
    provenance: Provenance,
    writer: Option<ArrowWriter<File>>,
    schema: Option<SchemaRef>,
    columns: Vec<String>,
    kinds: BTreeMap<String, PropertyKind>,
    geometry_types: Vec<&'static str>,
    bbox: Option<[f64; 4]>,
    feature_count: usize,
    skipped_coordinate_rows: usize,
    published: bool,
}

impl ConversionStream {
    /// Begin writing `artifact_path`. Nothing appears at that path until
    /// [`finish`](Self::finish) succeeds.
    pub fn create(artifact_path: &Path, provenance: Provenance) -> Result<Self, String> {
        if let Some(parent) = artifact_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "could not create the conversion directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let partial_path = partial_path(artifact_path);
        // A leftover partial from an interrupted run is scratch, never input.
        let _ = std::fs::remove_file(&partial_path);
        Ok(Self {
            artifact_path: artifact_path.to_path_buf(),
            partial_path,
            provenance,
            writer: None,
            schema: None,
            columns: Vec::new(),
            kinds: BTreeMap::new(),
            geometry_types: Vec::new(),
            bbox: None,
            feature_count: 0,
            skipped_coordinate_rows: 0,
            published: false,
        })
    }

    /// Rows the source carried that the inspection could not use. Reported on
    /// the artifact rather than dropped in silence.
    pub fn note_skipped_rows(&mut self, rows: usize) {
        self.skipped_coordinate_rows = self.skipped_coordinate_rows.saturating_add(rows);
    }

    pub fn feature_count(&self) -> usize {
        self.feature_count
    }

    /// Append one bounded batch of parsed features.
    ///
    /// The first call pins the column order and the type ladder; every later
    /// call is built against them and refused by name if it does not fit.
    pub fn append(&mut self, features: &[Value], columns_hint: &[String]) -> Result<(), String> {
        if features.is_empty() {
            return Ok(());
        }
        let built = match self.schema.as_ref() {
            None => {
                let built = batch::build(features, columns_hint, Layout::GeoParquet)?;
                self.columns = built.columns.clone();
                self.kinds = schema::infer_property_kinds(features, &self.columns);
                self.schema = Some(built.batch.schema());
                let properties = geoparquet::writer_properties(
                    geoparquet::row_group_rows(&built.batch),
                    Vec::new(),
                );
                let file = File::create(&self.partial_path).map_err(|error| {
                    format!(
                        "could not open the conversion artifact {}: {error}",
                        self.partial_path.display()
                    )
                })?;
                self.writer = Some(
                    ArrowWriter::try_new(file, built.batch.schema(), Some(properties)).map_err(
                        |error| format!("could not open the GeoParquet writer: {error}"),
                    )?,
                );
                built
            }
            Some(pinned) => {
                self.check_fit(features)?;
                let built = batch::build_with_kinds(
                    features,
                    &self.columns,
                    &self.kinds,
                    Layout::GeoParquet,
                )?;
                if built.batch.schema() != *pinned {
                    return Err(schema_drift(pinned, &built.batch.schema()));
                }
                built
            }
        };
        self.absorb(&built);
        let writer = self
            .writer
            .as_mut()
            .ok_or("the GeoParquet stream has no open writer")?;
        writer
            .write(&built.batch)
            .map_err(|error| format!("could not write the GeoParquet batch: {error}"))?;
        Ok(())
    }

    /// Close the file and publish it at its real path.
    pub fn finish(mut self, layer_name: &str) -> Result<StreamedConversion, String> {
        self.provenance.layer_name = Some(layer_name.to_string());
        self.provenance.skipped_coordinate_rows = Some(self.skipped_coordinate_rows);
        if self.writer.is_none() {
            // A conversion that saw no feature still produces a readable,
            // correctly-described empty artifact rather than nothing at all.
            let built = batch::build(&[], &[], Layout::GeoParquet)?;
            let properties =
                geoparquet::writer_properties(geoparquet::row_group_rows(&built.batch), Vec::new());
            let file = File::create(&self.partial_path).map_err(|error| {
                format!(
                    "could not open the conversion artifact {}: {error}",
                    self.partial_path.display()
                )
            })?;
            let mut writer = ArrowWriter::try_new(file, built.batch.schema(), Some(properties))
                .map_err(|error| format!("could not open the GeoParquet writer: {error}"))?;
            writer
                .write(&built.batch)
                .map_err(|error| format!("could not write the GeoParquet batch: {error}"))?;
            self.schema = Some(built.batch.schema());
            self.writer = Some(writer);
        }

        let mut writer = self
            .writer
            .take()
            .ok_or("the GeoParquet stream has no open writer")?;
        let geo = geoparquet::geo_metadata(&self.geometry_types, self.bbox);
        writer.append_key_value_metadata(KeyValue::new("geo".to_string(), geo.to_string()));
        for entry in geoparquet::provenance_metadata(&self.provenance) {
            writer.append_key_value_metadata(entry);
        }
        let file = writer
            .into_inner()
            .map_err(|error| format!("could not finish the GeoParquet file: {error}"))?;
        // Durable before visible: a rename that outruns its own bytes would
        // publish an artifact a crash could leave truncated.
        file.sync_all().map_err(|error| {
            format!(
                "could not flush the conversion artifact {}: {error}",
                self.partial_path.display()
            )
        })?;
        drop(file);

        let byte_count = std::fs::metadata(&self.partial_path)
            .map_err(|error| {
                format!(
                    "could not measure the conversion artifact {}: {error}",
                    self.partial_path.display()
                )
            })?
            .len() as usize;
        std::fs::rename(&self.partial_path, &self.artifact_path).map_err(|error| {
            format!(
                "could not publish the conversion artifact {}: {error}",
                self.artifact_path.display()
            )
        })?;
        self.published = true;

        Ok(StreamedConversion {
            artifact_path: self.artifact_path.clone(),
            layer_name: layer_name.to_string(),
            feature_count: self.feature_count,
            column_count: self.columns.len(),
            columns: self.columns.clone(),
            byte_count,
            skipped_coordinate_rows: self.skipped_coordinate_rows,
            geometry_type: single_geometry_type(&self.geometry_types),
        })
    }

    /// Give up on the artifact, leaving nothing behind. The caller's own work
    /// is untouched — abandoning a conversion costs analysis, not the import.
    pub fn abandon(mut self) {
        self.writer = None;
        let _ = std::fs::remove_file(&self.partial_path);
    }

    fn absorb(&mut self, built: &FeatureBatch) {
        self.feature_count = self.feature_count.saturating_add(built.batch.num_rows());
        for name in geoparquet::geometry_type_names(&built.geometries) {
            if !self.geometry_types.contains(&name) {
                self.geometry_types.push(name);
            }
        }
        self.geometry_types.sort_unstable();
        if let Some(bbox) = bbox_of(&built.geometries) {
            self.bbox = Some(match self.bbox {
                None => bbox,
                Some(current) => [
                    current[0].min(bbox[0]),
                    current[1].min(bbox[1]),
                    current[2].max(bbox[2]),
                    current[3].max(bbox[3]),
                ],
            });
        }
    }

    /// Every reason a batch could not be written into the pinned schema, named.
    fn check_fit(&self, features: &[Value]) -> Result<(), String> {
        for column in schema::columns_for_features(features) {
            if !self.columns.contains(&column) {
                return Err(format!(
                    "column `{column}` appeared after the artifact schema was fixed on {} column(s); \
                     a GeoParquet file holds one schema",
                    self.columns.len()
                ));
            }
        }
        let observed = schema::infer_property_kinds(features, &self.columns);
        for column in &self.columns {
            let pinned = self
                .kinds
                .get(column)
                .copied()
                .unwrap_or(PropertyKind::Text);
            let seen = observed.get(column).copied().unwrap_or(PropertyKind::Null);
            if !schema::accepts(pinned, seen) {
                return Err(format!(
                    "column `{column}` holds {seen:?} values after the artifact schema fixed it as \
                     {pinned:?}; writing them would lose data"
                ));
            }
        }
        Ok(())
    }
}

impl Drop for ConversionStream {
    fn drop(&mut self) {
        if !self.published {
            self.writer = None;
            let _ = std::fs::remove_file(&self.partial_path);
        }
    }
}

fn schema_drift(pinned: &SchemaRef, observed: &SchemaRef) -> String {
    let names = |schema: &SchemaRef| {
        schema
            .fields()
            .iter()
            .map(|field| format!("{}:{}", field.name(), field.data_type()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "the artifact schema is fixed as [{}] and this batch is [{}]",
        names(pinned),
        names(observed)
    )
}

fn single_geometry_type(types: &[&'static str]) -> Option<String> {
    match types {
        [only] => Some((*only).to_string()),
        [] => None,
        _ => Some("Mixed".to_string()),
    }
}

fn partial_path(artifact_path: &Path) -> PathBuf {
    let mut name = artifact_path.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    artifact_path.with_file_name(name)
}

/// What a converted artifact says about itself, read from its footer alone.
#[derive(Clone, Debug, Serialize)]
pub struct ArtifactSummary {
    pub layer_name: Option<String>,
    pub feature_count: usize,
    pub columns: Vec<String>,
    pub byte_count: usize,
    pub skipped_coordinate_rows: usize,
    pub geometry_type: Option<String>,
    pub source_digest: Option<String>,
    pub conversion: Option<String>,
}

/// Bounded read-back of a converted artifact as Arrow IPC chunks.
///
/// This is what makes the artifact the source of record rather than a
/// by-product: re-importing an already-converted source reads THIS, and the
/// original is never parsed a second time (§0.8a).
pub struct ArtifactReader {
    reader: parquet::arrow::arrow_reader::ParquetRecordBatchReader,
    summary: ArtifactSummary,
    schema: SchemaRef,
}

impl ArtifactReader {
    pub fn open(artifact_path: &Path, batch_rows: usize) -> Result<Self, String> {
        let file = File::open(artifact_path).map_err(|error| {
            format!(
                "could not open the converted artifact {}: {error}",
                artifact_path.display()
            )
        })?;
        let byte_count = file
            .metadata()
            .map_err(|error| {
                format!(
                    "could not measure the converted artifact {}: {error}",
                    artifact_path.display()
                )
            })?
            .len() as usize;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|error| {
            format!(
                "could not read the converted artifact {}: {error}",
                artifact_path.display()
            )
        })?;
        let metadata = builder.metadata().clone();
        let schema = builder.schema().clone();
        let file_metadata = metadata.file_metadata();
        let feature_count = usize::try_from(file_metadata.num_rows().max(0)).unwrap_or(0);
        let columns = builder
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .filter(|name| name != FEATURE_ID_COLUMN && name != PARQUET_GEOMETRY_COLUMN)
            .collect::<Vec<_>>();
        let entry = |key: &str| -> Option<String> {
            file_metadata
                .key_value_metadata()?
                .iter()
                .find(|kv| kv.key == key)
                .and_then(|kv| kv.value.clone())
        };
        let geometry_type = entry("geo")
            .and_then(|geo| serde_json::from_str::<Value>(&geo).ok())
            .and_then(|geo| {
                let types = geo["columns"][PARQUET_GEOMETRY_COLUMN]["geometry_types"]
                    .as_array()?
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>();
                match types.len() {
                    0 => None,
                    1 => Some(types[0].clone()),
                    _ => Some("Mixed".to_string()),
                }
            });
        let summary = ArtifactSummary {
            layer_name: entry(geoparquet::LAYER_NAME_KEY),
            feature_count,
            columns,
            byte_count,
            skipped_coordinate_rows: entry(geoparquet::SKIPPED_ROWS_KEY)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            geometry_type,
            source_digest: entry("ds:source_digest"),
            conversion: entry("ds:conversion"),
        };
        let reader = builder
            .with_batch_size(batch_rows.max(1))
            .build()
            .map_err(|error| {
                format!(
                    "could not read the converted artifact {}: {error}",
                    artifact_path.display()
                )
            })?;
        Ok(Self {
            reader,
            summary,
            schema,
        })
    }

    pub fn summary(&self) -> &ArtifactSummary {
        &self.summary
    }

    /// The actual Parquet schema, rather than a caller's declared column list.
    pub fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    /// Bounded native read without converting through browser IPC. Both read
    /// methods advance the same cursor and propagate corrupt-page errors.
    pub fn next_batch(&mut self) -> Result<Option<RecordBatch>, String> {
        self.reader
            .next()
            .transpose()
            .map_err(|error| format!("could not read the converted artifact: {error}"))
    }

    /// The next bounded chunk, in the Arrow IPC layout the browser working set
    /// already decodes. `None` when the artifact is exhausted.
    pub fn next_chunk(&mut self) -> Result<Option<ArtifactChunk>, String> {
        let Some(batch) = self.next_batch()? else {
            return Ok(None);
        };
        Ok(Some(ArtifactChunk {
            rows: batch.num_rows(),
            ipc: crate::ipc::batch_to_ipc(&to_ipc_layout(&batch)?)?,
        }))
    }
}

/// One bounded chunk of a converted artifact. The row count comes from the
/// batch itself, so a caller never has to decode the bytes again to learn how
/// many features it just received.
pub struct ArtifactChunk {
    pub ipc: Vec<u8>,
    pub rows: usize,
}

/// The GeoParquet layout to the Arrow IPC one: 32-bit offsets widen to 64-bit
/// and the geometry column takes its browser-contract name (§2). Nothing else
/// changes, which is why a chunk read back from an artifact is interchangeable
/// with one the conversion emitted.
fn to_ipc_layout(batch: &RecordBatch) -> Result<RecordBatch, String> {
    let mut fields = Vec::with_capacity(batch.num_columns());
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(batch.num_columns());
    for (index, field) in batch.schema().fields().iter().enumerate() {
        let column = batch.column(index);
        let name = if field.name() == PARQUET_GEOMETRY_COLUMN {
            IPC_GEOMETRY_COLUMN
        } else {
            field.name().as_str()
        };
        let (data_type, array): (DataType, ArrayRef) = match field.data_type() {
            DataType::Utf8 => {
                let values = column
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| format!("column `{name}` is not Utf8"))?;
                (
                    DataType::LargeUtf8,
                    Arc::new(
                        (0..values.len())
                            .map(|row| (!values.is_null(row)).then(|| values.value(row)))
                            .collect::<LargeStringArray>(),
                    ),
                )
            }
            DataType::Binary => {
                let values = column
                    .as_any()
                    .downcast_ref::<BinaryArray>()
                    .ok_or_else(|| format!("column `{name}` is not Binary"))?;
                (
                    DataType::LargeBinary,
                    Arc::new(
                        (0..values.len())
                            .map(|row| (!values.is_null(row)).then(|| values.value(row)))
                            .collect::<LargeBinaryArray>(),
                    ),
                )
            }
            other => (other.clone(), Arc::clone(column)),
        };
        fields.push(Field::new(name, data_type, true));
        arrays.push(array);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .map_err(|error| format!("could not re-layout the artifact batch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity as convert;
    use serde_json::json;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn workspace(tag: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ds-columnar-stream-{tag}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn point(index: usize) -> Value {
        json!({
            "geometry": {"type": "Point", "coordinates": [30.0 + index as f64 / 1000.0, -2.0]},
            "properties": {"name": format!("p{index}"), "n": index as i64}
        })
    }

    #[test]
    fn schema_identity_tracks_fields_not_provenance_or_map_order() {
        use std::collections::HashMap;
        let field = |reverse: bool| {
            let entries = if reverse {
                [("z", "last"), ("a", "first")]
            } else {
                [("a", "first"), ("z", "last")]
            };
            Field::new("value", DataType::Int64, true).with_metadata(
                entries
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            )
        };
        let schema = Schema::new(vec![field(false)]);
        let identity = schema_digest(&schema).unwrap();
        assert_eq!(
            identity,
            schema_digest(&Schema::new(vec![field(true)])).unwrap()
        );
        let with_provenance = Schema::new_with_metadata(
            vec![field(true)],
            HashMap::from([("ds:source_digest".to_string(), "another-source".to_string())]),
        );
        assert_eq!(identity, schema_digest(&with_provenance).unwrap());
        for changed in [
            Schema::new(vec![Field::new("other", DataType::Int64, true)]),
            Schema::new(vec![Field::new("value", DataType::Float64, true)]),
            Schema::new(vec![Field::new("value", DataType::Int64, false)]),
            Schema::new(vec![Field::new("value", DataType::Int64, true)]),
        ] {
            assert_ne!(identity, schema_digest(&changed).unwrap());
        }
        let fields = vec![field(false), Field::new("second", DataType::Utf8, true)];
        let mut reversed = fields.clone();
        reversed.reverse();
        assert_ne!(
            schema_digest(&Schema::new(fields)).unwrap(),
            schema_digest(&Schema::new(reversed)).unwrap()
        );
    }

    #[test]
    fn native_batches_preserve_identity_geometry_and_share_the_ipc_cursor() {
        let root = workspace("native-readback");
        let artifact = root.join("out.parquet");
        let features: Vec<Value> = (0..5)
            .map(|i| {
                let mut feature = point(i);
                feature["id"] = json!(format!("stable-{i}"));
                feature
            })
            .collect();
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        stream.append(&features, &[]).unwrap();
        stream.finish("points").unwrap();
        let mut reader = ArtifactReader::open(&artifact, 2).unwrap();
        let expected_schema = schema_digest(reader.schema()).unwrap();
        let batch = reader.next_batch().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 2);
        let native_features = crate::batch_to_features(&batch, 2).unwrap();
        assert_eq!(native_features[0]["id"], features[0]["id"]);
        assert_eq!(native_features[0]["properties"], features[0]["properties"]);
        assert_eq!(native_features[1]["geometry"], features[1]["geometry"]);
        assert_eq!(
            schema_digest(batch.schema().as_ref()).unwrap(),
            expected_schema
        );
        let ids = batch
            .column_by_name(FEATURE_ID_COLUMN)
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(ids.value(0), "stable-0");
        let geometry = batch
            .column_by_name(PARQUET_GEOMETRY_COLUMN)
            .unwrap()
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap();
        assert_eq!(
            ds_geo_lite::wkb::geometry_from_wkb(geometry.value(1), 2).unwrap(),
            features[1]["geometry"]
        );
        let ipc = reader.next_chunk().unwrap().unwrap();
        assert_eq!(ipc.rows, 2);
        assert_eq!(crate::ipc::ipc_to_rows(&ipc.ipc).unwrap()[0]["name"], "p2");
        assert_eq!(reader.next_batch().unwrap().unwrap().num_rows(), 1);
        assert!(reader.next_batch().unwrap().is_none());
        assert!(reader.next_chunk().unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_source_larger_than_one_batch_converts_in_bounded_batches() {
        let root = workspace("bounded");
        let artifact = root.join("out.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        for chunk in 0..5 {
            let features: Vec<Value> = (0..1_024).map(|row| point(chunk * 1_024 + row)).collect();
            stream
                .append(&features, &["name".to_string(), "n".to_string()])
                .unwrap();
        }
        let out = stream.finish("points").unwrap();

        assert_eq!(out.feature_count, 5 * 1_024);
        assert_eq!(out.layer_name, "points");
        assert_eq!(out.geometry_type.as_deref(), Some("Point"));
        assert!(out.byte_count > 0);
        let bytes = std::fs::read(&artifact).unwrap();
        assert!(bytes.starts_with(b"PAR1") && bytes.ends_with(b"PAR1"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_streamed_artifact_describes_itself_exactly_as_a_single_pass_one_does() {
        let root = workspace("parity");
        let features: Vec<Value> = (0..64).map(point).collect();
        let hint = vec!["name".to_string(), "n".to_string()];

        let single = {
            let built = batch::build(&features, &hint, Layout::GeoParquet).unwrap();
            geoparquet::write(&built, &Provenance::default()).unwrap()
        };
        let artifact = root.join("streamed.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        // Halved, so the streamed file is genuinely more than one batch.
        stream.append(&features[..32], &hint).unwrap();
        stream.append(&features[32..], &hint).unwrap();
        stream.finish("points").unwrap();

        let read_geo = |bytes: &[u8]| -> Value {
            use parquet::file::reader::FileReader;
            let reader = parquet::file::reader::SerializedFileReader::new(bytes::Bytes::from(
                bytes.to_vec(),
            ))
            .unwrap();
            let value = reader
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
            serde_json::from_str(&value).unwrap()
        };
        let streamed = std::fs::read(&artifact).unwrap();
        assert_eq!(
            read_geo(&single),
            read_geo(&streamed),
            "one converter, one description of what it wrote"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_artifact_reads_back_as_the_bounded_chunks_the_import_consumes() {
        let root = workspace("readback");
        let artifact = root.join("out.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        stream.note_skipped_rows(7);
        let features: Vec<Value> = (0..2_500).map(point).collect();
        stream
            .append(&features, &["name".to_string(), "n".to_string()])
            .unwrap();
        stream.finish("points").unwrap();

        let mut reader = ArtifactReader::open(&artifact, 1_024).unwrap();
        assert_eq!(reader.summary().layer_name.as_deref(), Some("points"));
        assert_eq!(reader.summary().feature_count, 2_500);
        assert_eq!(reader.summary().skipped_coordinate_rows, 7);
        assert_eq!(reader.summary().geometry_type.as_deref(), Some("Point"));
        assert_eq!(reader.summary().columns, vec!["name", "n"]);

        let mut sizes = Vec::new();
        let mut rows = Vec::new();
        while let Some(chunk) = reader.next_chunk().unwrap() {
            let batch = crate::ipc::ipc_to_rows(&chunk.ipc).unwrap();
            assert_eq!(batch.len(), chunk.rows, "the chunk counts what it carries");
            sizes.push(batch.len());
            rows.extend(batch);
            // The geometry survives the round trip in the browser's own column.
            assert!(
                crate::ipc::ipc_to_geometry_wkb(&chunk.ipc)
                    .unwrap()
                    .iter()
                    .all(Option::is_some)
            );
        }
        assert!(
            sizes.iter().all(|size| *size <= 1_024),
            "bounded: {sizes:?}"
        );
        assert_eq!(rows.len(), 2_500);
        assert_eq!(rows[0]["name"], json!("p0"));
        assert_eq!(rows[2_499]["n"], json!(2_499));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_unfinished_conversion_leaves_nothing_a_later_run_could_reuse() {
        let root = workspace("atomic");
        let artifact = root.join("out.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        stream.append(&[point(0)], &[]).unwrap();
        assert!(!artifact.exists(), "nothing is published mid-stream");
        stream.abandon();

        assert!(!artifact.exists());
        assert!(!partial_path(&artifact).exists(), "the partial is removed");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_dropped_conversion_removes_its_partial_too() {
        let root = workspace("dropped");
        let artifact = root.join("out.parquet");
        {
            let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
            stream.append(&[point(0)], &[]).unwrap();
        }
        assert!(!artifact.exists());
        assert!(!partial_path(&artifact).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_value_that_will_not_fit_the_pinned_schema_is_refused_by_name() {
        let root = workspace("pinned");
        let artifact = root.join("out.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        stream
            .append(&[json!({"properties": {"n": 1}})], &["n".to_string()])
            .unwrap();

        let error = stream
            .append(
                &[json!({"properties": {"n": "not a number"}})],
                &["n".to_string()],
            )
            .unwrap_err();

        assert!(error.contains('n'), "names the column: {error}");
        assert!(
            error.contains("lose data"),
            "says what it protects: {error}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_column_that_appears_after_the_schema_is_fixed_is_refused_by_name() {
        let root = workspace("late-column");
        let artifact = root.join("out.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        stream
            .append(&[json!({"properties": {"a": 1}})], &["a".to_string()])
            .unwrap();

        let error = stream
            .append(
                &[json!({"properties": {"a": 2, "late": 3}})],
                &["a".to_string()],
            )
            .unwrap_err();

        assert!(error.contains("late"), "names the column: {error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_narrower_batch_still_fits_the_wider_pinned_column() {
        let root = workspace("widening");
        let artifact = root.join("out.parquet");
        let mut stream = ConversionStream::create(&artifact, Provenance::default()).unwrap();
        // Pinned Float64 by the first batch...
        stream
            .append(&[json!({"properties": {"n": 1.5}})], &["n".to_string()])
            .unwrap();
        // ...and an integer is representable in it, so it is not a conflict.
        stream
            .append(&[json!({"properties": {"n": 2}})], &["n".to_string()])
            .unwrap();
        // A text column absorbs anything, because that is what it stores.
        stream
            .append(
                &[json!({"properties": {"n": 3, "name": "x"}})],
                &["n".to_string()],
            )
            .unwrap_err();
        let out = stream.finish("points").unwrap();
        assert_eq!(out.feature_count, 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_streamed_digest_is_the_digest_of_the_same_bytes() {
        let root = workspace("digest");
        let path = root.join("source.csv");
        let bytes: Vec<u8> = (0..(3 * DIGEST_CHUNK_BYTES + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        File::create(&path).unwrap().write_all(&bytes).unwrap();

        assert_eq!(digest_file(&path).unwrap(), convert::digest(&bytes));
        std::fs::remove_dir_all(root).unwrap();
    }
}
