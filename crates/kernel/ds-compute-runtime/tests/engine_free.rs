//! Guard the compute runtime's actual normal dependency closure.
use std::process::Command;

#[test]
fn normal_dependency_graph_has_no_embedded_computation_or_report_owner() {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "--offline", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .args([
            "--package",
            "ds-compute-runtime",
            "--edges",
            "normal",
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
        "ds-network",
        "ds-grid-physics",
        "ds-profile",
        "ds-cleaning",
        "ds-graph",
        "ds-io",
        "ds-solar-runtime",
        "ds-solar-engine",
        "ds-solar-report",
        "ds-solar-portfolio",
        "ds-solar-project",
        "ds-solar-native",
        "ds-solar-addon",
        // DECOUPLING-013: the addon transport and host IO are Core's own.
        "ds-solar-io",
        "ds-solar-process",
    ] {
        assert!(
            !graph
                .lines()
                .any(|line| line.split_whitespace().next() == Some(forbidden)),
            "native execution linked {forbidden} instead of the standalone component"
        );
    }
    assert!(
        graph
            .lines()
            .any(|line| line.starts_with("ds-solar-host "))
    );
    assert!(
        graph
            .lines()
            .any(|line| line.starts_with("ds-network-client "))
    );
}
