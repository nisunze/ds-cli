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

fn isolated_cli(args: &[&str]) -> Value {
    let profile = TestDir::new("isolated-profile");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("HOME", &profile.0)
        .env("USERPROFILE", &profile.0)
        .env("APPDATA", &profile.0)
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

fn cli_envelope(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .output()
        .expect("ds runs");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "ds {} returned no envelope ({error}): {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn capability_input<'a>(envelope: &'a Value, name: &str) -> &'a Value {
    envelope["data"]["command"]["inputs"]
        .as_array()
        .expect("capability inputs")
        .iter()
        .find(|input| input["name"] == name)
        .unwrap_or_else(|| panic!("capability has no `{name}` input"))
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

// MCP preserves every CLI descriptor field. Since 2026-09-22 nothing in a
// descriptor names the terminal sign-in — every signed-out remedy is the one
// `ds account connect` sentence — so the belt-and-braces scrub in
// `ds-cli-mcp` rewrites nothing, and the two descriptors are equal.
fn assert_mcp_descriptor_with_device_link(actual: &Value, cli_descriptor: Value) {
    assert_eq!(actual, &cli_descriptor);
    assert_no_terminal_sign_in("descriptor", &actual.to_string());
}

/// The words that would send a person to a terminal sign-in. No MCP answer
/// may carry them; `ds-cli-mcp` publishes the same list.
fn assert_no_terminal_sign_in(label: &str, text: &str) {
    let lower = text.to_lowercase();
    for banned in ds_cli_mcp::surface::TERMINAL_SIGN_IN_WORDS {
        assert!(
            !lower.contains(banned),
            "{label} names the terminal sign-in (`{banned}`): {text}"
        );
    }
}

/// The one signed-out sentence is the same on both sides of the executable
/// boundary: `ds-cli-mcp` cannot depend on `ds-cli-auth`, so it spells the
/// sentence itself, and this is what holds the two copies equal.
#[test]
fn the_mcp_signed_out_remedy_is_the_cli_signed_out_remedy() {
    assert_eq!(
        ds_cli_mcp::surface::DEVICE_LINK_REMEDY,
        ds_cli_auth::SIGNED_OUT_REMEDY
    );
    assert_no_terminal_sign_in("remedy", ds_cli_auth::SIGNED_OUT_REMEDY);
    assert_no_terminal_sign_in("instructions", ds_cli_auth::APPROVAL_INSTRUCTIONS);
}

/// The whole published surface — instructions, every tool description, every
/// catalogue row, every descriptor, every skill resource — names no terminal
/// sign-in. This is the gate the owner asked for on 2026-09-22 after a
/// non-technical engineer was told to open a terminal: the words are banned
/// everywhere an MCP host can read, not only in the one field that leaked.
///
/// Cost is kept in mind: every descriptor is read once through the CLI in
/// schema mode (what the MCP server itself reads at startup, and what a
/// `describe` answers verbatim — `assert_mcp_descriptor_with_device_link`
/// holds that equality), and the live MCP sweep describes the sign-in
/// commands themselves rather than resolving availability for four hundred.
#[test]
fn no_published_mcp_text_names_a_terminal_sign_in() {
    let bundle = TestDir::new("clean-skills");
    let version = cli(&["version", "--output", "json"]);
    let source_sha = version["data"]["source_sha"].as_str().expect("source sha");
    write_skill_bundle(&bundle.0, source_sha);

    // Every command the surface publishes, read from the catalogue itself so
    // the sweep is derived from the live surface rather than a list that
    // could go stale.
    let routed: Vec<Chapter> = Chapter::ALL
        .iter()
        .copied()
        .filter(|chapter| *chapter != Chapter::Catalog)
        .collect();
    let mut requests = vec![
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": {} } }),
        json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "identity" } } }),
        json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "query": "sign in" } } }),
    ];
    let mut next_id = 100;
    for chapter in &routed {
        requests.push(json!({ "jsonrpc": "2.0", "id": next_id, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "chapter": chapter.token() } } }));
        next_id += 1;
    }
    // The sign-in commands are the ones whose descriptors said the banned
    // words; their live describe answers are read through the router.
    for id in [
        "account.connect",
        "auth.status",
        "auth.link.begin",
        "auth.link.status",
        "auth.link.complete",
        "auth.logout",
    ] {
        requests.push(
            json!({ "jsonrpc": "2.0", "id": next_id, "method": "tools/call", "params": {
            "name": ds_cli_mcp::surface::chapter_tool_name(Chapter::Project),
            "arguments": { "operation": "describe", "command": id }
        } }),
        );
        next_id += 1;
    }
    for name in ["ds", "ds-mcp-host"] {
        requests.push(json!({ "jsonrpc": "2.0", "id": next_id, "method": "resources/read", "params": { "uri": format!("ds-skill://bundle/{name}/SKILL.md") } }));
        next_id += 1;
    }
    let (responses, _) = mcp_with_env(
        &["--exposure", "chapters"],
        &requests,
        &[("DS_CLI_SKILLS_BUNDLE", bundle.0.as_path())],
    );
    assert_eq!(
        responses.len(),
        requests.len() + 1,
        "one answer per request"
    );
    let mut published: Vec<String> = Vec::new();
    for response in &responses {
        assert!(
            response.get("error").is_none() || response["id"] == 999,
            "an MCP call in the sweep was refused: {response}"
        );
        assert_no_terminal_sign_in("MCP answer", &response.to_string());
        if let Some(commands) = response["result"]["structuredContent"]["commands"].as_array() {
            published.extend(
                commands
                    .iter()
                    .filter_map(|row| row["id"].as_str().map(str::to_string)),
            );
        }
    }
    assert!(published.iter().any(|id| id == "account.connect"));
    assert!(
        published
            .iter()
            .all(|id| id != "auth.login" && id != "auth.link.approve"),
        "{published:?}"
    );

    // Every published descriptor, as the server reads it at startup and as
    // `describe` answers it.
    for id in &published {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(["capabilities", id, "--output", "json"])
            .env("DS_CLI_SCHEMA_ONLY", "1")
            .output()
            .expect("ds runs");
        assert!(output.status.success(), "ds capabilities {id} failed");
        assert_no_terminal_sign_in(
            &format!("descriptor `{id}`"),
            &String::from_utf8_lossy(&output.stdout),
        );
    }

    // The instructions name the one sign-in, and the catalogue's answer to
    // a person's own words for it is that command.
    let instructions = response(&responses, 1)["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(
        instructions.contains("ds account connect"),
        "{instructions}"
    );
    assert!(instructions.contains("account.connect"), "{instructions}");
    assert!(
        instructions.contains("Link a trusted device"),
        "{instructions}"
    );
    assert_eq!(
        response(&responses, 6)["result"]["structuredContent"]["results"][0]["id"],
        "account.connect"
    );

    // The broad command exposure builds its tool descriptions from the same
    // descriptors; the typed profiles are swept in
    // `every_specialized_profile_is_bounded_and_catalogued`.
    let (broad, _) = mcp(
        &["--exposure", "commands"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        ],
    );
    for response in &broad {
        assert_no_terminal_sign_in("broad command exposure", &response.to_string());
    }
}

/// The skill resources a host reads are the repository's own skills, and
/// none of them names the terminal sign-in either. The MCP sweep above reads
/// a fixture bundle; this reads the real documents that ship.
#[test]
fn no_shipped_skill_names_a_terminal_sign_in() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
    let mut seen = 0;
    for entry in fs::read_dir(&skills).expect("skills directory").flatten() {
        let document = entry.path().join("SKILL.md");
        if !document.is_file() {
            continue;
        }
        seen += 1;
        let text = fs::read_to_string(&document).expect("skill document");
        assert_no_terminal_sign_in(&document.display().to_string(), &text);
    }
    assert!(seen > 0, "no skills under {}", skills.display());
}

