//! Stamp verifiable build identity into the binary.
//!
//! Release provenance is mandatory: a packaged `ds` must be able to say
//! exactly which source it was built from, for which target, at what
//! optimization, and whether the tree was clean. Packaging supplies the pin
//! through the environment so the answer comes from the release process
//! rather than from whatever `git` happened to be on PATH; a developer build
//! falls back to reading the working tree and says so.
//!
//! Nothing here fails the build. A binary that cannot determine its source is
//! honest about that — `unknown` is a truthful answer and a broken build is
//! not an improvement.

use std::process::Command;

mod build_pin;

fn main() {
    println!("cargo:rerun-if-env-changed=DS_CLI_SOURCE_SHA");
    println!("cargo:rerun-if-env-changed=DS_CLI_SOURCE_DIRTY");
    println!("cargo:rerun-if-env-changed=DS_NATIVE_CLIENT_PROFILE_SHA256");
    println!("cargo:rerun-if-env-changed=DS_RELEASE_PIN_DS_NETWORK");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../pins/ds-client-core.rev");

    let pinned = std::env::var("DS_CLI_SOURCE_SHA")
        .ok()
        .filter(|sha| !sha.is_empty());

    let (sha, dirty) = match pinned {
        // A packaging pin is authoritative. Its dirty state is declared, not
        // inferred: the release process already refused a dirty tree, or
        // deliberately allowed one for a development lane.
        Some(sha) => {
            let dirty = std::env::var("DS_CLI_SOURCE_DIRTY").as_deref() == Ok("1");
            (sha, dirty)
        }
        None => (git_sha().unwrap_or_else(|| "unknown".into()), git_dirty()),
    };

    println!("cargo:rustc-env=DS_BUILD_SHA={sha}");
    println!("cargo:rustc-env=DS_BUILD_DIRTY={}", u8::from(dirty));
    let native_client_core_sha = include_str!("../../pins/ds-client-core.rev").trim();
    let native_client_core_sha = if native_client_core_sha.len() == 40
        && native_client_core_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        native_client_core_sha
    } else {
        "unknown"
    };
    println!("cargo:rustc-env=DS_NATIVE_CLIENT_CORE_SHA={native_client_core_sha}");
    let native_client_profile_sha256 = std::env::var("DS_NATIVE_CLIENT_PROFILE_SHA256")
        .ok()
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!(
        "cargo:rustc-env=DS_NATIVE_CLIENT_PROFILE_CATALOG_SHA256={native_client_profile_sha256}"
    );
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    let release_pin = std::env::var("DS_RELEASE_PIN_DS_NETWORK").ok();
    let ds_network_pin = build_pin::resolve_ds_network_pin(&profile, release_pin.as_deref())
        .unwrap_or_else(|error| {
            panic!("{error}");
        });
    println!(
        "cargo:rustc-env=DS_NETWORK_SOURCE_SHA={}",
        ds_network_pin.source_sha.as_deref().unwrap_or("")
    );
    println!(
        "cargo:rustc-env=DS_NETWORK_SOURCE_STATE={}",
        ds_network_pin.state
    );
    println!(
        "cargo:rustc-env=DS_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".into())
    );
    println!("cargo:rustc-env=DS_BUILD_PROFILE={}", profile);
}

fn git_sha() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|sha| !sha.is_empty())
}

/// Untracked files count as dirty.
///
/// A binary built from a tree with new, uncommitted sources was not built
/// from the commit its SHA names. `ds-web`'s release helper closes over
/// untracked inputs for the same reason; this agrees with it rather than
/// reporting a cleaner tree than exists.
fn git_dirty() -> bool {
    Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}
