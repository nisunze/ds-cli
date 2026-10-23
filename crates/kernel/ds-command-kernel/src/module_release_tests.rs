use super::*;
use serde_json::json;

fn catalog() -> Value {
    json!({"schema":MODULE_RELEASE_SCHEMA,"component":"solar","channel":"canary","product_id":"rw.datasolutions.module.solar.canary","version":"0.1.0-r2","protocol":MODULE_PROTOCOL,"released_at":"2026-10-13T08:00:00Z","status":"active","notes":"Module release","assets":[{
        "platform":"linux","arch":"x86_64","kind":"module-executable","protocol":MODULE_PROTOCOL,"url":"https://storage.googleapis.com/dedicated-public-releases/modules/solar/canary/0.1.0-r1/solar","size_bytes":10,"sha256":"a".repeat(64),"source_sha":"b".repeat(40),"built_at":"2026-10-12T08:00:00Z","source_revisions":{"ds-solar":"b".repeat(40),"ds-command-kernel":"c".repeat(40)},"engines":[{"engine":"ds-solar","release":"ds-solar@0.1.0+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","manifest_sha256":"d".repeat(64)}]
    }]})
}

#[test]
fn accepts_carried_platform_sources_without_inventing_a_vector() {
    let value = catalog();
    let admitted = validate_module_release("solar", "canary", &value).unwrap();
    assert_eq!(admitted.assets[0].source_revisions.len(), 2);
    assert!(admitted.assets[0].url.contains("0.1.0-r1"));
    assert_eq!(serde_json::to_value(admitted).unwrap(), value);
}

fn network_catalog() -> Value {
    let mut value = catalog();
    value["component"] = json!("network");
    value["product_id"] = json!("rw.datasolutions.module.network.canary");
    value["protocol"] = json!(NETWORK_PROTOCOL);
    let asset = &mut value["assets"][0];
    asset["protocol"] = json!(NETWORK_PROTOCOL);
    asset["url"] = json!("https://storage.googleapis.com/dedicated-public-releases/modules/network/canary/0.1.0-r1/ds-network");
    asset["source_revisions"] = json!({"ds-network":"b".repeat(40)});
    asset["engines"][0]["engine"] = json!("ds-network");
    asset["engines"][0]["release"] = json!(format!("ds-network@0.1.0+{}", "b".repeat(40)));
    value
}

#[test]
fn network_uses_its_declared_engine_protocol_and_executable() {
    let mut value = network_catalog();
    let admitted = validate_module_release("network", "canary", &value).unwrap();
    assert_eq!(admitted.component, ModuleComponent::Network);
    assert_eq!(serde_json::to_value(admitted).unwrap(), value);
    let asset = &mut value["assets"][0];
    asset["platform"] = json!("windows");
    asset["url"] = json!("https://storage.googleapis.com/dedicated-public-releases/modules/network/canary/0.1.0-r1/ds-network.exe");
    assert!(validate_module_release("network", "canary", &value).is_ok());
    for (path, replacement) in [
        ("/protocol", json!(MODULE_PROTOCOL)),
        ("/assets/0/protocol", json!(MODULE_PROTOCOL)),
        ("/assets/0/url", json!("https://storage.googleapis.com/dedicated-public-releases/modules/network/canary/0.1.0-r1/network.exe")),
        ("/assets/0/engines/0/engine", json!("ds-network-reporter")),
        ("/assets/0/source_revisions", json!({"ds-network-reporter":"b".repeat(40)})),
    ] {
        let mut invalid = value.clone();
        *invalid.pointer_mut(path).unwrap() = replacement;
        assert!(validate_module_release("network", "canary", &invalid).is_err(), "{path}");
    }
    assert!(validate_module_release("reporter", "canary", &value).is_err());
}

#[test]
fn component_lane_and_legacy_identity_are_fenced() {
    assert_eq!(
        validate_module_release("shell", "canary", &catalog())
            .unwrap_err()
            .code,
        "module_release_component_invalid"
    );
    assert_eq!(
        validate_module_release("solar", "beta", &catalog())
            .unwrap_err()
            .code,
        "module_release_channel_invalid"
    );
    assert_eq!(
        validate_module_release("reporter", "canary", &catalog())
            .unwrap_err()
            .code,
        "module_release_identity_mismatch"
    );
    let mut value = catalog();
    value.as_object_mut().unwrap().remove("schema");
    assert_eq!(
        validate_module_release("solar", "canary", &value)
            .unwrap_err()
            .code,
        "module_release_invalid"
    );
}