#[test]
fn auth_mcp_profile_advertises_only_device_link_sign_in() {
    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "auth-context"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        ],
    );
    let instructions = response(&responses, 1)["result"]["instructions"]
        .as_str()
        .expect("MCP instructions");
    assert!(instructions.contains("account.connect"), "{instructions}");
    assert_no_terminal_sign_in("auth-context instructions", instructions);
    let names = response(&responses, 2)["result"]["tools"]
        .as_array()
        .expect("MCP tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(names.contains("account_connect"), "{names:?}");
    assert!(names.contains("auth_link_begin"));
    assert!(names.contains("auth_link_complete"));
    assert!(!names.contains("auth_login"));
    assert!(!names.contains("auth_link_approve"));
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
    assert_eq!(identity["mcp"]["call_timeout_seconds"], 3600);
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
    let install = isolated_cli(&["mcp", "install", "--output", "json"]);
    assert_eq!(install["data"]["source_sha"], version["data"]["source_sha"]);
    assert_eq!(install["data"]["written"], json!(false));
    let registration_name = install["data"]["registration_name"]
        .as_str()
        .expect("registration name");
    assert_eq!(
        install["data"]["entry"]["servers"][registration_name]["args"],
        json!(["mcp", "serve", "--exposure", "chapters"])
    );
    assert_eq!(
        response(&responses, 1)["result"]["serverInfo"]["name"],
        install["data"]["server_name"]
    );
    assert_eq!(
        response(&responses, 1)["result"]["serverInfo"]["title"],
        install["data"]["server_title"]
    );
    assert_eq!(identity["mcp"]["registration_name"], registration_name);
    assert_eq!(
        identity["mcp"]["release_lane"],
        install["data"]["release_lane"]
    );
    assert_eq!(
        identity["mcp"]["runtime_platform"],
        install["data"]["runtime_platform"]
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
        .env("DS_CLI_SCHEMA_ONLY", "1")
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
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_survey", "arguments": { "operation": "invoke", "command": "map.view", "arguments": { "desktop-descriptor": descriptor } } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_diagnostics", "arguments": { "operation": "capabilities" } } }),
        ],
    );
    let install = isolated_cli(&["mcp", "install", "--output", "json"]);
    assert_eq!(
        response(&responses, 1)["result"]["serverInfo"]["name"],
        install["data"]["server_name"]
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
    let _discovery = isolated_cli(&["mcp", "install", "--output", "json"]);

    // Generic has no target, but --write must still hit confirmation before
    // adapter-specific refusal logic.
    let profile = TestDir::new("unconfirmed-install");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "mcp", "install", "--host", "generic", "--write", "--output", "json",
        ])
        .env("HOME", &profile.0)
        .env("USERPROFILE", &profile.0)
        .env("APPDATA", &profile.0)
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
fn antigravity_proposal_is_distinct_from_gemini_cli_and_remains_read_only() {
    let home = TestDir::new("antigravity-install");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "mcp",
            "install",
            "--host",
            "antigravity",
            "--output",
            "json",
        ])
        .env("HOME", &home.0)
        .env("USERPROFILE", &home.0)
        .output()
        .expect("ds runs");
    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("one CLI envelope");
    let data = &envelope["data"];
    let registration_name = data["registration_name"]
        .as_str()
        .expect("registration name");
    let expected = home
        .0
        .join(".gemini")
        .join("config")
        .join("mcp_config.json");
    assert_eq!(data["host"], "antigravity");
    assert_eq!(data["path"], expected.display().to_string());
    assert_eq!(data["change"], "would_create");
    assert_eq!(data["written"], false);
    assert_eq!(
        data["entry"]["mcpServers"][registration_name]["command"],
        data["executable"]
    );
    assert!(!expected.exists(), "a proposal must not touch host config");
    assert_ne!(
        expected,
        home.0.join(".gemini").join("settings.json"),
        "Antigravity and Gemini CLI do not share a configuration target"
    );
}

#[test]
fn codex_install_plans_writes_and_reports_the_restart_handoff_without_vscode() {
    let home = TestDir::new("codex-install");
    let path = home.0.join(".codex").join("config.toml");
    let invoke = |extra: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(["mcp", "install", "--host", "codex"])
            .args(extra)
            .args(["--output", "json"])
            .env("HOME", &home.0)
            .env("USERPROFILE", &home.0)
            .output()
            .expect("ds runs");
        let envelope: Value = serde_json::from_slice(&output.stdout).expect("one CLI envelope");
        (output.status, envelope)
    };

    let (status, planned) = invoke(&[]);
    assert!(status.success());
    assert_eq!(planned["data"]["path"], path.display().to_string());
    assert_eq!(planned["data"]["change"], "would_create");
    assert_eq!(planned["data"]["written"], false);
    assert_eq!(planned["data"]["restart_required"], false);
    assert!(!path.exists(), "the proposal must remain read-only");

    let (status, written) = invoke(&["--write", "--yes"]);
    assert!(status.success());
    assert_eq!(written["data"]["change"], "created");
    assert_eq!(written["data"]["written"], true);
    assert_eq!(written["data"]["changed"], true);
    assert_eq!(written["data"]["restart_required"], true);
    assert!(
        written["data"]["restart_handoff"]
            .as_str()
            .unwrap()
            .contains("fully quit and restart Codex")
    );
    let config = fs::read_to_string(&path).expect("Codex config");
    let registration_name = written["data"]["registration_name"]
        .as_str()
        .expect("registration name");
    assert!(config.contains(&format!("[mcp_servers.{registration_name}]")));
    assert!(config.contains("\"mcp\", \"serve\""));

    let (status, repeated) = invoke(&["--write", "--yes"]);
    assert!(status.success());
    assert_eq!(repeated["data"]["change"], "unchanged");
    assert_eq!(repeated["data"]["changed"], false);
    assert_eq!(repeated["data"]["restart_required"], false);
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
                Profile::SolarMigration,
                Profile::SolarApplication,
                Profile::SolarRun,
                Profile::SolarDashboard,
                Profile::SolarDelivery,
                Profile::SolarPortfolioBatch,
            ][..],
        ),
        ("style.", &[Profile::Styles, Profile::PrintStyles][..]),
    ] {
        let expected: BTreeSet<String> = live
            .iter()
            .filter(|id| {
                id.starts_with(prefix)
                    || (prefix == "map.design."
                        && matches!(
                            id.as_str(),
                            "design.features.select"
                                | "design.intake.upload"
                                | "design.lv.project-export"
                                | "design.lv.process"
                                | "design.known-columns.list"
                                | "design.known-columns.set"
                        ))
            })
            .cloned()
            .collect();
        assert!(!expected.is_empty(), "no live `{prefix}*` commands");

        let mut listed: BTreeSet<String> = BTreeSet::new();
        for profile in profiles.iter().copied() {
            for id in profile.command_ids() {
                assert!(
                    listed.insert((*id).to_string())
                        || (prefix == "solar."
                            && matches!(
                                *id,
                                "solar.engine" | "solar.results.read" | "solar.project.result"
                            )),
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
        Profile::SurveyMedia,
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
        "survey, form-factory, survey-projects, survey-media, survey-migration, and layers must partition the live Survey chapter"
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
    assert!(!survey_project_names.contains("survey_photo_rotate"));

    // The photo workflow is its own bounded profile: what this machine holds,
    // the one rotation and its publication, nothing of forms or templates.
    let (media_responses, _) = mcp(
        &["--exposure", "commands", "--profile", "survey-media"],
        &[json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list" })],
    );
    let media = response(&media_responses, 5)["result"]["tools"]
        .as_array()
        .expect("survey-media tools");
    let media_names = media
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        media_names,
        BTreeSet::from([
            "ds_catalog",
            "ds_diagnostics",
            "survey_entries_read",
            "survey_local_status",
            "survey_moments_list",
            "survey_moments_read",
            "survey_photo_fetch",
            "survey_photo_publish",
            "survey_photo_rotate",
            "survey_photo_rotate-local",
        ])
    );

    let (migration_responses, _) = mcp(
        &["--exposure", "commands", "--profile", "survey-migration"],
        &[json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" })],
    );
    let migration = response(&migration_responses, 4)["result"]["tools"]
        .as_array()
        .expect("survey-migration tools");
    assert_eq!(
        migration.len(),
        5,
        "catalog and diagnostics plus bounded import and the project-to-project \
         plan/apply; native Survey workspace is retired"
    );
    assert_eq!(migration[0]["name"], "ds_catalog");
    assert_eq!(migration[1]["name"], "ds_diagnostics");
    let tool = |name: &str| {
        migration
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("survey-migration publishes {name}"))
    };
    let import = tool("survey_entries_import");
    assert_eq!(import["title"], "survey.entries.import");
    assert_eq!(
        import["inputSchema"]["properties"]["confirm"]["type"],
        "boolean"
    );
    assert!(import["inputSchema"]["properties"].get("project").is_some());
    // Migration is stateless: both projects are required operands of both
    // steps, and only the apply writes.
    for (name, writes) in [
        ("survey_migrate_plan", false),
        ("survey_migrate_apply", true),
    ] {
        let step = tool(name);
        let required = step["inputSchema"]["required"]
            .as_array()
            .expect("required inputs");
        for input in ["source-project", "project"] {
            assert!(
                required.iter().any(|entry| entry == input),
                "{name} requires --{input}"
            );
        }
        assert_eq!(
            step["inputSchema"]["properties"].get("confirm").is_some(),
            writes,
            "{name}"
        );
    }
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
    assert_eq!(
        settings["inputSchema"]["required"],
        json!(["project", "form"])
    );
    assert!(
        settings["inputSchema"]["properties"]
            .get("project")
            .is_some()
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
    assert_eq!(query["inputSchema"]["required"], json!(["project", "form"]));
    assert_eq!(
        query["inputSchema"]["properties"]["filter"]["type"],
        "array"
    );
    assert_eq!(
        query["inputSchema"]["properties"]["group-by"]["type"],
        "array"
    );
    for forbidden in [
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
    assert_eq!(
        entries["inputSchema"]["required"],
        json!(["project", "form", "bbox"])
    );
    assert_eq!(
        entries["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["bbox", "form", "lane", "limit", "project"])
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
        json!(["project", "form", "updated-after"])
    );
    assert_eq!(
        changes["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "cursor",
            "form",
            "lane",
            "limit",
            "project",
            "updated-after"
        ])
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
            "project",
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
            "project",
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
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ds_workstation", "arguments": { "operation": "invoke", "command": "workstation.plan", "arguments": { "component": "libreoffice", "platform": "windows" } } } }),
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
            "libreoffice",
            "--platform",
            "windows",
            "--output",
            "json",
        ])
    );
}

#[test]
fn the_assets_chapter_is_routed_and_describes_the_live_command() {
    // A new chapter is only worth its root-help line if an agent can actually
    // reach it: the router must be advertised, the catalogue must answer for
    // it, and `describe` must return the live descriptor rather than a second
    // schema written by hand.
    let (responses, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "chapter": "assets" } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_assets", "arguments": { "operation": "describe", "command": "assets.list" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "ds_assets", "arguments": { "operation": "describe", "command": "assets.backup.plan" } } }),
        ],
    );

    let router = ds_cli_mcp::surface::chapter_tool_name(Chapter::Assets);
    assert_eq!(router, "ds_assets");
    let tools = response(&responses, 1)["result"]["tools"]
        .as_array()
        .expect("tools");
    assert!(
        tools.iter().any(|tool| tool["name"] == router),
        "the assets chapter publishes no router; `ds assets` would be unreachable over MCP"
    );

    let catalogue = &response(&responses, 2)["result"]["structuredContent"];
    assert_eq!(catalogue["chapter"], "assets");
    assert_eq!(catalogue["tool"], router);
    let published: BTreeSet<&str> = catalogue["commands"]
        .as_array()
        .expect("chapter commands")
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = [
        "assets.list",
        "assets.tree",
        "assets.versions",
        "assets.read",
        "assets.preview",
        "assets.classify",
        "assets.promote",
        "assets.attach",
        "assets.ingest",
        "assets.folder",
        "assets.reference",
        "assets.resolve",
        "assets.maps",
        "assets.map.publish",
        "assets.backup.plan",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        published, expected,
        "the assets chapter must project exactly the registered assets commands"
    );

    // The live descriptor is unchanged apart from MCP's device-link advice.
    assert_mcp_descriptor_with_device_link(
        &response(&responses, 3)["result"]["structuredContent"],
        cli(&["capabilities", "assets.list", "--output", "json"]),
    );
    assert_mcp_descriptor_with_device_link(
        &response(&responses, 4)["result"]["structuredContent"],
        cli(&["capabilities", "assets.backup.plan", "--output", "json"]),
    );
}

#[test]
fn map_design_open_is_projected_by_catalog_chapter_and_typed_profile() {
    let (responses, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "command": "map.design.open" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_design", "arguments": { "operation": "describe", "command": "map.design.open" } } }),
        ],
    );
    assert_eq!(
        response(&responses, 1)["result"]["structuredContent"]["next"]["tool"],
        "ds_design"
    );
    assert_eq!(
        response(&responses, 2)["result"]["structuredContent"],
        cli(&["capabilities", "map.design.open", "--output", "json"])
    );

    let (profile, _) = mcp(
        &["--exposure", "commands", "--profile", "design-edit"],
        &[json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })],
    );
    let tool = response(&profile, 3)["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|tool| tool["name"] == "map_design_open")
        .expect("map.design.open typed leaf");
    assert_eq!(tool["title"], "map.design.open");
    assert_eq!(tool["inputSchema"]["required"], json!(["transformer"]));
    assert_eq!(
        tool["inputSchema"]["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["desktop-descriptor", "transformer"])
    );

    // The typed leaf reaches this command without VS Code or another UI host
    // mediating it. An absent explicit descriptor is stopped by MCP's bounded
    // pairing gate before command dispatch, with the canonical command still
    // named in the DS envelope.
    let temp = TestDir::new("design-open-descriptor");
    let missing = temp.0.join("no-desktop.json");
    let missing = missing.to_string_lossy().into_owned();
    let (invoked, _) = mcp(
        &["--exposure", "commands", "--profile", "design-edit"],
        &[json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "map_design_open",
                "arguments": {
                    "transformer": "agasharu",
                    "desktop-descriptor": missing,
                }
            }
        })],
    );
    let envelope = &response(&invoked, 4)["result"]["structuredContent"];
    assert_eq!(envelope["command"], "map.design.open");
    assert_eq!(envelope["error"]["code"], "desktop_not_paired");
}

