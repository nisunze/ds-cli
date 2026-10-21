//! Facts exported by these UI bytes. Core separately owns release admission.

#[cfg_attr(
    feature = "wasm",
    wasm_bindgen::prelude::wasm_bindgen(js_name = "uiEngineBuildIdentity")
)]
pub fn ui_engine_build_identity() -> String {
    include_str!(concat!(env!("OUT_DIR"), "/ui-engine-build.json")).to_owned()
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../build_ui.rs"]
mod build_ui;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn identity_contains_only_the_closed_ui_producer_facts() {
        let text = ui_engine_build_identity();
        assert_eq!(text, ui_engine_build_identity());
        let identity: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(identity["schema"], "ds.ui-engine-build/v1");
        assert_eq!(identity["component"], "command-kernel");
        assert_eq!(identity["protocol"], "ds.ui-wasm/v1");
        assert_eq!(identity["ui_contract"], 1);
        assert_eq!(
            identity
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "schema",
                "component",
                "protocol",
                "ui_contract",
                "source_sha",
                "cargo_lock_sha256",
                "rustc",
                "target",
                "profile",
                "config_sha256"
            ])
        );
        assert!(identity["rustc"].as_str().unwrap().starts_with("rustc "));
        assert_eq!(identity["cargo_lock_sha256"].as_str().unwrap().len(), 64);
        assert!(!text.contains('\n'));
    }

    #[test]
    fn only_wasm_release_requires_the_explicit_producer_stamps() {
        assert!(build_ui::requires_release_stamps(
            "wasm32-unknown-unknown",
            "release"
        ));
        assert!(!build_ui::requires_release_stamps(
            "x86_64-unknown-linux-gnu",
            "release"
        ));
        assert!(!build_ui::requires_release_stamps(
            "x86_64-pc-windows-msvc",
            "release"
        ));
        assert!(!build_ui::requires_release_stamps(
            "wasm32-unknown-unknown",
            "debug"
        ));
        assert_eq!(
            build_ui::stamp("source", 40, false, None).unwrap(),
            "development"
        );
        assert!(build_ui::stamp("source", 40, true, None).is_err());
        assert!(build_ui::stamp("config", 64, true, None).is_err());
    }

    #[test]
    fn supplied_stamps_are_never_silently_repaired() {
        let source = "0123456789abcdef0123456789abcdef01234567";
        let config = "a".repeat(64);
        assert_eq!(
            build_ui::stamp("source", 40, true, Some(source)).unwrap(),
            source
        );
        assert_eq!(
            build_ui::stamp("config", 64, true, Some(&config)).unwrap(),
            config
        );
        for invalid in [
            "HEAD",
            "development",
            "ABCDEF0123456789abcdef0123456789abcdef0123",
            "0123456789abcdef0123456789abcdef01234567\n",
        ] {
            assert!(build_ui::stamp("source", 40, false, Some(invalid)).is_err());
        }
        for invalid in ["", "deadbeef", &"A".repeat(64), &format!("{}\n", config)] {
            assert!(build_ui::stamp("config", 64, true, Some(invalid)).is_err());
        }
    }
}
