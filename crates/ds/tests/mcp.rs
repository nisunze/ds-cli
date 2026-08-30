//! Real stdio coverage for chapter routing, typed profiles, and CLI parity.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Stdio};

use ds_cli_contract::spec::Chapter;
use ds_cli_mcp::surface::Profile;
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
    assert!(
        output.status.success(),
        "MCP server failed for `{}`: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
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
fn broad_server_has_declared_stable_tools_and_reports_build_identity() {
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
    // One router per chapter, with the catalogue standing in for its own.
    // Derived from the declaration so a new chapter cannot ship unreachable
    // while this test still reads an old literal count.
    assert_eq!(tools.len(), Chapter::ALL.len());
    assert_eq!(tools[0]["name"], "ds_catalog");
    for chapter in Chapter::ALL {
        let name = ds_cli_mcp::surface::chapter_tool_name(*chapter);
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "chapter `{chapter}` publishes no tool"
        );
    }
    let version = cli(&["version", "--output", "json"]);
    assert_eq!(
        response(&responses, 1)["result"]["serverInfo"]["sourceSha"],
        version["data"]["source_sha"]
    );
    // `--yes` because `mcp.install` is `machine_write`; without `--write` it
    // still only prints, and this call writes nothing.
    let install = cli(&["mcp", "install", "--output", "json", "--yes"]);
    assert_eq!(install["data"]["source_sha"], version["data"]["source_sha"]);
    assert_eq!(install["data"]["written"], json!(false));
    assert_eq!(
        install["data"]["entry"]["servers"]["ds"]["args"],
        json!(["mcp", "serve", "--exposure", "chapters"])
    );
    assert!(
        stderr.contains(&format!("serving {}", Chapter::ALL.len())),
        "{stderr}"
    );
}

#[test]
fn installing_the_host_entry_is_gated_and_names_its_own_gate() {
    // F7: the effect class was `local_file_write`, which is not in the
    // confirmation set, so the `--write --yes` this command's help, its
    // reference doc and the `ds-mcp-host` skill all asked for was decorative.
    // Nothing is written here: the refusal happens before the handler runs.
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "install", "--output", "json"])
        .output()
        .expect("ds runs");
    assert_eq!(output.status.code(), Some(2), "an unconfirmed gate exits 2");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("one CLI envelope");
    assert_eq!(envelope["error"]["code"], "confirmation_required");

    let descriptor = cli(&["capabilities", "mcp.install", "--output", "json"]);
    let descriptor = &descriptor["data"]["command"];
    assert_eq!(descriptor["effect"], "machine_write");
    assert_eq!(descriptor["confirmation_required"], json!(true));
    let codes: BTreeSet<String> = descriptor["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().expect("code").to_string())
        .collect();
    assert!(
        codes.contains("confirmation_required"),
        "a gate a caller cannot discover from the descriptor is not a contract: {codes:?}"
    );
    assert!(
        codes.contains("mcp_capabilities_unavailable"),
        "`build_identity` runs before the entry is printed: {codes:?}"
    );
}

#[test]
fn by_command_profiles_still_partition_the_live_registry() {
    // F36: chapter membership is declared once, on the command. Split
    // profiles are not — they hand-list command ids, and an id nobody added
    // is simply unreachable through its profile with every unit test still
    // green. The lists partitioned the registry when written; this is what
    // makes that a fact rather than a snapshot.
    // Discovery is tiered on purpose, so the id set is walked domain by
    // domain rather than read from a flat index that does not exist.
    let mut live: BTreeSet<String> = BTreeSet::new();
    for domain in cli(&["capabilities", "--output", "json"])["data"]["domains"]
        .as_array()
        .expect("domains")
    {
        let domain = domain["id"].as_str().expect("domain id");
        for command in cli(&["capabilities", domain, "--output", "json"])["data"]["commands"]
            .as_array()
            .expect("commands")
        {
            live.insert(command["id"].as_str().expect("id").to_string());
        }
    }
    assert!(!live.is_empty(), "the live registry must not be empty");

    for (prefix, profiles) in [
        (
            "map.design.",
            &[Profile::DesignEdit, Profile::DesignRun][..],
        ),
        (
            "solar.",
            &[
                Profile::SolarInput,
                Profile::SolarRun,
                Profile::SolarDelivery,
            ][..],
        ),
    ] {
        let expected: BTreeSet<String> = live
            .iter()
            .filter(|id| {
                id.starts_with(prefix)
                    || (prefix == "map.design."
                        && matches!(
                            id.as_str(),
                            "design.features.select"
                                | "design.lv.project-export"
                                | "design.lv.process"
                        ))
            })
            .cloned()
            .collect();
        assert!(!expected.is_empty(), "no live `{prefix}*` commands");

        let mut listed: BTreeSet<String> = BTreeSet::new();
        for profile in profiles.iter().copied() {
            for id in profile.command_ids() {
                assert!(
                    listed.insert((*id).to_string()),
                    "`{id}` is claimed by more than one `{prefix}*` profile"
                );
            }
        }
        let missing: Vec<&String> = expected.difference(&listed).collect();
        assert!(
            missing.is_empty(),
            "these live `{prefix}*` commands reach no profile: {missing:?}"
        );
        let stale: Vec<&String> = listed.difference(&expected).collect();
        assert!(
            stale.is_empty(),
            "these profile entries name no live command: {stale:?}"
        );
    }

    let expected_survey: BTreeSet<String> = live
        .iter()
        .filter(|id| {
            cli(&["capabilities", id, "--output", "json"])["data"]["command"]["chapter"] == "survey"
        })
        .cloned()
        .collect();
    let mut listed_survey = BTreeSet::new();
    for profile in [
        Profile::Survey,
        Profile::FormFactory,
        Profile::SurveyProjects,
        Profile::Layers,
    ] {
        for id in profile.command_ids() {
            assert!(
                listed_survey.insert((*id).to_string()),
                "`{id}` is claimed by more than one Survey profile"
            );
        }
    }
    assert_eq!(
        listed_survey, expected_survey,
        "survey, form-factory, survey-projects, and layers must partition the live Survey chapter"
    );
}

