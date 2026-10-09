//! The whole-model spotting receipt, read once for both receipt doors.
//!
//! A `ds dsgrid run` of `plan_whole_model_spotting` returns one receipt: the
//! exact source package, its authored revision and the batch of sealed
//! per-alignment plans. Two commands consume it. `dsgrid spotting
//! preview-receipt` turns a receipt whose diagnostic rejected rows were
//! truncated into a visualization-only package; `dsgrid spotting
//! apply-receipt` lands a complete receipt's digest-sealed plans as one
//! engineering revision. Both pin the exact receipt bytes, bind them to the
//! exact source package and revision, and accept only a complete successful
//! whole-model batch. This module is that shared gate, so the two doors
//! refuse with the same codes for the same receipt.

use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_grid_engine::SpottingPlan;
use ds_grid_exchange::package::GridPackage;
use serde::Deserialize;
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

#[derive(Deserialize)]
pub(crate) struct RunReceipt {
    v: u32,
    command: String,
    status: String,
    pub data: RunReceiptData,
}

#[derive(Deserialize)]
pub(crate) struct RunReceiptData {
    pub source: ReceiptSource,
    pub operation: ReceiptOperation,
    staged: bool,
    persisted: bool,
    pub result: ReceiptResult,
    #[serde(default)]
    pub more: ReceiptMore,
}

#[derive(Deserialize)]
pub(crate) struct ReceiptSource {
    pub model_id: String,
    pub package_revision: u64,
    pub authored_revision: String,
    pub package_sha256: String,
}

#[derive(Deserialize)]
pub(crate) struct ReceiptOperation {
    pub id: String,
}

#[derive(Deserialize)]
pub(crate) struct ReceiptResult {
    pub operation_id: String,
    pub model_revision: String,
    pub batch: ReceiptBatch,
}

#[derive(Deserialize)]
pub(crate) struct ReceiptBatch {
    pub operation_id: String,
    pub model_revision: String,
    pub requested_alignments: usize,
    pub completed_plans: usize,
    pub refused_requests: usize,
    pub items: Vec<ReceiptPlanItem>,
}

#[derive(Deserialize)]
pub(crate) struct ReceiptPlanItem {
    pub plan: SpottingPlan,
}

#[derive(Default, Deserialize)]
pub(crate) struct ReceiptMore {
    #[serde(default)]
    pub truncated: Vec<ReceiptTruncation>,
}

#[derive(Deserialize)]
pub(crate) struct ReceiptTruncation {
    pub field: String,
    pub total: usize,
    pub shown: usize,
    pub withheld: usize,
    pub limit: usize,
}

/// The exact receipt bytes, pinned by digest and decoded.
pub(crate) struct PinnedReceipt {
    /// Lowercase hex SHA-256 of the receipt file as stored.
    pub sha256: String,
    pub receipt: RunReceipt,
}

/// Read, pin and decode one receipt file, then check it is a successful
/// read-only whole-model spotting proposal.
pub(crate) fn read_pinned(
    raw_path: &str,
    expected_sha256: &str,
    encoding: ReceiptEncoding,
    bounds: ReceiptBounds,
) -> Result<PinnedReceipt, Failure> {
    let bytes = read_receipt_bytes(raw_path, bounds.file_bytes)?;
    let sha256 = verify_receipt_sha256(&bytes, expected_sha256)?;
    let receipt = decode_receipt(&bytes, raw_path, encoding, bounds.expanded_bytes)?;
    validate_receipt_operation(&receipt)?;
    Ok(PinnedReceipt { sha256, receipt })
}

