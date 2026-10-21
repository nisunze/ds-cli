//! The whole-model spotting receipt file, read once for both receipt doors.
//!
//! A `ds dsgrid run` of `plan_whole_model_spotting` returns one receipt: the
//! exact source package, its authored revision and the batch of sealed
//! per-alignment plans. Two commands consume it. `dsgrid spotting
//! preview-receipt` turns a receipt whose diagnostic rejected rows were
//! truncated into a visualization-only package; `dsgrid spotting
//! apply-receipt` lands a complete receipt's digest-sealed plans as one
//! engineering revision. This host reads the exact file under its bound, pins
//! its bytes by digest and expands a Zstandard receipt under its expansion
//! bound. The Network owner decodes the sealed plans and enforces the shared
//! receipt gate (operation, source package, revision, complete batch), so the
//! two doors refuse with the same codes for the same receipt.

use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use serde_json::json;
use sha2::{Digest, Sha256};

/// How a receipt file may be encoded.
#[derive(Clone, Copy)]
pub(crate) enum ReceiptEncoding {
    /// Zstandard-compressed JSON only (`.json.zst`).
    Compressed,
    /// Zstandard-compressed or plain JSON, told apart by the frame magic.
    CompressedOrPlain,
}

/// Read bounds of one receipt door.
#[derive(Clone, Copy)]
pub(crate) struct ReceiptBounds {
    /// Largest receipt file read, as stored.
    pub file_bytes: u64,
    /// Largest JSON a compressed receipt may expand to.
    pub expanded_bytes: u64,
}

/// The exact receipt bytes, pinned by digest and expanded to plain JSON.
pub(crate) struct PinnedReceipt {
    /// Lowercase hex SHA-256 of the receipt file as stored.
    pub sha256: String,
    /// The plain JSON the owner decodes.
    pub json: Vec<u8>,
}

/// The staged input name every receipt door uses.
pub(crate) const RECEIPT_INPUT: &str = "receipt.json";

/// Read, pin and expand one receipt file.
pub(crate) fn read_pinned(
    raw_path: &str,
    expected_sha256: &str,
    encoding: ReceiptEncoding,
    bounds: ReceiptBounds,
) -> Result<PinnedReceipt, Failure> {
    let bytes = read_receipt_bytes(raw_path, bounds.file_bytes)?;
    let sha256 = verify_receipt_sha256(&bytes, expected_sha256)?;
    let json = expand_receipt(bytes, raw_path, encoding, bounds.expanded_bytes)?;
    Ok(PinnedReceipt { sha256, json })
}

fn read_receipt_bytes(raw_path: &str, max_bytes: u64) -> Result<Vec<u8>, Failure> {
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("receipt_not_found", format!("cannot read '{raw_path}'"))
            .remedy("pass one spotting receipt file")
            .detail(json!({"detail": error.kind().to_string()}))
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("receipt_not_found", format!("'{raw_path}' is not a file"))
                .remedy("pass one spotting receipt file"),
        );
    }
    if metadata.len() > max_bytes {
        return Err(
            Failure::invalid("receipt_too_large", "receipt is above the read bound")
                .remedy("use one bounded spotting receipt")
                .detail(json!({"byte_len": metadata.len(), "max_byte_len": max_bytes})),
        );
    }
    std::fs::read(path).map_err(|error| {
        Failure::failed("receipt_unreadable", format!("cannot read '{raw_path}'"))
            .remedy("check the path and preserve the original receipt")
            .detail(json!({"detail": error.kind().to_string()}))
    })
}

/// The Zstandard frame magic number, little-endian on disk.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

fn expand_receipt(
    bytes: Vec<u8>,
    path: &str,
    encoding: ReceiptEncoding,
    max_expanded_bytes: u64,
) -> Result<Vec<u8>, Failure> {
    let compressed = bytes.starts_with(&ZSTD_MAGIC);
    match (compressed, encoding) {
        (true, _) => expand(&bytes, path, max_expanded_bytes),
        (false, ReceiptEncoding::CompressedOrPlain) => Ok(bytes),
        (false, ReceiptEncoding::Compressed) => Err(Failure::invalid(
            "receipt_invalid",
            format!("'{path}' is not a readable Zstandard receipt"),
        )
        .remedy("preserve the original compressed DS receipt")),
    }
}

fn expand(bytes: &[u8], path: &str, max_expanded_bytes: u64) -> Result<Vec<u8>, Failure> {
    let decoder = zstd::stream::read::Decoder::new(bytes).map_err(|error| {
        Failure::invalid(
            "receipt_invalid",
            format!("'{path}' is not a readable Zstandard receipt"),
        )
        .remedy("preserve the original compressed DS receipt")
        .detail(json!({"detail": error.to_string()}))
    })?;
    let mut limited = decoder.take(max_expanded_bytes + 1);
    let mut expanded = Vec::new();
    limited.read_to_end(&mut expanded).map_err(|error| {
        Failure::invalid("receipt_invalid", "compressed receipt could not be decoded")
            .remedy("preserve the original compressed DS receipt")
            .detail(json!({"detail": error.to_string()}))
    })?;
    if expanded.len() as u64 > max_expanded_bytes {
        return Err(Failure::invalid(
            "receipt_too_large",
            "expanded receipt is above the read bound",
        )
        .remedy("use one bounded spotting receipt")
        .detail(json!({"byte_len": expanded.len(), "max_byte_len": max_expanded_bytes})));
    }
    Ok(expanded)
}

fn normalize_sha256(raw: &str) -> Result<String, Failure> {
    let hex = raw.strip_prefix("sha256:").unwrap_or(raw);
    if !ds_cli_contract::util::is_sha256_hex(hex, ds_cli_contract::util::HexCase::Any) {
        return Err(Failure::invalid(
            "receipt_digest_invalid",
            "expected receipt digest is not SHA-256 hex",
        )
        .remedy("copy the raw receipt SHA-256 from its manifest"));
    }
    Ok(hex.to_ascii_lowercase())
}

pub(crate) fn verify_receipt_sha256(bytes: &[u8], expected: &str) -> Result<String, Failure> {
    let expected = normalize_sha256(expected)?;
    let actual = sha256_hex(bytes);
    if actual != expected {
        return Err(Failure::conflict(
            "receipt_digest_mismatch",
            "receipt bytes do not match the expected SHA-256",
        )
        .remedy("use the receipt whose bytes match the manifest digest")
        .detail(json!({"expected": expected, "actual": actual})));
    }
    Ok(actual)
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "spotting_receipt_tests.rs"]
mod tests;
