#[path = "../build_support.rs"]
mod build_support;

use std::collections::BTreeMap;

#[test]
fn release_config_requires_the_captured_producer_digest() {
    let digest = "a".repeat(64);
    assert_eq!(
        build_support::config_sha("release", Some(&digest)).unwrap(),
        Some(digest.as_str())
    );
    assert_eq!(build_support::config_sha("debug", None).unwrap(), None);
    for invalid in [
        None,
        Some(""),
        Some("HEAD"),
        Some("deadbeef"),
        Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    ] {
        assert!(build_support::config_sha("release", invalid).is_err());
    }
    assert!(build_support::config_sha("debug", Some("HEAD")).is_err());
}

#[test]
fn release_requires_an_explicit_exact_source_sha() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(build_support::source_sha("release", Some(sha)), Ok(sha));
    assert_eq!(build_support::source_sha("debug", None), Ok("development"));
    for invalid in [
        None,
        Some("development"),
        Some("HEAD"),
        Some("0123456"),
        Some("ABCDEF0123456789abcdef0123456789abcdef0123"),
        Some("0123456789abcdef0123456789abcdef01234567\n"),
    ] {
        assert!(build_support::source_sha("release", invalid).is_err());
    }
    assert!(build_support::source_sha("debug", Some("HEAD")).is_err());
}

#[test]
fn config_encoding_binds_names_flags_and_declared_absence_without_ambiguity() {
    let input = BTreeMap::from([("file:.cargo/config.toml".to_owned(), None)]);
    let absent = build_support::encode_config(&input);
    assert_eq!(absent, build_support::encode_config(&input));
    let empty = BTreeMap::from([("file:.cargo/config.toml".to_owned(), Some(vec![]))]);
    assert_ne!(absent, build_support::encode_config(&empty));
    let flags = BTreeMap::from([(
        "env:CARGO_ENCODED_RUSTFLAGS".to_owned(),
        Some(b"-C\x1ftarget-feature=+simd128".to_vec()),
    )]);
    assert_ne!(absent, build_support::encode_config(&flags));
    let split = BTreeMap::from([("a".to_owned(), Some(b"bc".to_vec()))]);
    let joined = BTreeMap::from([("ab".to_owned(), Some(b"c".to_vec()))]);
    assert_ne!(
        build_support::encode_config(&split),
        build_support::encode_config(&joined)
    );
}
