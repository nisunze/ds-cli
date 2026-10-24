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
//! artifact's append log. When the source has been read to its end, the
//! Network engine's streamed writer replays that log into the artifact. When
//! the artifact for this exact reading of this exact file already exists, the
//! session reads THAT, through the engine, and the source is not parsed at all.
//!
//! Nothing here decides a format. Schema, types, row groups, `geo` metadata
//! and provenance are the converter's, which runs in the Network engine
//! (`columnar_io_v1`); the identity is `ds-columnar`'s. This module decides
//! only WHERE an artifact lives, moves its bytes, and what the frontend is told.

use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::columnar_io::{
    GeoparquetStreamParamsV1, GeoparquetStreamResultV1, GeoparquetSummaryV1,
};
use ds_network_contracts::grid_operation::MAX_GRID_OPERATION_INPUT_BYTES;
use ds_network_contracts::source_io::SourceReadingV1;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Rows per chunk when replaying an artifact. The bound the streamed import
/// already delivers, so a replayed import hands the browser identically sized
/// work to a freshly converted one.
pub(crate) const ARTIFACT_REPLAY_ROWS: usize = 1024;

/// What the import tells the frontend about its analytical artifact.
///
/// Every identity here is the converter's. The shell computes none of it: it
/// asks `ds-columnar` where the artifact for a reading belongs, and reports
/// what the engine's conversion or the artifact itself said.
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
/// source reading the converter already folds in.
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
    input: &SourceReadingV1,
    layer_name: Option<&str>,
    reading: &ReadingParameters,
) -> Result<ColumnarPlan, String> {
    let source_digest = ds_columnar::digest_file(source_path)?;
    let input = serde_json::to_value(input)
        .map_err(|error| format!("could not encode the source reading: {error}"))?;
    let conversion_id = ds_columnar::conversion_identity(
        &source_digest,
        file_name,
        Some(&input),
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
    conversion: &GeoparquetStreamResultV1,
    elapsed_seconds: f64,
) -> NativeColumnarConversion {
    NativeColumnarConversion {
        artifact_path: plan.artifact_path.clone(),
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
    summary: &GeoparquetSummaryV1,
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
/// its own layer and a conversion learns it from the reader.
pub(crate) fn fallback_layer_name(input: &SourceReadingV1, file_name: &str) -> String {
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

/// An engine failure in the words the import reports: the converter's own
/// refusal unchanged, anything else with its code.
pub(crate) fn engine_message(failure: NetworkFailure) -> String {
    if failure.is_source_refusal() || failure.is_columnar_refusal() {
        failure.message
    } else {
        format!("{}: {}", failure.code(), failure.message)
    }
}

/// The appends one conversion makes, spooled beside the artifact's address
/// while the source is read. Nothing appears at the artifact's path until
/// [`finish`](Self::finish) has the engine replay the whole log and the bytes
/// it returns are durable: publication is still a rename.
pub(crate) struct ArtifactSpool {
    artifact_path: PathBuf,
    spool_path: PathBuf,
    writer: Option<BufWriter<File>>,
    bytes: u64,
    source_digest: String,
    conversion: String,
}

fn sibling(artifact_path: &Path, suffix: &str) -> PathBuf {
    let mut name = artifact_path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    artifact_path.with_file_name(name)
}

impl ArtifactSpool {
    pub(crate) fn create(plan: &ColumnarPlan) -> Result<Self, String> {
        let artifact_path = plan.artifact_path.clone();
        if let Some(parent) = artifact_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "could not create the conversion directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let spool_path = sibling(&artifact_path, ".appends");
        // Leftovers from an interrupted run are scratch, never input.
        let _ = std::fs::remove_file(sibling(&artifact_path, ".partial"));
        let file = File::create(&spool_path).map_err(|error| {
            format!(
                "could not open the conversion log {}: {error}",
                spool_path.display()
            )
        })?;
        Ok(Self {
            artifact_path,
            spool_path,
            writer: Some(BufWriter::new(file)),
            bytes: 0,
            source_digest: plan.source_digest.clone(),
            conversion: plan.conversion_id.clone(),
        })
    }

    /// One bounded append: these features, the column order hint, and the
    /// rows the reader rejected for unusable coordinates in this batch.
    pub(crate) fn append(
        &mut self,
        features: &[Value],
        columns: &[String],
        skipped_coordinate_rows: usize,
    ) -> Result<(), String> {
        let line = serde_json::to_vec(&json!({
            "features": features,
            "columns": columns,
            "skipped_coordinate_rows": skipped_coordinate_rows,
        }))
        .map_err(|error| format!("could not encode the conversion log: {error}"))?;
        self.bytes = self.bytes.saturating_add(line.len() as u64 + 1);
        if self.bytes > MAX_GRID_OPERATION_INPUT_BYTES {
            return Err(format!(
                "the analytical artifact's features exceed the Network engine's {} MiB staged \
                 input bound; the layer import is unaffected",
                MAX_GRID_OPERATION_INPUT_BYTES / (1024 * 1024)
            ));
        }
        let writer = self.writer.as_mut().ok_or("the conversion log is closed")?;
        writer
            .write_all(&line)
            .and_then(|()| writer.write_all(b"\n"))
            .map_err(|error| format!("could not write the conversion log: {error}"))
    }

    /// Have the engine replay the log into the artifact, then publish it.
    pub(crate) fn finish(
        mut self,
        engine: &CapturedNetworkClient,
        layer_name: &str,
    ) -> Result<GeoparquetStreamResultV1, String> {
        let mut writer = self.writer.take().ok_or("the conversion log is closed")?;
        writer
            .flush()
            .map_err(|error| format!("could not flush the conversion log: {error}"))?;
        drop(writer);
        let log = std::fs::read(&self.spool_path)
            .map_err(|error| format!("could not read the conversion log: {error}"))?;
        let (conversion, bytes) = engine
            .geoparquet_stream(
                GeoparquetStreamParamsV1 {
                    source_digest: Some(self.source_digest.clone()),
                    conversion: Some(self.conversion.clone()),
                    layer_name: layer_name.to_string(),
                },
                log,
                &|| false,
            )
            .map_err(engine_message)?;
        let partial = sibling(&self.artifact_path, ".partial");
        let written = File::create(&partial)
            .and_then(|mut file| {
                file.write_all(&bytes)?;
                // Durable before visible: a rename that outruns its own
                // bytes would publish an artifact a crash could truncate.
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&partial, &self.artifact_path));
        if let Err(error) = written {
            let _ = std::fs::remove_file(&partial);
            return Err(format!(
                "could not publish the conversion artifact {}: {error}",
                self.artifact_path.display()
            ));
        }
        Ok(conversion)
    }

    /// Give up on the artifact, leaving nothing behind.
    pub(crate) fn abandon(self) {}
}

impl Drop for ArtifactSpool {
    fn drop(&mut self) {
        self.writer = None;
        let _ = std::fs::remove_file(&self.spool_path);
    }
}

/// One bounded chunk replayed out of an artifact, in the browser layout.
pub(crate) struct ReplayChunk {
    pub ipc: Vec<u8>,
    pub rows: usize,
}

/// Bounded read-back of a converted artifact as Arrow IPC chunks, decoded by
/// the Network engine one page at a time. Re-importing an already-converted
/// source reads THIS, and the original is never parsed a second time (§0.8a).
pub(crate) struct ArtifactReplay {
    engine: CapturedNetworkClient,
    artifact_path: PathBuf,
    summary: GeoparquetSummaryV1,
    pending: VecDeque<ReplayChunk>,
    next_row: Option<usize>,
}

impl ArtifactReplay {
    /// Opening reads the first page: an artifact the engine cannot read is
    /// not an artifact, so a truncated file converts again.
    pub(crate) fn open(
        artifact_path: &Path,
        engine: CapturedNetworkClient,
    ) -> Result<Self, String> {
        let mut replay = Self {
            engine,
            artifact_path: artifact_path.to_path_buf(),
            summary: GeoparquetSummaryV1 {
                layer_name: None,
                feature_count: 0,
                columns: Vec::new(),
                byte_count: 0,
                skipped_coordinate_rows: 0,
                geometry_type: None,
                source_digest: None,
                conversion: None,
                schema_sha256: String::new(),
                geo: None,
            },
            pending: VecDeque::new(),
            next_row: Some(0),
        };
        replay.fetch()?;
        Ok(replay)
    }

    pub(crate) fn summary(&self) -> &GeoparquetSummaryV1 {
        &self.summary
    }

    fn fetch(&mut self) -> Result<(), String> {
        let Some(start_row) = self.next_row.take() else {
            return Ok(());
        };
        let held = std::fs::read(&self.artifact_path).map_err(|error| {
            format!(
                "could not open the converted artifact {}: {error}",
                self.artifact_path.display()
            )
        })?;
        let name = self
            .artifact_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let page = self
            .engine
            .geoparquet_ipc_page(&name, &held, ARTIFACT_REPLAY_ROWS, start_row, &|| false)
            .map_err(engine_message)?;
        if start_row > 0 && page.summary != self.summary {
            return Err("the converted artifact changed while it was replayed".into());
        }
        self.summary = page.summary;
        self.pending.extend(
            page.chunks
                .into_iter()
                .map(|(rows, ipc)| ReplayChunk { ipc, rows }),
        );
        self.next_row = page.next_row;
        Ok(())
    }

    /// The next bounded chunk. `None` when the artifact is exhausted.
    pub(crate) fn next_chunk(&mut self) -> Result<Option<ReplayChunk>, String> {
        if self.pending.is_empty() {
            self.fetch()?;
        }
        Ok(self.pending.pop_front())
    }
}

#[cfg(test)]
#[path = "columnar_tests.rs"]
mod tests;
