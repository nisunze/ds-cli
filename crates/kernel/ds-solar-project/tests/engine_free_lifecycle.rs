//! The native lifecycle consumer must not acquire a linked calculation engine.
#[test]
fn default_lifecycle_normal_and_build_dependencies_exclude_computation() {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "tree",
            "--offline",
            "--locked",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            "-p",
            "ds-solar-project",
            "--no-default-features",
            "-e",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph = String::from_utf8(output.stdout).unwrap();
    for forbidden in [
        "ds-solar-runtime v",
        "ds-solar-engine v",
        "ds-solar-report v",
        "ds-solar-portfolio v",
    ] {
        assert!(
            !graph.lines().any(|line| line.starts_with(forbidden)),
            "{graph}"
        );
    }
}
