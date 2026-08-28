//! `ds mcp serve` — the stdio JSON-RPC loop.
//!
//! MCP over stdio is newline-delimited JSON-RPC 2.0: one request per line on
//! stdin, one response per line on stdout, nothing else on stdout. Logs go
//! to stderr. The methods a host needs to use tools are exactly four —
//! `initialize`, `notifications/initialized`, `tools/list`, `tools/call` —
//! plus `ping`; everything else answers "method not found", which a host
//! treats as "unsupported", not as an error.

use std::io::{self, BufRead, Write};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::surface::{Exposure, Profile, Surface};
use crate::tools;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub static COMMAND: Command = Command {
    id: "mcp.serve",
    path: &["mcp", "serve"],
    contract: 2,
    chapter: ds_cli_contract::spec::Chapter::Catalog,
    summary: "Serve chapter or typed `ds` tools over MCP.",
    purpose: "\
Serves generated chapter routers or typed command tools over MCP stdio. Every \
view comes from this executable's live descriptors and dispatches the same \
`ds … --output json` command. It adds no credential, listener, cache, project \
state, or authority.",
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
    output: "\
Nothing on stdout but MCP responses. On exit, a JSON summary: tools served, \
calls answered, and why the loop ended.",
    examples: &[
        Example {
            command: "ds mcp serve --exposure chapters",
            note: "Default: 12 stable chapter tools.",
            runnable: false,
        },
        Example {
            command: "ds mcp serve --exposure commands --profile pls",
            note: "Typed PLS workspace, backup and diagnostics.",
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
    let exposure = Exposure::from_token(inputs.value("exposure").unwrap_or("chapters"))
        .expect("the command parser enforces exposure choices");
    let profile = inputs.value("profile").map(|value| {
        Profile::from_token(value).expect("the command parser enforces profile choices")
    });
    let surface = Surface::new(exposure, profile, tools::discover_tools(&executable)?)?;
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
        let Some(response) = handle(&request, &executable, &surface, &build, &mut calls) else {
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
    calls: &mut usize,
) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let is_notification = id.is_none() || method.starts_with("notifications/");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": negotiated_version(&params),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "ds",
                "title": "DS command line",
                "version": env!("CARGO_PKG_VERSION"),
                "sourceSha": build["source_sha"],
                "dirty": build["dirty"],
            },
            "instructions": surface.instructions(),
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": surface.tool_list() })),
        "tools/call" => {
            *calls += 1;
            call(&params, executable, surface)
        }
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
        json!({ "source_sha": "0123456789012345678901234567890123456789", "dirty": false })
    }

    #[test]
    fn initialize_lists_tools_and_negotiates_a_known_version() {
        let mut calls = 0;
        let exe = std::path::PathBuf::from("ds");
        let surface = surface(Exposure::Chapters);
        let init = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}), &exe, &surface, &build(), &mut calls).expect("response");
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "ds");
        let list = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &exe,
            &surface,
            &build(),
            &mut calls,
        )
        .expect("response");
        // One router per chapter. Derived rather than literal so a new chapter
        // cannot ship unreachable while this still reads an old count.
        assert_eq!(
            list["result"]["tools"].as_array().unwrap().len(),
            ds_cli_contract::spec::Chapter::ALL.len()
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
        assert!(
            handle(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                &exe,
                &surface,
                &build(),
                &mut calls
            )
            .is_none()
        );
        let unknown = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
            &exe,
            &surface,
            &build(),
            &mut calls,
        )
        .expect("response");
        assert_eq!(unknown["error"]["code"], -32601);
        let missing = handle(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}),
            &exe,
            &surface,
            &build(),
            &mut calls,
        )
        .expect("response");
        assert_eq!(missing["error"]["code"], -32602);
        assert_eq!(calls, 1);
    }
}