#[test]
fn exact_admin_boundaries_are_projected_by_catalog_chapter_and_typed_profile() {
    let (responses, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "command": "data.admin-bounds.read" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_data", "arguments": { "operation": "describe", "command": "data.admin-bounds.list" } } }),
        ],
    );
    assert_eq!(
        response(&responses, 1)["result"]["structuredContent"]["next"]["tool"],
        "ds_data"
    );
    assert_mcp_descriptor_with_device_link(
        &response(&responses, 2)["result"]["structuredContent"],
        cli(&["capabilities", "data.admin-bounds.list", "--output", "json"]),
    );

    let (profile, _) = mcp(
        &["--exposure", "commands", "--profile", "admin-bounds"],
        &[json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })],
    );
    let tools = response(&profile, 3)["result"]["tools"]
        .as_array()
        .expect("tools");
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "ds_catalog",
            "ds_diagnostics",
            "data_admin-bounds_attach",
            "data_admin-bounds_list",
            "data_admin-bounds_read",
        ])
    );
    let read = tools
        .iter()
        .find(|tool| tool["name"] == "data_admin-bounds_read")
        .expect("exact boundary read leaf");
    assert_eq!(read["title"], "data.admin-bounds.read");
    assert_eq!(read["inputSchema"]["required"], json!(["code"]));
    for command in ["data.admin-bounds.list", "data.admin-bounds.read"] {
        let descriptor = cli(&["capabilities", command, "--output", "json"]);
        let codes = descriptor["data"]["command"]["refusals"]
            .as_array()
            .expect("admin boundary refusals")
            .iter()
            .map(|refusal| refusal["code"].as_str().expect("refusal code"))
            .collect::<BTreeSet<_>>();
        // The scope refusal is this domain's own and is raised before anything
        // is sent; the rest are the national authority's, composed from the
        // native user path so a new one reaches these reads too.
        for code in [
            "invalid_admin_scope",
            "headless_signed_out",
            "auth_transient",
            "auth_response_unreadable",
        ] {
            assert!(
                codes.contains(code),
                "{command} no longer declares stable refusal `{code}`: {codes:?}"
            );
        }
    }
    // The static annotation covers every invocation shape. `--geometry-out`
    // writes a local file, so the leaf must stay conservatively false even
    // when this particular call omits that path.
    assert_eq!(read["annotations"]["readOnlyHint"], false);
}

#[test]
fn admin_scope_choices_remain_closed_in_capabilities_and_mcp() {
    let levels = json!(["province", "district", "sector", "cell", "village"]);
    let list_descriptor = cli(&["capabilities", "data.admin-bounds.list", "--output", "json"]);
    let read_descriptor = cli(&["capabilities", "data.admin-bounds.read", "--output", "json"]);
    assert_eq!(
        capability_input(&list_descriptor, "country")["choices"],
        json!(["rwanda"])
    );
    assert_eq!(
        capability_input(&list_descriptor, "country")["default"],
        "rwanda"
    );
    assert_eq!(
        capability_input(&list_descriptor, "level")["choices"],
        levels
    );
    assert_eq!(
        capability_input(&read_descriptor, "country")["choices"],
        json!(["rwanda"])
    );
    assert_eq!(
        capability_input(&read_descriptor, "country")["default"],
        "rwanda"
    );

    let (profile, _) = mcp(
        &["--exposure", "commands", "--profile", "admin-bounds"],
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    let tools = response(&profile, 1)["result"]["tools"]
        .as_array()
        .expect("admin tools");
    let list = tools
        .iter()
        .find(|tool| tool["name"] == "data_admin-bounds_list")
        .expect("admin list leaf");
    let read = tools
        .iter()
        .find(|tool| tool["name"] == "data_admin-bounds_read")
        .expect("admin read leaf");
    assert_eq!(
        list["inputSchema"]["properties"]["country"]["enum"],
        json!(["rwanda"])
    );
    assert_eq!(
        list["inputSchema"]["properties"]["country"]["default"],
        "rwanda"
    );
    assert_eq!(list["inputSchema"]["properties"]["level"]["enum"], levels);
    assert_eq!(
        read["inputSchema"]["properties"]["country"]["enum"],
        json!(["rwanda"])
    );
    assert_eq!(
        read["inputSchema"]["properties"]["country"]["default"],
        "rwanda"
    );
}

#[test]
fn design_edit_profile_exposes_known_columns_as_the_external_field_authority() {
    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "design-edit"],
        &[json!({ "jsonrpc": "2.0", "id": 19, "method": "tools/list" })],
    );
    let tools = response(&responses, 19)["result"]["tools"]
        .as_array()
        .expect("tools");
    let names = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(names.contains("design_known-columns_list"));
    assert!(names.contains("design_known-columns_set"));
}

