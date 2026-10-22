//! The analytical artifact a large Desktop import produces, or replays.
//!
//! `NATIVE_DUCKDB_ANALYTICS_IMPLEMENTATION_PLAN.md` §0.8a: "A large local file
//! is converted to GeoParquet on arrival, and that conversion becomes the
//! analytical source of record for the import. Nothing large is parsed twice,
//! and no analytical path reads the original a second time."
//!
//! So conversion is not a step beside the import — it IS the import's read.
//! The streaming session in `native_compute` parses each bounded batch once and
//! sends it two places: to the browser as an Arrow IPC chunk, and to the
//! `ds-columnar` stream that writes the artifact. When the artifact for this
//! exact reading of this exact file already exists, the session reads THAT and
//! the source is not parsed at all.
//!
//! Nothing here decides a format. Schema, types, row groups, `geo` metadata,
//! provenance and identity all belong to `ds-columnar` (§0.8c); this module
//! decides only WHERE an artifact lives and what the frontend is told about it.

use ds_columnar::{ArtifactSummary, StreamedConversion};
use ds_io::SourceLayerInput;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Rows per chunk when replaying an artifact. The bound the streamed import
/// already delivers, so a replayed import hands the browser identically sized
/// work to a freshly converted one.
pub(crate) const ARTIFACT_REPLAY_ROWS: usize = 1024;

/// What the import tells the frontend about its analytical artifact.
///
/// Every identity here is the converter's. The shell computes none of it: it
/// asks `ds-columnar` where the artifact for a reading belongs, and reports
/// what the conversion or the artifact itself said.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeColumnarConversion {
    pub artifact_path: PathBuf,
    /// True when this exact reading of these exact bytes was already converted,
    /// so the source was neither parsed nor written again.
    pub reused: bool,
    pub layer_name: String,
    pub feature_count: usize,
    pub column_count: usize,
    pub source_digest: String,
    pub conversion_id: String,
    pub byte_count: usize,
    pub skipped_coordinate_rows: usize,
    pub elapsed_seconds: f64,
}

/// Where the artifact for one reading of one source belongs.
///
/// `source_digest` is the sha256 of the source bytes, streamed rather than
/// buffered — the identity has to be exact for reuse to be safe, and a
/// 1.15 GiB source must never become a 1.15 GiB allocation to compute it. The
/// cost is one extra sequential read on a fresh conversion; on a re-import it
/// is the only read of the source there is.
#[derive(Clone, Debug)]
pub(crate) struct ColumnarPlan {
    pub source_digest: String,
    pub conversion_id: String,
    pub artifact_path: PathBuf,
}

/// The post-parse parameters that change what this driver writes, on top of the
/// `ds-io` input the converter already folds in.
///
/// The Desktop import renumbers rows, promotes an elevation column and locks a
/// column set before the features reach `ds-columnar`, so two readings that
/// differ only in those are two different artifacts. Leaving them out would let
/// an import reuse an artifact written for a different elevation column.
#[derive(Clone, Debug, Default)]
pub(crate) struct ReadingParameters {
    pub data_start: usize,
    pub columns: Vec<String>,
    pub z_column: Option<String>,
}

impl ReadingParameters {
    fn to_value(&self) -> Value {
        json!({
            "dataStart": self.data_start,
            "columns": self.columns,
            "zColumn": self.z_column,
        })
    }
}

pub(crate) fn plan(
    root: &Path,
    source_path: &Path,
    file_name: &str,
    input: &SourceLayerInput,
    layer_name: Option<&str>,
    reading: &ReadingParameters,
) -> Result<ColumnarPlan, String> {
    let source_digest = ds_columnar::digest_file(source_path)?;
    let conversion_id = ds_columnar::conversion_identity(
        &source_digest,
        file_name,
        Some(input),
        layer_name,
        Some(&reading.to_value()),
    )?;
    let artifact_path = root
        .join(&source_digest)
        .join(format!("{conversion_id}.parquet"));
    Ok(ColumnarPlan {
        source_digest,
        conversion_id,
        artifact_path,
    })
}

/// The receipt for a conversion this run produced.
pub(crate) fn receipt_for_conversion(
    plan: &ColumnarPlan,
    conversion: &StreamedConversion,
    elapsed_seconds: f64,
) -> NativeColumnarConversion {
    NativeColumnarConversion {
        artifact_path: conversion.artifact_path.clone(),
        reused: false,
        layer_name: conversion.layer_name.clone(),
        feature_count: conversion.feature_count,
        column_count: conversion.column_count,
        source_digest: plan.source_digest.clone(),
        conversion_id: plan.conversion_id.clone(),
        byte_count: conversion.byte_count,
        skipped_coordinate_rows: conversion.skipped_coordinate_rows,
        elapsed_seconds,
    }
}

