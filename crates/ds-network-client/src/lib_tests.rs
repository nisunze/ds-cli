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
