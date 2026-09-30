//! The exact global metadata surface must never select a head or write a file.
use std::fs;

mod common;

fn pin_args() -> Vec<&'static str> {
    vec![
        "library",
        "global",
        "resolve-member",
        "--library-id",
        "library_1",
        "--release-id",
        "release_1",
        "--relative-path",
        "pls-cadd/criteria/Base.CRI",
        "--expected-digest",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--output",
        "json",
    ]
}

#[test]
fn metadata_read_refuses_file_output_and_preserves_existing_bytes() {
    let dir = std::env::temp_dir().join(format!("ds-member-overwrite-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let existing = dir.join("Base.CRI");
    fs::write(&existing, b"operator-owned-native-bytes").unwrap();
    for path in [&existing, &dir.join("new.CRI")] {
        let mut args = pin_args();
        args.extend(["--out", path.to_str().unwrap()]);
        let (envelope, code) = common::json(&args);
        assert_ne!(code, 0);
        assert_eq!(envelope["error"]["code"], "unknown_flag");
    }
    assert_eq!(fs::read(&existing).unwrap(), b"operator-owned-native-bytes");
    assert!(!dir.join("new.CRI").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn release_is_required_and_invalid_digest_refuses_before_authentication() {
    let mut args = pin_args();
    let release = args.iter().position(|arg| *arg == "--release-id").unwrap();
    args.drain(release..release + 2);
    let (envelope, code) = common::json(&args);
    assert_ne!(code, 0);
    assert_eq!(envelope["error"]["code"], "missing_input");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("release-id")
    );

    let mut args = pin_args();
    let digest = args
        .iter()
        .position(|arg| *arg == "--expected-digest")
        .unwrap();
    args[digest + 1] = "SHA256:wrong";
    let (envelope, code) = common::json(&args);
    assert_ne!(code, 0);
    assert_eq!(envelope["error"]["code"], "catalog_member_pin_invalid");
}

#[test]
fn exact_member_discovery_declares_its_native_authority_and_metadata_limit() {
    let (envelope, code) = common::json(&[
        "capabilities",
        "library.global.resolve-member",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0);
    let command = &envelope["data"]["command"];
    assert_eq!(command["effect"], "read_only");
    assert_eq!(command["authority"], "headless_user");
    for name in [
        "library-id",
        "release-id",
        "relative-path",
        "expected-digest",
    ] {
        let input = command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == name)
            .unwrap();
        assert_eq!(input["required"], true);
    }
    assert!(command["output"].as_str().unwrap().contains("8 KiB"));
    assert!(
        command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["name"] != "project")
    );
    assert!(
        command["refusals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "catalog_member_not_found")
    );

    let (search, code) = common::json(&[
        "capabilities",
        "--search",
        "member",
        "--limit",
        "50",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0);
    assert!(
        search["data"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "library.global.resolve-member")
    );
}
