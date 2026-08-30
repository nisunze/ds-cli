//! Descriptor → MCP tool, and MCP arguments → `ds` argv.
//!
//! Both directions are pure functions over the JSON `ds capabilities` emits,
//! so the mapping is testable without a process and the tool list can never
//! say something the CLI did not.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter};
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
    pub chapter: Chapter,
    /// Parsed from the same live descriptor that supplied every other tool
    /// field. It is the only input to the MCP desktop gate.
    pub authority: Authority,
    pub path: Vec<String>,
    pub description: String,
    pub input_schema: Value,
    pub confirmation_required: bool,
    pub inputs: Vec<Input>,
    /// The authoritative tier-3 descriptor this tool was generated from.
    pub descriptor: Value,
}

const PAIR_POLL_INTERVAL: Duration = Duration::from_millis(200);
const PAIR_POLL_ATTEMPTS: usize = 50;

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
    let chapter = Chapter::from_token(command.get("chapter")?.as_str()?)?;
    let authority = Authority::from_token(command.get("authority")?.as_str()?)?;
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
    let authority_token = command
        .get("authority")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut description = format!("{summary}\n\n{purpose}");
    if !output.is_empty() {
        description.push_str(&format!("\n\nReturns: {output}"));
    }
    description.push_str(&format!(
        "\n\nEffect: {effect}. Authority: {authority_token}."
    ));
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
        chapter,
        authority,
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
        descriptor: command.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopState {
    Absent,
    Paired {
        signed_in: bool,
        project_selected: bool,
    },
}

/// Ensure exactly the authority the descriptor names, before dispatching an
/// MCP invocation. Discovery, `describe`, and every `Authority::None` tool
/// bypass this entirely.
pub fn ensure_desktop(tool: &Tool, arguments: &Value, executable: &PathBuf) -> Result<(), Failure> {
    if !tool.authority.requires_desktop() {
        return Ok(());
    }
    let named_descriptor = arguments
        .get("desktop-descriptor")
        .and_then(Value::as_str)
        .is_some()
        || std::env::var_os("DS_DESKTOP_DESCRIPTOR").is_some_and(|value| !value.is_empty());
    let descriptor = arguments
        .get("desktop-descriptor")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut status = || desktop_status(executable, descriptor.as_deref());
    let mut launch = || launch_installed_desktop(executable);
    let mut wait = || thread::sleep(PAIR_POLL_INTERVAL);
    ensure_desktop_with(
        tool.authority,
        named_descriptor,
        &mut status,
        &mut launch,
        &mut wait,
    )
}

/// The deterministic gate behind [`ensure_desktop`]. It has injectable
/// observation, launch, and wait steps so its no-launch and bounded-launch
/// guarantees are testable without an installed desktop.
fn ensure_desktop_with<S, L, W>(
    authority: Authority,
    named_descriptor: bool,
    status: &mut S,
    launch: &mut L,
    wait: &mut W,
) -> Result<(), Failure>
where
    S: FnMut() -> Result<DesktopState, Failure>,
    L: FnMut() -> Result<(), Failure>,
    W: FnMut(),
{
    if !authority.requires_desktop() {
        return Ok(());
    }
    match status()? {
        state @ DesktopState::Paired { .. } => return authority_ready(authority, state),
        DesktopState::Absent if named_descriptor => {
            return Err(not_paired("named descriptor did not publish a session"));
        }
        DesktopState::Absent => {}
    }

    // One invocation gets one launch attempt. Retrying an unchanged MCP call
    // must not turn into a process fan-out.
    launch()?;
    for _ in 0..PAIR_POLL_ATTEMPTS {
        wait();
        match status()? {
            state @ DesktopState::Paired { .. } => return authority_ready(authority, state),
            DesktopState::Absent => {}
        }
    }
    Err(not_paired(
        "desktop launch did not publish a paired session before the 10 second bound",
    ))
}

