use super::{ReceiptEncoding, expand_receipt, sha256_hex, verify_receipt_sha256};

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
    let plain = br#"{"v":1}"#.to_vec();
    let compressed_only =
        expand_receipt(plain.clone(), "r.json", ReceiptEncoding::Compressed, 1024)
            .expect_err("a compressed-only door refuses plain JSON");
    assert_eq!(compressed_only.code(), "receipt_invalid");
    assert!(compressed_only.message().contains("Zstandard"));
    assert_eq!(
        expand_receipt(
            plain.clone(),
            "r.json",
            ReceiptEncoding::CompressedOrPlain,
            1024
        )
        .unwrap(),
        plain
    );
}

#[test]
fn a_compressed_receipt_expands_to_exactly_the_plain_one() {
    let plain = serde_json::to_vec(&serde_json::json!({
        "v": 1,
        "command": "dsgrid.run",
        "status": "ok",
        "data": {"source": {"model_id": "m", "package_revision": 3}}
    }))
    .unwrap();
    let compressed = zstd::stream::encode_all(plain.as_slice(), 3).unwrap();
    let expanded =
        expand_receipt(compressed, "r", ReceiptEncoding::CompressedOrPlain, 1 << 20).unwrap();
    assert_eq!(expanded, plain);
}

#[test]
fn a_compressed_receipt_is_bounded_after_expansion() {
    let expanded = vec![b' '; 4096];
    let compressed = zstd::stream::encode_all(expanded.as_slice(), 3).unwrap();
    let refusal = expand_receipt(
        compressed,
        "r.json.zst",
        ReceiptEncoding::CompressedOrPlain,
        1024,
    )
    .expect_err("an expansion beyond the bound refuses");
    assert_eq!(refusal.code(), "receipt_too_large");
}
