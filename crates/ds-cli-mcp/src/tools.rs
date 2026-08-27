//! Descriptor → MCP tool, and MCP arguments → `ds` argv.
//!
//! Both directions are pure functions over the JSON `ds capabilities` emits,
//! so the mapping is testable without a process and the tool list can never
//! say something the CLI did not.

use std::path::PathBuf;
use std::process::Command;

use ds_cli_contract::outcome::Failure;
use serde_json::{Map, Value, json};

/// One `ds` command as an MCP tool, plus what is needed to call it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    /// MCP tool name: the dotted command id with `.` → `_` (the MCP name
    /// grammar has no dots). `map.design.report` → `map_design_report`.
    pub name: String,
    /// The dotted command id, kept as the tool title so a host shows the
    /// same word the skills use.
    pub id: String,
    pub path: Vec<String>,
    pub description: String,
    pub input_schema: Value,
    pub confirmation_required: bool,
    pub inputs: Vec<Input>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub name: String,
    pub kind: String,
}

/// The property that maps onto `--yes`. Hosts cannot press a confirmation
/// prompt, so an effectful command declares this boolean instead; without
/// it the CLI refuses exactly as it would on a terminal, and the host sees
/// that refusal with its remedy.
pub const CONFIRM_PROPERTY: &str = "confirm";

pub fn tool_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build one tool from a tier-3 `ds capabilities <id>` descriptor.
pub fn tool_from_descriptor(command: &Value) -> Option<Tool> {
    let id = command.get("id")?.as_str()?.to_string();
    let path: Vec<String> = command
        .get("path")?
        .as_array()?
        .iter()
        .filter_map(|part| part.as_str().map(str::to_string))
        .collect();
    if path.is_empty() {
        return None;
    }
    let confirmation_required = command
        .get("confirmation_required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut inputs = Vec::new();
    for input in command
        .get("inputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = input.get("name").and_then(Value::as_str) else {
            continue;
        };
        let kind = input
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("value")
            .to_string();
        let summary = input.get("summary").and_then(Value::as_str).unwrap_or("");
        let value_hint = input.get("value").and_then(Value::as_str).unwrap_or("");
        let description = if value_hint.is_empty() {
            summary.to_string()
        } else {
            format!("{summary} ({value_hint})")
        };
        let mut schema = match kind.as_str() {
            "switch" => json!({ "type": "boolean" }),
            "repeated" => json!({ "type": "array", "items": { "type": "string" } }),
            _ => json!({ "type": "string" }),
        };
        if let Some(choices) = input.get("choices").and_then(Value::as_array)
            && !choices.is_empty()
        {
            schema["enum"] = Value::Array(choices.clone());
        }
        if let Some(default) = input.get("default")
            && !default.is_null()
        {
            schema["default"] = default.clone();
        }
        schema["description"] = Value::String(description);
        if input
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            required.push(Value::String(name.to_string()));
        }
        properties.insert(name.to_string(), schema);
        inputs.push(Input {
            name: name.to_string(),
            kind,
        });
    }
    if confirmation_required {
        properties.insert(
            CONFIRM_PROPERTY.to_string(),
            json!({
                "type": "boolean",
                "description": "This command has an effect and the CLI requires confirmation. Pass true only when the user's intent authorizes exactly this effect and scope (maps to `--yes`).",
            }),
        );
    }
    let summary = command.get("summary").and_then(Value::as_str).unwrap_or("");
    let purpose = command.get("purpose").and_then(Value::as_str).unwrap_or("");
    let output = command.get("output").and_then(Value::as_str).unwrap_or("");
    let effect = command.get("effect").and_then(Value::as_str).unwrap_or("");
    let authority = command
        .get("authority")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut description = format!("{summary}\n\n{purpose}");
    if !output.is_empty() {
        description.push_str(&format!("\n\nReturns: {output}"));
    }
    description.push_str(&format!("\n\nEffect: {effect}. Authority: {authority}."));
    if confirmation_required {
        description.push_str(&format!(" Requires `{CONFIRM_PROPERTY}: true`."));
    }
    let refusals: Vec<String> = command
        .get("refusals")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|refusal| {
            let code = refusal.get("code")?.as_str()?;
            let when = refusal.get("when").and_then(Value::as_str).unwrap_or("");
            Some(format!("`{code}` — {when}"))
        })
        .collect();
    if !refusals.is_empty() {
        description.push_str("\n\nRefuses with: ");
        description.push_str(&refusals.join("; "));
    }
    Some(Tool {
        name: tool_name(&id),
        id,
        path,
        description,
        input_schema: json!({
            "type": "object",
            "properties": Value::Object(properties),
            "required": required,
            "additionalProperties": false,
        }),
        confirmation_required,
        inputs,
    })
}