#[test]
fn map_design_version_history_projects_through_catalog_chapter_and_typed_profile() {
    let ids = ["map.design.version.play", "map.design.version.compare"];
    let mut calls = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        calls.push(json!({ "jsonrpc":"2.0", "id":index * 2 + 1, "method":"tools/call", "params":{ "name":"ds_catalog", "arguments":{ "command":id } } }));
        calls.push(json!({ "jsonrpc":"2.0", "id":index * 2 + 2, "method":"tools/call", "params":{ "name":"ds_design", "arguments":{ "operation":"describe", "command":id } } }));
    }
    let (responses, _) = mcp(&["--exposure", "chapters"], &calls);
    for (index, id) in ids.iter().enumerate() {
        let catalog_id = (index * 2 + 1) as i64;
        let chapter_id = (index * 2 + 2) as i64;
        assert_eq!(
            response(&responses, catalog_id)["result"]["structuredContent"]["next"]["tool"],
            "ds_design",
            "{id}",
        );
        assert_eq!(
            response(&responses, chapter_id)["result"]["structuredContent"],
            cli(&["capabilities", id, "--output", "json"]),
            "{id}",
        );
    }

    let (profile, _) = mcp(
        &["--exposure", "commands", "--profile", "design-edit"],
        &[json!({ "jsonrpc":"2.0", "id":20, "method":"tools/list" })],
    );
    let tools = response(&profile, 20)["result"]["tools"]
        .as_array()
        .expect("tools");
    for (name, required) in [
        ("map_design_version_play", json!(["transformer", "version"])),
        (
            "map_design_version_compare",
            json!(["transformer", "from", "to"]),
        ),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing typed leaf {name}"));
        assert_eq!(tool["inputSchema"]["required"], required, "{name}");
    }
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
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "ds_vector_tiles", "arguments": { "operation": "invoke", "command": "tile.generate", "arguments": { "type": "survey", "project": "test-project" } } } }),
        ],
    );
    assert_eq!(response(&responses, 1)["error"]["code"], -32602);
    assert!(
        response(&responses, 1)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("ds_vector_tiles")
    );
    // Once the router has resolved a command, a misplaced confirmation is
    // that command's refusal: an `isError` result with a DS code and remedy,
    // not a protocol error. Routing mistakes (ids 1 and 4) stay protocol
    // errors, because no command was resolved.
    for (id, expected) in [(2, "does not accept confirmation"), (3, "chapter envelope")] {
        let result = &response(&responses, id)["result"];
        assert_eq!(result["isError"], true);
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], "mcp_arguments_invalid", "{error}");
        assert!(
            error["message"].as_str().unwrap().contains(expected),
            "{error}"
        );
        assert!(
            error["remedy"]
                .as_str()
                .is_some_and(|remedy| !remedy.is_empty())
        );
    }
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
        "auth-context",
        "printing",
        "grid",
        "grid-native",
        "grid-corrections",
        "grid-local-model",
        "clearance",
        "pls",
        "pls-desktop",
        "pls-library",
        "library-governance",
        "survey",
        "form-factory",
        "survey-projects",
        "survey-media",
        "survey-migration",
        "design-edit",
        "design-run",
        "design-migration",
        "map",
        "styles",
        "print-styles",
        "layers",
        "tiling",
        "project",
        "correspondence",
        "solar-input",
        "solar-migration",
        "solar-application",
        "solar-run",
        "solar-delivery",
        "solar-portfolio-batch",
        "solar-dashboard",
        "operations",
        "project-operations",
    ] {
        let (responses, _) = mcp(
            &["--exposure", "commands", "--profile", profile],
            &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
        );
        let tools = response(&responses, 1)["result"]["tools"]
            .as_array()
            .expect("tools");
        let maximum = match profile {
            // The broad Grid profile carries the existing file/report tools
            // plus the existing local-lifecycle/publication leaves. Agents that
            // need only that workflow use `grid-local-model` below.
            // Project cache preparation stays in the narrower
            // grid-local-model profile so this broad router remains bounded.
            // Native creation completes the file workflow: one new bounded
            // leaf. Focused clients can use grid-native or grid-local-model.
            // Native project list/download complete the same model workflow.
            // 2026-09-20 (contract 02): `dsgrid model show|link` and
            // `dsgrid-exchange sync` — the working copy's record, its pin to a
            // live PLS-CADD workspace, and the write back into it. A router
            // that imports from PLS-CADD and cannot deliver to it is half a
            // workflow.
            // 2026-09-21: native structure import and atomic batch editing
            // add two file-authoring leaves; see the profile's matching limit.
            // 2026-09-25: `dsgrid replace-structure` beside the import.
            "grid" => 29,
            // Seventeen working-copy leaves plus bootstrap: the four
            // 2026-09-21 leaves (`dsgrid model forget`, `dsgrid structure
            // admin-refresh`, `dsgrid profile labels set|show`) were routed
            // here on 2026-09-22 from the broad `grid` router they had pushed
            // past its budget. The governed head's versions, retire and
            // restore joined on 2026-09-24 for the same reason (e62edf85).
            // Include project model enumeration beside per-model versions;
            // otherwise this profile cannot discover which IDs to inspect.
            // The six package-asset leaves (a version's original PLS-CADD
            // upload and its delivered backup) joined on 2026-09-25.
            // `dsgrid alignment gap show|set` joined on 2026-09-25.
            // 2026-09-25, versions and submissions (+21): `dsgrid project
            // show|compare|exports list|publish|download|bump-version|update|
            // set-approval|backup download`, `dsgrid model unlink`, the seven
            // MV governance `design version` leaves and
            // the four `design attachment` leaves an MV revision carries — the
            // owner's order is that versioning and its attachments have no
            // missing verb, and they are one workflow with publication.
            // Four more attachment actions complete that workflow.
            "grid-local-model" => 55,
            // The file-in/file-out engine workflow; `dsgrid replace-structure`
            // joined `import-structure` here on 2026-09-25.
            "grid-native" => 17,
            // Shared/manual form resolve and save belong to city input work.
            // Editable city creation completes the no-GIS entry point.
            "solar-input" => 18,
            // Governed reads, project-form settings, templates,
            // create-from-template and the three working-area form leaves
            // (read, choose, forget which forms the map loads); the photo
            // leaves moved to survey-media.
            "survey-projects" => 21,
            // Held survey photos (list, read), the one rotation and its
            // publication, and the offline file rotation, plus bootstrap.
            "survey-media" => 10,
            "design-edit" => 23,
            // Twenty-six printing leaves plus bootstrap: city-vector input,
            // local rendering and standalone map delivery complete the headless
            // workflow beside the retained desktop-owned operations.
            "printing" => 28,
            // Sixteen layer leaves plus bootstrap: the layer drawer's profile
            // also carries this machine's prepared local layer catalogue,
            // which is the same "one host's own layers" workflow as the local
            // tile references beside it.
            "layers" => 19,
            // Sixteen operations leaves plus bootstrap. The one that raised
            // this from seventeen on 2026-09-18 is `ds feedback note`: an
            // operations agent that can read the backlog and close a report but
            // cannot say why a report it touched stays open leaves the next
            // reader nothing but the full text to rescan.
            // The one before that is `ds desktop list`: every instance-targeted
            // refusal an agent can meet tells it to name an instance, and this
            // is the only tool that says which instances exist.
            "operations" => 18,
            // Sixteen leaves plus both bootstrap tools: `report project
            // publish` and the two `report outbox` commands joined the
            // delivery workflow, because a profile that produces report
            // artifacts and cannot publish them strands its own output; and
            // `report project compute` (2026-09-20) is the same individual
            // report produced in the cloud, which is how edge and cloud
            // production are proven to meet in one project.
            "project-operations" => 18,
            // Seventeen leaves plus bootstrap: the three task-geometry leaves
            // (2026-09-20) let an agent that read a comment naming structures
            // say WHERE the task is — the proposal a person confirms and the
            // read the map paints from.
            "project" => 22,
            _ => 16,
        };
        assert!(
            (2..=maximum).contains(&tools.len()),
            "{profile}: {}",
            tools.len()
        );
        // No typed profile's tool descriptions send a person to a terminal
        // sign-in; the broad surface is held to the same words in
        // `no_published_mcp_text_names_a_terminal_sign_in`.
        assert_no_terminal_sign_in(&format!("profile `{profile}`"), &responses[0].to_string());
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
    assert!(published["printing"].contains("map_print_schema"));
    assert!(published["printing"].contains("desktop_printing_map_export"));
    assert!(published["printing"].contains("report_layout_edit"));
    assert!(published["printing"].contains("report_transformers"));
    assert!(published["printing"].contains("report_plan"));
    assert!(published["printing"].contains("assets_map_publish"));
    assert!(published["printing"].contains("report_artifact_remove"));
    // The headless production loop is reachable through the printing profile.
    for headless in [
        "data_project-cache_status",
        "data_project-cache_seed",
        "report_project_settings",
        "report_project_outputs_set",
        "report_project_export",
    ] {
        assert!(
            published["printing"].contains(headless),
            "the printing profile must project the headless loop: {headless}"
        );
    }
    assert!(!published["grid"].contains("report_transformers"));
    assert!(!published["grid"].contains("report_plan"));
    assert!(!published["grid"].contains("dsgrid_backup_preview"));
    let (backup, _) = mcp(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_catalog", "arguments": { "command": "dsgrid.backup.preview" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_grid_model", "arguments": { "operation": "describe", "command": "dsgrid.backup.preview" } } }),
        ],
    );
    assert_eq!(
        response(&backup, 1)["result"]["structuredContent"]["next"]["tool"],
        "ds_grid_model"
    );
    let global = cli(&["capabilities", "dsgrid.backup.preview", "--output", "json"]);
    assert_eq!(global["data"]["command"]["availability"], "available");
    assert_eq!(response(&backup, 2)["result"]["structuredContent"], global);
    for native in [
        "dsgrid_create",
        "dsgrid_inspect",
        "dsgrid_validate",
        "dsgrid_describe",
        "dsgrid_run",
        "dsgrid_apply",
        "dsgrid-exchange_inspect",
        "dsgrid-exchange_plan",
        "dsgrid-exchange_convert",
    ] {
        assert!(
            published["grid-native"].contains(native),
            "native grid workflow is missing {native}"
        );
    }
    assert!(!published["grid-native"].contains("dsgrid_model_create-local"));
    assert!(!published["grid-native"].contains("dsgrid_publish-version"));
    assert!(!published["grid-native"].contains("dsgrid_apply-batch"));
    assert!(!published["grid-native"].contains("dsgrid-exchange_sync"));
    assert!(published["grid-corrections"].contains("dsgrid_apply-correction"));
    assert!(!published["grid-corrections"].contains("dsgrid_apply-batch"));
    assert!(
        published["grid-local-model"].contains("dsgrid_model_list")
            && published["grid-local-model"].contains("dsgrid_model_show")
            && published["grid-local-model"].contains("dsgrid_model_link")
            && published["grid-local-model"].contains("dsgrid_model_create-local")
            && published["grid-local-model"].contains("dsgrid_model_import-external")
            && published["grid-local-model"].contains("dsgrid_model_set-active")
            && published["grid-local-model"].contains("dsgrid_model_prepare-project")
            && published["grid-local-model"].contains("dsgrid_project_list")
            && published["grid-local-model"].contains("dsgrid_project_versions")
            && published["grid-local-model"].contains("dsgrid_project_geojson")
            && published["grid-local-model"].contains("dsgrid_publish-version")
            && published["grid-local-model"].contains("dsgrid_asset_extract")
            && published["grid-local-model"].contains("dsgrid_asset_attach")
            && published["grid-local-model"].contains("dsgrid_project_asset_extract"),
        "the grid-local-model profile must project the complete model and project-cache lifecycle"
    );
    // Package assets live with the version lifecycle alone: neither the broad
    // `grid` router nor the file-in/file-out `grid-native` one carries them.
    for leaf in ["dsgrid_asset_list", "dsgrid_project_asset_list"] {
        assert!(published["grid-local-model"].contains(leaf));
        assert!(!published["grid"].contains(leaf), "{leaf} widened `grid`");
        assert!(
            !published["grid-native"].contains(leaf),
            "{leaf} widened `grid-native`"
        );
    }
    // The correspondence workflow is its own profile (2026-09-20): the
    // letters an agent files and the tasks it schedules are two jobs, and
    // one bounded project profile could not carry both.
    assert!(
        published["correspondence"].contains("pm_record_create")
            && published["correspondence"].contains("pm_record_reply")
            && published["correspondence"].contains("pm_party_create")
            && published["correspondence"].contains("pm_task_block")
            && published["correspondence"].contains("pm_plan"),
        "the correspondence profile carries file, reply, party, block and the plan"
    );
    assert!(
        published["project"].contains("pm_task_create") && published["project"].contains("pm_plan"),
        "the project profile keeps the task workflow and the plan"
    );
    assert!(
        !published["project"].contains("pm_record_create"),
        "filing a letter is the correspondence profile's job"
    );
    assert!(
        published["clearance"].contains("dsgrid_feature-codes_report")
            && published["clearance"].contains("dsgrid_feature-codes_import")
            && published["clearance"].contains("dsgrid_feature-codes_migrate")
            && published["clearance"].contains("dsgrid_feature-codes_export")
            && published["clearance"].contains("dsgrid_criteria_show")
            && published["clearance"].contains("dsgrid_criteria_clearance_set")
            && published["clearance"].contains("dsgrid_analyse_clearance"),
        "the grid-clearance profile must project the whole feature-code and clearance workflow"
    );
    assert!(!published["grid-native"].contains("dsgrid_analyse_clearance"));
    assert!(
        published["pls"].contains("pls_backup-create"),
        "the PLS profile must expose the live backup command without a second MCP schema"
    );
    assert!(
        published["pls-desktop"].contains("pls_desktop_deliver")
            && published["pls-desktop"].contains("pls_desktop_dialogs")
            && !published["pls"].contains("pls_desktop_deliver"),
        "the PLS-CADD desktop verbs are their own profile, projected from the live registry"
    );
    assert!(
        published["design-edit"].contains("map_design_open"),
        "the design-edit profile must project the canonical visible context-entry command"
    );
    assert!(
        published["design-edit"].contains("map_design_pin"),
        "the design-edit profile must project the map-owned Working-set command"
    );
    assert!(
        published["design-edit"].contains("design_known-columns_set"),
        "the design-edit profile must expose the know_columns mutation"
    );
    assert!(
        published["styles"].contains("style_label_plan")
            && published["styles"].contains("style_label_set"),
        "the styles profile must expose reviewed label planning and publication"
    );
    assert!(
        published["print-styles"].contains("style_seed_plan")
            && published["print-styles"].contains("style_seed_create")
            && published["print-styles"].contains("style_print_plan")
            && published["print-styles"].contains("style_print_create"),
        "the print-styles profile must expose create-only source and print workflows"
    );
    assert!(
        published["map"].is_disjoint(&published["styles"]),
        "map navigation and style authoring must remain separate profiles"
    );
    assert!(
        published["map"].is_disjoint(&published["print-styles"])
            && published["styles"].is_disjoint(&published["print-styles"]),
        "print style creation must stay separate from map navigation and ordinary style edits"
    );
    assert!(
        published["project-operations"].contains("design_transformer_inventory")
            && !published["project-operations"].contains("design_transformer_download"),
        "the project-operations profile exposes the headless transformer lifecycle; \
         window-cache warming was retired on 2026-09-20"
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
            &[
                "solar-input",
                // Migration is its own operator workflow inside the Solar
                // domain, exactly as `design-migration` is inside Design.
                "solar-migration",
                "solar-application",
                "solar-run",
                "solar-delivery",
                "solar-portfolio-batch",
                "solar-dashboard",
            ][..],
        ),
        (
            "pls_",
            &["pls", "pls-desktop", "pls-library", "library-governance"][..],
        ),
    ] {
        let expected = all
            .iter()
            .filter(|name| {
                name.starts_with(prefix)
                    || (prefix == "map_design_"
                        && matches!(
                            name.as_str(),
                            "design_features_select"
                                | "design_intake_upload"
                                | "design_known-columns_list"
                                | "design_known-columns_set"
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
            let overlap = union
                .intersection(current)
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let permitted = if prefix == "solar_" {
                BTreeSet::from(["solar_engine", "solar_results_read", "solar_project_result"])
            } else {
                BTreeSet::new()
            };
            assert!(
                overlap.is_subset(&permitted),
                "{profile} has an undeclared sibling overlap: {overlap:?}"
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

#[test]
fn auth_context_profile_hands_off_only_non_secret_native_identity_commands() {
    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "auth-context"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "auth_status", "arguments": {} } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "auth_login", "arguments": { "email": "operator@example.com" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "auth_link_approve", "arguments": {} } }),
        ],
    );
    let tools = response(&responses, 1)["result"]["tools"]
        .as_array()
        .expect("tools");
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "ds_catalog",
            "ds_diagnostics",
            "account_connect",
            "auth_device_list",
            "auth_device_read",
            "auth_device_revoke",
            "auth_link_begin",
            "auth_link_complete",
            "auth_link_status",
            "auth_project_list",
            "auth_status",
        ]
    );
    assert_eq!(
        response(&responses, 2)["result"]["structuredContent"],
        cli_envelope(&["auth", "status", "--output", "json"]),
        "the MCP principal projection must be the exact CLI envelope"
    );
    assert_eq!(response(&responses, 3)["error"]["code"], -32602);
    assert_eq!(response(&responses, 4)["error"]["code"], -32602);
    assert!(names.iter().all(|name| !name.contains("login")
        && !name.contains("logout")
        && name != &"auth_link_approve"));
    for tool in tools {
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .expect("tool properties");
        for secret in [
            "device_code",
            "code_verifier",
            "private_key",
            "access_token",
            "signature",
            "password",
        ] {
            assert!(
                !properties.contains_key(secret),
                "MCP input exposed {secret}"
            );
        }
    }

    let (broad, _) = mcp(
        &["--exposure", "commands"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "auth_login", "arguments": { "email": "operator@example.com" } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "auth_link_approve", "arguments": { "request": "req_01", "device-fingerprint": format!("sha256:{}", "a".repeat(64)), "lane": "stable", "confirm": true } } }),
        ],
    );
    let broad_names = response(&broad, 1)["result"]["tools"]
        .as_array()
        .expect("broad tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert!(!broad_names.contains(&"auth_login"));
    assert!(!broad_names.contains(&"auth_link_approve"));
    assert!(!broad_names.contains(&"server_serve"));
    assert!(broad_names.contains(&"server_submit"));
    assert!(broad_names.contains(&"server_status"));
    assert_eq!(response(&broad, 2)["error"]["code"], -32602);
    assert_eq!(response(&broad, 3)["error"]["code"], -32602);
}

