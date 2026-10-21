use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::Path, process::Command};

#[path = "build_support.rs"]
mod build_support;

#[derive(serde::Serialize)]
struct UiEngineBuild<'a> {
    schema: &'static str,
    component: &'static str,
    protocol: &'static str,
    ui_contract: u32,
    source_sha: &'a str,
    cargo_lock_sha256: String,
    rustc: String,
    target: String,
    profile: String,
    config_sha256: String,
}

fn main() {
    println!("cargo:rerun-if-env-changed=DS_INTERACTIONS_SOURCE_SHA");
    let profile = env::var("PROFILE").expect("Cargo sets PROFILE");
    let source = env::var("DS_INTERACTIONS_SOURCE_SHA").ok();
    let source_sha = build_support::source_sha(&profile, source.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let crate_root = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = crate_root.parent().unwrap().parent().unwrap();
    let lock = root.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let cargo_lock_sha256 = digest(&fs::read(lock).expect("Cargo.lock is a build identity input"));
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
    let identity = UiEngineBuild {
        schema: "ds.ui-engine-build/v1",
        component: "interactions",
        protocol: "ds.ui-wasm/v1",
        ui_contract: 1,
        source_sha,
        cargo_lock_sha256,
        rustc,
        target: env::var("TARGET").expect("Cargo sets TARGET"),
        profile: profile.clone(),
        config_sha256: {
            println!("cargo:rerun-if-env-changed=DS_UI_BUILD_CONFIG_SHA256");
            let stamp = env::var("DS_UI_BUILD_CONFIG_SHA256").ok();
            build_support::config_sha(&profile, stamp.as_deref())
                .unwrap_or_else(|error| panic!("{error}"))
                .map(str::to_owned)
                .unwrap_or_else(|| config_digest(root))
        },
    };
    fs::write(
        std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ui-engine-build.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .expect("write compiled UI identity");
}

fn config_digest(root: &Path) -> String {
    // Key optional inputs even when absent. No ambient Git HEAD or timestamp.
    let mut inputs = BTreeMap::<String, Option<Vec<u8>>>::new();
    for name in [
        "Cargo.toml",
        ".cargo/config",
        ".cargo/config.toml",
        "rust-toolchain",
        "rust-toolchain.toml",
        "crates/ds-canvas/Cargo.toml",
        "crates/ds-canvas-graph/Cargo.toml",
        "crates/ds-map-interaction/Cargo.toml",
        "crates/ds-interactions-wasm/Cargo.toml",
        "crates/ds-interactions-wasm/build.rs",
        "crates/ds-interactions-wasm/build_support.rs",
        "crates/ds-interactions-wasm/.cargo/config",
        "crates/ds-interactions-wasm/.cargo/config.toml",
    ] {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("cannot read UI configuration {}: {error}", path.display()),
        };
        inputs.insert(format!("file:{name}"), bytes);
    }
    for name in ["CARGO_ENCODED_RUSTFLAGS", "OPT_LEVEL", "DEBUG"] {
        println!("cargo:rerun-if-env-changed={name}");
        inputs.insert(
            format!("env:{name}"),
            env::var_os(name).map(|v| v.into_string().expect("build config is UTF-8").into_bytes()),
        );
    }
    for (name, value) in env::vars().filter(|(name, _)| name.starts_with("CARGO_FEATURE_")) {
        inputs.insert(format!("env:{name}"), Some(value.into_bytes()));
    }
    digest(&build_support::encode_config(&inputs))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
