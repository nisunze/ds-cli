//! `ds mcp serve` — the stdio JSON-RPC loop.
//!
//! MCP over stdio is newline-delimited JSON-RPC 2.0: one request per line on
//! stdin, one response per line on stdout, nothing else on stdout. Logs go
//! to stderr. The methods a host needs to use tools are exactly four —
//! `initialize`, `notifications/initialized`, `tools/list`, `tools/call` —
//! plus `ping`; everything else answers "method not found", which a host
//! treats as "unsupported", not as an error. Receipt-backed guidance is
//! available through `resources/list` and `resources/read` without preloading
//! documents into initialization.

use std::io::{self, BufRead, Write};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::surface::{Exposure, Profile, Surface};
use crate::{resources::SkillResources, tools};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub static COMMAND: Command = Command {
    id: "mcp.serve",
    path: &["mcp", "serve"],
    contract: 4,
    chapter: ds_cli_contract::spec::Chapter::Catalog,
    summary: "Serve chapter or typed `ds` tools over MCP.",
    purpose: "\
Serves live `ds` contracts over MCP stdio as compact chapters or bounded typed \
profiles. Calls run `ds`. Startup is headless; dependencies and skills load on demand. It owns no credential, listener, \
cache, project state, or authority.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg {
            name: "exposure",
            kind: ArgKind::Value,
            value: "<chapters|commands>",
            required: false,
            default: Some("chapters"),
            choices: crate::surface::EXPOSURES,
            summary: "Publish compact chapter routers or typed command tools.",
        },
        Arg {
            name: "profile",
            kind: ArgKind::Value,
            value: "<name>",
            required: false,
            default: None,
            choices: crate::surface::PROFILE_IDS,
            summary: "Filter typed tools to one operator workflow.",
        },
    ],
    output: "MCP responses on stdout; an exit summary on stderr.",
    examples: &[
        Example {
            command: "ds mcp serve --exposure chapters",
            note: "Broad server.",
            runnable: false,
        },
        Example {
            command: "ds mcp serve --exposure commands --profile pls",
            note: "Typed PLS profile.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::CAPABILITIES_UNAVAILABLE,
        crate::STDIO_UNAVAILABLE,
        crate::DESKTOP_NOT_PAIRED,
        crate::DESKTOP_SIGNED_OUT,
        crate::PROFILE_EXPOSURE_INVALID,
        crate::PROFILE_TOO_BROAD,
    ],
    reference: Some("docs/reference/mcp.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let executable = tools::cli_executable()?;
    let build = tools::build_identity(&executable)?;
    let source_sha = build["source_sha"].as_str().ok_or_else(|| {
        Failure::failed(
            "mcp_capabilities_unavailable",
            "`ds version` omitted its source SHA",
        )
        .remedy("run `ds version --output json` and repair this executable")
    })?;
    let resources = SkillResources::load(source_sha);
    let exposure = Exposure::from_token(inputs.value("exposure").unwrap_or("chapters"))
        .expect("the command parser enforces exposure choices");
    let profile = inputs.value("profile").map(|value| {
        Profile::from_token(value).expect("the command parser enforces profile choices")
    });
    let identity = bootstrap_identity(&executable, &build, exposure, profile, &resources);
    let surface = Surface::new(exposure, profile, tools::discover_tools(&executable)?)?
        .with_identity(identity);
    eprintln!(
        "ds mcp: serving {} {} tools{} from {}",
        surface.published_count(),
        surface.exposure().token(),
        surface
            .profile()
            .map(|profile| format!(" for profile {}", profile.token()))
            .unwrap_or_default(),
        executable.display()
    );
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut calls = 0usize;
    let mut reason = "stdin closed";
    let mut lines = stdin.lock().lines();
    loop {
        let line = match lines.next() {
            None => break,
            Some(Ok(line)) => line,
            Some(Err(error)) => {
                return Err(stdio_failure(format!("reading stdin failed: {error}")));
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_line(
                    &stdout,
                    &error_response(Value::Null, -32700, &format!("parse error: {error}")),
                )?;
                continue;
            }
        };
        let Some(response) = handle(
            &request,
            &executable,
            &surface,
            &build,
            &resources,
            &mut calls,
        ) else {
            continue; // a notification
        };
        write_line(&stdout, &response)?;
        if request.get("method").and_then(Value::as_str) == Some("shutdown") {
            reason = "host asked to shut down";
            break;
        }
    }
    // stdout belongs to the protocol until the process ends: no envelope, no
    // human summary may follow the last response. Log the summary where the
    // host's own log goes and leave.
    let summary = json!({
        "tools": surface.published_count(),
        "exposure": surface.exposure().token(),
        "profile": surface.profile().map(Profile::token),
        "calls": calls,
        "ended": reason,
    });
    eprintln!("ds mcp: {}", render(&summary).trim_end());
    std::process::exit(0);
}

pub fn render(data: &Value) -> String {
    format!(
        "mcp server ended ({}) — {} tools, {} calls\n",
        data["ended"].as_str().unwrap_or("?"),
        data["tools"],
        data["calls"],
    )
}

fn stdio_failure(message: String) -> Failure {
    Failure::failed("mcp_stdio_unavailable", message).remedy(
        "start the server from an MCP host as a stdio server; it is not an interactive command",
    )
}

fn write_line(stdout: &io::Stdout, value: &Value) -> Result<(), Failure> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|error| stdio_failure(format!("encoding a response failed: {error}")))?;
    bytes.push(b'\n');
    let mut handle = stdout.lock();
    handle
        .write_all(&bytes)
        .and_then(|_| handle.flush())
        .map_err(|error| stdio_failure(format!("writing stdout failed: {error}")))
}

