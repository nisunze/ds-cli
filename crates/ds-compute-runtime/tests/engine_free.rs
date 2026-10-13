//! Guard the detached compute runtime's actual normal dependency closure.
use std::process::Command;

#[test]
fn normal_dependency_graph_has_no_solar_computation_or_report_owner() {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "--offline", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .args(["--edges", "normal", "--prefix", "none", "--format", "{p}"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph = String::from_utf8(output.stdout).unwrap();
    for forbidden in [
        "ds-solar-runtime",
        "ds-solar-engine",
        "ds-solar-report",
        "ds-solar-portfolio",
        "ds-solar-project",
        "ds-solar-native",
        "ds-solar-addon",
    ] {
        assert!(
            !graph
                .lines()
                .any(|line| line.split_whitespace().next() == Some(forbidden)),
            "native prepared execution linked {forbidden}"
        );
    }
    assert!(graph
        .lines()
        .any(|line| line.starts_with("ds-solar-process ")));
}
