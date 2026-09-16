use serde_json::Value;
use std::process::Command;

fn ds(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
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
