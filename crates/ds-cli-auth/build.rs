//! Bind native Server admission to the same packaged application version the
//! desktop edge authority signs. This is a build input, never a crate-local
//! convenience version.

fn main() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    );
    let desktop_manifest = manifest_dir.join("../../../ds-command-kernel/native-app-version.txt");
    println!("cargo:rerun-if-changed={}", desktop_manifest.display());
    let source = std::fs::read_to_string(&desktop_manifest)
        .expect("native Server build requires the kernel-owned application version");
    let version = source.trim();
    assert!(
        !version.is_empty()
            && version
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+')),
        "kernel application version is malformed"
    );
    println!("cargo:rustc-env=DS_NATIVE_APP_VERSION={version}");
}
