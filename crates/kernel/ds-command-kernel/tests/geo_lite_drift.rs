//! Snapshot integrity and source-workspace ownership fences.
//!
//! Neutral geometry owns its identity/comparison snapshots in ds-geo-lite;
//! Kernel owns the processor-preset snapshot it consumes. Their original bytes,
//! digests and Network provenance remain pinned independently of this move.
//! These checks also keep every source crate in the one workspace and prevent
//! portable geometry from reaching back into an application or UI owner.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn kernel_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    kernel_root()
        .join("../../..")
        .canonicalize()
        .expect("the source workspace root resolves")
}

fn geometry_root() -> PathBuf {
    workspace_root().join("crates/shared/ds-geo-lite")
}

fn vendor_roots() -> [PathBuf; 2] {
    [
        kernel_root().join("vendor/ds-geo-lite"),
        geometry_root().join("vendor"),
    ]
}

fn manifest(root: &Path) -> Value {
    let path = root.join("MANIFEST.json");
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

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", root.display()))
    {
        let path = entry.expect("readable source entry").path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

#[test]
fn vendored_bytes_match_the_manifest_digests() {
    for root in vendor_roots() {
        let manifest = manifest(&root);
        let entries = entries(&manifest);
        assert!(!entries.is_empty(), "{} lists no snapshots", root.display());
        for entry in entries {
            let vendored = field(entry, "vendored");
            let path = root.join(vendored);
            assert_eq!(
                digest(&path),
                field(entry, "sha256"),
                "{} was edited in place; change Network and run the snapshot sync script",
                path.display()
            );
        }
    }
}

#[test]
fn the_vendor_trees_hold_exactly_the_manifest_files() {
    for root in vendor_roots() {
        let manifest = manifest(&root);
        let listed: BTreeSet<String> = entries(&manifest)
            .iter()
            .map(|entry| field(entry, "vendored").to_owned())
            .collect();
        let on_disk: BTreeSet<String> = files_under(&root)
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .expect("snapshot stays within its owner")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .filter(|relative| relative != "MANIFEST.json")
            .collect();
        assert_eq!(
            on_disk,
            listed,
            "{} snapshot inventory drifted",
            root.display()
        );
    }
}

#[test]
fn every_entry_names_its_ds_network_origin() {
    for root in vendor_roots() {
        let manifest = manifest(&root);
        assert_eq!(manifest["ds_network"]["repo"].as_str(), Some("ds-network"));
        let commit = manifest["ds_network"]["commit"]
            .as_str()
            .expect("snapshot names its Network commit");
        assert_eq!(commit.len(), 40, "Network commit must be a full SHA");
        for entry in entries(&manifest) {
            let vendored = field(entry, "vendored");
            assert_eq!(field(entry, "source_commit").len(), 40);
            let sources = entry["sources"]
                .as_array()
                .unwrap_or_else(|| panic!("{vendored} names no Network source"));
            assert!(!sources.is_empty(), "{vendored} names no Network source");
            let partial = entry["partial"]
                .as_bool()
                .unwrap_or_else(|| panic!("{vendored} must declare whether it is partial"));
            if partial {
                assert!(entry["items"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty()));
                assert!(entry["why"].as_str().is_some_and(|why| !why.is_empty()));
            } else {
                assert_eq!(sources.len(), 1, "{vendored} copies exactly one whole file");
            }
        }
    }
}

#[test]
fn geometry_and_processor_snapshots_have_their_consuming_owners() {
    let [kernel, geometry] = vendor_roots().map(|root| manifest(&root));
    let pinned = |manifest: &Value| -> BTreeSet<String> {
        entries(manifest)
            .iter()
            .map(|entry| field(entry, "vendored").to_owned())
            .collect()
    };
    assert_eq!(
        pinned(&kernel),
        BTreeSet::from(["network/processor_presets.json".to_owned()])
    );
    assert_eq!(
        pinned(&geometry),
        BTreeSet::from(["geo/identity.rs".to_owned()])
    );
    assert!(!kernel_root().join("vendor/ds-geo-lite/geo").exists());
}

/// The optional live comparison retains the same external-checkout contract.
#[test]
fn vendored_bytes_match_live_ds_network() {
    let network_root = workspace_root().join("../ds-network");
    if !network_root.join("crates/ds-geo").is_dir() {
        eprintln!(
            "ds-network absent at {}; checking pinned snapshots only",
            network_root.display()
        );
        return;
    }
    for root in vendor_roots() {
        let manifest = manifest(&root);
        for entry in entries(&manifest) {
            let vendored = field(entry, "vendored");
            let sources = entry["sources"].as_array().expect("sources");
            for source in sources {
                let relative = field(source, "path");
                assert_eq!(
                    digest(&network_root.join(relative)),
                    field(source, "sha256"),
                    "Network/{relative} drifted from {}; run the snapshot sync script",
                    root.join(vendored).display()
                );
            }
            if !entry["partial"].as_bool().expect("partial flag") {
                assert_eq!(
                    std::fs::read(network_root.join(field(&sources[0], "path"))).unwrap(),
                    std::fs::read(root.join(vendored)).unwrap(),
                    "{} is not byte-identical to its Network origin",
                    root.join(vendored).display()
                );
            }
        }
    }
}

fn dependency_reaches(text: &str, forbidden: &[&str]) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|code| {
            (code.contains('=') || code.starts_with('[')) && !code.starts_with("description")
        })
        .filter(|code| forbidden.iter().any(|owner| code.contains(owner)))
        .map(str::to_owned)
        .collect()
}

