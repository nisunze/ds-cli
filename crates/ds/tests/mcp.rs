//! Real stdio coverage for chapter routing, typed profiles, and CLI parity.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use ds_cli_contract::spec::Chapter;
use ds_cli_mcp::surface::Profile;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ds-mcp-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temp directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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
    mcp_with_env(args, requests, &[])
}

fn mcp_with_env(
    args: &[&str],
    requests: &[Value],
    environment: &[(&str, &Path)],
) -> (Vec<Value>, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command.args(["mcp", "serve"]).args(args);
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command
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

fn write_skill_bundle(root: &Path, source_sha: &str) {
    let documents = [
        ("ds", "---\nname: ds\n---\n# DS\nUse deployed ds.\n"),
        (
            "ds-mcp-host",
            "---\nname: ds-mcp-host\n---\n# DS MCP host\nUse the live MCP contract.\n",
        ),
    ];
    let mut files = Vec::new();
    for (name, text) in documents {
        let relative = format!("skills/{name}/SKILL.md");
        let path = root.join(&relative);
        fs::create_dir_all(path.parent().unwrap()).expect("skill directory");
        fs::write(&path, text).expect("skill document");
        files.push(json!({
            "path": relative,
            "sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
        }));
    }
    fs::write(
        root.join("receipt.json"),
        serde_json::to_vec(&json!({
            "contract": "ds-cli-skills-bundle/v3",
            "source": "ds-cli",
            "source_sha": source_sha,
            "dirty": false,
            "skills": ["ds", "ds-mcp-host"],
            "files": files,
        }))
        .expect("receipt JSON"),
    )
    .expect("receipt");
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
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "identity" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": {} } }),
        ],
    );
    let tools = response(&responses, 2)["result"]["tools"]
        .as_array()
        .expect("tools");
    // One router per chapter, with the catalogue standing in for its own,
    // plus one bounded diagnostics bootstrap.
    // Derived from the declaration so a new chapter cannot ship unreachable
    // while this test still reads an old literal count.
    assert_eq!(tools.len(), Chapter::ALL.len() + 1);
    assert_eq!(tools[0]["name"], "ds_catalog");
    assert_eq!(tools[1]["name"], "ds_diagnostics");
    for chapter in Chapter::ALL {
        let name = ds_cli_mcp::surface::chapter_tool_name(*chapter);
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "chapter `{chapter}` publishes no tool"
        );
    }
    let version = cli(&["version", "--output", "json"]);
    let identity = &response(&responses, 3)["result"]["structuredContent"]["data"];
    assert_eq!(identity["source_sha"], version["data"]["source_sha"]);
    assert_eq!(identity["version"], version["data"]["version"]);
    assert_eq!(identity["mcp"]["transport"], "stdio");
    assert_eq!(
        response(&responses, 4)["result"]["structuredContent"]["identity"],
        *identity
    );
    assert!(
        response(&responses, 1)["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains(version["data"]["source_sha"].as_str().unwrap())
    );
    let install = cli(&["mcp", "install", "--output", "json"]);
    assert_eq!(install["data"]["source_sha"], version["data"]["source_sha"]);
    assert_eq!(install["data"]["written"], json!(false));
    assert_eq!(
        install["data"]["entry"]["servers"]["ds"]["args"],
        json!(["mcp", "serve", "--exposure", "chapters"])
    );
    assert!(
        install["data"]["supported_hosts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|host| host["token"] == "claude-desktop")
    );
    assert_eq!(install["data"]["connection"]["transport"], "stdio");
    assert!(
        stderr.contains(&format!("serving {}", Chapter::ALL.len() + 1)),
        "{stderr}"
    );
}

#[test]
fn mcp_only_agent_reads_receipt_verified_skills_without_a_skills_home() {
    let temp = TestDir::new("resources");
    let bundle = temp.0.join("bundle");
    fs::create_dir_all(&bundle).expect("bundle root");
    let version = cli(&["version", "--output", "json"]);
    let source_sha = version["data"]["source_sha"].as_str().unwrap();
    write_skill_bundle(&bundle, source_sha);
    let missing_home = temp.0.join("no-agent-home");

    let (responses, _) = mcp_with_env(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": { "uri": "ds-skill://bundle/ds/SKILL.md" } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/read", "params": { "uri": "ds-skill://bundle/ds-mcp-host/SKILL.md" } }),
            json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": { "uri": "file:///etc/passwd" } }),
            json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "identity" } } }),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": {} } }),
        ],
        &[("DS_CLI_SKILLS_BUNDLE", &bundle), ("HOME", &missing_home)],
    );

    assert_eq!(
        response(&responses, 1)["result"]["capabilities"]["resources"]["subscribe"],
        false
    );
    let resources = response(&responses, 2)["result"]["resources"]
        .as_array()
        .expect("resources");
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0]["name"], "ds");
    assert_eq!(resources[1]["name"], "ds-mcp-host");
    assert!(
        response(&responses, 3)["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Use deployed ds")
    );
    assert!(
        response(&responses, 4)["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("live MCP contract")
    );
    assert_eq!(response(&responses, 5)["error"]["code"], -32602);
    let identity = &response(&responses, 6)["result"]["structuredContent"]["data"];
    assert_eq!(identity["skills"]["source_sha"], source_sha);
    assert_eq!(identity["skills"]["count"], 2);
    assert_eq!(identity["skills"]["requires_skills_home"], false);
    assert_eq!(
        response(&responses, 7)["result"]["structuredContent"]["skill_resources"],
        identity["skills"]
    );
    assert!(
        !missing_home.exists(),
        "MCP resources must not create an agent skills home"
    );
}

