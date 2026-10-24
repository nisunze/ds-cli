//! The Kernel UI links no Parquet or Arrow (24 Oct 2026). The page's row
//! windows leave the Kernel as typed JSON; the Network unit encodes the IPC
//! chunks its table operations read (`tableChunk`) and counts their rows
//! (`tableRowCount`), and the Kernel admits the answers. Every build target
//! the release unit declares is checked on wasm32, so a new direct or
//! transitive codec link fails here.

use std::path::Path;
use std::process::Command;

fn declared_targets() -> Vec<(String, String)> {
    let unit = Path::new(env!("CARGO_MANIFEST_DIR")).join("release/wasm-unit.json");
    let unit: serde_json::Value =
        serde_json::from_slice(&std::fs::read(unit).expect("the release unit is readable"))
            .expect("the release unit is JSON");
    let targets = unit["targets"]
        .as_object()
        .expect("the unit declares targets");
    assert!(!targets.is_empty());
    targets
        .iter()
        .map(|(name, target)| {
            let features: Vec<&str> = target["features"]
                .as_array()
                .expect("each target names its features")
                .iter()
                .map(|feature| feature.as_str().expect("a feature is a name"))
                .collect();
            (name.clone(), features.join(","))
        })
        .collect()
}

/// The packages in the Kernel UI's normal/build graph that pull in `codec`.
fn dependents(features: &str, codec: &str) -> Vec<String> {
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
            features,
            "--edges",
            "normal,build",
            "--invert",
            codec,
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("cargo resolves the dependency graph");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // A codec absent from the whole lockfile, or from this graph, is not linked.
    if !output.status.success() && stderr.contains("did not match any packages") {
        return Vec::new();
    }
    assert!(output.status.success(), "{stderr}");
    String::from_utf8(output.stdout)
        .expect("cargo tree prints UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_kernel_ui_links_no_parquet_or_arrow_on_any_declared_target() {
    for (target, features) in declared_targets() {
        for codec in ["parquet", "arrow-array", "arrow-ipc", "arrow-schema"] {
            let found = dependents(&features, codec);
            assert!(
                found.is_empty(),
                "the Kernel UI ({target}: {features}) links {codec}; let the Network unit \
                 encode, decode or count the chunk and admit its answer:\n{}",
                found.join("\n")
            );
        }
    }
}

#[test]
fn the_check_sees_a_codec_the_graph_does_link() {
    // Control: the same query finds ds-columnar's own Arrow dependency, so an
    // empty answer above is the graph's, not a query that can never match.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["tree", "--locked", "--offline", "--manifest-path"])
        .arg(&manifest)
        .args([
            "--package",
            "ds-columnar",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            "artifacts",
            "--edges",
            "normal,build",
            "--invert",
            "arrow-ipc",
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("ds-columnar"));
}
