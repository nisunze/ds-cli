//! Headless Solar reads refuse without an explicit project; they never
//! borrow a saved selection or a paired window.

use std::process::Command;

use serde_json::Value;

fn ds(args: &[&str]) -> (Value, i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let envelope = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("ds did not return JSON ({error}): {stdout}{stderr}"));
    (envelope, output.status.code().unwrap_or(-1), stdout, stderr)
}

#[test]
fn solar_city_reads_cannot_silently_borrow_saved_selection() {
    for args in [
        vec!["solar", "cities", "--output", "json"],
        vec![
            "solar",
            "input",
            "capture",
            "--city",
            "a",
            "--out",
            "unused.intake.json",
            "--output",
            "json",
        ],
    ] {
        let (envelope, code, stdout, stderr) = ds(&args);
        assert_eq!(code, 2, "{stdout}{stderr}");
        assert_eq!(envelope["error"]["code"], "missing_input");
    }
}

#[test]
fn solar_portfolio_analysis_is_headless_and_requires_explicit_project() {
    let (descriptor, code, stdout, stderr) = ds(&[
        "capabilities",
        "solar.portfolio.analysis",
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let command = &descriptor["data"]["command"];
    assert_eq!(command["effect"], "read_only");
    assert_eq!(command["authority"], "headless_project");
    let inputs = command["inputs"].as_array().unwrap();
    for name in ["project", "portfolio"] {
        assert_eq!(
            inputs.iter().find(|input| input["name"] == name).unwrap()["required"],
            true
        );
    }
    assert!(
        !inputs
            .iter()
            .any(|input| input["name"] == "desktop-descriptor")
    );
    let (envelope, code, stdout, stderr) = ds(&[
        "solar",
        "portfolio",
        "analysis",
        "--portfolio",
        "pf_1",
        "--output",
        "json",
    ]);
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert_eq!(envelope["error"]["code"], "missing_input");
}