/// The enumeration answers through MCP, and the gate never stands in front of
/// it.
///
/// Every instance-targeted refusal an agent can meet — `desktop_ambiguous`,
/// `desktop_target_not_live`, `desktop_project_not_open` — tells it to name an
/// instance, and this is the only tool that says which instances exist. So it
/// declares no desktop authority and is reachable on a machine with nothing
/// running, exactly like `ds desktop status`: a gate in front of it would make
/// the one call that explains the situation the one call that refuses to.
///
/// The app-data root is redirected to an empty directory, so what this proves
/// is the projection and the gate — never a probe of the operator's own
/// running DS GridDesign.
#[test]
fn the_instance_enumeration_is_projected_and_never_gated_on_what_it_reports() {
    let machine = TestDir::new("no-instances");
    let (responses, _) = mcp_with_env(
        &["--exposure", "chapters"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "ds_operations", "arguments": { "operation": "describe", "command": "desktop.list" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "ds_operations", "arguments": { "operation": "invoke", "command": "desktop.list", "arguments": {} } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "ds_project", "arguments": { "operation": "describe", "command": "desktop.status" } } }),
        ],
        &[("XDG_DATA_HOME", &machine.0)],
    );

    let described = &response(&responses, 1)["result"]["structuredContent"];
    assert_eq!(described["data"]["command"]["id"], "desktop.list");
    assert_eq!(
        described["data"]["command"]["authority"], "none",
        "the enumeration must not require the authority it exists to report on"
    );

    // Nothing is running, and that is an answer rather than a refusal — so an
    // agent can tell "no instance" from "several" without a desktop at all.
    let listed = &response(&responses, 2)["result"]["structuredContent"];
    assert_eq!(listed["status"], "ok", "{listed}");
    assert_eq!(listed["data"]["live"], 0);
    assert_eq!(listed["data"]["instances"], json!([]));
    assert_eq!(listed["data"]["compatible"], Value::Null);

    // And the instance a tool call is for is a declared input, so an agent
    // that met an ambiguity can answer it in the same vocabulary the terminal
    // uses: `--target desktop:<instance_id>`.
    let status = &response(&responses, 3)["result"]["structuredContent"];
    let target = status["data"]["command"]["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "target")
        .expect("`ds desktop status` publishes the host it runs against");
    assert_eq!(target["value"], "<desktop|desktop:instance|server>");
    assert!(
        target["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("DS_TARGET")),
        "the input must name its session default: {target}"
    );
}

/// One host flag, and no second spelling of the same question.
///
/// `--target` means "which host runs this" wherever it carries the host
/// placeholder, and the commands that use the word for something else are a
/// closed, named set. A new command that spelled a *host* choice differently —
/// `--host`, `--instance`, `--desktop-instance` — would split one contract in
/// two, and an agent that learned one would be wrong about the other.
#[test]
fn the_host_is_one_flag_and_the_other_targets_are_a_closed_set() {
    /// `--target` in these commands names something in the domain, not a host:
    /// a panel to open, an export format, an editor to configure. They predate
    /// the host flag and are listed rather than renamed, because a published
    /// command contract is not renamed for tidiness.
    const NOT_A_HOST: &[&str] = &[
        "map.ui.open",
        "design.config.rule-set.duplicate",
        "dsgrid-exchange.plan",
        "dsgrid-exchange.convert",
        "workstation.plan",
        "workstation.configure",
    ];
    const HOST_PLACEHOLDER: &str = "<desktop|desktop:instance|server>";
    /// `ds mcp install --host` names an MCP host *program* — Claude Code,
    /// Codex — which is a different thing from the machine that runs a
    /// command. It is a published contract and is listed rather than renamed;
    /// what this closes is the *next* one.
    ///
    /// The fifteen `ds style` commands were here too, until 2026-09-18: they
    /// chose between a native user and a paired window as `--host
    /// native|desktop` before the host became one flag. Collapsing them onto
    /// the one route a style document has took the question away rather than
    /// renaming it, which is why the list is one entry again.
    const OLDER_HOST_SPELLING: &[&str] = &["mcp.install"];

    let index = cli(&["capabilities", "--output", "json"]);
    let mut hosts = Vec::new();
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        for command in cli(&["capabilities", id, "--output", "json"])["data"]["commands"]
            .as_array()
            .expect("commands")
        {
            let command = cli(&[
                "capabilities",
                command["id"].as_str().expect("command id"),
                "--output",
                "json",
            ]);
            let command = &command["data"]["command"];
            let id = command["id"].as_str().expect("command id").to_owned();
            for input in command["inputs"].as_array().expect("inputs") {
                if input["name"] != "target" {
                    // No second flag may ask this question under another name.
                    // One older spelling exists and is named below; a third
                    // would mean an agent that learned one is wrong about the
                    // next.
                    assert!(
                        input["value"] != HOST_PLACEHOLDER,
                        "`{id}` declares `--{}` with the host grammar. The host \
                         is `--target`.",
                        input["name"]
                    );
                    assert!(
                        input["name"] != "host" || OLDER_HOST_SPELLING.contains(&id.as_str()),
                        "`{id}` chooses an execution host as `--host`. The host \
                         is `--target <desktop|desktop:instance|server>`; only \
                         the commands that shipped the older spelling keep it."
                    );
                    continue;
                }
                if NOT_A_HOST.contains(&id.as_str()) {
                    assert_ne!(
                        input["value"], HOST_PLACEHOLDER,
                        "`{id}` is listed as using `--target` for something \
                         other than a host, and it now names hosts. Remove it \
                         from the list."
                    );
                    continue;
                }
                assert_eq!(
                    input["value"], HOST_PLACEHOLDER,
                    "`{id}` declares `--target` with another grammar. One flag, \
                     one meaning: either it names a host, or it belongs in the \
                     closed list of commands that use the word for something else."
                );
                hosts.push(id.clone());
            }
        }
    }
    assert!(
        hosts.iter().any(|id| id == "desktop.status"),
        "the host flag is published by the commands that route on it: {hosts:?}"
    );
}

