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
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::tools::{self, Tool};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub static COMMAND: Command = Command {
    id: "mcp.serve",
    path: &["mcp", "serve"],
    contract: 1,
    summary: "Serve every `ds` command to an MCP host over stdio.",
    purpose: "\
Runs a Model Context Protocol server on stdio for the host that launched it \
(VS Code, Copilot, Claude, Cursor, Codex). The tool list is built at startup \
from this executable's own `ds capabilities`, one descriptor per command, so \
it cannot differ from the CLI. Each tool call runs `ds … --output json` and \
returns the envelope verbatim; a refusal stays a refusal. Effectful tools \
take `confirm: true`, which maps onto `--yes`. No credential, listener or \
cache is added; pairing stays the only authority. Exits when stdin closes.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "\
Nothing on stdout but MCP responses. On exit, a JSON summary: tools served, \
calls answered, and why the loop ended.",
    examples: &[Example {
        command: "ds mcp serve",
        note: "Only ever launched by an MCP host as a stdio server; see `ds mcp install` for the host entry.",
        runnable: false,
    }],
    refusals: &[crate::CAPABILITIES_UNAVAILABLE, crate::STDIO_UNAVAILABLE],
    reference: Some("docs/reference/mcp.md"),
    availability: crate::always,
};

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let executable = tools::cli_executable()?;
    let tools = tools::discover_tools(&executable)?;
    eprintln!(
        "ds mcp: serving {} tools from {}",
        tools.len(),
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
        let Some(response) = handle(&request, &executable, &tools, &mut calls) else {
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
    let summary = json!({ "tools": tools.len(), "calls": calls, "ended": reason });
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
    tools: &[Tool],
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
            "serverInfo": { "name": "ds", "title": "DS command line", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Every tool is one `ds` command; read its description for effect, authority and refusals. Effectful tools need `confirm: true`, which maps to `--yes`. Results are the CLI's JSON envelope: branch on `status`, and follow `error.remedy` when it refuses.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools.iter().map(tool_json).collect::<Vec<_>>() })),
        "tools/call" => {
            *calls += 1;
            call(&params, executable, tools)
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

pub fn tool_json(tool: &Tool) -> Value {
    json!({
        "name": tool.name,
        "title": tool.id,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "annotations": {
            "title": tool.id,
            "readOnlyHint": !tool.confirmation_required,
            "openWorldHint": false,
        },
    })
}

fn call(
    params: &Value,
    executable: &std::path::PathBuf,
    tools: &[Tool],
) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
        return Err((-32602, format!("unknown tool: {name}")));
    };
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let argv = tools::argv_for_call(tool, &arguments).map_err(|message| (-32602, message))?;
    let (code, stdout, stderr) =
        tools::run_cli(executable, &argv).map_err(|message| (-32000, message))?;
    let envelope: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    let is_error = code != 0
        || envelope
            .as_ref()
            .and_then(|e| e.get("status"))
            .and_then(Value::as_str)
            != Some("ok");
    let text = if stdout.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    };
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    });
    if let Some(envelope) = envelope {
        result["structuredContent"] = envelope;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> Tool {
        tools::tool_from_descriptor(&json!({
            "id": "shell.status", "path": ["shell", "status"], "summary": "Where ds resolves from.",
            "purpose": "Reports which executable ds resolves to on this PATH.", "output": "paths",
            "effect": "discovery", "authority": "none", "confirmation_required": false, "inputs": [], "refusals": []
        }))
        .expect("tool")
    }

    #[test]
    fn initialize_lists_tools_and_negotiates_a_known_version() {
        let mut calls = 0;
        let exe = std::path::PathBuf::from("ds");
        let init = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}), &exe, &[tool()], &mut calls).expect("response");
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "ds");
        let list = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &exe,
            &[tool()],
            &mut calls,
        )
        .expect("response");
        assert_eq!(list["result"]["tools"][0]["name"], "shell_status");
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
        assert!(
            handle(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                &exe,
                &[],
                &mut calls
            )
            .is_none()
        );
        let unknown = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
            &exe,
            &[],
            &mut calls,
        )
        .expect("response");
        assert_eq!(unknown["error"]["code"], -32601);
        let missing = handle(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}),
            &exe,
            &[],
            &mut calls,
        )
        .expect("response");
        assert_eq!(missing["error"]["code"], -32602);
        assert_eq!(calls, 1);
    }
}