fn authority_ready(authority: Authority, state: DesktopState) -> Result<(), Failure> {
    let DesktopState::Paired {
        signed_in,
        project_selected,
    } = state
    else {
        return Err(not_paired("desktop is not paired"));
    };
    if authority.requires_signed_in_user() && !signed_in {
        return Err(Failure::unauthorized(
            "desktop_signed_out",
            "the paired DS GridDesign session is signed out",
        )
        .remedy("sign in to DS GridDesign, then retry the MCP tool call")
        .next("ds desktop status"));
    }
    if authority.requires_project() && !project_selected {
        return Err(Failure::unauthorized(
            "desktop_signed_out",
            "the paired DS GridDesign session has no selected project",
        )
        .remedy("select the intended project in DS GridDesign, then retry the MCP tool call")
        .next("ds desktop status"));
    }
    Ok(())
}

fn not_paired(detail: &str) -> Failure {
    Failure::unavailable("desktop_not_paired", "no paired DS GridDesign session is available")
        .remedy("start DS GridDesign and sign in, then retry the MCP tool call")
        .next("ds desktop status")
        .detail(json!({ "mcp_desktop_gate": detail, "wait_bound_ms": PAIR_POLL_ATTEMPTS as u64 * PAIR_POLL_INTERVAL.as_millis() as u64 }))
}

fn desktop_status(executable: &PathBuf, descriptor: Option<&str>) -> Result<DesktopState, Failure> {
    let mut argv = vec![
        "desktop".to_string(),
        "status".to_string(),
        "--output".to_string(),
        "json".to_string(),
    ];
    if let Some(descriptor) = descriptor {
        argv.insert(2, descriptor.to_string());
        argv.insert(2, "--desktop-descriptor".to_string());
    }
    let (code, stdout, stderr) = run_cli(executable, &argv).map_err(|message| {
        Failure::unavailable("desktop_not_paired", "desktop status could not be read")
            .remedy("start DS GridDesign and retry the MCP tool call")
            .detail(json!({ "mcp_desktop_gate": bounded(&message) }))
    })?;
    let envelope: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        Failure::unavailable(
            "desktop_not_paired",
            "desktop status returned no readable envelope",
        )
        .remedy("start DS GridDesign and retry the MCP tool call")
        .detail(json!({ "mcp_desktop_gate": bounded(&stderr) }))
    })?;
    if code != 0 || envelope.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(not_paired(
            "desktop status refused before pairing completed",
        ));
    }
    let data = &envelope["data"];
    if !data["paired"].as_bool().unwrap_or(false) {
        return Ok(DesktopState::Absent);
    }
    Ok(DesktopState::Paired {
        signed_in: data["signed_in"].as_bool().unwrap_or(false),
        project_selected: data["project"]
            .as_str()
            .is_some_and(|project| !project.is_empty()),
    })
}

fn bounded(value: &str) -> String {
    value
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

fn launch_installed_desktop(executable: &Path) -> Result<(), Failure> {
    let application = installed_desktop(executable)?;
    Command::new(application)
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            not_paired(&format!(
                "installed desktop could not start: {}",
                error.kind()
            ))
        })
}

