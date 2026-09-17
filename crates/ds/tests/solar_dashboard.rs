use serde_json::Value;
use std::process::Command;

fn ds(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        // Pass executable availability; selector refusals must happen before
        // attempting to execute this deliberately incompatible owner.
        .env("DS_SOLAR_BIN", std::env::current_exe().unwrap())
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).expect("public DS envelope")
}
#[test]
fn dashboard_reads_require_explicit_source_identity_and_never_a_desktop() {
    let descriptor = ds(&["capabilities", "solar.results.read", "--output", "json"]);
    let command = &descriptor["data"]["command"];
    assert_eq!(command["contract"], 2);
    assert_eq!(command["authority"], "none");
    let inputs = command["inputs"].as_array().unwrap();
    for name in ["source", "project", "run-id", "city"] {
        assert!(
            inputs
                .iter()
                .any(|input| input["name"] == name && input["required"] == true)
        );
    }
    assert!(
        !inputs
            .iter()
            .any(|input| input["name"] == "desktop-descriptor")
    );
    let missing = ds(&[
        "solar",
        "results",
        "read",
        "--run-id",
        "r",
        "--city",
        "c",
        "--section",
        "site",
        "--output",
        "json",
    ]);
    assert_eq!(missing["status"], "error");
    assert_ne!(missing["error"]["code"], "desktop_not_paired");
    let compose = ds(&[
        "capabilities",
        "solar.dashboard.compose",
        "--output",
        "json",
    ]);
    assert_eq!(compose["data"]["command"]["authority"], "none");
    assert_eq!(compose["data"]["command"]["effect"], "local_file_write");
}

#[test]
fn portfolio_dashboard_requires_membership_and_refuses_mixed_selectors_before_io() {
    let common = [
        "solar",
        "dashboard",
        "compose",
        "--source",
        "absent-source",
        "--project",
        "p",
        "--run-id",
        "r",
        "--section",
        "portfolio_finance",
        "--out",
        "unused-dashboard",
        "--output",
        "json",
    ];
    let missing = ds(&common);
    assert_eq!(missing["error"]["code"], "missing_input");
    let mut mixed = common.to_vec();
    mixed.extend([
        "--city",
        "a",
        "--portfolio",
        "pf",
        "--membership-revision",
        "sha256:invalid",
    ]);
    let refused = ds(&mixed);
    assert_eq!(refused["error"]["code"], "solar_dashboard_context_invalid");
}