/// Map a `tools/call` argument object onto the argv `ds` expects after the
/// command path. Unknown properties are refused here rather than forwarded
/// as flags: the CLI would refuse them too, but naming the property keeps
/// the host's mistake visible as its own.
pub fn argv_for_call(tool: &Tool, arguments: &Value) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = tool.path.clone();
    let object = match arguments {
        Value::Null => Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err("arguments must be an object".to_string()),
    };
    // Unknown properties first, so the host's mistake is named before any
    // mapping happens.
    for key in object.keys() {
        if key != CONFIRM_PROPERTY && !tool.inputs.iter().any(|input| &input.name == key) {
            return Err(format!("`{key}` is not an input of `{}`", tool.id));
        }
    }
    // Declared order, not object order: `serde_json::Map` sorts keys, and a
    // host may send them in any order. The argv is then reproducible.
    let mut positional: Vec<String> = Vec::new();
    for input in &tool.inputs {
        let Some(value) = object.get(&input.name) else {
            continue;
        };
        let key = &input.name;
        match input.kind.as_str() {
            "switch" => match value {
                Value::Bool(true) => argv.push(format!("--{key}")),
                Value::Bool(false) | Value::Null => {}
                _ => return Err(format!("`{key}` must be a boolean")),
            },
            "repeated" => {
                let items = match value {
                    Value::Array(items) => items.clone(),
                    Value::Null => Vec::new(),
                    other => vec![other.clone()],
                };
                for item in items {
                    argv.push(format!("--{key}"));
                    argv.push(
                        scalar(&item).ok_or_else(|| format!("`{key}` items must be scalars"))?,
                    );
                }
            }
            "positional" => {
                if !value.is_null() {
                    positional
                        .push(scalar(value).ok_or_else(|| format!("`{key}` must be a scalar"))?);
                }
            }
            _ => {
                if value.is_null() {
                    continue;
                }
                argv.push(format!("--{key}"));
                argv.push(scalar(value).ok_or_else(|| format!("`{key}` must be a scalar"))?);
            }
        }
    }
    argv.extend(positional);
    match object.get(CONFIRM_PROPERTY) {
        Some(Value::Bool(true)) => argv.push("--yes".to_string()),
        None | Some(Value::Bool(false)) | Some(Value::Null) => {}
        Some(_) => return Err(format!("`{CONFIRM_PROPERTY}` must be a boolean")),
    }
    argv.push("--output".to_string());
    argv.push("json".to_string());
    Ok(argv)
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// Where the CLI lives: the executable serving MCP is the executable every
/// tool call runs, so the host talks to exactly one build.
pub fn cli_executable() -> Result<PathBuf, Failure> {
    std::env::current_exe().map_err(|error| {
        Failure::failed(
            "mcp_capabilities_unavailable",
            format!("could not resolve this executable's path: {error}"),
        )
        .remedy(
            "run `ds capabilities --output json` by hand and fix what it reports before serving",
        )
    })
}

