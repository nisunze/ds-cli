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