fn installed_desktop(executable: &Path) -> Result<PathBuf, Failure> {
    #[cfg(windows)]
    {
        let sibling = sibling_application(executable).map(|path| {
            let exists = path.is_file();
            (path, exists)
        });
        let fallback = local_applications()
            .into_iter()
            .filter(|candidate| candidate.is_file())
            .collect();
        match select_installed_desktop(sibling, fallback) {
            ApplicationSelection::Selected(application) => Ok(application),
            ApplicationSelection::MissingSibling(application) => Err(not_paired(&format!(
                "the installed ds belongs to {}, but its sibling application is missing",
                application.display()
            ))),
            ApplicationSelection::None => Err(not_paired(
                "no installed DS GridDesign application was found beside ds or in LOCALAPPDATA",
            )),
            ApplicationSelection::Ambiguous(_) => Err(not_paired(
                "more than one installed DS GridDesign application was found; start the intended one or provide its descriptor",
            )),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = executable;
        Err(not_paired(
            "automatic desktop launch is currently available only in the installed Windows package",
        ))
    }
}

/// The only product layouts an installed `ds.exe` may claim. The directory
/// itself carries the Stable/Canary identity, so a side-by-side install never
/// makes its own sibling ambiguous.
#[cfg(any(windows, test))]
fn sibling_application(executable: &std::path::Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    let application = match directory.file_name()?.to_str()? {
        "DS GridDesign" => "DS GridDesign.exe",
        "DS GridDesign Canary" => "DS GridDesign Canary.exe",
        _ => return None,
    };
    Some(directory.join(application))
}

#[cfg(windows)]
fn local_applications() -> Vec<PathBuf> {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    let base = PathBuf::from(base);
    vec![
        base.join("DS GridDesign").join("DS GridDesign.exe"),
        base.join("DS GridDesign Canary")
            .join("DS GridDesign Canary.exe"),
    ]
}

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
enum ApplicationSelection {
    Selected(PathBuf),
    MissingSibling(PathBuf),
    None,
    Ambiguous(Vec<PathBuf>),
}

/// Select a desktop without using filesystem state, so side-by-side product
/// layout is testable on every host. A recognized sibling is an owner proof;
/// fallbacks are consulted only when the running `ds` has no such identity.
#[cfg(any(windows, test))]
fn select_installed_desktop(
    sibling: Option<(PathBuf, bool)>,
    mut fallback: Vec<PathBuf>,
) -> ApplicationSelection {
    if let Some((application, exists)) = sibling {
        return if exists {
            ApplicationSelection::Selected(application)
        } else {
            ApplicationSelection::MissingSibling(application)
        };
    }
    fallback.sort();
    fallback.dedup();
    match fallback.as_slice() {
        [] => ApplicationSelection::None,
        [application] => ApplicationSelection::Selected(application.clone()),
        _ => ApplicationSelection::Ambiguous(fallback),
    }
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
        if key == CONFIRM_PROPERTY && !tool.confirmation_required {
            return Err(format!(
                "`{}` does not declare `{CONFIRM_PROPERTY}`",
                tool.id
            ));
        }
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

/// Installation channel evidence available from the executable path. Desktop
/// Stable and Canary packages have closed sibling layouts. Other layouts are
/// deliberately reported as unlabeled rather than guessed to be development
/// or one of the release lanes.
pub fn install_profile(executable: &Path) -> &'static str {
    match executable
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
    {
        Some("DS GridDesign") => "stable",
        Some("DS GridDesign Canary") => "canary",
        _ => "unlabeled",
    }
}

/// Run `ds <argv…>` and return (exit code, stdout, stderr).
pub fn run_cli(executable: &PathBuf, argv: &[String]) -> Result<(i32, String, String), String> {
    run_cli_with_schema_mode(executable, argv, false)
}

fn run_cli_with_schema_mode(
    executable: &PathBuf,
    argv: &[String],
    schema_only: bool,
) -> Result<(i32, String, String), String> {
    let mut command = Command::new(executable);
    command
        .args(argv)
        .env("DS_MCP_CHILD", "1")
        .env_remove("DS_MCP_SCHEMA_ONLY");
    if schema_only {
        command.env("DS_MCP_SCHEMA_ONLY", "1");
    }
    let output = command
        .output()
        .map_err(|error| format!("could not start `{}`: {error}", executable.display()))?;
    Ok((
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Read this exact executable's build identity for MCP and skill provenance.
pub fn build_identity(executable: &PathBuf) -> Result<Value, Failure> {
    let argv = [
        "version".to_string(),
        "--output".to_string(),
        "json".to_string(),
    ];
    let (code, stdout, stderr) = run_cli(executable, &argv).map_err(|message| {
        Failure::failed("mcp_capabilities_unavailable", message)
            .remedy("run `ds version --output json` and repair this executable")
    })?;
    let envelope: Value = serde_json::from_str(&stdout).map_err(|error| {
        Failure::failed(
            "mcp_capabilities_unavailable",
            format!("`ds version` emitted no envelope ({error}): {stderr}"),
        )
        .remedy("run `ds version --output json` and repair this executable")
    })?;
    if code != 0 || envelope.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(Failure::failed(
            "mcp_capabilities_unavailable",
            "`ds version --output json` refused",
        )
        .remedy("run `ds version --output json` and repair this executable")
        .detail(envelope));
    }
    Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
}

/// Read one `ds capabilities …` envelope and return its `data`.
fn capabilities(
    executable: &PathBuf,
    selector: Option<&str>,
    schema_only: bool,
) -> Result<Value, Failure> {
    let mut argv = vec!["capabilities".to_string()];
    if let Some(selector) = selector {
        argv.push(selector.to_string());
    }
    argv.push("--output".to_string());
    argv.push("json".to_string());
    let (code, stdout, stderr) =
        run_cli_with_schema_mode(executable, &argv, schema_only).map_err(|message| {
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
    let index = capabilities(executable, None, true)?;
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
        let tier = capabilities(executable, Some(domain_id), true)?;
        for command in tier
            .get("commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = command.get("id").and_then(Value::as_str) else {
                continue;
            };
            let descriptor = capabilities(executable, Some(id), true)?;
            let command = descriptor.get("command").ok_or_else(|| {
                Failure::failed(
                    "mcp_capabilities_unavailable",
                    format!("`ds capabilities {id}` omitted its command descriptor"),
                )
                .remedy("repair the command registry and rebuild this exact `ds` executable")
            })?;
            let tool = tool_from_descriptor(command).ok_or_else(|| {
                Failure::failed(
                    "mcp_capabilities_unavailable",
                    format!("`ds capabilities {id}` has no valid MCP chapter or schema"),
                )
                .remedy("assign the command exactly one valid chapter and rebuild `ds`")
                .detail(command.clone())
            })?;
            tools.push(tool);
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tools)
}

/// Resolve one command's ordinary live descriptor on an explicit catalogue
/// request. Startup discovery deliberately uses the unchecked schema mode;
/// this function is the lazy availability boundary.
pub fn live_command_descriptor(executable: &PathBuf, id: &str) -> Result<Value, Failure> {
    let data = capabilities(executable, Some(id), false)?;
    data.get("command").cloned().ok_or_else(|| {
        Failure::failed(
            "mcp_capabilities_unavailable",
            format!("`ds capabilities {id}` omitted its command descriptor"),
        )
        .remedy("repair the command registry and rebuild this exact `ds` executable")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> Value {
        json!({
            "id": "map.design.report",
            "chapter": "design",
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
        assert_eq!(tool.authority, Authority::Project);
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
    fn confirm_is_rejected_for_a_command_that_does_not_declare_it() {
        let mut descriptor = descriptor();
        descriptor["confirmation_required"] = json!(false);
        let tool = tool_from_descriptor(&descriptor).expect("tool");
        let error = argv_for_call(&tool, &json!({ "confirm": true })).unwrap_err();
        assert!(error.contains("does not declare `confirm`"), "{error}");
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

    #[test]
    fn headless_authority_never_observes_waits_or_launches_a_desktop() {
        for authority in [
            Authority::None,
            Authority::HeadlessUser,
            Authority::HeadlessProject,
        ] {
            let mut observed = 0usize;
            let mut launched = 0usize;
            let mut waited = 0usize;
            ensure_desktop_with(
                authority,
                false,
                &mut || {
                    observed += 1;
                    Ok(DesktopState::Absent)
                },
                &mut || {
                    launched += 1;
                    Ok(())
                },
                &mut || waited += 1,
            )
            .expect("headless command is ready without desktop work");
            assert_eq!((observed, launched, waited), (0, 0, 0));
        }
    }

    #[test]
    fn paired_authority_launches_once_and_waits_only_to_the_declared_bound() {
        let mut observations = 0usize;
        let mut launched = 0usize;
        let mut waited = 0usize;
        let failure = ensure_desktop_with(
            Authority::DesktopPairing,
            false,
            &mut || {
                observations += 1;
                Ok(DesktopState::Absent)
            },
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || waited += 1,
        )
        .expect_err("no descriptor ever appears");
        assert_eq!(failure.code(), "desktop_not_paired");
        assert_eq!(launched, 1, "one call must not fan out app launches");
        assert_eq!(waited, PAIR_POLL_ATTEMPTS);
        assert_eq!(observations, PAIR_POLL_ATTEMPTS + 1);
    }

    #[test]
    fn an_already_running_desktop_is_never_duplicated() {
        let mut launched = 0usize;
        ensure_desktop_with(
            Authority::DesktopUser,
            false,
            &mut || {
                Ok(DesktopState::Paired {
                    signed_in: true,
                    project_selected: false,
                })
            },
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || panic!("a paired desktop must not be polled"),
        )
        .expect("signed-in desktop user is ready");
        assert_eq!(launched, 0);
    }

    #[test]
    fn signed_out_desktop_refuses_without_a_second_launch() {
        let mut launched = 0usize;
        let failure = ensure_desktop_with(
            Authority::Project,
            false,
            &mut || {
                Ok(DesktopState::Paired {
                    signed_in: false,
                    project_selected: false,
                })
            },
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || panic!("a paired desktop must not be polled"),
        )
        .expect_err("sign-out is an authority refusal");
        assert_eq!(failure.code(), "desktop_signed_out");
        assert_eq!(launched, 0);
    }

    #[test]
    fn a_named_descriptor_is_never_replaced_by_an_automatic_launch() {
        let mut launched = 0usize;
        let failure = ensure_desktop_with(
            Authority::DesktopPairing,
            true,
            &mut || Ok(DesktopState::Absent),
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || panic!("a named descriptor must not enter launch polling"),
        )
        .expect_err("named descriptor remains authoritative");
        assert_eq!(failure.code(), "desktop_not_paired");
        assert_eq!(launched, 0);
    }

    #[test]
    fn sibling_product_layout_selects_its_own_stable_or_canary_app_first() {
        let stable_ds = PathBuf::from("C:/Users/test/AppData/Local/DS GridDesign/ds.exe");
        let stable = sibling_application(&stable_ds).expect("stable sibling layout");
        assert_eq!(
            stable,
            PathBuf::from("C:/Users/test/AppData/Local/DS GridDesign/DS GridDesign.exe")
        );
        let selected = select_installed_desktop(
            Some((stable.clone(), true)),
            vec![
                stable.clone(),
                PathBuf::from(
                    "C:/Users/test/AppData/Local/DS GridDesign Canary/DS GridDesign Canary.exe",
                ),
            ],
        );
        assert_eq!(selected, ApplicationSelection::Selected(stable.clone()));

        let canary_ds = PathBuf::from("C:/Users/test/AppData/Local/DS GridDesign Canary/ds.exe");
        let canary = sibling_application(&canary_ds).expect("canary sibling layout");
        assert_eq!(
            canary,
            PathBuf::from(
                "C:/Users/test/AppData/Local/DS GridDesign Canary/DS GridDesign Canary.exe"
            )
        );
        assert_eq!(
            select_installed_desktop(Some((canary.clone(), true)), vec![stable]),
            ApplicationSelection::Selected(canary)
        );
    }

    #[test]
    fn fallback_refuses_true_ambiguity_and_never_crosses_a_missing_owned_sibling() {
        let stable = PathBuf::from("C:/Users/test/AppData/Local/DS GridDesign/DS GridDesign.exe");
        let canary = PathBuf::from(
            "C:/Users/test/AppData/Local/DS GridDesign Canary/DS GridDesign Canary.exe",
        );
        assert_eq!(
            select_installed_desktop(None, vec![stable.clone(), canary.clone()]),
            ApplicationSelection::Ambiguous(vec![stable.clone(), canary.clone()])
        );
        assert_eq!(
            select_installed_desktop(Some((stable.clone(), false)), vec![canary]),
            ApplicationSelection::MissingSibling(stable)
        );
    }
}