/// Run `ds <argv…>` and return (exit code, stdout, stderr).
pub fn run_cli(executable: &PathBuf, argv: &[String]) -> Result<(i32, String, String), String> {
    let output = Command::new(executable)
        .args(argv)
        .env("DS_MCP_CHILD", "1")
        .output()
        .map_err(|error| format!("could not start `{}`: {error}", executable.display()))?;
    Ok((
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Read one `ds capabilities …` envelope and return its `data`.
fn capabilities(executable: &PathBuf, selector: Option<&str>) -> Result<Value, Failure> {
    let mut argv = vec!["capabilities".to_string()];
    if let Some(selector) = selector {
        argv.push(selector.to_string());
    }
    argv.push("--output".to_string());
    argv.push("json".to_string());
    let (code, stdout, stderr) = run_cli(executable, &argv).map_err(|message| {
        Failure::failed("mcp_capabilities_unavailable", message).remedy(
            "run `ds capabilities --output json` by hand and fix what it reports before serving",
        )
    })?;
    let envelope: Value = serde_json::from_str(&stdout).map_err(|error| {
        Failure::failed(
            "mcp_capabilities_unavailable",
            format!(
                "`ds capabilities {}` emitted no envelope ({error}): {stderr}",
                selector.unwrap_or("")
            ),
        )
        .remedy(
            "run `ds capabilities --output json` by hand and fix what it reports before serving",
        )
    })?;
    if code != 0 || envelope.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(Failure::failed(
            "mcp_capabilities_unavailable",
            format!(
                "`ds capabilities {}` refused: {}",
                selector.unwrap_or(""),
                envelope["error"]["message"].as_str().unwrap_or("unknown")
            ),
        )
        .remedy(
            "run `ds capabilities --output json` by hand and fix what it reports before serving",
        )
        .detail(envelope["error"].clone()));
    }
    Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
}

/// Every tool this executable can serve — built from the live tiers, never
/// from a table. The `mcp` domain itself is excluded: a server that lists
/// "start a server" as a tool is a loop, not a capability.
pub fn discover_tools(executable: &PathBuf) -> Result<Vec<Tool>, Failure> {
    let index = capabilities(executable, None)?;
    let mut tools = Vec::new();
    for domain in index
        .get("domains")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(domain_id) = domain.get("id").and_then(Value::as_str) else {
            continue;
        };
        if domain_id == crate::DOMAIN.id {
            continue;
        }
        let tier = capabilities(executable, Some(domain_id))?;
        for command in tier
            .get("commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = command.get("id").and_then(Value::as_str) else {
                continue;
            };
            let descriptor = capabilities(executable, Some(id))?;
            if let Some(tool) = descriptor.get("command").and_then(tool_from_descriptor) {
                tools.push(tool);
            }
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> Value {
        json!({
            "id": "map.design.report",
            "path": ["map", "design", "report"],
            "summary": "Export one transformer's report locally.",
            "purpose": "Runs the local Network Reporter export for one transformer and reports each artifact.",
            "output": "Artifact evidence.",
            "effect": "artifact_write",
            "authority": "project",
            "confirmation_required": true,
            "inputs": [
                { "name": "transformer", "kind": "value", "required": true, "summary": "The transformer whose design layers to work on.", "value": "<name>" },
                { "name": "layer", "kind": "repeated", "required": false, "summary": "Restrict to these layers.", "value": "<name>" },
                { "name": "dry-run", "kind": "switch", "required": false, "summary": "Report without writing anything." },
                { "name": "format", "kind": "value", "required": false, "summary": "Output format for the artifact.", "value": "<fmt>", "choices": ["xlsx", "shp"] }
            ],
            "refusals": [
                { "code": "desktop_not_paired", "when": "no DS GridDesign session is running", "remedy": "start it" }
            ]
        })
    }

    #[test]
    fn descriptor_becomes_a_tool_with_a_schema_the_cli_would_accept() {
        let tool = tool_from_descriptor(&descriptor()).expect("tool");
        assert_eq!(tool.name, "map_design_report");
        assert_eq!(tool.id, "map.design.report");
        assert_eq!(tool.path, ["map", "design", "report"]);
        assert!(tool.confirmation_required);
        let props = &tool.input_schema["properties"];
        assert_eq!(props["transformer"]["type"], "string");
        assert_eq!(props["layer"]["type"], "array");
        assert_eq!(props["dry-run"]["type"], "boolean");
        assert_eq!(props["format"]["enum"], json!(["xlsx", "shp"]));
        assert_eq!(props[CONFIRM_PROPERTY]["type"], "boolean");
        assert_eq!(tool.input_schema["required"], json!(["transformer"]));
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert!(tool.description.contains("desktop_not_paired"));
    }

    #[test]
    fn arguments_map_onto_the_exact_cli_argv() {
        let tool = tool_from_descriptor(&descriptor()).expect("tool");
        let argv = argv_for_call(
            &tool,
            &json!({ "transformer": "T-1", "layer": ["lv_poles", "customers"], "dry-run": true, "confirm": true }),
        )
        .expect("argv");
        assert_eq!(
            argv,
            [
                "map",
                "design",
                "report",
                "--transformer",
                "T-1",
                "--layer",
                "lv_poles",
                "--layer",
                "customers",
                "--dry-run",
                "--yes",
                "--output",
                "json",
            ]
        );
    }

    #[test]
    fn unknown_properties_and_wrong_types_are_refused_by_name() {
        let tool = tool_from_descriptor(&descriptor()).expect("tool");
        let unknown =
            argv_for_call(&tool, &json!({ "transformer": "T-1", "nope": 1 })).unwrap_err();
        assert!(unknown.contains("`nope`"), "{unknown}");
        let wrong =
            argv_for_call(&tool, &json!({ "transformer": "T-1", "dry-run": "yes" })).unwrap_err();
        assert!(wrong.contains("`dry-run`"), "{wrong}");
        let scalar_only = argv_for_call(&tool, &json!({ "transformer": { "a": 1 } })).unwrap_err();
        assert!(scalar_only.contains("`transformer`"), "{scalar_only}");
    }

    #[test]
    fn a_false_or_absent_confirm_never_passes_yes() {
        let tool = tool_from_descriptor(&descriptor()).expect("tool");
        let argv =
            argv_for_call(&tool, &json!({ "transformer": "T-1", "confirm": false })).expect("argv");
        assert!(!argv.iter().any(|token| token == "--yes"));
        let argv = argv_for_call(&tool, &json!({ "transformer": "T-1" })).expect("argv");
        assert!(!argv.iter().any(|token| token == "--yes"));
    }

    #[test]
    fn tool_names_follow_the_mcp_grammar() {
        assert_eq!(
            tool_name("dsgrid-exchange.convert"),
            "dsgrid-exchange_convert"
        );
        assert_eq!(
            tool_name("map.design.batch.process"),
            "map_design_batch_process"
        );
    }
}