#[test]
fn refuses_malformed_targets_provenance_urls_and_unknown_payloads() {
    for (path, replacement) in [
        ("/assets/0/platform", json!("macos")),
        ("/assets/0/arch", json!("aarch64")),
        ("/assets/0/protocol", json!("ds.module/v2")),
        ("/assets/0/sha256", json!("A".repeat(64))),
        ("/assets/0/source_revisions/ds-solar", json!("e".repeat(40))),
        ("/assets/0/engines/0/engine", json!("ds-network-reporter")),
        ("/assets/0/engines/0/manifest_sha256", json!("")),
        ("/assets/0/url", json!("https://example.com/solar")),
        ("/assets/0/url", json!("https://storage.googleapis.com/bucket/modules/solar/stable/0.1.0-r1/solar")),
        ("/assets/0/url", json!("https://storage.googleapis.com/bucket/modules/solar/canary/latest/solar")),
        ("/assets/0/url", json!("https://storage.googleapis.com/bucket/modules/solar/canary/0.1.0-r1/solar?command=run")),
        ("/status", json!("draft")),
        ("/notes", json!("x".repeat(8001))),
    ] {
        let mut value = catalog();
        *value.pointer_mut(path).unwrap() = replacement;
        assert!(validate_module_release("solar", "canary", &value).is_err(), "{path}");
    }
    let mut value = catalog();
    let duplicate = value["assets"][0].clone();
    value["assets"].as_array_mut().unwrap().push(duplicate);
    assert!(validate_module_release("solar", "canary", &value).is_err());
}

#[test]
fn withdrawal_status_is_preserved_and_never_grants_installation() {
    let mut value = catalog();
    value["status"] = json!("blocked");
    assert_eq!(
        validate_module_release("solar", "canary", &value)
            .unwrap()
            .status,
        "blocked"
    );
}

/// Owner 22 Oct (B6): a publisher may add catalog members; every installed
/// Core admits the same release and never reads the addition. Added members
/// stay bounded and a missing required member still refuses with the typed
/// `module_release_invalid`. (`"command"` here was once a refusal case.)
#[test]
fn additive_catalog_members_are_admitted_bounded_and_never_read() {
    let admitted = validate_module_release("solar", "canary", &catalog()).unwrap();
    let mut additive = catalog();
    additive["command"] = json!("run");
    additive["signatures"] = json!([{"keyid": "k1", "sig": "x".repeat(88)}]);
    additive["assets"][0]["releasedAt"] = json!("2026-10-13T08:00:00Z");
    additive["assets"][0]["engines"][0]["capabilities"] = json!(["future_v1"]);
    let newer = validate_module_release("solar", "canary", &additive).unwrap();
    assert_eq!(newer, admitted);
    assert_eq!(serde_json::to_value(&newer).unwrap(), catalog());

    let mut network = network_catalog();
    network["future"] = json!({"nested": true});
    assert_eq!(
        validate_module_release("network", "canary", &network).unwrap(),
        validate_module_release("network", "canary", &network_catalog()).unwrap()
    );

    let invalid = |value: &Value| {
        validate_module_release("solar", "canary", value)
            .unwrap_err()
            .code
    };
    let mut oversized = catalog();
    oversized["future"] = json!("x".repeat(8001));
    assert_eq!(invalid(&oversized), "module_release_invalid");
    let mut long_name = catalog();
    long_name["f".repeat(65)] = json!(1);
    assert_eq!(invalid(&long_name), "module_release_invalid");
    let mut many = catalog();
    for index in 0..33 {
        many[format!("future_{index}")] = json!(index);
    }
    assert_eq!(invalid(&many), "module_release_invalid");
    for (parent, key) in [
        ("", "version"),
        ("/assets/0", "sha256"),
        ("/assets/0/engines/0", "manifest_sha256"),
    ] {
        let mut missing = catalog();
        missing
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert_eq!(
            invalid(&missing),
            "module_release_invalid",
            "{parent}/{key}"
        );
    }
    let mut wrong = catalog();
    wrong["assets"][0]["size_bytes"] = json!("10");
    assert_eq!(invalid(&wrong), "module_release_invalid");
}