/// Answer one request. `None` means the message was a notification (no id)
/// and gets no response — that is the protocol, not a dropped message.
pub fn handle(
    request: &Value,
    executable: &std::path::PathBuf,
    surface: &Surface,
    build: &Value,
    resources: &SkillResources,
    calls: &mut usize,
) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let is_notification = id.is_none() || method.starts_with("notifications/");
    let result = match method {
        "initialize" => {
            let server_identity = crate::identity::ServerIdentity::current();
            Ok(json!({
            "protocolVersion": negotiated_version(&params),
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false }
            },
            "serverInfo": {
                "name": server_identity.protocol_name(),
                "title": server_identity.title(),
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": format!(
                "{} Bootstrap: executable {}; source {}; build profile {}; skill resources {}. Use ds_diagnostics(operation=identity) for structured identity and resources/list then resources/read for one receipt-verified skill. Desktop and map checks are command-lazy.",
                surface.instructions(),
                executable.display(),
                build["source_sha"].as_str().unwrap_or("unknown"),
                build["profile"].as_str().unwrap_or("unknown"),
                resources.identity()["status"].as_str().unwrap_or("unavailable"),
            ),
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": surface.tool_list() })),
        "tools/call" => {
            *calls += 1;
            call(&params, executable, surface)
        }
        "resources/list" => Ok(resources.list()),
        "resources/read" => resources.read(&params),
        "shutdown" => Ok(Value::Null),
        _ if is_notification => return None,
        other => Err((-32601, format!("method not found: {other}"))),
    };
    if is_notification {
        return None;
    }
    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err((code, message)) => error_response(id.unwrap_or(Value::Null), code, &message),
    })
}

