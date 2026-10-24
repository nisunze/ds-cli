//! The host half of reading a surveyor's design file (DECOUPLING-013).
//!
//! Design intake and the archival exported-design import admit the file here:
//! a regular, bounded file, and for a GeoPackage a closed one with no live
//! SQLite journal beside it. The Network engine reads the exact bytes behind
//! the closed `gis_source_io_v1` calls. A reader's refusal keeps its own words;
//! an engine that is absent or cannot serve the call is a typed refusal.
use ds_cli_contract::Failure;
use ds_cli_contract::spec::Refusal;
use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::source_io::{GeopackageLayersV1, SourceLayersV1, SourceReaderV1};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub(crate) const REFUSALS: [Refusal; ds_network_client::SOURCE_ENGINE_REFUSALS.len()] =
    source_refusals();

const fn source_refusals() -> [Refusal; ds_network_client::SOURCE_ENGINE_REFUSALS.len()] {
    let first = ds_network_client::SOURCE_ENGINE_REFUSALS[0];
    let mut out = [Refusal {
        code: first.code,
        when: first.when,
        remedy: first.remedy,
    }; ds_network_client::SOURCE_ENGINE_REFUSALS.len()];
    let mut index = 0;
    while index < out.len() {
        let row = ds_network_client::SOURCE_ENGINE_REFUSALS[index];
        out[index] = Refusal {
            code: row.code,
            when: row.when,
            remedy: row.remedy,
        };
        index += 1;
    }
    out
}

/// A reader's refusal (its words) or a typed engine refusal.
pub(crate) enum ReadFailure {
    Source(String),
    Engine(Failure),
}

impl ReadFailure {
    /// State a reader refusal in the caller's own code.
    pub(crate) fn into_failure(self, source: impl FnOnce(String) -> Failure) -> Failure {
        match self {
            Self::Source(message) => source(message),
            Self::Engine(failure) => failure,
        }
    }
}

fn engine_failure(error: NetworkFailure) -> ReadFailure {
    if error.is_source_refusal() {
        return ReadFailure::Source(error.message);
    }
    if unknown_owner_cause(&error) {
        return ReadFailure::Engine(unknown_engine_failure(&error));
    }
    let code = error.code();
    let message = error.message.clone();
    ReadFailure::Engine(match code {
        "network_engine_missing" | "module_removed" => {
            Failure::unavailable("network_engine_missing", message).remedy(
                "install Network or set DS_NETWORK_ENGINE_BIN to a compatible executable; design files are read there",
            )
        }
        "network_engine_incompatible" => Failure::unavailable("network_engine_incompatible", message)
            .remedy("select the insertion-order Network engine that serves gis_source_io_v1"),
        "network_serialization_incompatible" => Failure::unavailable("network_serialization_incompatible", message)
            .remedy("select the insertion-order Network engine that serves gis_source_io_v1"),
        "network_engine_path_invalid"
        | "network_engine_invalid"
        | "network_engine_unqualified"
        | "network_engine_identity_invalid"
        | "network_engine_changed"
        | "network_engine_refused"
        | "network_workers_invalid"
        | "network_scratch_invalid"
        | "network_request_invalid"
        | "network_request_io"
        | "grid_task_bound_refused"
        | "network_receipt_bounded"
        | "network_receipt_invalid"
        | "network_refusal_invalid"
        | "network_result_invalid"
        | "network_result_digest_mismatch"
        | "network_input_changed"
        | "callee_cancelled"
        | "callee_timed_out"
        | "callee_wait_failed"
        | "callee_supervision_failed"
        | "network_task_cancelled"
        | "network_control_unavailable"
        | "network_control_invalid"
        | "network_input_invalid"
        | "network_output_invalid"
        | "network_artifact_io"
        | "callee_stdin_failed"
        | "fast_lv_input_too_large"
        | "fast_lv_input_invalid"
        | "fast_lv_schema_unsupported"
        | "fast_lv_bound_refused"
        | "fast_lv_output_too_large"
        | "fast_lv_result_encoding_failed"
        | "fast_lv_publication_invalid"
        | "design_snapshot_processing_failed" => Failure::failed(code, message),
        _ => unknown_engine_failure(&error),
    })
}

fn unknown_engine_failure(error: &NetworkFailure) -> Failure {
    Failure::failed(
        "network_engine_refusal_unknown",
        "The Network engine returned an unrecognized source-read failure.",
    )
    .remedy("retain the bounded cause and use a compatible Network component")
    .detail(error.bounded_source_cause())
}

fn unknown_owner_cause(error: &NetworkFailure) -> bool {
    error.owner_code().is_some_and(|code| {
        code != "module_removed"
            && code != ds_network_client::SOURCE_UNREADABLE
            && !ds_network_client::SOURCE_ENGINE_REFUSALS
                .iter()
                .any(|row| row.code == code)
    })
}

fn capture() -> Result<CapturedNetworkClient, ReadFailure> {
    let lane = ds_cli_auth::reliability::release_lane();
    match lane {
        "local" => CapturedNetworkClient::capture(1, &|| false),
        _ => CapturedNetworkClient::capture_for_lane(lane, 1, &|| false),
    }
    .map_err(engine_failure)
}

/// One staged file read through the named reader.
pub(crate) fn layers(
    name: &str,
    bytes: Vec<u8>,
    reader: SourceReaderV1,
) -> Result<SourceLayersV1, ReadFailure> {
    capture()?
        .source_layers(name, bytes, reader, &|| false)
        .map_err(engine_failure)
}

/// The named tables of one closed GeoPackage, admitted here exactly as the
/// reader always admitted it: a canonical regular file within the byte bound,
/// with no live `-wal`, `-shm` or `-journal` beside it.
pub(crate) fn geopackage_tables(
    path: &Path,
    requested: &[String],
    max_rows: usize,
    max_bytes: usize,
) -> Result<GeopackageLayersV1, ReadFailure> {
    let bytes = closed_geopackage(path, max_bytes).map_err(ReadFailure::Source)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let tables = capture()?
        .geopackage_layers(bytes, requested, max_rows, max_bytes, &|| false)
        .map_err(engine_failure)?;
    if tables.source_sha256 != digest {
        return Err(ReadFailure::Source(
            "GeoPackage source changed while its layers were read".into(),
        ));
    }
    Ok(tables)
}

fn closed_geopackage(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let source =
        std::fs::canonicalize(path).map_err(|e| format!("cannot open GeoPackage source: {e}"))?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut adjacent = source.as_os_str().to_os_string();
        adjacent.push(suffix);
        if Path::new(&adjacent).exists() {
            return Err(
                "GeoPackage source has a live SQLite journal sidecar; use a closed frozen file"
                    .into(),
            );
        }
    }
    let file =
        std::fs::File::open(&source).map_err(|e| format!("cannot hash GeoPackage source: {e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("cannot inspect GeoPackage source: {e}"))?;
    if !metadata.is_file() || metadata.len() > max_bytes as u64 {
        return Err(format!(
            "GeoPackage source must be a regular file within {max_bytes} bytes"
        ));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read GeoPackage bytes: {e}"))?;
    if bytes.len() > max_bytes {
        return Err("GeoPackage source grew past its byte bound".into());
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "source_read_tests.rs"]
mod tests;