/// The live registry, walked tier by tier: every command id with its domain.
fn live_commands() -> BTreeMap<String, String> {
    let mut live = BTreeMap::new();
    for domain in cli(&["capabilities", "--output", "json"])["data"]["domains"]
        .as_array()
        .expect("domains")
    {
        let domain = domain["id"].as_str().expect("domain id");
        for command in cli(&["capabilities", domain, "--output", "json"])["data"]["commands"]
            .as_array()
            .expect("commands")
        {
            live.insert(
                command["id"].as_str().expect("id").to_string(),
                domain.to_string(),
            );
        }
    }
    assert!(!live.is_empty(), "the live registry must not be empty");
    live
}

fn tools_by_title(responses: &[Value], id: i64) -> BTreeMap<String, Value> {
    let mut tools = BTreeMap::new();
    for tool in response(responses, id)["result"]["tools"]
        .as_array()
        .expect("tools")
    {
        let title = tool["title"].as_str().expect("title").to_string();
        assert!(
            tools.insert(title.clone(), tool.clone()).is_none(),
            "`{title}` is published twice"
        );
    }
    tools
}

/// Every registered command is exactly one MCP tool, or one of the named
/// exclusions — with no hand edit anywhere when a command is added. A verb
/// another branch registers appears here by construction; a verb whose
/// descriptor cannot be projected faithfully stops `ds mcp serve` from
/// starting, and this test with it.
#[test]
fn every_registered_command_is_exactly_one_mcp_tool_or_a_named_exclusion() {
    let live = live_commands();
    let never: BTreeSet<&str> = ds_cli_mcp::tools::NEVER_TOOLS
        .iter()
        .map(|(id, reason)| {
            assert!(!reason.is_empty(), "`{id}` is excluded without a reason");
            assert!(
                live.contains_key(*id),
                "`{id}` is excluded from MCP but is no longer a live command"
            );
            *id
        })
        .collect();
    let expected: BTreeSet<String> = live
        .iter()
        .filter(|(id, domain)| domain.as_str() != "mcp" && !never.contains(id.as_str()))
        .map(|(id, _)| id.clone())
        .collect();

    // Typed exposure: one leaf per command, named by the stable rule.
    let (responses, _) = mcp(
        &["--exposure", "commands"],
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    let tools = tools_by_title(&responses, 1);
    let leaves: BTreeSet<String> = tools
        .keys()
        .filter(|title| title.as_str() != "DS diagnostics")
        .cloned()
        .collect();
    let missing: Vec<&String> = expected.difference(&leaves).collect();
    assert!(
        missing.is_empty(),
        "registered commands with no MCP tool: {missing:?}"
    );
    let extra: Vec<&String> = leaves.difference(&expected).collect();
    assert!(
        extra.is_empty(),
        "MCP tools with no registered command: {extra:?}"
    );
    let mut names = BTreeSet::new();
    for (id, tool) in &tools {
        let name = tool["name"].as_str().expect("name");
        assert!(
            names.insert(name.to_string()),
            "two tools are named `{name}`"
        );
        assert!(
            !name.is_empty()
                && name.len() <= 64
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "`{name}` breaks the tool-name grammar hosts accept"
        );
        if id != "DS diagnostics" {
            assert_eq!(name, id.replace('.', "_"), "the stable name rule moved");
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        }
    }

    // Chapter exposure: every command is reachable through exactly one
    // router, and the catalogue lists it there.
    let requests: Vec<Value> = Chapter::ALL
        .iter()
        .filter(|chapter| **chapter != Chapter::Catalog)
        .enumerate()
        .map(|(index, chapter)| {
            json!({ "jsonrpc": "2.0", "id": index as i64 + 1, "method": "tools/call",
                    "params": { "name": "ds_catalog", "arguments": { "chapter": chapter.token() } } })
        })
        .collect();
    let (responses, _) = mcp(&["--exposure", "chapters"], &requests);
    let mut routed: BTreeMap<String, String> = BTreeMap::new();
    for request in &requests {
        let listing =
            &response(&responses, request["id"].as_i64().unwrap())["result"]["structuredContent"];
        let router = listing["tool"].as_str().expect("router").to_string();
        for command in listing["commands"].as_array().expect("commands") {
            let id = command["id"].as_str().expect("id").to_string();
            if let Some(other) = routed.insert(id.clone(), router.clone()) {
                panic!("`{id}` is routed by both `{other}` and `{router}`");
            }
        }
    }
    let routed_ids: BTreeSet<String> = routed.keys().cloned().collect();
    assert_eq!(
        routed_ids, expected,
        "the chapter routers must reach exactly the registered commands"
    );
}

/// One command's descriptor as the server itself reads it at startup: the
/// schema, without resolving live availability.
fn schema(id: &str) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["capabilities", id, "--output", "json"])
        .env("DS_CLI_SCHEMA_ONLY", "1")
        .output()
        .expect("ds runs");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("one CLI envelope");
    envelope["data"]["command"].clone()
}