/// The receipt must name this exact package: model identity, package
/// revision and package bytes. Returns the package digest it was checked
/// against, `sha256:`-prefixed as the receipt records it.
pub(crate) fn verify_source_package(
    receipt: &RunReceipt,
    package: &GridPackage,
    package_bytes: &[u8],
) -> Result<String, Failure> {
    let source_package_sha = format!("sha256:{}", sha256_hex(package_bytes));
    let source = &receipt.data.source;
    if source.model_id != package.manifest.model.model_id.as_str()
        || source.package_revision != package.manifest.model.model_revision
        || source.package_sha256 != source_package_sha
    {
        return Err(Failure::conflict(
            "receipt_model_mismatch",
            "receipt does not name this exact model package",
        )
        .remedy("use the exact base package named by the receipt")
        .detail(json!({
            "receipt_model_id": source.model_id,
            "model_id": package.manifest.model.model_id.as_str(),
            "receipt_package_revision": source.package_revision,
            "package_revision": package.manifest.model.model_revision,
            "receipt_package_sha256": source.package_sha256,
            "package_sha256": source_package_sha,
        })));
    }
    Ok(source_package_sha)
}

/// Every plan, and the receipt itself, must be authored against the
/// package's current revision.
pub(crate) fn verify_revision(receipt: &RunReceipt, current_revision: &str) -> Result<(), Failure> {
    let batch = &receipt.data.result.batch;
    if receipt.data.source.authored_revision != current_revision
        || receipt.data.result.model_revision != current_revision
        || batch.model_revision != current_revision
        || batch
            .items
            .iter()
            .any(|item| item.plan.model_revision.as_str() != current_revision)
    {
        return Err(Failure::conflict(
            "receipt_revision_mismatch",
            "receipt plans were authored against another model revision",
        )
        .remedy("use the exact base package revision named by the receipt")
        .detail(json!({
            "receipt_revision": receipt.data.source.authored_revision,
            "model_revision": current_revision,
        })));
    }
    Ok(())
}

/// The batch must hold one plan for every requested alignment and no
/// refused one: a partial batch is not a whole-model proposal.
pub(crate) fn verify_complete_batch(receipt: &RunReceipt) -> Result<(), Failure> {
    let batch = &receipt.data.result.batch;
    if batch.requested_alignments == 0
        || batch.completed_plans != batch.requested_alignments
        || batch.refused_requests != 0
        || batch.items.len() != batch.completed_plans
    {
        return Err(Failure::conflict(
            "receipt_incomplete",
            "whole-model batch is missing plans or contains refused alignments",
        )
        .remedy("use a complete successful whole-model proposal receipt")
        .detail(json!({
            "requested_alignments": batch.requested_alignments,
            "completed_plans": batch.completed_plans,
            "refused_requests": batch.refused_requests,
            "plan_items": batch.items.len(),
        })));
    }
    Ok(())
}

