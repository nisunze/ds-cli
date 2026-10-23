//! DECOUPLING-013: geometry that selects, measures or changes data runs in
//! the Network container. Core reaches it through typed engine tasks (feature
//! selection is the engine's `feature_selection` operation) and keeps only
//! portable primitives (`ds-geo-lite`), so neither `ds`, the native Kernel nor
//! the Kernel UI (the page reaches the Network unit through the grid unit envelope)
//! links Network's `ds-geo`. A new direct or transitive link fails here.

use std::path::Path;
use std::process::Command;

/// The packages in `package`'s normal/build graph that pull in `ds-geo`.
fn geo_dependents(package: &str) -> Vec<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["tree", "--locked", "--offline", "--manifest-path"])
        .arg(&manifest)
        .args([
            "--package",
            package,
            "--edges",
            "normal,build",
            "--invert",
            "ds-geo",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("cargo resolves the dependency graph");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("cargo tree prints UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn core_links_no_network_geometry() {
    let dependents = geo_dependents("ds");
    assert!(
        dependents.is_empty(),
        "ds links ds-geo; send the computation to the Network engine as a typed \
         task and keep only ds-geo-lite primitives in Core:\n{}",
        dependents.join("\n")
    );
}

#[test]
fn the_native_kernel_links_no_network_geometry() {
    let dependents = geo_dependents("ds-command-kernel");
    assert!(
        dependents.is_empty(),
        "the native Kernel links ds-geo; only the browser build's page-geometry \
         owners may, until they reach the Network unit:\n{}",
        dependents.join("\n")
    );
}

#[test]
fn the_kernel_ui_links_no_network_geometry() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["tree", "--locked", "--offline", "--manifest-path"])
        .arg(&manifest)
        .args([
            "--package",
            "ds-command-kernel",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            "wasm,platform-web,lane-production",
            "--edges",
            "normal,build",
            "--invert",
            "ds-geo",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("cargo resolves the Kernel UI graph");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dependents = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");
    assert!(
        dependents.trim().is_empty(),
        "the Kernel UI links ds-geo; the page reaches Network geometry through \
         the Network unit's grid unit envelope:\n{dependents}"
    );
}
