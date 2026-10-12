//! The local reference cohort is the same closed CLI and MCP surface.
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn cli(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn local_reference_descriptors_expose_only_the_native_file_boundary() {
    for id in ["solar.reference.plan", "solar.reference.import"] {
        let result = cli(&["capabilities", id, "--output", "json"]);
        let descriptor = &result["data"]["command"];
        assert_eq!(descriptor["authority"], "none");
        assert_eq!(descriptor["effect"], "local_file_write");
        assert_eq!(descriptor["requires"], "server");
        let names = descriptor["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| input["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        for forbidden in [
            "token", "api-key", "url", "lane", "command", "argv", "python",
        ] {
            assert!(!names.contains(&forbidden), "{id} exposed {forbidden}");
        }
    }
    let domain = cli(&["capabilities", "solar", "--output", "json"]);
    let ids = domain["data"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"solar.reference.plan"));
    assert!(ids.contains(&"solar.reference.import"));
    assert!(!ids.contains(&"solar.reference.acquire"));
}

#[test]
fn solar_input_mcp_profile_has_exact_local_file_controls_and_no_retired_acquisition() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "mcp",
            "serve",
            "--exposure",
            "commands",
            "--profile",
            "solar-input",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|response| response["id"] == 1)
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 18);
    assert!(
        !tools
            .iter()
            .any(|tool| tool["name"] == "solar_project_result")
    );
    let result = cli(&["capabilities", "solar.project.result", "--output", "json"]);
    assert_eq!(result["data"]["command"]["id"], "solar.project.result");
    assert!(
        !tools
            .iter()
            .any(|tool| tool["name"] == "solar_reference_acquire")
    );
    for id in ["solar.reference.plan", "solar.reference.import"] {
        let name = id.replace('.', "_");
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        let descriptor = cli(&["capabilities", id, "--output", "json"]);
        let expected = descriptor["data"]["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg["name"].as_str().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(properties, expected);
    }
}