#[test]
fn form_factory_and_survey_projects_keep_their_distinct_mapless_contracts() {
    // These two profiles are deliberately adjacent but not interchangeable:
    // one manages global master schemas, the other project bindings/templates
    // and new-project instantiation. Describe is discovery, so this proof must
    // not need an application session or cause MCP's desktop gate to launch.
    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "form-factory"],
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    let form_factory = response(&responses, 1)["result"]["tools"]
        .as_array()
        .expect("form-factory tools");
    let form_factory_names = form_factory
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<BTreeSet<_>>();
    assert!(form_factory_names.contains("survey_form_lifecycle"));
    assert!(!form_factory_names.contains("survey_project-form_settings"));
    assert!(!form_factory_names.contains("survey_project_create-from-template"));
    let lifecycle = form_factory
        .iter()
        .find(|tool| tool["name"] == "survey_form_lifecycle")
        .expect("form lifecycle tool");
    assert_eq!(lifecycle["title"], "survey.form.lifecycle");
    assert_eq!(
        lifecycle["inputSchema"]["properties"]["confirm"]["type"],
        "boolean"
    );
    assert_eq!(
        lifecycle["inputSchema"]["properties"]["action"]["enum"],
        json!([
            "duplicate",
            "publish",
            "unpublish",
            "archive",
            "restore",
            "delete"
        ])
    );

    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "survey-projects"],
        &[json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })],
    );
    let survey_projects = response(&responses, 3)["result"]["tools"]
        .as_array()
        .expect("survey-project tools");
    let survey_project_names = survey_projects
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<BTreeSet<_>>();
    assert!(survey_project_names.contains("survey_project_create-from-template"));
    assert!(survey_project_names.contains("survey_project-form_settings"));
    assert!(!survey_project_names.contains("survey_form_lifecycle"));
    let creation = survey_projects
        .iter()
        .find(|tool| tool["name"] == "survey_project_create-from-template")
        .expect("create-from-template tool");
    assert_eq!(creation["title"], "survey.project.create-from-template");
    assert_eq!(
        creation["inputSchema"]["properties"]["project-name"]["type"],
        "string"
    );
    assert_eq!(
        creation["inputSchema"]["properties"]["confirm"]["type"],
        "boolean"
    );
    let settings = survey_projects
        .iter()
        .find(|tool| tool["name"] == "survey_project-form_settings")
        .expect("native selected-project settings tool");
    assert_eq!(settings["title"], "survey.project-form.settings");
    assert_eq!(settings["inputSchema"]["required"], json!(["form"]));
    assert!(
        settings["inputSchema"]["properties"]
            .get("project")
            .is_none()
    );
    assert!(
        settings["inputSchema"]["properties"]
            .get("desktop-descriptor")
            .is_none()
    );
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
        "pls-library",
        "library-governance",
        "survey",
        "form-factory",
        "survey-projects",
        "design-edit",
        "design-run",
        "map",
        "layers",
        "tiling",
        "project",
        "solar-input",
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
    assert!(
        published["pls"].contains("pls_backup-create"),
        "the PLS profile must expose the live backup command without a second MCP schema"
    );

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
    for (prefix, profiles) in [
        ("map_design_", &["design-edit", "design-run"][..]),
        (
            "solar_",
            &["solar-input", "solar-run", "solar-delivery"][..],
        ),
        ("pls_", &["pls", "pls-library", "library-governance"][..]),
    ] {
        let expected = all
            .iter()
            .filter(|name| {
                name.starts_with(prefix)
                    || (prefix == "map_design_"
                        && matches!(
                            name.as_str(),
                            "design_features_select"
                                | "design_lv_project-export"
                                | "design_lv_process"
                        ))
                    || (prefix == "pls_" && name.starts_with("library_"))
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut union = BTreeSet::new();
        for profile in profiles {
            let current = &published[profile];
            assert!(
                union.is_disjoint(current),
                "{profile} overlaps a sibling profile"
            );
            union.extend(current.iter().cloned());
        }
        assert_eq!(union, expected, "{profiles:?} must partition `{prefix}*`");
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
