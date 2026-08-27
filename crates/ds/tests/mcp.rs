//! Real stdio coverage for chapter routing, typed profiles, and CLI parity.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn cli(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .output()
        .expect("ds runs");
    assert!(
        output.status.success(),
        "ds {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("one CLI envelope")
}

fn mcp(args: &[&str], requests: &[Value]) -> (Vec<Value>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "serve"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP server starts");
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for request in requests {
            serde_json::to_writer(&mut *stdin, request).expect("request");
            stdin.write_all(b"\n").expect("newline");
        }
        serde_json::to_writer(
            &mut *stdin,
            &json!({ "jsonrpc": "2.0", "id": 999, "method": "shutdown" }),
        )
        .expect("shutdown");
        stdin.write_all(b"\n").expect("newline");
    }
    let output = child.wait_with_output().expect("MCP server exits");
    assert!(output.status.success(), "MCP server failed");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 protocol output");
    let responses = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("non-JSON MCP stdout ({error}): {line}"))
        })
        .collect();
    (
        responses,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn response(responses: &[Value], id: i64) -> &Value {
    responses
        .iter()
        .find(|value| value["id"].as_i64() == Some(id))
        .unwrap_or_else(|| panic!("no response for id {id}: {responses:?}"))
}

#[test]
fn broad_server_has_twelve_stable_tools_and_reports_build_identity() {
    let (responses, stderr) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        ],
    );
    let tools = response(&responses, 2)["result"]["tools"]
        .as_array()
        .expect("tools");
    assert_eq!(tools.len(), 12);
    assert_eq!(tools[0]["name"], "ds_catalog");
    assert!(tools.iter().any(|tool| tool["name"] == "ds_pls_cadd"));
    assert!(tools.iter().any(|tool| tool["name"] == "ds_survey"));
    assert!(tools.iter().any(|tool| tool["name"] == "ds_vector_tiles"));
    assert!(tools.iter().any(|tool| tool["name"] == "ds_workstation"));
    let version = cli(&["version", "--output", "json"]);
    assert_eq!(
        response(&responses, 1)["result"]["serverInfo"]["sourceSha"],
        version["data"]["source_sha"]
    );
    let install = cli(&["mcp", "install", "--output", "json"]);
    assert_eq!(install["data"]["source_sha"], version["data"]["source_sha"]);
    assert_eq!(
        install["data"]["entry"]["servers"]["ds"]["args"],
        json!(["mcp", "serve", "--exposure", "chapters"])
    );
    assert!(stderr.contains("serving 12"), "{stderr}");
}

#[test]
fn chapter_describe_and_invoke_return_the_exact_cli_envelopes() {
    let (responses, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_operations", "arguments": { "operation": "describe", "command": "shell.status" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_operations", "arguments": { "operation": "invoke", "command": "shell.status", "arguments": {} } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_workstation", "arguments": { "operation": "describe", "command": "workstation.plan" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ds_workstation", "arguments": { "operation": "invoke", "command": "workstation.plan", "arguments": { "component": "qgis", "platform": "windows" } } } }),
        ],
    );
    assert_eq!(
        response(&responses, 1)["result"]["structuredContent"],
        cli(&["capabilities", "shell.status", "--output", "json"])
    );
    assert_eq!(
        response(&responses, 2)["result"]["structuredContent"],
        cli(&["shell", "status", "--output", "json"])
    );
    assert_eq!(
        response(&responses, 3)["result"]["structuredContent"],
        cli(&["capabilities", "workstation.plan", "--output", "json"])
    );
    assert_eq!(
        response(&responses, 4)["result"]["structuredContent"],
        cli(&[
            "workstation",
            "plan",
            "--component",
            "qgis",
            "--platform",
            "windows",
            "--output",
            "json",
        ])
    );
}

#[test]
fn chapter_routing_refuses_escape_and_confirmation_misuse() {
    let (responses, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_survey", "arguments": { "operation": "invoke", "command": "tile.generate", "arguments": {} } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_operations", "arguments": { "operation": "invoke", "command": "shell.status", "arguments": {}, "confirm": true } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_vector_tiles", "arguments": { "operation": "invoke", "command": "tile.generate", "arguments": { "confirm": true } } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ds_operations", "arguments": { "operation": "invoke", "command": "definitely.not-a-command", "arguments": {} } } }),
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "ds_vector_tiles", "arguments": { "operation": "invoke", "command": "tile.generate", "arguments": { "type": "survey" } } } }),
        ],
    );
    assert_eq!(response(&responses, 1)["error"]["code"], -32602);
    assert!(
        response(&responses, 1)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("ds_vector_tiles")
    );
    assert!(
        response(&responses, 2)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not accept confirmation")
    );
    assert!(
        response(&responses, 3)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("chapter envelope")
    );
    assert!(
        response(&responses, 4)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("ds_catalog")
    );
    assert_eq!(
        response(&responses, 5)["result"]["structuredContent"]["status"],
        "error"
    );
    assert_eq!(
        response(&responses, 5)["result"]["structuredContent"]["error"]["code"],
        "confirmation_required"
    );
}

#[test]
fn every_specialized_profile_is_bounded_and_catalogued() {
    let mut published = BTreeMap::<&str, BTreeSet<String>>::new();
    for profile in [
        "grid",
        "pls",
        "survey",
        "design-edit",
        "design-run",
        "map",
        "project",
        "solar-run",
        "solar-delivery",
        "operations",
    ] {
        let (responses, _) = mcp(
            &["--exposure", "commands", "--profile", profile],
            &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
        );
        let tools = response(&responses, 1)["result"]["tools"]
            .as_array()
            .expect("tools");
        assert!(
            (2..=15).contains(&tools.len()),
            "{profile}: {}",
            tools.len()
        );
        assert_eq!(tools[0]["name"], "ds_catalog", "{profile}");
        published.insert(
            profile,
            tools
                .iter()
                .skip(1)
                .map(|tool| tool["name"].as_str().unwrap().to_string())
                .collect(),
        );
    }

    let (compatibility, _) = mcp(
        &["--exposure", "commands"],
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    let all = response(&compatibility, 1)["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    for (prefix, first, second) in [
        ("map_design_", "design-edit", "design-run"),
        ("solar_", "solar-run", "solar-delivery"),
    ] {
        let expected = all
            .iter()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect::<BTreeSet<_>>();
        let first_set = &published[first];
        let second_set = &published[second];
        assert!(
            first_set.is_disjoint(second_set),
            "{first} overlaps {second}"
        );
        assert_eq!(
            first_set
                .union(second_set)
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected,
            "{first} and {second} must partition `{prefix}*`"
        );
    }

    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "pls"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "tile_generate", "arguments": {} } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "command": "tile.generate" } } }),
        ],
    );
    assert_eq!(response(&responses, 1)["error"]["code"], -32602);
    assert_eq!(response(&responses, 2)["error"]["code"], -32602);
}
