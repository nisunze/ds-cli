use ds_command_kernel::{solar_portfolio_membership_identity, solar_snapshot};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[test]
fn governed_solar_fingerprint_corpus() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/solar-input-base-vectors.json")).unwrap();
    assert_eq!(corpus["schema"], "ds.solar-input-base-vectors/v1");
    let mut seen = BTreeMap::new();
    for kind in ["city", "portfolio", "report"] {
        for v in corpus[kind].as_array().unwrap() {
            let id = v["id"].as_str().unwrap();
            let fingerprint = match kind {
                "city" => {
                    let base: Value =
                        serde_json::from_str(v["base_json"].as_str().unwrap()).unwrap();
                    assert_eq!(
                        solar_snapshot::input_base::encode(&base).unwrap(),
                        v["canonical"].as_str().unwrap(),
                        "{id}"
                    );
                    let result: Value = serde_json::from_str(&solar_snapshot::evaluate(&serde_json::to_vec(&json!({
                        "operation":"build","root":"project_solar","city_id":"city_a","base":base
                    })).unwrap()).unwrap()).unwrap();
                    assert_eq!(
                        result["input_base_fingerprint"], v["fingerprint"],
                        "public kernel request: {id}"
                    );
                    solar_snapshot::input_base::fingerprint(&base).unwrap()
                }
                "portfolio" => {
                    let result: Value = serde_json::from_str(
                        &solar_portfolio_membership_identity(
                            &serde_json::to_vec(&json!({
                                "portfolio_id":v["portfolio_id"],"cities":v["cities"]
                            }))
                            .unwrap(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    result["fingerprint"].as_str().unwrap().to_owned()
                }
                "report" => format!(
                    "{:x}",
                    Sha256::digest(v["bytes"].as_str().unwrap().as_bytes())
                ),
                _ => unreachable!(),
            };
            assert_eq!(fingerprint, v["fingerprint"].as_str().unwrap(), "{id}");
            if let Some(other) = v["same_as"].as_str().filter(|s| !s.is_empty()) {
                assert_eq!(
                    seen.get(other),
                    Some(&fingerprint),
                    "{id} must be unchanged"
                );
            }
            if let Some(other) = v["different_from"].as_str().filter(|s| !s.is_empty()) {
                assert!(seen.contains_key(other), "missing comparison: {other}");
                assert_ne!(seen.get(other), Some(&fingerprint), "{id} must be moved");
            }
            assert!(
                seen.insert(id.to_owned(), fingerprint).is_none(),
                "duplicate vector: {id}"
            );
        }
    }
}
