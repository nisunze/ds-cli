#[test]
fn the_identity_names_the_build_and_never_carries_the_key() {
    let identity: serde_json::Value = serde_json::from_str(super::identity()).unwrap();
    if let Some(key) = super::PLATFORM_API_KEY {
        assert!(!super::identity().contains(key));
    }
    assert!(identity.get("api_key_sha256").is_some());
    assert_eq!(identity["lane"].as_str(), super::LANE);
    assert_eq!(identity["gateway_origin"].as_str(), super::GATEWAY_ORIGIN);
}

#[test]
fn every_release_channel_speaks_through_one_declared_lane() {
    let unit: serde_json::Value =
        serde_json::from_str(include_str!("../release/wasm-unit.json")).unwrap();
    let declared = unit["releaseTargets"].as_object().unwrap();
    assert_eq!(super::RELEASE_CHANNELS.len(), declared.len());
    for (channel, lane) in super::RELEASE_CHANNELS {
        let target = declared[*channel].as_str().unwrap();
        let features = unit["targets"][target]["features"].as_array().unwrap();
        assert!(features.iter().any(|f| f == &format!("lane-{lane}")[..]));
    }
}

#[test]
fn a_channel_without_its_gateway_names_the_missing_variable() {
    assert_eq!(super::release_gateway("nightly"), Err("releaseTargets"));
    for (channel, lane) in super::RELEASE_CHANNELS {
        let origin = if *lane == "canary" {
            super::RELEASE_ORIGIN_CANARY
        } else {
            super::RELEASE_ORIGIN_PRODUCTION
        };
        match super::release_gateway(channel) {
            Ok(gateway) => assert_eq!(Some(gateway.origin), origin),
            Err(variable) => assert!(variable.starts_with("DS_")),
        }
    }
}