#[test]
fn diagnostics_reuse_the_exact_cli_envelopes_in_a_typed_profile() {
    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "survey-migration"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "doctor" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "shell.status" } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "capabilities" } } }),
        ],
    );
    assert_eq!(
        response(&responses, 1)["result"]["structuredContent"],
        cli(&["doctor", "--output", "json"])
    );
    assert_eq!(
        response(&responses, 2)["result"]["structuredContent"],
        cli(&["shell", "status", "--output", "json"])
    );
    assert_eq!(
        response(&responses, 3)["result"]["structuredContent"],
        cli(&["capabilities", "--output", "json"])
    );
}

#[test]
fn startup_schema_discovery_does_not_resolve_command_availability() {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["capabilities", "solar.engine", "--output", "json"])
        .env("DS_MCP_SCHEMA_ONLY", "1")
        .output()
        .expect("schema discovery runs");
    assert!(output.status.success());
    let descriptor: Value = serde_json::from_slice(&output.stdout).expect("descriptor envelope");
    assert_eq!(descriptor["data"]["command"]["availability"], "unchecked");

    let live = cli(&["capabilities", "solar.engine", "--output", "json"]);
    assert_ne!(live["data"]["command"]["availability"], "unchecked");
}

#[test]
fn a_map_refusal_is_lazy_and_does_not_end_the_headless_server() {
    let temp = TestDir::new("descriptor");
    let missing_descriptor = temp.0.join("no-desktop.json");
    let descriptor = missing_descriptor.to_string_lossy().into_owned();
    let (responses, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_survey", "arguments": { "operation": "invoke", "command": "map.layer.list", "arguments": { "desktop-descriptor": descriptor } } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "capabilities" } } }),
        ],
    );
    assert_eq!(
        response(&responses, 1)["result"]["serverInfo"]["name"],
        "ds"
    );
    assert_eq!(
        response(&responses, 2)["result"]["structuredContent"]["error"]["code"],
        "desktop_not_paired"
    );
    assert_eq!(
        response(&responses, 3)["result"]["structuredContent"]["status"],
        "ok"
    );
}