fn leaf<'a>(tools: &'a BTreeMap<String, Value>, id: &str) -> &'a Value {
    tools
        .get(id)
        .unwrap_or_else(|| panic!("`{id}` publishes no tool"))
}

/// Schemas are the live descriptors: types, closed sets on the value (or on
/// each item of a repeated input), required inputs, defaults, the MCP
/// confirmation only where the gate applies, and a command's own `confirm`
/// or `yes` input left as that command's input.
#[test]
fn typed_schemas_are_generated_from_the_live_descriptors() {
    let (responses, _) = mcp(
        &["--exposure", "commands"],
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    let tools = tools_by_title(&responses, 1);
    for (id, tool) in &tools {
        if id == "DS diagnostics" {
            continue;
        }
        let descriptor = schema(id);
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .expect("properties");
        let mut declared = BTreeSet::new();
        for input in descriptor["inputs"].as_array().expect("inputs") {
            let name = input["name"].as_str().unwrap();
            declared.insert(name.to_string());
            let property = &properties[name];
            match input["kind"].as_str().unwrap() {
                "switch" => assert_eq!(property["type"], "boolean", "{id} {name}"),
                "repeated" => {
                    assert_eq!(property["type"], "array", "{id} {name}");
                    assert!(property.get("enum").is_none(), "{id} {name}");
                    if let Some(choices) = input.get("choices") {
                        assert_eq!(&property["items"]["enum"], choices, "{id} {name}");
                    }
                }
                _ => {
                    assert_eq!(property["type"], "string", "{id} {name}");
                    if let Some(choices) = input.get("choices") {
                        assert_eq!(&property["enum"], choices, "{id} {name}");
                    }
                    if let Some(default) = input.get("default") {
                        assert_eq!(&property["default"], default, "{id} {name}");
                    }
                }
            }
            let required = tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!(name));
            assert_eq!(required, input["required"] == true, "{id} {name}");
        }
        // The only property a descriptor does not declare is the MCP
        // confirmation, and it appears exactly where the CLI gate applies.
        let gated = descriptor["confirmation_required"] == true;
        for name in properties.keys() {
            assert!(
                declared.contains(name) || (name == "confirm" && gated),
                "{id} publishes undeclared property `{name}`"
            );
        }
        if gated {
            assert_eq!(properties["confirm"]["type"], "boolean", "{id}");
        }
    }

    let inspect = leaf(&tools, "dsgrid.inspect");
    let include = &inspect["inputSchema"]["properties"]["include"];
    assert_eq!(include["type"], "array");
    assert!(
        include["items"]["enum"]
            .as_array()
            .is_some_and(|choices| !choices.is_empty())
    );

    // `design.force-gate.check` owns a string input named `confirm`.
    let gate = leaf(&tools, "design.force-gate.check");
    assert_eq!(
        gate["inputSchema"]["properties"]["confirm"]["type"],
        "string"
    );

    // The typed dsgrid mutations own a `--yes` switch that writes their
    // revision; they are not behind the CLI's central gate.
    let retype = leaf(&tools, "dsgrid.structure.retype");
    assert_eq!(
        retype["inputSchema"]["properties"]["yes"]["type"],
        "boolean"
    );
    assert!(retype["inputSchema"]["properties"].get("confirm").is_none());

    // A preview-capable global write says how to preview without confirming.
    let task = leaf(&tools, "pm.task.create");
    assert!(
        task["inputSchema"]["properties"]["confirm"]["description"]
            .as_str()
            .unwrap()
            .contains("Required unless `dry-run` is true")
    );
    for job in ["solar.run.start", "server.submit"] {
        assert!(
            leaf(&tools, job)["description"]
                .as_str()
                .unwrap()
                .contains("Runs as a job"),
            "{job}"
        );
    }
}

