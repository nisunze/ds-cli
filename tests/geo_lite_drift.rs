//! The named proof that `vendor/ds-geo-lite/` is still the ds-network source it
//! claims to be.
//!
//! The kernel must build with `ds-network` absent from the filesystem
//! (`docs/contracts/ds-kernel-network-boundary.md`), and the portable geometry
//! it needs is still MAINTAINED in ds-network. So the kernel compiles a
//! digest-pinned SNAPSHOT of those files instead of linking the engine
//! (`docs/contracts/ds-portable-geometry.md`). Parity is by construction —
//! the bytes are the same bytes — and this test is what keeps that true:
//!
//! 1. **Always.** Every vendored file hashes to the `sha256` recorded in
//!    `MANIFEST.json`, and the vendor tree holds exactly the manifest's files
//!    — no stray copy, no quiet local edit, no forgotten deletion. This half
//!    runs on a machine that has never heard of ds-network.
//! 2. **When `../ds-network` is on disk.** Every source the manifest names
//!    still hashes to its recorded `sha256`, and every whole-file entry is
//!    byte-identical to the live source. Upstream moving is not a failure of
//!    the snapshot, but it IS drift, and it is named with the exact path so a
//!    human runs `scripts/sync-geo-lite.sh` rather than discovering the
//!    divergence months later in a field build.
//!
//! One entry is deliberately PARTIAL: `geo/haversine.rs` is three items lifted
//! out of three `ds-geo-ops` files whose siblings pull the whole primitives
//! tree. Its bytes are ours, so half 2 checks its SOURCES' digests instead —
//! upstream touching any of the three fails here and asks for a re-derivation.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn kernel_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn vendor_root() -> PathBuf {
    kernel_root().join("vendor/ds-geo-lite")
}

fn manifest() -> Value {
    let path = vendor_root().join("MANIFEST.json");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
}

fn digest(path: &Path) -> String {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

fn entries(manifest: &Value) -> &Vec<Value> {
    manifest["files"]
        .as_array()
        .expect("MANIFEST.json has no `files` array")
}

fn field<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry[key]
        .as_str()
        .unwrap_or_else(|| panic!("manifest entry is missing `{key}`: {entry}"))
}

/// Every file listed on disk under the vendor root, relative, `/`-joined.
fn vendored_files_on_disk() -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, found: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("vendor/ds-geo-lite is not readable") {
            let path = entry.expect("unreadable vendor entry").path();
            if path.is_dir() {
                walk(&path, root, found);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("vendor walk left its root")
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative != "MANIFEST.json" {
                    found.insert(relative);
                }
            }
        }
    }
    let root = vendor_root();
    let mut found = BTreeSet::new();
    walk(&root, &root, &mut found);
    found
}

#[test]
fn vendored_bytes_match_the_manifest_digests() {
    let manifest = manifest();
    let entries = entries(&manifest);
    assert!(
        !entries.is_empty(),
        "MANIFEST.json lists no files — the snapshot cannot be empty"
    );

    for entry in entries {
        let vendored = field(entry, "vendored");
        let recorded = field(entry, "sha256");
        let path = vendor_root().join(vendored);
        assert!(
            path.is_file(),
            "MANIFEST.json lists vendor/ds-geo-lite/{vendored}, which is not on disk"
        );
        let actual = digest(&path);
        assert_eq!(
            actual, recorded,
            "vendor/ds-geo-lite/{vendored} was edited in place. \
             The snapshot is not a place to fix code: change it in ds-network, \
             then run scripts/sync-geo-lite.sh"
        );
    }
}

#[test]
fn the_vendor_tree_holds_exactly_the_manifest_files() {
    let manifest = manifest();
    let listed: BTreeSet<String> = entries(&manifest)
        .iter()
        .map(|entry| field(entry, "vendored").to_string())
        .collect();
    let on_disk = vendored_files_on_disk();

    let unlisted: Vec<&String> = on_disk.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "vendor/ds-geo-lite holds files MANIFEST.json does not pin: {unlisted:?}. \
         Add them to scripts/sync-geo-lite.sh or delete them"
    );
    let missing: Vec<&String> = listed.difference(&on_disk).collect();
    assert!(
        missing.is_empty(),
        "MANIFEST.json pins files that are gone: {missing:?}"
    );
}

