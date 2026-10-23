//! Addon identity admission tests for `addons.rs`.
use super::*;

fn network_identity(count: usize) -> Value {
    let mut declared = vec!["lv_processing_v1".to_owned()];
    declared.extend((1..count).map(|index| format!("future_capability_{index}")));
    json!({
        "schema": "ds.engine-build/v1",
        "module": "network",
        "engineIdentity": {
            "schema": "ds.network-engine/v1",
            "protocolMajor": 1,
            "protocolMinor": 0,
            "coreVersion": "0.1.0",
            "sourceSha": "a".repeat(40),
            "dependencyClosureSha256": "b".repeat(64),
            "environment": "native",
            "runtime": {
                "environment": "native",
                "logicalCpus": 8,
                "internalWorkerThreads": 2
            },
            "jsonObjectOrder": "sorted",
            "capabilities": declared
        }
    })
}

#[test]
fn network_accepts_33_and_64_capabilities_preserving_additive_tokens() {
    for count in [33, 64] {
        let identity = network_identity(count);
        assert_eq!(
            capabilities("network", &identity),
            Some(identity["engineIdentity"]["capabilities"].clone()),
            "{count} capabilities must remain available"
        );
    }
}

#[test]
fn network_refuses_65_capabilities() {
    assert!(capabilities("network", &network_identity(65)).is_none());
}

#[test]
fn network_refuses_invalid_native_protocol_and_identity_fields() {
    // Owner 22 Oct: a different schema revision or protocol integer is no
    // longer a refusal (see the test below); another document family, a wrong
    // type or an out-of-bound fact still is.
    for (pointer, invalid) in [
        ("/schema", json!("ds.reporter-build/v1")),
        ("/module", json!("reporter")),
        ("/engineIdentity/schema", json!("ds.reporter-engine/v1")),
        ("/engineIdentity/protocolMajor", json!("1")),
        ("/engineIdentity/protocolMajor", json!(-1)),
        ("/engineIdentity/environment", json!("wasm")),
        ("/engineIdentity/runtime/environment", json!("wasm")),
        ("/engineIdentity/runtime/logicalCpus", json!(0)),
        ("/engineIdentity/runtime/logicalCpus", json!(65537)),
        ("/engineIdentity/runtime/internalWorkerThreads", json!(0)),
        ("/engineIdentity/runtime/internalWorkerThreads", json!(9)),
        ("/engineIdentity/coreVersion", json!("")),
        ("/engineIdentity/coreVersion", json!("v".repeat(129))),
        ("/engineIdentity/sourceSha", Value::Null),
        ("/engineIdentity/sourceSha", json!("g".repeat(40))),
        (
            "/engineIdentity/dependencyClosureSha256",
            json!("g".repeat(64)),
        ),
        ("/engineIdentity/capabilities/0", json!("")),
        ("/engineIdentity/capabilities/0", json!("x".repeat(257))),
    ] {
        let mut identity = network_identity(33);
        *identity.pointer_mut(pointer).unwrap() = invalid.clone();
        assert!(
            capabilities("network", &identity).is_none(),
            "invalid {pointer}: {invalid} must be refused"
        );
    }
}

#[test]
fn a_newer_network_with_additive_facts_and_other_revisions_is_available_unchanged() {
    let current = network_identity(2);
    let mut newer = current.clone();
    newer["schema"] = json!("ds.engine-build/v2");
    newer["futureBuildFact"] = json!({"sbom": "sha256"});
    newer["build"] = json!({"target": "x86_64-unknown-linux-gnu", "profile": "release",
        "rustc": "rustc 1.93.0", "cargoLockSha256": "c".repeat(64), "futureLinker": "lld"});
    let engine = &mut newer["engineIdentity"];
    engine["schema"] = json!("ds.network-engine/v3");
    engine["protocolMajor"] = json!(2);
    engine["protocolMinor"] = json!(9);
    engine["futureIdentityFact"] = json!([1, 2]);
    engine["runtime"]["futureRuntimeFact"] = json!(true);
    assert_eq!(
        capabilities("network", &newer),
        capabilities("network", &current)
    );
    assert_eq!(
        capabilities("network", &newer),
        Some(current["engineIdentity"]["capabilities"].clone())
    );
}

#[test]
fn a_network_identity_missing_a_required_fact_is_refused() {
    for (parent, key) in [
        ("", "module"),
        ("", "engineIdentity"),
        ("/engineIdentity", "coreVersion"),
        ("/engineIdentity", "capabilities"),
        ("/engineIdentity", "runtime"),
        ("/engineIdentity/runtime", "logicalCpus"),
    ] {
        let mut identity = network_identity(2);
        identity
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(
            capabilities("network", &identity).is_none(),
            "{parent}/{key} is required"
        );
    }
}

#[test]
fn reporter_build_info_is_admitted_by_family_not_revision() {
    let reporter = json!({"schema": "ds.engine-build/v1", "service": "ds-network-reporter",
        "binary": "ds-report", "schemas": ["ds.engine-build/v1"]});
    assert!(capabilities("reporter", &reporter).is_some());
    let mut newer = reporter.clone();
    newer["schema"] = json!("ds.engine-build/v2");
    newer["futureFact"] = json!(true);
    assert_eq!(
        capabilities("reporter", &newer),
        capabilities("reporter", &reporter)
    );
    for (key, invalid) in [
        ("schema", json!("ds.solar-build/v1")),
        ("schema", json!(1)),
        ("service", json!("ds-solar")),
        ("schemas", json!("ds.engine-build/v1")),
    ] {
        let mut changed = reporter.clone();
        changed[key] = invalid;
        assert!(capabilities("reporter", &changed).is_none(), "{key}");
    }
}

#[test]
fn network_identity_remains_bounded_to_16_kib() {
    let mut identity = network_identity(64);
    for capability in identity["engineIdentity"]["capabilities"]
        .as_array_mut()
        .unwrap()
    {
        *capability = json!("x".repeat(240));
    }
    let mut remaining = 16 * 1024 - serde_json::to_vec(&identity).unwrap().len();
    for capability in identity["engineIdentity"]["capabilities"]
        .as_array_mut()
        .unwrap()
    {
        let added = remaining.min(16);
        *capability = json!("x".repeat(240 + added));
        remaining -= added;
    }
    assert_eq!(remaining, 0);
    assert_eq!(serde_json::to_vec(&identity).unwrap().len(), 16 * 1024);
    assert!(capabilities("network", &identity).is_some());

    identity["engineIdentity"]["coreVersion"] = json!("0.1.0x");
    assert_eq!(serde_json::to_vec(&identity).unwrap().len(), 16 * 1024 + 1);
    assert!(capabilities("network", &identity).is_none());
}
