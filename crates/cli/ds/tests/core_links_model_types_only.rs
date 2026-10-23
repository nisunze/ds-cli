//! DECOUPLING-013: Core holds the DS Grid model only as the types the Network
//! contracts carry. Reading, validating, transforming or writing model data
//! is the Network engine's, reached through a typed task, so no Core crate
//! depends on `ds-grid-model` directly, and the copy the contracts bring in
//! is compiled without the model's `algorithms` or `arrow` features. A new
//! direct link, or a feature that brings the rules or the table codec back
//! into `ds`, fails here.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// `ds-grid-model` and its direct dependents in the normal/build graph of
/// `ds`, one `name features=[…]` line each, the model itself first.
fn grid_model_in_core() -> Vec<String> {
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
            "ds-grid-model",
            "--depth",
            "1",
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
    String::from_utf8(output.stdout)
        .expect("cargo tree prints UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn name(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or_default()
}

#[test]
fn core_reaches_the_grid_model_only_through_the_network_contracts() {
    let lines = grid_model_in_core();
    assert_eq!(lines.first().map(|line| name(line)), Some("ds-grid-model"));
    let dependents: BTreeSet<&str> = lines.iter().skip(1).map(|line| name(line)).collect();
    assert_eq!(
        dependents,
        BTreeSet::from(["ds-network-contracts"]),
        "only the contracts may link ds-grid-model into ds; route model work through a \
         Network engine task instead:\n{}",
        lines.join("\n")
    );
}

#[test]
fn the_grid_model_in_core_carries_no_rules_or_table_codec() {
    let lines = grid_model_in_core();
    let model = lines.first().expect("ds links the contracts' model types");
    assert!(
        model.ends_with("features=[]"),
        "ds compiles ds-grid-model with features; its algorithms and arrow codec belong \
         to the Network engine: {model}"
    );
}
