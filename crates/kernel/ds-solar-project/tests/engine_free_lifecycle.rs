//! The application lifecycle never links a calculation engine, not even behind
//! an optional feature: Core reaches the Solar engine by request/response only.
#[test]
fn lifecycle_normal_and_build_dependencies_exclude_computation_in_every_feature_set() {
    for features in ["--no-default-features", "--all-features"] {
        let output = std::process::Command::new(env!("CARGO"))
            .args([
                "tree",
                "--offline",
                "--locked",
                "--manifest-path",
                concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
                "-p",
                "ds-solar-project",
                features,
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
            "ds-solar-addon v",
            "ds-solar-cli v",
            "solar v",
        ] {
            assert!(
                !graph.lines().any(|line| line.starts_with(forbidden)),
                "{features}: {graph}"
            );
        }
    }
}
