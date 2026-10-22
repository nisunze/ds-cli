//! Build facts for the independently published UI unit, never install authority.

use sha2::{Digest, Sha256};
use std::{env, fs, path::Path, process::Command};

pub fn stamp<'a>(
    name: &str,
    length: usize,
    required: bool,
    value: Option<&'a str>,
) -> Result<&'a str, String> {
    match value {
        Some(value)
            if value.len() == length
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            Ok(value)
        }
        None if !required => Ok("development"),
        _ => Err(format!(
            "UI release identity requires {name} as {length} lowercase hexadecimal characters from the captured producer"
        )),
    }
}

pub fn requires_release_stamps(target: &str, profile: &str) -> bool {
    target.starts_with("wasm32-") && profile == "release"
}

pub fn emit(workspace: &Path) {
    for name in ["DS_COMMAND_KERNEL_SOURCE_SHA", "DS_UI_BUILD_CONFIG_SHA256"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let target = env::var("TARGET").expect("Cargo sets TARGET");
    let profile = env::var("PROFILE").expect("Cargo sets PROFILE");
    let required = requires_release_stamps(&target, &profile);
    let source_input = env::var("DS_COMMAND_KERNEL_SOURCE_SHA").ok();
    let config_input = env::var("DS_UI_BUILD_CONFIG_SHA256").ok();
    let source = stamp(
        "DS_COMMAND_KERNEL_SOURCE_SHA",
        40,
        required,
        source_input.as_deref(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let config = stamp(
        "DS_UI_BUILD_CONFIG_SHA256",
        64,
        required,
        config_input.as_deref(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let lock = workspace.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let lock_sha256 = format!(
        "{:x}",
        Sha256::digest(fs::read(lock).expect("Cargo.lock is a UI build identity input"))
    );
    let rustc = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("-vV")
        .output()
        .expect("rustc -vV must run while stamping UI identity");
    assert!(rustc.status.success(), "rustc -vV failed");
    let rustc = String::from_utf8(rustc.stdout)
        .expect("rustc -vV is UTF-8")
        .lines()
        .collect::<Vec<_>>()
        .join("; ");
    let identity = serde_json::json!({
        "schema": "ds.ui-engine-build/v1",
        "component": "command-kernel",
        "protocol": "ds.ui-wasm/v1",
        "ui_contract": 1,
        "source_sha": source,
        "cargo_lock_sha256": lock_sha256,
        "rustc": rustc,
        "target": target,
        "profile": profile,
        "config_sha256": config,
    });
    fs::write(
        std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ui-engine-build.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .expect("write compiled UI identity");
}