fn validate_receipt_operation(receipt: &RunReceipt) -> Result<(), Failure> {
    if receipt.v != 1
        || receipt.command != "dsgrid.run"
        || receipt.status != "ok"
        || receipt.data.staged
        || receipt.data.persisted
        || receipt.data.operation.id != "plan_whole_model_spotting"
        || receipt.data.result.operation_id != "plan_whole_model_spotting"
        || receipt.data.result.batch.operation_id != "plan_optimum_spotting_batch"
    {
        return Err(Failure::invalid(
            "receipt_invalid",
            "receipt is not a successful read-only whole-model spotting proposal",
        )
        .remedy("use one successful dsgrid.run whole-model spotting receipt"));
    }
    Ok(())
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

fn decode_receipt(
    bytes: &[u8],
    path: &str,
    encoding: ReceiptEncoding,
    max_expanded_bytes: u64,
) -> Result<RunReceipt, Failure> {
    let compressed = bytes.starts_with(&ZSTD_MAGIC);
    let expanded;
    let json: &[u8] = match (compressed, encoding) {
        (true, _) => {
            expanded = expand(bytes, path, max_expanded_bytes)?;
            &expanded
        }
        (false, ReceiptEncoding::CompressedOrPlain) => bytes,
        (false, ReceiptEncoding::Compressed) => {
            return Err(Failure::invalid(
                "receipt_invalid",
                format!("'{path}' is not a readable Zstandard receipt"),
            )
            .remedy("preserve the original compressed DS receipt"));
        }
    };
    serde_json::from_slice(json).map_err(|error| {
        Failure::invalid("receipt_invalid", "receipt bytes are not a DS CLI receipt")
            .remedy("use one successful dsgrid.run whole-model spotting receipt")
            .detail(json!({"detail": error.to_string()}))
    })
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
mod tests {
    use super::{ReceiptEncoding, decode_receipt, sha256_hex, verify_receipt_sha256};

    #[test]
    fn verifies_exact_receipt_bytes_before_reading_them() {
        let receipt = b"exact receipt bytes";
        let expected = sha256_hex(receipt);
        assert_eq!(verify_receipt_sha256(receipt, &expected).unwrap(), expected);
        assert_eq!(
            verify_receipt_sha256(receipt, &format!("sha256:{expected}")).unwrap(),
            expected
        );
        assert_eq!(
            verify_receipt_sha256(receipt, &"0".repeat(64))
                .unwrap_err()
                .code(),
            "receipt_digest_mismatch"
        );
        assert_eq!(
            verify_receipt_sha256(receipt, "not-a-digest")
                .unwrap_err()
                .code(),
            "receipt_digest_invalid"
        );
    }

    #[test]
    fn plain_json_is_admitted_only_where_the_door_admits_it() {
        // Not a receipt either way; what matters is which refusal is reached.
        let plain = br#"{"v":1}"#;
        let compressed_only = decode_receipt(plain, "r.json", ReceiptEncoding::Compressed, 1024)
            .err()
            .expect("a compressed-only door refuses plain JSON");
        assert_eq!(compressed_only.code(), "receipt_invalid");
        assert!(compressed_only.message().contains("Zstandard"));
        let either = decode_receipt(plain, "r.json", ReceiptEncoding::CompressedOrPlain, 1024)
            .err()
            .expect("an incomplete receipt still refuses");
        assert_eq!(either.code(), "receipt_invalid");
        assert!(either.message().contains("not a DS CLI receipt"));
    }

    #[test]
    fn a_compressed_receipt_reads_exactly_as_the_plain_one() {
        let plain = serde_json::to_vec(&serde_json::json!({
            "v": 1,
            "command": "dsgrid.run",
            "status": "ok",
            "data": {
                "source": {
                    "model_id": "m",
                    "package_revision": 3,
                    "authored_revision": "rev:abc",
                    "package_sha256": "sha256:00"
                },
                "operation": {"id": "plan_whole_model_spotting"},
                "staged": false,
                "persisted": false,
                "result": {
                    "operation_id": "plan_whole_model_spotting",
                    "model_revision": "rev:abc",
                    "batch": {
                        "operation_id": "plan_optimum_spotting_batch",
                        "model_revision": "rev:abc",
                        "requested_alignments": 0,
                        "completed_plans": 0,
                        "refused_requests": 0,
                        "items": []
                    }
                }
            }
        }))
        .unwrap();
        let compressed = zstd::stream::encode_all(plain.as_slice(), 3).unwrap();
        let read = |bytes: &[u8]| {
            let receipt = decode_receipt(bytes, "r", ReceiptEncoding::CompressedOrPlain, 1 << 20)
                .unwrap_or_else(|refusal| panic!("{}", refusal.message()));
            (
                receipt.data.source.model_id,
                receipt.data.source.package_revision,
                receipt.data.result.batch.model_revision,
            )
        };
        assert_eq!(read(&plain), read(&compressed));
        assert_eq!(read(&plain).1, 3);
    }

    #[test]
    fn a_compressed_receipt_is_bounded_after_expansion() {
        let expanded = vec![b' '; 4096];
        let compressed = zstd::stream::encode_all(expanded.as_slice(), 3).unwrap();
        let refusal = decode_receipt(
            &compressed,
            "r.json.zst",
            ReceiptEncoding::CompressedOrPlain,
            1024,
        )
        .err()
        .expect("an expansion beyond the bound refuses");
        assert_eq!(refusal.code(), "receipt_too_large");
    }
}