/// The receipt for an artifact that already existed. Everything is read out of
/// the artifact's own footer, so a reused conversion reports what is actually on
/// disk rather than what some side record remembered about it.
pub(crate) fn receipt_for_artifact(
    plan: &ColumnarPlan,
    artifact_path: &Path,
    summary: &ArtifactSummary,
    fallback_layer_name: &str,
    elapsed_seconds: f64,
) -> NativeColumnarConversion {
    NativeColumnarConversion {
        artifact_path: artifact_path.to_path_buf(),
        reused: true,
        layer_name: summary
            .layer_name
            .clone()
            .unwrap_or_else(|| fallback_layer_name.to_string()),
        feature_count: summary.feature_count,
        column_count: summary.columns.len(),
        source_digest: plan.source_digest.clone(),
        conversion_id: plan.conversion_id.clone(),
        byte_count: summary.byte_count,
        skipped_coordinate_rows: summary.skipped_coordinate_rows,
        elapsed_seconds,
    }
}

/// A layer name for a source that has not been parsed yet: the operator's
/// chosen name, else the file stem. Only ever a fallback — an artifact names
/// its own layer and a conversion learns it from `ds-io`.
pub(crate) fn fallback_layer_name(input: &SourceLayerInput, file_name: &str) -> String {
    input
        .layer_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            Path::new(file_name)
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn workspace(tag: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ds-web-native-columnar-{tag}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn source(root: &Path, bytes: &[u8]) -> PathBuf {
        let path = root.join("points.txt");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }

    fn input() -> SourceLayerInput {
        SourceLayerInput {
            sheet_key: "text".to_string(),
            separator: Some("whitespace".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "wgs84_lonlat".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: Some("points".to_string()),
            column_renames: vec![],
            drop_columns: vec![],
        }
    }

    #[test]
    fn an_artifact_is_addressed_by_the_converters_own_identity() {
        let root = workspace("address");
        let path = source(&root, b"x y\n30 -2\n");
        let store = root.join("columnar");

        let plan = plan(
            &store,
            &path,
            "points.txt",
            &input(),
            Some("points"),
            &ReadingParameters::default(),
        )
        .unwrap();

        assert_eq!(plan.source_digest.len(), 64);
        assert_eq!(plan.conversion_id.len(), 64);
        assert_eq!(
            plan.artifact_path,
            store
                .join(&plan.source_digest)
                .join(format!("{}.parquet", plan.conversion_id)),
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_different_post_parse_reading_of_the_same_bytes_is_a_different_artifact() {
        let root = workspace("reading");
        let path = source(&root, b"x y z\n30 -2 100\n");
        let store = root.join("columnar");
        let plain = ReadingParameters {
            data_start: 1,
            columns: vec!["x".into(), "y".into(), "z".into()],
            z_column: None,
        };
        let elevated = ReadingParameters {
            z_column: Some("z".into()),
            ..plain.clone()
        };

        let a = plan(&store, &path, "points.txt", &input(), None, &plain).unwrap();
        let b = plan(&store, &path, "points.txt", &input(), None, &elevated).unwrap();

        assert_eq!(a.source_digest, b.source_digest, "the same bytes");
        assert_ne!(
            a.conversion_id, b.conversion_id,
            "promoting elevation changes what is written"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_unchanged_reading_addresses_the_same_artifact_twice() {
        let root = workspace("stable");
        let path = source(&root, b"x y\n30 -2\n");
        let store = root.join("columnar");
        let reading = ReadingParameters {
            data_start: 1,
            columns: vec!["x".into(), "y".into()],
            z_column: None,
        };

        let a = plan(&store, &path, "points.txt", &input(), None, &reading).unwrap();
        let b = plan(&store, &path, "points.txt", &input(), None, &reading).unwrap();

        assert_eq!(a.artifact_path, b.artifact_path);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_layer_name_falls_back_to_the_file_stem_only_when_nothing_named_it() {
        let mut named = input();
        assert_eq!(
            fallback_layer_name(&named, "rwanda_sub1_orthocut.txt"),
            "points"
        );
        named.layer_name = Some("   ".to_string());
        assert_eq!(
            fallback_layer_name(&named, "rwanda_sub1_orthocut.txt"),
            "rwanda_sub1_orthocut",
        );
    }
}