fn bootstrap_identity(
    executable: &std::path::Path,
    build: &Value,
    exposure: Exposure,
    profile: Option<Profile>,
    resources: &SkillResources,
) -> Value {
    let server_identity = crate::identity::ServerIdentity::current();
    json!({
        "executable": executable.display().to_string(),
        "version": build["version"],
        "source_sha": build["source_sha"],
        "dirty": build["dirty"],
        "target": build["target"],
        "build_profile": build["profile"],
        "install_profile": tools::install_profile(executable),
        "native_client_core_source_sha": build["native_client_core_source_sha"],
        "command_kernel_source_sha": build["command_kernel_source_sha"],
        "native_client_profile_catalog_sha256": build["native_client_profile_catalog_sha256"],
        "ds_network_source_sha": build["ds_network_source_sha"],
        "ds_network_source_state": build["ds_network_source_state"],
        "mcp": {
            "server_name": server_identity.protocol_name(),
            "server_title": server_identity.title(),
            "registration_name": server_identity.registration_name(),
            "release_lane": server_identity.lane(),
            "runtime_platform": server_identity.platform(),
            "transport": "stdio",
            "exposure": exposure.token(),
            "profile": profile.map(Profile::token),
            "protocol": PROTOCOL_VERSION,
        },
        "skills": resources.identity(),
    })
}

fn negotiated_version(params: &Value) -> &'static str {
    // Offer what the host asked for when it is one we speak; otherwise ours.
    match params.get("protocolVersion").and_then(Value::as_str) {
        Some("2024-11-05") => "2024-11-05",
        Some("2025-03-26") => "2025-03-26",
        _ => PROTOCOL_VERSION,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn call(
    params: &Value,
    executable: &std::path::PathBuf,
    surface: &Surface,
) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    surface.call(name, &arguments, executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(exposure: Exposure) -> Surface {
        let tool =
        tools::tool_from_descriptor(&json!({
            "id": "shell.status", "path": ["shell", "status"], "summary": "Where ds resolves from.",
            "purpose": "Reports which executable ds resolves to on this PATH.", "output": "paths",
            "chapter": "operations", "effect": "discovery", "authority": "none", "confirmation_required": false, "inputs": [], "refusals": []
        }))
        .expect("tool");
        Surface::new(exposure, None, vec![tool]).expect("surface")
    }

    fn build() -> Value {
        json!({
            "version": "0.1.0",
            "source_sha": "0123456789012345678901234567890123456789",
            "dirty": false,
            "target": "test-target",
            "profile": "debug",
        })
    }

    fn resources() -> SkillResources {
        SkillResources::load("0123456789012345678901234567890123456789")
    }

    #[test]
    fn initialize_lists_tools_and_negotiates_a_known_version() {
        let mut calls = 0;
        let exe = std::path::PathBuf::from("ds");
        let surface = surface(Exposure::Chapters);
        let resources = resources();
        let init = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}), &exe, &surface, &build(), &resources, &mut calls).expect("response");
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        let identity = crate::identity::ServerIdentity::current();
        assert_eq!(
            init["result"]["serverInfo"]["name"],
            identity.protocol_name()
        );
        assert_eq!(init["result"]["serverInfo"]["title"], identity.title());
        let list = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &exe,
            &surface,
            &build(),
            &resources,
            &mut calls,
        )
        .expect("response");
        // One router per chapter. Derived rather than literal so a new chapter
        // cannot ship unreachable while this still reads an old count.
        assert_eq!(
            list["result"]["tools"].as_array().unwrap().len(),
            ds_cli_contract::spec::Chapter::ALL.len() + 1
        );
        assert_eq!(list["result"]["tools"][0]["name"], "ds_catalog");
        assert_eq!(
            list["result"]["tools"][0]["annotations"]["readOnlyHint"],
            true
        );
        assert_eq!(calls, 0);
    }

    #[test]
    fn notifications_get_no_response_and_unknown_methods_are_named() {
        let mut calls = 0;
        let exe = std::path::PathBuf::from("ds");
        let surface = surface(Exposure::Chapters);
        let resources = resources();
        assert!(
            handle(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                &exe,
                &surface,
                &build(),
                &resources,
                &mut calls
            )
            .is_none()
        );
        let listed = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
            &exe,
            &surface,
            &build(),
            &resources,
            &mut calls,
        )
        .expect("response");
        assert!(listed["result"]["resources"].is_array());
        let missing = handle(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}),
            &exe,
            &surface,
            &build(),
            &resources,
            &mut calls,
        )
        .expect("response");
        assert_eq!(missing["error"]["code"], -32602);
        assert_eq!(calls, 1);
    }
}