/// Annotations are the effect class and nothing else, for every tool.
#[test]
fn every_tool_is_annotated_from_its_live_effect() {
    let (responses, _) = mcp(
        &["--exposure", "commands"],
        &[json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })],
    );
    for (id, tool) in tools_by_title(&responses, 1) {
        if id == "DS diagnostics" {
            continue;
        }
        let description = tool["description"].as_str().unwrap();
        let effect = description
            .split("Effect: ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .unwrap_or_else(|| panic!("{id} does not state its effect"));
        let effect = ds_cli_contract::spec::Effect::from_token(effect)
            .unwrap_or_else(|| panic!("{id} states unknown effect `{effect}`"));
        let hints = ds_cli_mcp::tools::hints(effect);
        let annotations = &tool["annotations"];
        assert_eq!(annotations["readOnlyHint"], hints.read_only, "{id}");
        assert_eq!(annotations["destructiveHint"], hints.destructive, "{id}");
        assert_eq!(annotations["idempotentHint"], hints.idempotent, "{id}");
        assert_eq!(annotations["openWorldHint"], false, "{id}");
    }
    // The statement in each description is the descriptor's own effect.
    for (id, token) in [
        ("tile.generate", "global_write"),
        ("shell.status", "discovery"),
        ("map.draw", "local_ui"),
        ("workstation.install", "machine_write"),
    ] {
        assert_eq!(
            cli(&["capabilities", id, "--output", "json"])["data"]["command"]["effect"],
            token,
            "{id}"
        );
    }
}

/// A signed-out, profile-free machine: the protected native state lives in
/// an empty temporary directory, so a call that passes the gate is refused
/// by authority — locally — and never reaches a service.
fn signed_out(label: &str) -> TestDir {
    TestDir::new(label)
}

fn signed_out_mcp(home: &TestDir, args: &[&str], requests: &[Value]) -> Vec<Value> {
    mcp_with_env(
        args,
        requests,
        &[
            ("HOME", &home.0),
            ("DS_CONFIG_HOME", &home.0),
            ("XDG_CONFIG_HOME", &home.0),
        ],
    )
    .0
}

fn signed_out_cli(home: &TestDir, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("HOME", &home.0)
        .env("DS_CONFIG_HOME", &home.0)
        .env("XDG_CONFIG_HOME", &home.0)
        .env("DS_CLI_NONINTERACTIVE", "1")
        .output()
        .expect("ds runs");
    serde_json::from_slice(&output.stdout).expect("one CLI envelope")
}

fn structured(responses: &[Value], id: i64) -> &Value {
    &response(responses, id)["result"]["structuredContent"]
}

/// The global-write gate through typed tools: nothing confirms but
/// `confirm: true`, a caller value spelled like the flag confirms nothing, a
/// declared preview needs no confirmation and refuses one, and a CLI refusal
/// arrives as a tool error carrying its code and remedy.
#[test]
fn global_writes_are_confirmed_only_by_confirm_and_previews_need_none() {
    let home = signed_out("gate");
    let responses = signed_out_mcp(
        &home,
        &["--exposure", "commands"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "tile_generate", "arguments": { "type": "survey", "project": "p-test" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "tile_generate", "arguments": { "type": "survey", "project": "--yes" } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "tile_generate", "arguments": { "type": "survey", "project": "p-test", "confirm": "yes" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "tile_generate", "arguments": { "type": "survey", "project": "p-test", "confirm": true } } }),
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "pm_task_create", "arguments": { "title": "t", "project": "p-test", "dry-run": true } } }),
            json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": { "name": "pm_task_create", "arguments": { "title": "t", "project": "p-test", "dry-run": true, "confirm": true } } }),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "pm_task_create", "arguments": { "title": "t", "project": "p-test" } } }),
        ],
    );
    for id in [1, 2, 7] {
        let envelope = structured(&responses, id);
        assert_eq!(response(&responses, id)["result"]["isError"], true, "{id}");
        assert_eq!(
            envelope["error"]["code"], "confirmation_required",
            "{id}: {envelope}"
        );
        assert!(
            envelope["error"]["remedy"]
                .as_str()
                .is_some_and(|remedy| !remedy.is_empty()),
            "{id}: a refusal carries its remedy"
        );
    }
    let wrong_type = structured(&responses, 3);
    assert_eq!(wrong_type["error"]["code"], "mcp_arguments_invalid");
    assert_eq!(wrong_type["command"], "tile.generate");
    // `confirm: true` is `--yes`: the gate is passed and the command's own
    // authority answers, exactly as the same confirmed call in a terminal.
    assert_eq!(
        *structured(&responses, 4),
        signed_out_cli(
            &home,
            &[
                "tile",
                "generate",
                "--type=survey",
                "--project=p-test",
                "--yes",
                "--output",
                "json"
            ]
        )
    );
    assert_ne!(
        structured(&responses, 4)["error"]["code"],
        "confirmation_required"
    );
    // A declared preview passes without confirmation and answers as the CLI.
    assert_ne!(
        structured(&responses, 5)["error"]["code"],
        "confirmation_required"
    );
    assert_eq!(
        *structured(&responses, 5),
        signed_out_cli(
            &home,
            &[
                "pm",
                "task",
                "create",
                "--title=t",
                "--project=p-test",
                "--dry-run",
                "--output",
                "json"
            ]
        )
    );
    let confirmed_preview = structured(&responses, 6);
    assert_eq!(confirmed_preview["error"]["code"], "mcp_arguments_invalid");
    assert!(
        confirmed_preview["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--dry-run")
    );
}

/// A caller's value reaches the command as that value, whatever it spells:
/// before 2026-09-25 a title beginning with `--` was refused as a missing
/// value, and a title of `-h` answered the help descriptor with status `ok`
/// — the write silently never ran.
#[test]
fn argument_values_reach_the_command_verbatim() {
    let home = signed_out("verbatim");
    let titles = [
        "--- a Markdown rule",
        "-h",
        "--help",
        "--version",
        "--output",
    ];
    let requests: Vec<Value> = titles
        .iter()
        .enumerate()
        .map(|(index, title)| {
            json!({ "jsonrpc": "2.0", "id": index as i64 + 1, "method": "tools/call",
                    "params": { "name": "pm_task_create", "arguments": { "title": title, "project": "p-test", "dry-run": true } } })
        })
        .collect();
    let responses = signed_out_mcp(&home, &["--exposure", "commands"], &requests);
    for (index, title) in titles.iter().enumerate() {
        let envelope = structured(&responses, index as i64 + 1);
        assert_eq!(envelope["command"], "pm.task.create", "{title}: {envelope}");
        assert!(
            envelope["data"].get("path").is_none(),
            "`{title}` answered the help descriptor instead of running: {envelope}"
        );
        assert_ne!(envelope["error"]["code"], "missing_value", "{title}");
        assert_eq!(
            *envelope,
            signed_out_cli(
                &home,
                &[
                    "pm",
                    "task",
                    "create",
                    &format!("--title={title}"),
                    "--project=p-test",
                    "--dry-run",
                    "--output",
                    "json",
                ]
            ),
            "{title}"
        );
    }
}

/// initialize → tools/list → tools/call on a read-only typed tool, with a
/// progress token the host supplied: the answer is the CLI's envelope, and a
/// call that ends inside one progress interval sends no progress at all.
#[test]
fn typed_protocol_smoke_returns_the_cli_envelope() {
    let (responses, _) = mcp(
        &["--exposure", "commands", "--profile", "operations"],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "shell_status", "arguments": {}, "_meta": { "progressToken": "p-1" } } }),
        ],
    );
    assert_eq!(
        response(&responses, 1)["result"]["protocolVersion"],
        "2025-06-18"
    );
    let tools = tools_by_title(&responses, 2);
    let shell = leaf(&tools, "shell.status");
    assert_eq!(shell["annotations"]["readOnlyHint"], true);
    assert_eq!(shell["annotations"]["destructiveHint"], false);
    let result = &response(&responses, 3)["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(
        result["structuredContent"],
        cli(&["shell", "status", "--output", "json"])
    );
    assert_eq!(
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap(),
        result["structuredContent"],
        "the text block is the same envelope, serialized"
    );
    assert!(
        responses
            .iter()
            .all(|message| message["method"] != "notifications/progress"),
        "a quick call reports no progress"
    );
}

#[test]
fn an_unbounded_or_malformed_call_timeout_is_refused_before_serving() {
    for value in ["0", "86401", "soon"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(["mcp", "serve", "--call-timeout", value, "--output", "json"])
            .stdin(Stdio::null())
            .output()
            .expect("ds runs");
        assert!(!output.status.success(), "{value}");
        let envelope: Value = serde_json::from_slice(&output.stdout).expect("one envelope");
        assert_eq!(envelope["error"]["code"], "invalid_number", "{value}");
    }
}