#[test]
fn every_entry_names_its_ds_network_origin() {
    let manifest = manifest();
    assert_eq!(
        manifest["ds_network"]["repo"].as_str(),
        Some("ds-network"),
        "MANIFEST.json must name the repository the snapshot came from"
    );
    let commit = manifest["ds_network"]["commit"]
        .as_str()
        .expect("MANIFEST.json must name the ds-network commit");
    assert_eq!(commit.len(), 40, "ds-network commit must be a full SHA");

    for entry in entries(&manifest) {
        let vendored = field(entry, "vendored");
        assert_eq!(
            field(entry, "source_commit").len(),
            40,
            "{vendored} must name the full ds-network commit it was taken from"
        );
        let sources = entry["sources"]
            .as_array()
            .unwrap_or_else(|| panic!("{vendored} names no ds-network source"));
        assert!(!sources.is_empty(), "{vendored} names no ds-network source");
        let partial = entry["partial"]
            .as_bool()
            .unwrap_or_else(|| panic!("{vendored} does not say whether it is partial"));
        if partial {
            assert!(
                entry["items"].as_array().is_some_and(|i| !i.is_empty()),
                "{vendored} is partial and must list the items it carries"
            );
            assert!(
                entry["why"].as_str().is_some_and(|w| !w.is_empty()),
                "{vendored} is partial and must say why the cut is at item level"
            );
        } else {
            assert_eq!(
                sources.len(),
                1,
                "{vendored} is a whole-file copy of exactly one source"
            );
        }
    }
}

/// The half that can only run where ds-network is checked out. Absence is not a
/// failure — that is the whole point of the severance — so this test reports
/// the skip and passes.
#[test]
fn vendored_bytes_match_live_ds_network() {
    let network_root = kernel_root().join("../ds-network");
    if !network_root.join("crates/ds-geo").is_dir() {
        eprintln!(
            "ds-network is not on disk at {} — digest half only. \
             This is the severance working as designed.",
            network_root.display()
        );
        return;
    }

    let manifest = manifest();
    for entry in entries(&manifest) {
        let vendored = field(entry, "vendored");
        let partial = entry["partial"].as_bool().unwrap_or(false);
        let sources = entry["sources"].as_array().expect("sources");

        for source in sources {
            let relative = field(source, "path");
            let recorded = field(source, "sha256");
            let live = network_root.join(relative);
            assert!(
                live.is_file(),
                "ds-network no longer has {relative}, which vendor/ds-geo-lite/{vendored} \
                 was taken from. Re-cut the snapshot with scripts/sync-geo-lite.sh"
            );
            assert_eq!(
                digest(&live),
                recorded,
                "ds-network/{relative} has moved since vendor/ds-geo-lite/{vendored} was \
                 taken from it. Run scripts/sync-geo-lite.sh{}",
                if partial {
                    " and re-derive the partial entry by hand"
                } else {
                    ""
                }
            );
        }

        if !partial {
            let relative = field(&sources[0], "path");
            let live = std::fs::read(network_root.join(relative)).expect("live source");
            let snapshot = std::fs::read(vendor_root().join(vendored)).expect("snapshot");
            assert!(
                live == snapshot,
                "vendor/ds-geo-lite/{vendored} is not byte-identical to ds-network/{relative}"
            );
        }
    }
}

/// The snapshot exists so the kernel has no Cargo edge to ds-network. Guard the
/// manifests directly: a path dependency that reaches `../ds-network` is the
/// defect this whole lane removed, and it is one line to reintroduce.
#[test]
fn no_kernel_manifest_reaches_ds_network() {
    for relative in [
        "Cargo.toml",
        "crates/ds-geo-lite/Cargo.toml",
        "crates/ds-canvas-lite/Cargo.toml",
    ] {
        let path = kernel_root().join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"));
        for line in text.lines() {
            // Comments — and the `description` prose, which says out loud that
            // these are snapshots of ds-network source — may name ds-network.
            // A dependency on it may not.
            let code = line.split('#').next().unwrap_or("").trim();
            let names_it = code.contains("ds-network");
            let reaches = names_it
                && (code.contains("path")
                    || code.contains("git")
                    || code.starts_with("ds-network ")
                    || code.starts_with("ds-network="));
            assert!(!reaches, "{relative} reaches ds-network: {}", line.trim());
        }
    }
}

/// A Cargo edge is not the only way to need ds-network on disk. `include_str!`,
/// `include_bytes!` and `#[path]` reach across the repository line just as
/// hard, and they fail LATER — the library still compiles, and only
/// `cargo test` on a machine without ds-network discovers it. One did:
/// `process_settings`'s catalogue. It is snapshotted like everything else now,
/// and this is what keeps the next one from being written.
#[test]
fn no_kernel_source_reaches_ds_network() {
    fn walk(dir: &Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("source tree is readable") {
            let path = entry.expect("unreadable source entry").path();
            if path.is_dir() {
                walk(&path, offenders);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // This file is the one place allowed to name the path: its live
            // half looks for ds-network precisely to report that it is absent.
            if path.file_name().and_then(|n| n.to_str()) == Some("geo_lite_drift.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source is UTF-8");
            for (number, line) in text.lines().enumerate() {
                if line.contains("ds-network/") {
                    offenders.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    let mut offenders = Vec::new();
    walk(&kernel_root().join("src"), &mut offenders);
    walk(&kernel_root().join("tests"), &mut offenders);
    assert!(
        offenders.is_empty(),
        "these read ds-network off the filesystem, so the kernel cannot build or \
         test without it. Snapshot what they need with scripts/sync-geo-lite.sh:\n{}",
        offenders.join("\n")
    );
}
