//! GeoParquet and Arrow IPC are encoded and decoded in the Network container.
//! Core stages and stores columnar bytes and reaches the converter through the
//! engine's typed `columnar_io_v1` grid operations; it links `ds-columnar`
//! only without its `artifacts` (identity, digests, column order, constants).
//! So neither `ds` nor the native Kernel compiles Parquet or Arrow. A new
//! direct or transitive link fails here. (The Kernel UI still links them for
//! the page's own IPC chunks; OPEN.md section 23 holds that move.)

use std::path::Path;
use std::process::Command;

/// The packages in `package`'s normal/build graph that pull in `codec`.
fn dependents(package: &str, codec: &str) -> Vec<String> {
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
            codec,
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .expect("cargo resolves the dependency graph");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // A codec absent from the whole lockfile cannot be linked either.
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
fn core_and_the_native_kernel_link_no_parquet_or_arrow() {
    for package in ["ds", "ds-command-kernel"] {
        for codec in ["parquet", "arrow-array", "arrow-ipc"] {
            let found = dependents(package, codec);
            assert!(
                found.is_empty(),
                "{package} links {codec}; encode or decode it in the Network engine \
                 (`columnar_io_v1`) and move the bytes:\n{}",
                found.join("\n")
            );
        }
    }
}