#[test]
fn install_discovery_is_blind_but_writing_stays_gated() {
    let discovery = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "install", "--output", "json"])
        .output()
        .expect("ds runs");
    assert!(discovery.status.success());

    // Generic has no target, but --write must still hit confirmation before
    // adapter-specific refusal logic.
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "mcp", "install", "--host", "generic", "--write", "--output", "json",
        ])
        .output()
        .expect("ds runs");
    assert_eq!(output.status.code(), Some(2), "an unconfirmed gate exits 2");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("one CLI envelope");
    assert_eq!(envelope["error"]["code"], "confirmation_required");

    let descriptor = cli(&["capabilities", "mcp.install", "--output", "json"]);
    let descriptor = &descriptor["data"]["command"];
    assert_eq!(descriptor["effect"], "machine_write");
    assert_eq!(descriptor["confirmation_required"], json!(true));
    assert_eq!(descriptor["confirmation_trigger"], "--write");
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
        Profile::SurveyMigration,
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
        "survey, form-factory, survey-projects, survey-migration, and layers must partition the live Survey chapter"
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
    assert!(survey_project_names.contains("survey_query"));
    assert!(survey_project_names.contains("survey_entries_select"));
    assert!(survey_project_names.contains("survey_entries_changes"));
    assert!(survey_project_names.contains("survey_entries_create"));
    assert!(!survey_project_names.contains("survey_form_lifecycle"));
    assert!(!survey_project_names.contains("survey_entries_import"));

    let (migration_responses, _) = mcp(
        &["--exposure", "commands", "--profile", "survey-migration"],
        &[json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" })],
    );
    let migration = response(&migration_responses, 4)["result"]["tools"]
        .as_array()
        .expect("survey-migration tools");
    assert_eq!(
        migration.len(),
        3,
        "catalog and diagnostics plus one bounded import leaf"
    );
    assert_eq!(migration[0]["name"], "ds_catalog");
    assert_eq!(migration[1]["name"], "ds_diagnostics");
    assert_eq!(migration[2]["name"], "survey_entries_import");
    assert_eq!(migration[2]["title"], "survey.entries.import");
    assert_eq!(
        migration[2]["inputSchema"]["properties"]["confirm"]["type"],
        "boolean"
    );
    assert!(
        migration[2]["inputSchema"]["properties"]
            .get("project")
            .is_none()
    );
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
    let query = survey_projects
        .iter()
        .find(|tool| tool["name"] == "survey_query")
        .expect("native selected-project Survey query tool");
    assert_eq!(query["title"], "survey.query");
    assert_eq!(query["inputSchema"]["required"], json!(["form"]));
    assert_eq!(
        query["inputSchema"]["properties"]["filter"]["type"],
        "array"
    );
    assert_eq!(
        query["inputSchema"]["properties"]["group-by"]["type"],
        "array"
    );
    for forbidden in [
        "project",
        "url",
        "body",
        "token",
        "raw",
        "entry",
        "media",
        "desktop-descriptor",
    ] {
        assert!(query["inputSchema"]["properties"].get(forbidden).is_none());
    }
    let entries = survey_projects
        .iter()
        .find(|tool| tool["name"] == "survey_entries_select")
        .expect("native selected-project Survey entry selection tool");
    assert_eq!(entries["title"], "survey.entries.select");
    assert_eq!(entries["inputSchema"]["required"], json!(["form", "bbox"]));
    assert_eq!(
        entries["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["bbox", "form", "lane", "limit"])
    );
    assert_eq!(
        entries["inputSchema"]["properties"]["limit"]["default"],
        "100"
    );
    let changes = survey_projects
        .iter()
        .find(|tool| tool["name"] == "survey_entries_changes")
        .expect("native selected-project Survey changes tool");
    assert_eq!(changes["title"], "survey.entries.changes");
    assert_eq!(
        changes["inputSchema"]["required"],
        json!(["form", "updated-after"])
    );
    assert_eq!(
        changes["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["cursor", "form", "lane", "limit", "updated-after"])
    );
    assert_eq!(
        changes["inputSchema"]["properties"]["limit"]["default"],
        "100"
    );
    let entry_create = survey_projects
        .iter()
        .find(|tool| tool["name"] == "survey_entries_create")
        .expect("native selected-project Survey create tool");
    assert_eq!(entry_create["title"], "survey.entries.create");
    assert_eq!(
        entry_create["inputSchema"]["required"],
        json!([
            "form",
            "doc-id",
            "idempotency-key",
            "created-at",
            "document"
        ])
    );
    assert_eq!(
        entry_create["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "confirm",
            "context-key",
            "created-at",
            "doc-id",
            "document",
            "form",
            "idempotency-key",
            "lane",
        ])
    );
    assert_eq!(
        entry_create["inputSchema"]["properties"]["confirm"]["type"],
        "boolean"
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
        "survey-migration",
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
        let maximum = if profile == "survey-projects" { 18 } else { 16 };
        assert!(
            (2..=maximum).contains(&tools.len()),
            "{profile}: {}",
            tools.len()
        );
        assert_eq!(tools[0]["name"], "ds_catalog", "{profile}");
        assert_eq!(tools[1]["name"], "ds_diagnostics", "{profile}");
        published.insert(
            profile,
            tools
                .iter()
                .skip(2)
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
