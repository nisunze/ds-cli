use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn closed_batch() -> Value {
    let digest = |byte: char| format!("sha256:{}", byte.to_string().repeat(64));
    let run = json!({
        "project_id": "project-1",
        "root": "solar",
        "run_id": "run-1",
        "cities": [{
            "city_id": "city-1",
            "input_digest": digest('a'),
            "result_digest": digest('b'),
        }],
    });
    let artifacts = json!([{
        "name": "cities/city-1/result.json",
        "role": "city_result",
        "media_type": "application/json",
        "byte_count": 42,
        "content_digest": digest('c'),
        "city_id": "city-1",
    }]);
    let content = json!({
        "schema_version": BATCH_SCHEMA,
        "run": run,
        "artifacts": artifacts,
    });
    let digest = ds_solar_contracts::digest::canonical_json_digest(&content).unwrap();
    json!({
        "schema_version": BATCH_SCHEMA,
        "batch_id": &digest[7..39],
        "run": content["run"],
        "artifacts": content["artifacts"],
        "state": {"state": "local_ready"},
        "batch_digest": digest,
    })
}

#[test]
fn closed_batch_manifest_identity_is_recomputed_before_projection() {
    let mut batch = closed_batch();
    validate_batch_receipt(&batch, Some("run-1")).expect("closed receipt is valid");

    batch["artifacts"][0]["byte_count"] = json!(43);
    let failure = validate_batch_receipt(&batch, Some("run-1")).expect_err("tampering must fail");
    assert_eq!(failure.code(), "batch_receipt_invalid");
    assert!(failure.message().contains("batch_digest"));
}

#[test]
fn oversized_batch_manifest_is_bounded_at_max_plus_one() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ds-cli-solar-oversized-batch-{}-{unique}.json",
        std::process::id()
    ));
    let file = std::fs::File::create(&path).expect("create sparse batch fixture");
    file.set_len(MAX_BATCH_RECEIPT_BYTES + 2)
        .expect("extend sparse batch fixture");
    drop(file);

    let failure = read_batch_receipt(&path, None).expect_err("oversized receipt must fail");
    assert_eq!(failure.code(), "batch_receipt_oversized");
    assert_eq!(
        failure.detail_value().expect("bounded detail")["observed_at_least"],
        MAX_BATCH_RECEIPT_BYTES + 1
    );
    let _ = std::fs::remove_file(path);
}

/// Reseal a receipt over the document as written, as a newer engine would.
fn resealed(mut batch: Value) -> Value {
    let digest = ds_solar_contracts::batch::batch_content_digest(&batch).unwrap();
    batch["batch_id"] = json!(&digest[7..39]);
    batch["batch_digest"] = json!(digest);
    batch
}

/// Contract 4: the receipt is the engine's reply. Members a newer engine adds
/// (anywhere) and a newer revision of the same family are accepted and bound
/// by its digest; another family, a missing required member and tampering
/// with an added member still refuse.
#[test]
fn a_newer_engine_receipt_with_additive_members_is_accepted() {
    let mut newer = closed_batch();
    newer["schema_version"] = json!("ds-solar.artifact-batch/v2");
    newer["provenance"] = json!({"engine": "ds-solar@future"});
    newer["run"]["placement"] = json!("native");
    newer["run"]["cities"][0]["weather_digest"] = json!(format!("sha256:{}", "d".repeat(64)));
    newer["artifacts"][0]["encoding"] = json!("identity");
    let newer = resealed(newer);
    validate_batch_receipt(&newer, Some("run-1")).expect("additive receipt is valid");

    let mut tampered = newer.clone();
    tampered["provenance"]["engine"] = json!("ds-solar@other");
    let failure = validate_batch_receipt(&tampered, Some("run-1")).unwrap_err();
    assert!(
        failure.message().contains("batch_digest"),
        "{}",
        failure.message()
    );

    let mut other = newer.clone();
    other["schema_version"] = json!("ds-solar.portfolio-batch/v1");
    let other = resealed(other);
    let failure = validate_batch_receipt(&other, Some("run-1")).unwrap_err();
    assert!(
        failure.message().contains("family"),
        "{}",
        failure.message()
    );

    for pointer in [
        "/run/cities/0/result_digest",
        "/artifacts/0/content_digest",
        "/run/root",
    ] {
        let mut missing = newer.clone();
        let (parent, member) = pointer.rsplit_once('/').unwrap();
        missing
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove(member);
        let failure = validate_batch_receipt(&resealed(missing), Some("run-1")).unwrap_err();
        assert_eq!(failure.code(), "batch_receipt_invalid", "{pointer}");
    }
}
