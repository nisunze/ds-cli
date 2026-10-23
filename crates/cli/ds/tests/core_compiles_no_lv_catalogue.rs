//! DECOUPLING-013: the LV processor catalogue (settings, numeric settings and
//! presets an LV process admits) is the Network engine's. Core asks the
//! installed engine for it through the typed `lv_processor_catalog` task and
//! admits every process request against that answer, so `ds` compiles
//! ds-network-contracts without the engine's own copy (`engine-catalogue`).
//! Enabling it anywhere in the `ds` graph fails here.

use std::path::Path;
use std::process::Command;

#[test]
fn core_compiles_no_lv_processor_catalogue() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["tree", "--locked", "--offline", "--manifest-path"])
        .arg(&manifest)
        .args([
            "--package",
            "ds",
            "--edges",
            "normal,build",
            "--invert",
            "ds-network-contracts",
            "--depth",
            "0",
            "--prefix",
            "none",
            "--format",
            "{p} features=[{f}]",
        ])
        .output()
        .expect("cargo resolves the ds dependency graph");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");
    let contracts = stdout.lines().next().unwrap_or_default();
    assert!(
        contracts.starts_with("ds-network-contracts "),
        "ds links the Network contracts: {stdout}"
    );
    assert!(
        !contracts.contains("engine-catalogue"),
        "ds compiles the Network engine's LV processor catalogue; read the installed \
         engine's lv_processor_catalog answer instead: {contracts}"
    );
}
