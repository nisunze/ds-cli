//! Bind native Server admission to the same packaged application version the
//! desktop edge authority signs. This is a build input, never a crate-local
//! convenience version.

fn main() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    );
    let desktop_manifest = manifest_dir.join("../../../ds-web/src-tauri/Cargo.toml");
    println!("cargo:rerun-if-changed={}", desktop_manifest.display());
    let source = std::fs::read_to_string(&desktop_manifest)
        .expect("native Server build requires the paired desktop package manifest");
    let version = source
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .and_then(|raw| raw.trim_matches('"').split_whitespace().next())
        .filter(|value| {
            !value.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_digit() || byte == b'.' || byte == b'-' || byte == b'+'
                })
        })
        .expect("paired desktop package version is absent or malformed");
    println!("cargo:rustc-env=DS_NATIVE_APP_VERSION={version}");
}