/// Product dependencies are package edges, never repository path substrings.
fn network_reachable(metadata: &Value) -> bool {
    let packages = metadata["packages"].as_array().expect("metadata packages");
    let roots: Vec<_> = packages
        .iter()
        .filter(|package| {
            matches!(
                package["name"].as_str(),
                Some("ds-command-kernel" | "ds-geo-lite")
            )
        })
        .map(|package| package["id"].as_str().expect("package id").to_owned())
        .collect();
    assert_eq!(roots.len(), 2, "both product roots must be present");
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolved package graph");
    let mut pending = roots;
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let package = packages
            .iter()
            .find(|package| package["id"].as_str() == Some(&id))
            .expect("resolved package");
        if package["name"] == "ds-network" {
            return true;
        }
        let node = nodes
            .iter()
            .find(|node| node["id"].as_str() == Some(&id))
            .expect("resolved node");
        for dependency in node["deps"].as_array().expect("resolved dependencies") {
            if dependency["dep_kinds"]
                .as_array()
                .expect("dependency kinds")
                .iter()
                .any(|kind| kind["kind"] != "dev")
            {
                pending.push(
                    dependency["pkg"]
                        .as_str()
                        .expect("dependency package")
                        .to_owned(),
                );
            }
        }
    }
    false
}

#[test]
fn no_kernel_or_geometry_manifest_reaches_ds_network() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--all-features",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("read the actual resolved dependency graph");
    assert!(
        output.status.success(),
        "metadata refused: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("complete Cargo metadata");
    assert!(
        !network_reachable(&metadata),
        "portable product graph reaches Network compute"
    );
}

#[test]
fn network_dependency_fence_rejects_renamed_transitive_and_build_edges() {
    use serde_json::json;
    let fixture = |kind: Value, engine: bool| {
        json!({
        "packages": [{"id":"kernel","name":"ds-command-kernel"},{"id":"geometry","name":"ds-geo-lite"},
            {"id":"contract","name":"ds-network-contracts","manifest_path":"/repo/ds-network/crates/contracts/Cargo.toml"},
            {"id":"engine","name": if engine { "ds-network" } else { "other-facts" }}],
        "resolve":{"nodes":[{"id":"kernel","deps":[{"name":"facts","pkg":"contract","dep_kinds":[{"kind":null}]}]},
            {"id":"geometry","deps":[]},{"id":"contract","deps":[{"name":"renamed_optional_engine","pkg":"engine","dep_kinds":[{"kind":kind}]}]},
            {"id":"engine","deps":[]}]}})
    };
    assert!(network_reachable(&fixture(Value::Null, true)));
    assert!(network_reachable(&fixture(json!("build"), true)));
    assert!(!network_reachable(&fixture(Value::Null, false)));
    assert!(!network_reachable(&fixture(json!("dev"), true)));
}

