//! Elevation and terrain are computed in the Network engine (DECOUPLING-013):
//! the `ds` binary links no elevation or terrain-sampling crate, so without a
//! Network engine every elevation command refuses typed, before it reads,
//! fetches or writes anything.
use std::process::Command;

fn ds(home: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env_remove("DS_NETWORK_ENGINE_BIN")
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .output()
        .expect("ds runs");
    assert!(!output.status.success());
    serde_json::from_slice(&output.stdout).expect("a JSON envelope")
}

#[test]
fn without_a_network_engine_elevation_refuses_typed_and_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("points.csv");
    std::fs::write(&source, "x,y\n30,-2\n").unwrap();
    let out = home.path().join("compared.csv");
    for args in [
        vec!["data", "terrain", "describe", "--output", "json"],
        vec![
            "data",
            "elevation",
            "compare",
            "--source",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--output",
            "json",
        ],
    ] {
        let envelope = ds(home.path(), &args);
        assert_eq!(
            envelope["error"]["code"], "network_engine_missing",
            "{envelope}"
        );
        assert_eq!(envelope["error"]["class"], "unavailable", "{envelope}");
    }
    assert!(!out.exists());
}
