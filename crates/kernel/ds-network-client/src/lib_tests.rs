use super::*;
#[test]
fn additive_capabilities_keep_a_bounded_forward_compatible_identity() {
    let mut identity: EngineIdentity = serde_json::from_value(serde_json::json!({
            "schema":"ds.network-engine/v1","coreVersion":"0.1.0","sourceSha":null,
            "dependencyClosureSha256":null,"protocolMajor":1,"protocolMinor":0,
            "environment":"native","jsonObjectOrder":"sorted",
            "runtime":{"environment":"native","internalWorkerThreads":1,"logicalCpus":2},
            "capabilities":["lv_processing_v1"]}))
    .unwrap();
    for index in 1..=64 {
        identity
            .capabilities
            .push(NetworkCapability::Unknown(format!("future_{index}_v1")));
        assert_eq!(
            validate_identity(&identity, 1).is_ok(),
            identity.capabilities.len() <= 64
        );
    }
    identity.capabilities.truncate(64);
    identity.capabilities.remove(0);
    assert!(
        validate_identity(&identity, 1).is_err(),
        "future metadata never grants required capability"
    );
}

fn identity_value() -> serde_json::Value {
    serde_json::json!({
        "schema":"ds.network-engine/v1","coreVersion":"0.1.0","sourceSha":"a".repeat(40),
        "dependencyClosureSha256":"b".repeat(64),"protocolMajor":1,"protocolMinor":0,
        "environment":"native","jsonObjectOrder":"sorted",
        "runtime":{"environment":"native","internalWorkerThreads":1,"logicalCpus":2},
        "capabilities":["lv_processing_v1","grid_query_v1"]})
}

/// Owner 22 Oct: a Network released alone with other protocol integers, another
/// identity revision and additive facts is admitted when it declares what this
/// client consumes; its receipts with additive members verify unchanged.
#[test]
fn a_newer_network_is_admitted_by_declared_capability_not_protocol_equality() {
    let current: EngineIdentity = serde_json::from_value(identity_value()).unwrap();
    let mut newer = identity_value();
    newer["schema"] = serde_json::json!("ds.network-engine/v2");
    newer["protocolMajor"] = serde_json::json!(2);
    newer["protocolMinor"] = serde_json::json!(4);
    newer["futureFact"] = serde_json::json!({"accelerator": "none"});
    newer["runtime"]["futureRuntimeFact"] = serde_json::json!(1);
    let newer: EngineIdentity = serde_json::from_value(newer).unwrap();
    assert!(validate_identity(&current, 1).is_ok());
    assert!(validate_identity(&newer, 1).is_ok());
    assert!(newer.is_qualified());

    let input_sha256 = "c".repeat(64);
    let mut receipt = serde_json::json!({
        "outcome":"completed","schema":"ds.network.task-result/v1",
        "inputSha256": input_sha256, "engineIdentity": identity_value(),
        "artifact":{"file":"result.json","byteCount":12,"sha256":"d".repeat(64),
            "schema":"ds.lv.process.result/v1"}});
    receipt["futureReceiptFact"] = serde_json::json!(true);
    receipt["artifact"]["mediaType"] = serde_json::json!("application/json");
    receipt["engineIdentity"]["futureFact"] = serde_json::json!(true);
    let receipt: NetworkReceiptV1 = serde_json::from_value(receipt).unwrap();
    assert_eq!(
        validate_receipt(&receipt, &current, &input_sha256, "ds.lv.process.result/v1").unwrap(),
        false
    );
}

#[test]
fn an_engine_lacking_a_required_fact_is_the_typed_engine_incompatible_refusal() {
    let without_capability = {
        let mut value = identity_value();
        value["capabilities"] = serde_json::json!(["grid_query_v1", "lv_processing_v2"]);
        serde_json::from_value::<EngineIdentity>(value).unwrap()
    };
    let other_family = {
        let mut value = identity_value();
        value["schema"] = serde_json::json!("ds.reporter-engine/v1");
        serde_json::from_value::<EngineIdentity>(value).unwrap()
    };
    let wasm = {
        let mut value = identity_value();
        value["environment"] = serde_json::json!("wasm");
        serde_json::from_value::<EngineIdentity>(value).unwrap()
    };
    for identity in [without_capability, other_family, wasm] {
        assert_eq!(
            validate_identity(&identity, 1).unwrap_err().code(),
            "network_engine_incompatible"
        );
    }
    // A missing required fact or a wrong type never decodes into an identity.
    let mut missing = identity_value();
    missing.as_object_mut().unwrap().remove("coreVersion");
    assert!(serde_json::from_value::<EngineIdentity>(missing).is_err());
    let mut wrong = identity_value();
    wrong["protocolMajor"] = serde_json::json!("2");
    assert!(serde_json::from_value::<EngineIdentity>(wrong).is_err());
}