/// Source includes are dependencies even when Cargo cannot see them.
#[test]
fn no_kernel_or_geometry_source_reaches_ds_network() {
    let mut offenders = Vec::new();
    for root in [kernel_root(), geometry_root()] {
        for subtree in ["src", "tests", "vendor"] {
            for path in files_under(&root.join(subtree)) {
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs")
                    || path.file_name().and_then(|name| name.to_str()) == Some("geo_lite_drift.rs")
                {
                    continue;
                }
                for (number, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
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
    }
    assert!(
        offenders.is_empty(),
        "source reaches Network:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn every_source_crate_belongs_to_the_single_workspace() {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    assert!(text.lines().any(|line| line.trim() == "[workspace]"));
    assert!(
        !text.lines().any(|line| line.trim() == "[package]"),
        "root must stay a virtual workspace"
    );
    let members = text
        .split("members = [")
        .nth(1)
        .expect("explicit workspace members");
    let declared: BTreeSet<String> = members
        .split(']')
        .next()
        .expect("closed members array")
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix('\"'))
        .map(|line| line.split('\"').next().expect("member path").to_owned())
        .collect();
    let mut found = BTreeSet::new();
    for group in ["cli", "kernel", "interactions", "shared"] {
        for entry in std::fs::read_dir(root.join("crates").join(group)).expect("owner group") {
            let dir = entry.expect("crate directory").path();
            let manifest = dir.join("Cargo.toml");
            if !manifest.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&manifest).expect("crate manifest");
            assert!(
                !text.lines().any(|line| line.trim() == "[workspace]"),
                "{} declares a detached workspace",
                manifest.display()
            );
            assert!(
                !dir.join("Cargo.lock").exists(),
                "{} has a private lockfile",
                dir.display()
            );
            found.insert(
                dir.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    assert_eq!(
        declared, found,
        "every source crate must be reached by workspace checks"
    );
    assert!(
        root.join("Cargo.lock").is_file(),
        "the one workspace lockfile is required"
    );
}

fn source_paths(text: &str) -> Vec<&str> {
    let pattern = regex::Regex::new(
        r#"(?s)(?:#\s*\[\s*path\s*=\s*|include_(?:str|bytes)!\s*\(\s*)"([^"\n]+)""#,
    )
    .expect("source include pattern");
    pattern
        .captures_iter(text)
        .map(|capture| capture.get(1).unwrap().as_str())
        .collect()
}

#[test]
fn source_path_inventory_captures_multiline_includes() {
    assert_eq!(
        source_paths("#[path = \"../owner.rs\"]\ninclude_str!(\n \"../../kernel/secret.json\"\n);"),
        ["../owner.rs", "../../kernel/secret.json"]
    );
}

#[test]
fn neutral_geometry_has_no_application_or_ui_source_edge() {
    let root = geometry_root().canonicalize().unwrap();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(dependency_reaches(
        &text,
        &[
            "ds-command-kernel",
            "ds-cli",
            "ds-interactions",
            "ds-canvas",
            "ds-map-interaction",
            "ds-profile-view"
        ]
    )
    .is_empty());
    for subtree in ["src", "tests", "vendor"] {
        for path in files_under(&root.join(subtree)) {
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let includes = source_paths(&text);
            assert_eq!(
                includes.len(),
                text.matches("#[path").count()
                    + text.matches("include_str!").count()
                    + text.matches("include_bytes!").count(),
                "{} has an unclassified source include; keep neutral source paths explicit",
                path.display()
            );
            for relative in includes {
                let target = path
                    .parent()
                    .unwrap()
                    .join(relative)
                    .canonicalize()
                    .unwrap_or_else(|error| {
                        panic!(
                            "{} has an unresolved source include {relative}: {error}",
                            path.display()
                        )
                    });
                assert!(
                    target.starts_with(&root),
                    "{} reaches outside neutral geometry: {}",
                    path.display(),
                    target.display()
                );
            }
        }
    }
}

/// Independent numerical proof remains necessary after removing the partial
/// haversine copy. The shared primitive must still agree with meridian arcs
/// and the spherical law of cosines; digest checks are not a math oracle.
#[test]
fn canonical_haversine_matches_an_independent_yardstick() {
    use ds_geo_lite::{haversine_distance_m, haversine_m, EARTH_RADIUS_M};

    assert_eq!(
        EARTH_RADIUS_M, 6_371_000.0,
        "the shared primitive retains the established spherical Earth radius"
    );

    // 1. A meridian arc is exact on a sphere: R * dlat, no trigonometry.
    //    This also catches a transposed (lng, lat) argument pair, which would
    //    read the same numbers as a parallel instead.
    for (lng, lat, dlat) in [
        (30.0_f64, 0.0_f64, 1.0_f64),
        (30.0, -1.95, 0.001),
        (-74.0, 40.7, 2.5),
        (179.9, -33.9, 0.25),
    ] {
        let expected = EARTH_RADIUS_M * dlat.to_radians();
        let actual = haversine_m(lng, lat, lng, lat + dlat);
        assert!(
            (actual - expected).abs() <= 1e-9 * expected,
            "meridian arc at ({lng}, {lat}) + {dlat}deg: {actual} m, closed form {expected} m"
        );
    }

    // 2. The spherical law of cosines — a different formulation of the same
    //    great-circle distance, stable at these separations.
    fn law_of_cosines(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
        let (p1, p2) = (a_lat.to_radians(), b_lat.to_radians());
        let dlng = (b_lng - a_lng).to_radians();
        EARTH_RADIUS_M * (p1.sin() * p2.sin() + p1.cos() * p2.cos() * dlng.cos()).acos()
    }
    for (a_lng, a_lat, b_lng, b_lat) in [
        (30.0589, -1.9441, 32.5825, 0.3476),   // Kigali -> Kampala
        (-0.1276, 51.5072, -74.0060, 40.7128), // London -> New York
        (30.0, 60.0, 31.0, 60.0),              // a short east-west hop, high latitude
        (179.5, -20.0, -179.5, -20.0),         // across the antimeridian
        (10.0, -5.0, 12.0, 5.0),               // across the equator
    ] {
        let actual = haversine_m(a_lng, a_lat, b_lng, b_lat);
        let expected = law_of_cosines(a_lng, a_lat, b_lng, b_lat);
        assert!(
            (actual - expected).abs() <= 1e-6 * expected,
            "({a_lng}, {a_lat}) -> ({b_lng}, {b_lat}): {actual} m, law of cosines {expected} m"
        );
        assert!(
            (haversine_m(b_lng, b_lat, a_lng, a_lat) - actual).abs() <= 1e-9,
            "distance is not symmetric"
        );
    }

    // 3. A point is zero metres from itself.
    assert_eq!(haversine_m(30.0589, -1.9441, 30.0589, -1.9441), 0.0);

    // 4. The two items are the same number; only the guard differs, and the
    //    guard is the item taken from `ds-geo-ops::lib.rs`.
    assert_eq!(
        haversine_m(30.0, -1.0, 31.0, -2.0),
        haversine_distance_m(30.0, -1.0, 31.0, -2.0)
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(haversine_m(bad, -1.0, 31.0, -2.0).is_nan());
        assert!(haversine_m(30.0, bad, 31.0, -2.0).is_nan());
        assert!(haversine_m(30.0, -1.0, bad, -2.0).is_nan());
        assert!(haversine_m(30.0, -1.0, 31.0, bad).is_nan());
    }
}

/// Transient UI implementation belongs to Interactions, never the Kernel build.
#[test]
fn kernel_has_no_ui_interaction_dependency_or_snapshot() {
    for root in [kernel_root(), geometry_root()] {
        let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(dependency_reaches(
            &text,
            &[
                "ds-interactions",
                "ds-map-interaction",
                "ds-canvas",
                "ds-profile-view"
            ]
        )
        .is_empty());
    }
    assert!(!workspace_root()
        .join("crates/kernel/ds-canvas-lite")
        .exists());
    for root in vendor_roots() {
        assert!(!root.join("canvas").exists());
    }
    assert!(!kernel_root().join("src/canvas.rs").exists());
}
