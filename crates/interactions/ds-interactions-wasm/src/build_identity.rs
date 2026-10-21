//! Producer facts only. The host separately admits a UI unit and its receipt.

use wasm_bindgen::prelude::*;

/// Return the versioned build facts of these compiled UI bytes.
#[wasm_bindgen(js_name = "uiEngineBuildIdentity")]
pub fn ui_engine_build_identity() -> String {
    include_str!(concat!(env!("OUT_DIR"), "/ui-engine-build.json")).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn identity_is_closed_and_reports_only_this_ui_producer() {
        let identity: serde_json::Value =
            serde_json::from_str(&ui_engine_build_identity()).unwrap();
        assert_eq!(identity["schema"], "ds.ui-engine-build/v1");
        assert_eq!(identity["component"], "interactions");
        assert_eq!(identity["protocol"], "ds.ui-wasm/v1");
        assert_eq!(identity["ui_contract"], 1);
        let expected = BTreeSet::from([
            "schema",
            "component",
            "protocol",
            "ui_contract",
            "source_sha",
            "cargo_lock_sha256",
            "rustc",
            "target",
            "profile",
            "config_sha256",
        ]);
        assert_eq!(
            identity
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected
        );
        for name in ["cargo_lock_sha256", "config_sha256"] {
            let value = identity[name].as_str().unwrap();
            assert_eq!(value.len(), 64);
            assert!(
                value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
        assert!(identity["rustc"].as_str().unwrap().starts_with("rustc "));
        assert!(!identity["target"].as_str().unwrap().is_empty());
        assert!(matches!(
            identity["profile"].as_str(),
            Some("debug" | "release")
        ));
        // Development is deliberately outside the installable exact-SHA contract.
        let source = identity["source_sha"].as_str().unwrap();
        assert!(
            source == "development"
                || (source.len() == 40
                    && source
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        );
    }

    #[test]
    fn identity_is_repeatable_and_excludes_installation_authority() {
        assert_eq!(ui_engine_build_identity(), ui_engine_build_identity());
        let value: serde_json::Value = serde_json::from_str(&ui_engine_build_identity()).unwrap();
        for field in [
            "admitted",
            "installed",
            "receipt",
            "core",
            "network",
            "kernel",
        ] {
            assert!(
                value.get(field).is_none(),
                "producer must not claim {field}"
            );
        }
    }

    #[test]
    fn identity_serialization_has_the_versioned_field_order() {
        let encoded = ui_engine_build_identity();
        let mut remaining = encoded.as_str();
        for field in [
            "schema",
            "component",
            "protocol",
            "ui_contract",
            "source_sha",
            "cargo_lock_sha256",
            "rustc",
            "target",
            "profile",
            "config_sha256",
        ] {
            let needle = format!("\"{field}\":");
            let index = remaining.find(&needle).expect("identity field in order");
            remaining = &remaining[index + needle.len()..];
        }
        assert!(!encoded.contains('\n'));
    }
}
