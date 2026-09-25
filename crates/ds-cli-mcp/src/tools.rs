//! Descriptor → MCP tool, and MCP arguments → `ds` argv.
//!
//! Both directions are pure functions over the JSON `ds capabilities` emits,
//! so the mapping is testable without a process and the tool list can never
//! say something the CLI did not.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Effect, Execution, Refusal};
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
    /// Parsed from the live descriptor. It is the only input to the tool's
    /// annotations; see [`hints`].
    pub effect: Effect,
    /// A `job` command answers with a handle to poll, not with its result.
    pub execution: Execution,
    /// Whether the command still needs the paired application window.
    pub requires_window: bool,
    pub path: Vec<String>,
    pub description: String,
    pub input_schema: Value,
    /// Whether this command can require confirmation. Static MCP annotations
    /// stay conservative even when `confirmation_trigger` makes only one
    /// invocation shape effectful.
    pub confirmation_required: bool,
    /// The validated boolean switch name (without `--`) that selects the
    /// effectful path. Absent means every invocation needs confirmation.
    pub confirmation_trigger: Option<String>,
    /// The validated boolean switch name (without `--`) whose presence turns
    /// a writing command into a preview that writes nothing and so needs no
    /// confirmation: the descriptor's `preview_switch`, `--dry-run`.
    pub preview_switch: Option<String>,
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

/// Commands that are live CLI contracts but never MCP tools, with why. The
/// `mcp` domain itself is excluded as a whole beside these: a server that
/// lists "start a server" as a tool is a loop, not a capability.
///
/// Every other registered command becomes exactly one tool, generated from
/// its live descriptor, and `crates/ds/tests/mcp.rs` holds that as a fact of
/// the registry rather than of this list.
pub const NEVER_TOOLS: &[(&str, &str)] = &[
    (
        "auth.login",
        "a person at a trusted terminal types a password; no host may",
    ),
    (
        "auth.link.approve",
        "approving a device link is the signed-in Desktop's act; a host approving its own link would authorize itself",
    ),
    (
        "server.serve",
        "a foreground host never returns a tool response; operators start it through the launcher or service",
    ),
];

/// Input names `ds` reads as its own global flags wherever they appear. A
/// command that declared one could never receive it, and an MCP property of
/// that name would reach the global instead, so such a descriptor is refused
/// rather than projected. `version` is absent on purpose: `ds` routes it to
/// a command that declares it, and `yes` is handled beside the confirmation
/// gate in [`tool_from_descriptor`].
const GLOBAL_FLAG_NAMES: &[&str] = &["output", "pretty", "no-color", "help"];

/// The bound for the probes this server makes of its own executable —
/// `ds version`, `ds capabilities`, `ds desktop status` — which answer from
/// declarations or one loopback handshake.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

/// How often a caller waiting on a long tool call is told it is still
/// running. Hosts that asked for progress receive one notification per tick.
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);

/// The most standard output one `ds` child may produce before the capture
/// stops keeping it. Every command bounds its own answer far below this; the
/// bound exists so one defective answer cannot exhaust the server's memory.
pub const STDOUT_CAPTURE_LIMIT: usize = 32 * 1024 * 1024;

/// Standard error is diagnostic only. Its tail is kept for the rare answer
/// that produced no envelope.
const STDERR_CAPTURE_LIMIT: usize = 16 * 1024;

/// How long the capture waits for a child's pipes to close after the child
/// itself has ended. A grandchild that inherited a pipe must not hold a tool
/// call open after the command it served has returned.
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// What an MCP host may assume about one tool, derived from its effect class
/// and nothing else. Blast radius is never inferred from a command's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hints {
    /// Changes nothing outside the process.
    pub read_only: bool,
    /// May replace or remove what exists, rather than only add to it.
    pub destructive: bool,
    /// Repeating the same call has no further effect.
    pub idempotent: bool,
}

/// The one mapping from effect class to MCP annotations.
///
/// Where an effect class does not settle a question, the answer is the
/// conservative one: a writing class is destructive and not idempotent,
/// because some command in it replaces what exists and none promises
/// otherwise. `local_ui` changes only what the paired window shows, so it is
/// additive. `proposal` persists nothing but spends model credit on every
/// call, so it is read-only and still not idempotent.
pub const fn hints(effect: Effect) -> Hints {
    match effect {
        Effect::Discovery | Effect::ReadOnly => Hints {
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Effect::Proposal => Hints {
            read_only: true,
            destructive: false,
            idempotent: false,
        },
        Effect::LocalUi => Hints {
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        Effect::LocalAuthState
        | Effect::LocalFileWrite
        | Effect::ArtifactWrite
        | Effect::MachineWrite
        | Effect::GlobalWrite => Hints {
            read_only: false,
            destructive: true,
            idempotent: false,
        },
    }
}

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
///
/// `None` means the descriptor cannot be projected faithfully, and the server
/// refuses to start rather than publish a tool that says less than the CLI:
/// an unknown chapter, authority, effect or execution token; a confirmation
/// trigger or preview switch that names no declared switch; or an input whose
/// name `ds` would read as one of its own global flags.
pub fn tool_from_descriptor(command: &Value) -> Option<Tool> {
    // A tool's description is read by every host before any call, so it is
    // scrubbed of terminal sign-in advice at the source, like every answer.
    let mut scrubbed = command.clone();
    crate::surface::scrub_descriptor(&mut scrubbed);
    let command = &scrubbed;
    let id = command.get("id")?.as_str()?.to_string();
    let chapter = Chapter::from_token(command.get("chapter")?.as_str()?)?;
    let authority = Authority::from_token(command.get("authority")?.as_str()?)?;
    let effect = Effect::from_token(command.get("effect")?.as_str()?)?;
    // Both are always present in a live descriptor. Absent reads as the
    // default; present and unknown is refused like any other token.
    let execution = match command.get("execution") {
        None => Execution::Sync,
        Some(value) => Execution::from_token(value.as_str()?)?,
    };
    let requires_window = match command.get("requires") {
        None => false,
        Some(value) => match value.as_str()? {
            "server" => false,
            "window" => true,
            _ => return None,
        },
    };
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
    let declared_confirmation_trigger = optional_token(command, "confirmation_trigger")?;
    let declared_preview_switch = optional_token(command, "preview_switch")?;
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
        // `ds` strips these before a command sees its inputs, so a property
        // of that name would steer the executable rather than the command.
        if GLOBAL_FLAG_NAMES.contains(&name) {
            return None;
        }
        // A declared `--yes` switch is the global confirmation under another
        // name. It is a command's own gate only where the CLI's central gate
        // does not apply; on a gated command it would confirm without the
        // `confirm` property ever being passed.
        if name == "yes" && (kind != "switch" || confirmation_required) {
            return None;
        }
        // A command may own an input called `confirm`. It stays that
        // command's input, which is only unambiguous while the MCP
        // confirmation property does not need the same name.
        if name == CONFIRM_PROPERTY && confirmation_required {
            return None;
        }
        let summary = input.get("summary").and_then(Value::as_str).unwrap_or("");
        let value_hint = input.get("value").and_then(Value::as_str).unwrap_or("");
        let description = if value_hint.is_empty() {
            summary.to_string()
        } else {
            format!("{summary} ({value_hint})")
        };
        let choices = input
            .get("choices")
            .and_then(Value::as_array)
            .filter(|choices| !choices.is_empty());
        let default = input.get("default").filter(|default| !default.is_null());
        let mut schema = match kind.as_str() {
            "switch" => json!({ "type": "boolean" }),
            "repeated" => {
                // The closed set constrains each item. On the array itself it
                // would demand that the array equal one string, which no
                // array can, and a validating host would refuse every call.
                let mut items = json!({ "type": "string" });
                if let Some(choices) = choices {
                    items["enum"] = Value::Array(choices.clone());
                }
                json!({ "type": "array", "items": items })
            }
            _ => {
                let mut scalar = json!({ "type": "string" });
                if let Some(choices) = choices {
                    scalar["enum"] = Value::Array(choices.clone());
                }
                scalar
            }
        };
        if let Some(default) = default {
            schema["default"] = if kind == "repeated" {
                json!([default])
            } else {
                default.clone()
            };
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
    let declared_switch = |token: &str| -> Option<String> {
        let name = token.strip_prefix("--")?;
        (!name.is_empty()
            && inputs
                .iter()
                .any(|input| input.name == name && input.kind == "switch"))
        .then(|| name.to_string())
    };
    let confirmation_trigger = match declared_confirmation_trigger {
        None => None,
        Some(trigger) => {
            if !confirmation_required {
                return None;
            }
            Some(declared_switch(trigger)?)
        }
    };
    let preview_switch = match declared_preview_switch {
        None => None,
        Some(preview) => Some(declared_switch(preview)?),
    };
    if confirmation_required {
        let description = match (&confirmation_trigger, &preview_switch) {
            (Some(trigger), _) => format!(
                "Required only when `{trigger}` is true. Pass true only when the user's intent authorizes exactly that effect and scope (maps to `--yes`)."
            ),
            (None, Some(preview)) => format!(
                "Required unless `{preview}` is true, which previews and writes nothing. Pass true only when the user's intent authorizes exactly this effect and scope (maps to `--yes`)."
            ),
            (None, None) => "This command has an effect and the CLI requires confirmation. Pass true only when the user's intent authorizes exactly this effect and scope (maps to `--yes`).".to_string(),
        };
        properties.insert(
            CONFIRM_PROPERTY.to_string(),
            json!({
                "type": "boolean",
                "description": description,
            }),
        );
    }
    let description = describe(
        command,
        effect,
        authority,
        execution,
        requires_window,
        confirmation_required,
        confirmation_trigger.as_deref(),
        preview_switch.as_deref(),
    );
    Some(Tool {
        name: tool_name(&id),
        id,
        chapter,
        authority,
        effect,
        execution,
        requires_window,
        path,
        description,
        input_schema: json!({
            "type": "object",
            "properties": Value::Object(properties),
            "required": required,
            "additionalProperties": false,
        }),
        confirmation_required,
        confirmation_trigger,
        preview_switch,
        inputs,
        descriptor: command.clone(),
    })
}

/// An optional descriptor string: absent is `Some(None)`; present but not a
/// string is `None`, which refuses the whole descriptor.
fn optional_token<'a>(command: &'a Value, key: &str) -> Option<Option<&'a str>> {
    match command.get(key) {
        None => Some(None),
        Some(Value::String(value)) => Some(Some(value.as_str())),
        Some(_) => None,
    }
}

/// The tool description: the command's own words, then the facts a host
/// needs before choosing it — what it changes, who it needs, how to confirm
/// or preview it, whether it answers with a job, and how it declines.
#[allow(clippy::too_many_arguments)]
fn describe(
    command: &Value,
    effect: Effect,
    authority: Authority,
    execution: Execution,
    requires_window: bool,
    confirmation_required: bool,
    confirmation_trigger: Option<&str>,
    preview_switch: Option<&str>,
) -> String {
    let summary = command.get("summary").and_then(Value::as_str).unwrap_or("");
    let purpose = command.get("purpose").and_then(Value::as_str).unwrap_or("");
    let output = command.get("output").and_then(Value::as_str).unwrap_or("");
    let mut description = format!("{summary}\n\n{purpose}");
    if !output.is_empty() {
        description.push_str(&format!("\n\nReturns: {output}"));
    }
    description.push_str(&format!(
        "\n\nEffect: {} ({}). Authority: {} ({}).",
        effect.token(),
        effect.gloss(),
        authority.token(),
        authority.gloss()
    ));
    if confirmation_required {
        match (confirmation_trigger, preview_switch) {
            (Some(trigger), _) => description.push_str(&format!(
                " Requires `{CONFIRM_PROPERTY}: true` only when `{trigger}` is true."
            )),
            (None, Some(preview)) => description.push_str(&format!(
                " Requires `{CONFIRM_PROPERTY}: true`, except with `{preview}: true`, which previews and writes nothing."
            )),
            (None, None) => {
                description.push_str(&format!(" Requires `{CONFIRM_PROPERTY}: true`."));
            }
        }
    } else if let Some(preview) = preview_switch {
        description.push_str(&format!(" `{preview}: true` previews and writes nothing."));
    }
    if execution == Execution::Job {
        description.push_str(
            " Runs as a job: answers at once with a handle; poll the command the answer names.",
        );
    }
    if requires_window {
        description.push_str(" Needs the paired DS GridDesign window.");
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
    description
}

impl Tool {
    /// Whether this exact invocation shape requires confirmation.
    ///
    /// The same decision `Command::confirmation_required_for` makes in the
    /// CLI, read from the descriptor: a declared trigger switch decides
    /// alone; otherwise a set preview switch means nothing is written; and
    /// otherwise the effect class decides.
    pub fn confirmation_required_for(&self, arguments: &Value) -> Result<bool, String> {
        if !self.confirmation_required {
            return Ok(false);
        }
        let object = match arguments {
            Value::Null => None,
            Value::Object(object) => Some(object),
            _ => return Err("arguments must be an object".to_string()),
        };
        let switch = |name: &str| match object.and_then(|object| object.get(name)) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(false),
            Some(Value::Bool(true)) => Ok(true),
            Some(_) => Err(format!("`{name}` must be a boolean")),
        };
        if let Some(trigger) = &self.confirmation_trigger {
            return switch(trigger);
        }
        if let Some(preview) = &self.preview_switch
            && switch(preview)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether `confirm` names one of this command's own inputs rather than
    /// the MCP confirmation property. The two never coexist: a descriptor
    /// that needs both is refused by [`tool_from_descriptor`].
    pub fn owns_confirm_input(&self) -> bool {
        self.inputs
            .iter()
            .any(|input| input.name == CONFIRM_PROPERTY)
    }
}

/// What the enumeration says about this machine, reduced to what the gate
/// decides on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DesktopState {
    /// No live instance at all — the one state in which a launch is allowed.
    Absent,
    /// Live instances exist and more than one could serve this call. Never a
    /// launch, and never a guess: the caller names one.
    Ambiguous(Vec<String>),
    Paired {
        signed_in: bool,
        project_selected: bool,
    },
}

/// More than one live instance, and nothing but the caller may choose. The
/// code and the remedy are the CLI's own: an agent that meets this through a
/// tool call and an operator who meets it in a terminal are told the same
/// thing, and re-run with the same argument.
const AMBIGUOUS: Refusal = Refusal {
    code: "desktop_ambiguous",
    when: "more than one live DS GridDesign instance could serve this tool call",
    remedy: "call desktop_list, then pass target=desktop:<instance_id>",
};

/// Ensure exactly the authority the tool names, before dispatching an MCP
/// invocation. Discovery, `describe`, and every `Authority::None` tool bypass
/// this entirely.
pub fn ensure_desktop(tool: &Tool, arguments: &Value, executable: &PathBuf) -> Result<(), Failure> {
    if !tool.authority.requires_desktop() {
        return Ok(());
    }
    let descriptor = arguments
        .get("desktop-descriptor")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let target = arguments
        .get("target")
        .and_then(Value::as_str)
        .map(str::to_owned);
    // A caller that named a runtime has named it. `--target desktop` names the
    // host and not an instance, so it is not a naming for this purpose; an
    // instance, a descriptor path, or either of their session defaults is.
    let named_runtime = descriptor.is_some()
        || std::env::var_os("DS_DESKTOP_DESCRIPTOR").is_some_and(|value| !value.is_empty())
        || target.as_deref().is_some_and(|value| value != "desktop")
        || std::env::var("DS_TARGET")
            .is_ok_and(|value| !value.trim().is_empty() && value.trim() != "desktop");
    let mut status = || desktop_status(executable, descriptor.as_deref(), target.as_deref());
    let mut launch = || launch_installed_desktop(executable);
    let mut wait = || thread::sleep(PAIR_POLL_INTERVAL);
    ensure_desktop_with(
        tool.authority,
        named_runtime,
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
    named_runtime: bool,
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
        // Live instances exist. Starting another would add a third runtime to
        // a machine that already cannot say which of two a call is for, so the
        // gate refuses with the argument that settles it.
        DesktopState::Ambiguous(instances) => return Err(ambiguous(&instances)),
        DesktopState::Absent if named_runtime => {
            return Err(not_paired("the named runtime did not publish a session"));
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
            DesktopState::Ambiguous(instances) => return Err(ambiguous(&instances)),
            DesktopState::Absent => {}
        }
    }
    Err(not_paired(
        "desktop launch did not publish a paired session before the 10 second bound",
    ))
}

fn ambiguous(instances: &[String]) -> Failure {
    Failure::invalid(
        AMBIGUOUS.code,
        "more than one DS GridDesign instance is running on this machine",
    )
    .remedy(AMBIGUOUS.remedy)
    .detail(json!({ "instances": instances }))
    .next("ds desktop list")
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

/// What `ds desktop status` says, read as the gate's three states.
///
/// It is the enumeration's own answer: since instances replaced install
/// profiles as the unit of pairing, that command finds every live instance,
/// proves each one alive with an authenticated handshake, and either describes
/// the one this call is for or refuses to choose between several. So the gate
/// asks the CLI rather than repeating its discovery, which is the only way an
/// MCP tool and the same command in a terminal can answer identically.
fn desktop_status(
    executable: &PathBuf,
    descriptor: Option<&str>,
    target: Option<&str>,
) -> Result<DesktopState, Failure> {
    // Caller values travel inside their flag token, exactly as in
    // [`argv_for_call`], so neither can be read as a flag of `ds`.
    let mut argv = vec!["desktop".to_string(), "status".to_string()];
    if let Some(descriptor) = descriptor {
        argv.push(format!("--desktop-descriptor={descriptor}"));
    }
    if let Some(target) = target {
        argv.push(format!("--target={target}"));
    }
    argv.push("--output".to_string());
    argv.push("json".to_string());
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
        if envelope["error"]["code"].as_str() == Some(AMBIGUOUS.code) {
            return Ok(DesktopState::Ambiguous(
                envelope["error"]["detail"]["instances"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|instance| instance["instance_id"].as_str().map(str::to_owned))
                    .collect(),
            ));
        }
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
    // The MCP server's stdin/stdout ARE the JSON-RPC channel. A GUI child that
    // inherited them would hold the host's pipe open for its whole lifetime and
    // could write into the protocol stream; it gets no standard streams at all.
    Command::new(application)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
///
/// No caller value can become a flag. `ds` reads `--yes`, `--output`,
/// `--help`/`-h` and `--version` as its own wherever they stand, so a value
/// passed as a separate token could confirm, re-format or divert the call —
/// and a value that merely begins with `--`, such as a Markdown rule, would
/// be refused as a missing value. Every value therefore travels inside its
/// own token as `--name=value`, and operands follow the `--` sentinel, after
/// which `ds` reads nothing as a flag. `--yes` is emitted only for `confirm`.
pub fn argv_for_call(tool: &Tool, arguments: &Value) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = tool.path.clone();
    let object = match arguments {
        Value::Null => Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err("arguments must be an object".to_string()),
    };
    let confirm_is_input = tool.owns_confirm_input();
    // Unknown properties first, so the host's mistake is named before any
    // mapping happens.
    for key in object.keys() {
        if key == CONFIRM_PROPERTY && !confirm_is_input {
            if !tool.confirmation_required {
                return Err(format!(
                    "`{}` does not declare `{CONFIRM_PROPERTY}`",
                    tool.id
                ));
            }
            continue;
        }
        if !tool.inputs.iter().any(|input| &input.name == key) {
            return Err(format!("`{key}` is not an input of `{}`", tool.id));
        }
    }
    let confirmation_required = tool.confirmation_required_for(&Value::Object(object.clone()))?;
    let confirmed = if confirm_is_input {
        false
    } else {
        match object.get(CONFIRM_PROPERTY) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            Some(_) => return Err(format!("`{CONFIRM_PROPERTY}` must be a boolean")),
        }
    };
    if confirmed && !confirmation_required {
        return Err(match (&tool.confirmation_trigger, &tool.preview_switch) {
            (Some(trigger), _) => format!(
                "`{CONFIRM_PROPERTY}` is accepted only when `--{trigger}` is true for `{}`",
                tool.id
            ),
            (None, Some(preview)) => format!(
                "`{CONFIRM_PROPERTY}` is not accepted with `--{preview}`: a preview of `{}` writes nothing and needs no confirmation",
                tool.id
            ),
            (None, None) => format!(
                "`{CONFIRM_PROPERTY}` is accepted only when this invocation of `{}` requires confirmation",
                tool.id
            ),
        });
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
                    let item =
                        scalar(&item).ok_or_else(|| format!("`{key}` items must be scalars"))?;
                    argv.push(format!("--{key}={item}"));
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
                let value = scalar(value).ok_or_else(|| format!("`{key}` must be a scalar"))?;
                argv.push(format!("--{key}={value}"));
            }
        }
    }
    if confirmed {
        argv.push("--yes".to_string());
    }
    argv.push("--output".to_string());
    argv.push("json".to_string());
    if !positional.is_empty() {
        argv.push("--".to_string());
        argv.extend(positional);
    }
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

/// Run `ds <argv…>` and return (exit code, stdout, stderr), within the probe
/// bound. Used for the server's own questions of its executable.
pub fn run_cli(executable: &PathBuf, argv: &[String]) -> Result<(i32, String, String), String> {
    run_cli_with_schema_mode(executable, argv, false)
}

fn run_cli_with_schema_mode(
    executable: &PathBuf,
    argv: &[String],
    schema_only: bool,
) -> Result<(i32, String, String), String> {
    let ran = run_bounded(executable, argv, schema_only, PROBE_TIMEOUT, &mut |_| {})?;
    if ran.timed_out {
        return Err(format!(
            "`{}` did not answer within {} seconds",
            executable.display(),
            PROBE_TIMEOUT.as_secs()
        ));
    }
    if ran.overflowed {
        return Err(format!(
            "`{}` wrote more than {STDOUT_CAPTURE_LIMIT} bytes",
            executable.display()
        ));
    }
    Ok((ran.code, ran.stdout, ran.stderr))
}

/// What one bounded `ds` child did.
#[derive(Debug)]
pub struct Ran {
    pub code: i32,
    pub stdout: String,
    /// The tail of standard error, at most a few kilobytes.
    pub stderr: String,
    /// The child ran past its bound and was stopped.
    pub timed_out: bool,
    /// Standard output exceeded [`STDOUT_CAPTURE_LIMIT`]; what was kept is
    /// not a whole answer.
    pub overflowed: bool,
    pub elapsed: Duration,
}

/// Run one tool call's `ds` child within `timeout`, calling `tick` with the
/// elapsed time every [`PROGRESS_INTERVAL`] while it runs.
pub fn run_cli_bounded(
    executable: &PathBuf,
    argv: &[String],
    timeout: Duration,
    tick: &mut dyn FnMut(Duration),
) -> Result<Ran, String> {
    run_bounded(executable, argv, false, timeout, tick)
}

fn run_bounded(
    executable: &PathBuf,
    argv: &[String],
    schema_only: bool,
    timeout: Duration,
    tick: &mut dyn FnMut(Duration),
) -> Result<Ran, String> {
    let mut command = Command::new(executable);
    // Both names are deliberately protocol-free, and stay that way. What the
    // child has to know is that no human is at a terminal, and that a schema
    // is enough without resolving live availability. Neither fact is about
    // MCP. A variable named for this protocol would put this protocol's name
    // inside the domain that reads it, and the next protocol would arrive as
    // an edit to that domain rather than to this crate.
    // `crates/ds/tests/protocol_boundary.rs` holds that line.
    command
        .args(argv)
        .env("DS_CLI_NONINTERACTIVE", "1")
        .env_remove("DS_CLI_SCHEMA_ONLY");
    if schema_only {
        command.env("DS_CLI_SCHEMA_ONLY", "1");
    }
    // This server's own stdin is the JSON-RPC channel; the child gets none.
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start `{}`: {error}", executable.display()))?;
    let stdout = Capture::start(child.stdout.take(), STDOUT_CAPTURE_LIMIT, false);
    let stderr = Capture::start(child.stderr.take(), STDERR_CAPTURE_LIMIT, true);
    let started = Instant::now();
    let mut next_tick = PROGRESS_INTERVAL;
    let mut pause = Duration::from_millis(2);
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) => {}
            Err(error) => return Err(format!("could not observe `ds`: {error}")),
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            // The bound is the last resort, and only this child is stopped:
            // an owner engine it started may still be finishing, which the
            // timeout refusal says.
            let _ = child.kill();
            let _ = child.wait();
            break (None, true);
        }
        if elapsed >= next_tick {
            tick(elapsed);
            next_tick += PROGRESS_INTERVAL;
        }
        thread::sleep(pause.min(timeout - elapsed));
        pause = (pause * 2).min(Duration::from_millis(100));
    };
    let elapsed = started.elapsed();
    // One grace for both pipes, not one each.
    let grace = Instant::now() + PIPE_GRACE;
    let (stdout, overflowed) = stdout.finish(grace);
    let (stderr, _) = stderr.finish(grace);
    Ok(Ran {
        code: status.and_then(|status| status.code()).unwrap_or(1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out,
        overflowed,
        elapsed,
    })
}

/// One pipe drained on its own thread, so a child that fills one pipe while
/// the server waits on the other can never deadlock, and a child that writes
/// without end cannot grow the server without end.
struct Capture {
    kept: Arc<Mutex<(Vec<u8>, bool)>>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Capture {
    /// Keep at most `limit` bytes: the head of the stream, or with `tail`
    /// its end. Everything past the limit is still read and discarded.
    fn start<R: Read + Send + 'static>(pipe: Option<R>, limit: usize, tail: bool) -> Self {
        let kept = Arc::new(Mutex::new((Vec::new(), false)));
        let reader = pipe.map(|mut pipe| {
            let kept = Arc::clone(&kept);
            thread::spawn(move || {
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let read = match pipe.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => read,
                    };
                    let Ok(mut kept) = kept.lock() else {
                        break;
                    };
                    let (bytes, overflowed) = &mut *kept;
                    bytes.extend_from_slice(&buffer[..read]);
                    if bytes.len() > limit {
                        *overflowed = true;
                        if tail {
                            let excess = bytes.len() - limit;
                            bytes.drain(..excess);
                        } else {
                            bytes.truncate(limit);
                        }
                    }
                }
            })
        });
        Self { kept, reader }
    }

    /// What was kept once the pipe closed, or once `deadline` passed — a
    /// grandchild holding the pipe open after the child ended does not hold
    /// the call open with it.
    fn finish(mut self, deadline: Instant) -> (Vec<u8>, bool) {
        if let Some(reader) = self.reader.take() {
            while !reader.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
        match self.kept.lock() {
            Ok(mut kept) => (std::mem::take(&mut kept.0), kept.1),
            Err(_) => (Vec::new(), true),
        }
    }
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

/// Read this exact executable's doctor result. The install descriptor uses
/// only the verified skill-bundle source SHA from it; a missing or stale
/// user-level skill copy never prevents a host from launching MCP.
pub fn doctor_identity(executable: &PathBuf) -> Result<Value, Failure> {
    let argv = [
        "doctor".to_string(),
        "--output".to_string(),
        "json".to_string(),
    ];
    let (code, stdout, stderr) = run_cli(executable, &argv).map_err(|message| {
        Failure::failed("mcp_capabilities_unavailable", message)
            .remedy("run `ds doctor --output json` and repair this executable")
    })?;
    let envelope: Value = serde_json::from_str(&stdout).map_err(|error| {
        Failure::failed(
            "mcp_capabilities_unavailable",
            format!("`ds doctor` emitted no envelope ({error}): {stderr}"),
        )
        .remedy("run `ds doctor --output json` and repair this executable")
    })?;
    if code != 0 || envelope.get("status").and_then(Value::as_str) != Some("ok") {
        return Err(Failure::failed(
            "mcp_capabilities_unavailable",
            "`ds doctor --output json` refused",
        )
        .remedy("run `ds doctor --output json` and repair this executable")
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
/// from a table. The `mcp` domain itself and [`NEVER_TOOLS`] are excluded; a
/// command registered anywhere else becomes a tool with no edit here.
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
            // Live, discoverable CLI contracts that must never become MCP
            // tools — including under the broad compatibility exposure. Each
            // carries its reason in the list.
            if NEVER_TOOLS.iter().any(|(never, _)| *never == id) {
                continue;
            }
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

    fn conditional_descriptor() -> Value {
        json!({
            "id": "mcp.install",
            "chapter": "catalog",
            "path": ["mcp", "install"],
            "contract": 3,
            "summary": "Print or write an MCP host entry.",
            "purpose": "The preview is read-only and --write changes machine settings.",
            "output": "Connection receipt.",
            "effect": "machine_write",
            "authority": "none",
            "confirmation_required": true,
            "confirmation_trigger": "--write",
            "inputs": [
                { "name": "write", "kind": "switch", "required": false, "summary": "Write the host entry." }
            ],
            "refusals": []
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
                "--transformer=T-1",
                "--layer=lv_poles",
                "--layer=customers",
                "--dry-run",
                "--yes",
                "--output",
                "json",
            ]
        );
    }

    /// `ds` reads `--yes`, `--output`, `--help`, `-h` and `--version` as its
    /// own wherever they stand. A caller's value must never be one of those
    /// tokens, and a value that merely begins with `--` — a Markdown rule, a
    /// YAML fence — must still arrive as that value.
    #[test]
    fn no_caller_value_can_become_a_flag_of_ds() {
        let tool = tool_from_descriptor(&descriptor()).expect("tool");
        for hostile in [
            "--yes",
            "--output",
            "human",
            "-h",
            "--help",
            "--version",
            "--- a rule",
        ] {
            let argv = argv_for_call(
                &tool,
                &json!({ "transformer": hostile, "layer": [hostile, "x"], "format": null }),
            )
            .expect("argv");
            let bare = &argv[3..argv.len() - 2];
            assert!(
                bare.iter()
                    .all(|token| token.starts_with("--transformer=")
                        || token.starts_with("--layer=")),
                "a caller value escaped its flag: {argv:?}"
            );
            assert!(
                !argv.iter().any(|token| token == "--yes"),
                "`{hostile}` confirmed the call without `confirm`: {argv:?}"
            );
            assert!(argv.contains(&format!("--transformer={hostile}")));
            assert!(argv.contains(&format!("--layer={hostile}")));
            assert_eq!(argv[argv.len() - 2..], ["--output", "json"]);
        }
    }

    #[test]
    fn operands_follow_the_sentinel_after_every_flag() {
        let mut descriptor = descriptor();
        descriptor["inputs"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "subject", "kind": "positional", "required": false, "summary": "The subject.", "value": "<s>" }));
        let tool = tool_from_descriptor(&descriptor).expect("tool");
        let argv = argv_for_call(
            &tool,
            &json!({ "transformer": "T-1", "subject": "--yes", "confirm": true }),
        )
        .expect("argv");
        assert_eq!(
            argv,
            [
                "map",
                "design",
                "report",
                "--transformer=T-1",
                "--yes",
                "--output",
                "json",
                "--",
                "--yes",
            ],
            "the operand `--yes` is an operand; only `confirm` produced the flag"
        );
    }

    /// A writing command whose descriptor names `--dry-run` as its preview
    /// switch needs no confirmation to preview — the same decision the CLI's
    /// gate makes — and a `confirm` sent with a preview is refused rather
    /// than forwarded as `--yes`.
    #[test]
    fn a_declared_preview_switch_is_the_one_unconfirmed_path_of_a_write() {
        let mut descriptor = descriptor();
        descriptor["effect"] = json!("global_write");
        descriptor["preview_switch"] = json!("--dry-run");
        let tool = tool_from_descriptor(&descriptor).expect("tool");
        assert_eq!(tool.preview_switch.as_deref(), Some("dry-run"));
        let preview = json!({ "transformer": "T-1", "dry-run": true });
        assert!(!tool.confirmation_required_for(&preview).unwrap());
        assert!(
            tool.confirmation_required_for(&json!({ "transformer": "T-1" }))
                .unwrap()
        );
        assert_eq!(
            argv_for_call(&tool, &preview).unwrap(),
            [
                "map",
                "design",
                "report",
                "--transformer=T-1",
                "--dry-run",
                "--output",
                "json"
            ]
        );
        let confirmed_preview = argv_for_call(
            &tool,
            &json!({ "transformer": "T-1", "dry-run": true, "confirm": true }),
        )
        .unwrap_err();
        assert!(
            confirmed_preview.contains("not accepted with `--dry-run`"),
            "{confirmed_preview}"
        );
        assert!(
            tool.input_schema["properties"][CONFIRM_PROPERTY]["description"]
                .as_str()
                .unwrap()
                .contains("Required unless `dry-run` is true")
        );
        assert!(tool.description.contains("except with `dry-run: true`"));

        let mut malformed = descriptor.clone();
        malformed["preview_switch"] = json!("--format");
        assert!(
            tool_from_descriptor(&malformed).is_none(),
            "a preview switch must name a declared switch"
        );
    }

    #[test]
    fn repeated_choices_constrain_each_item_and_defaults_keep_their_type() {
        let mut descriptor = descriptor();
        descriptor["inputs"].as_array_mut().unwrap().push(json!({
            "name": "include", "kind": "repeated", "required": false,
            "summary": "Sections.", "value": "<section>",
            "choices": ["spans", "sections"], "default": "spans"
        }));
        let tool = tool_from_descriptor(&descriptor).expect("tool");
        let include = &tool.input_schema["properties"]["include"];
        assert_eq!(include["type"], "array");
        assert!(
            include.get("enum").is_none(),
            "an enum on the array admits no array: {include}"
        );
        assert_eq!(include["items"]["enum"], json!(["spans", "sections"]));
        assert_eq!(include["default"], json!(["spans"]));
        let format = &tool.input_schema["properties"]["format"];
        assert_eq!(format["type"], "string");
        assert_eq!(format["enum"], json!(["xlsx", "shp"]));
    }

    #[test]
    fn inputs_named_for_globals_or_the_gate_fail_closed() {
        for name in ["output", "pretty", "no-color", "help"] {
            let mut descriptor = descriptor();
            descriptor["inputs"].as_array_mut().unwrap().push(
                json!({ "name": name, "kind": "value", "required": false, "summary": "x", "value": "<x>" }),
            );
            assert!(tool_from_descriptor(&descriptor).is_none(), "`{name}`");
        }
        // A gated command declaring `--yes` would confirm through an input.
        let mut gated_yes = descriptor();
        gated_yes["inputs"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "yes", "kind": "switch", "required": false, "summary": "x" }));
        assert!(tool_from_descriptor(&gated_yes).is_none());
        // The typed-mutation idiom — an ungated command whose own `--yes`
        // writes its revision — is that command's declared input.
        let mut own_yes = gated_yes.clone();
        own_yes["effect"] = json!("local_file_write");
        own_yes["confirmation_required"] = json!(false);
        let tool = tool_from_descriptor(&own_yes).expect("ungated own --yes");
        assert_eq!(tool.input_schema["properties"]["yes"]["type"], "boolean");
        assert!(
            !tool.input_schema["properties"]
                .as_object()
                .unwrap()
                .contains_key(CONFIRM_PROPERTY)
        );
        // A gated command cannot also own an input named `confirm`.
        let mut gated_confirm = descriptor();
        gated_confirm["inputs"].as_array_mut().unwrap().push(
            json!({ "name": "confirm", "kind": "value", "required": false, "summary": "x", "value": "<code>" }),
        );
        assert!(tool_from_descriptor(&gated_confirm).is_none());
    }

    /// `design.force-gate.check` owns a value input called `confirm`. It is
    /// that command's input — a string, sent as `--confirm=<code>` — and
    /// never the MCP confirmation, which the command does not need.
    #[test]
    fn a_command_owned_confirm_input_is_an_ordinary_input() {
        let tool = tool_from_descriptor(&json!({
            "id": "design.force-gate.check", "chapter": "design",
            "path": ["design", "force-gate", "check"],
            "summary": "s", "purpose": "p", "output": "o",
            "effect": "read_only", "authority": "none", "confirmation_required": false,
            "inputs": [{ "name": "confirm", "kind": "value", "required": false, "summary": "The operator's confirmation.", "value": "<code>" }],
            "refusals": []
        }))
        .expect("tool");
        assert!(tool.owns_confirm_input());
        assert_eq!(
            tool.input_schema["properties"][CONFIRM_PROPERTY]["type"],
            "string"
        );
        assert_eq!(
            argv_for_call(&tool, &json!({ "confirm": "ABC-123" })).unwrap(),
            [
                "design",
                "force-gate",
                "check",
                "--confirm=ABC-123",
                "--output",
                "json"
            ]
        );
        assert!(argv_for_call(&tool, &json!({ "confirm": true })).is_ok());
    }

    #[test]
    fn annotations_follow_the_effect_class_and_nothing_else() {
        let cases = [
            (Effect::Discovery, true, false, true),
            (Effect::ReadOnly, true, false, true),
            (Effect::Proposal, true, false, false),
            (Effect::LocalUi, false, false, false),
            (Effect::LocalAuthState, false, true, false),
            (Effect::LocalFileWrite, false, true, false),
            (Effect::ArtifactWrite, false, true, false),
            (Effect::MachineWrite, false, true, false),
            (Effect::GlobalWrite, false, true, false),
        ];
        assert_eq!(
            cases.len(),
            Effect::ALL.len(),
            "every effect class is mapped"
        );
        for (effect, read_only, destructive, idempotent) in cases {
            assert_eq!(
                hints(effect),
                Hints {
                    read_only,
                    destructive,
                    idempotent
                },
                "{effect}"
            );
            // The CLI's own gate and the host hint agree: nothing a host may
            // treat as read-only ever needs `--yes`.
            if read_only {
                assert!(!effect.needs_confirmation(), "{effect}");
            }
            if effect.needs_confirmation() {
                assert!(destructive, "{effect}");
            }
        }
    }

    #[test]
    fn descriptions_say_what_a_host_needs_before_choosing() {
        let mut descriptor = descriptor();
        descriptor["execution"] = json!("job");
        descriptor["requires"] = json!("window");
        let tool = tool_from_descriptor(&descriptor).expect("tool");
        assert_eq!(tool.execution, Execution::Job);
        assert!(tool.requires_window);
        for expected in [
            "Export one transformer's report locally.",
            "Returns: Artifact evidence.",
            "Effect: artifact_write (produces a durable artifact of record).",
            "Authority: project (signed in, with a project selected).",
            "Requires `confirm: true`.",
            "Runs as a job",
            "Needs the paired DS GridDesign window.",
            "`desktop_not_paired` — no DS GridDesign session is running",
        ] {
            assert!(
                tool.description.contains(expected),
                "missing `{expected}`:\n{}",
                tool.description
            );
        }
        let mut unknown = descriptor.clone();
        unknown["execution"] = json!("later");
        assert!(tool_from_descriptor(&unknown).is_none());
        let mut unknown = descriptor;
        unknown["effect"] = json!("write");
        assert!(tool_from_descriptor(&unknown).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_its_bound_is_stopped_and_reported() {
        let sleeper = PathBuf::from("/bin/sleep");
        let mut ticks = 0usize;
        let ran = run_bounded(
            &sleeper,
            &["5".to_string()],
            false,
            Duration::from_millis(300),
            &mut |_| ticks += 1,
        )
        .expect("sleep runs");
        assert!(ran.timed_out);
        assert!(ran.elapsed < Duration::from_secs(4), "{:?}", ran.elapsed);
        assert_eq!(ticks, 0, "a 300 ms call is not reported as long-running");
    }

    #[cfg(unix)]
    #[test]
    fn capture_keeps_a_bounded_head_and_reports_the_overflow() {
        let yes = PathBuf::from("/usr/bin/head");
        let ran = run_bounded(
            &yes,
            &[
                "-c".to_string(),
                (STDOUT_CAPTURE_LIMIT + 10).to_string(),
                "/dev/zero".to_string(),
            ],
            false,
            Duration::from_secs(60),
            &mut |_| {},
        )
        .expect("head runs");
        assert!(!ran.timed_out);
        assert!(ran.overflowed);
        assert_eq!(ran.stdout.len(), STDOUT_CAPTURE_LIMIT);
    }

    #[test]
    fn conditional_confirmation_preserves_preview_and_write_argv() {
        let tool = tool_from_descriptor(&conditional_descriptor()).expect("tool");
        assert_eq!(tool.confirmation_trigger.as_deref(), Some("write"));
        assert!(tool.confirmation_required, "annotations stay conservative");

        let preview = argv_for_call(&tool, &json!({})).unwrap();
        assert_eq!(preview, ["mcp", "install", "--output", "json"]);
        assert!(!tool.confirmation_required_for(&json!({})).unwrap());

        let unconfirmed_write = argv_for_call(&tool, &json!({ "write": true })).unwrap();
        assert_eq!(
            unconfirmed_write,
            ["mcp", "install", "--write", "--output", "json"]
        );
        assert!(
            tool.confirmation_required_for(&json!({ "write": true }))
                .unwrap(),
            "MCP preflight must route this to the CLI confirmation refusal before an owner runs"
        );

        let confirmed_write =
            argv_for_call(&tool, &json!({ "write": true, "confirm": true })).unwrap();
        assert_eq!(
            confirmed_write,
            ["mcp", "install", "--write", "--yes", "--output", "json"]
        );
        let misplaced = argv_for_call(&tool, &json!({ "confirm": true })).unwrap_err();
        assert!(
            misplaced.contains("only when `--write` is true"),
            "{misplaced}"
        );
    }

    #[test]
    fn malformed_confirmation_triggers_fail_closed() {
        for malformed in [json!("write"), json!("--missing"), json!(7), Value::Null] {
            let mut descriptor = conditional_descriptor();
            descriptor["confirmation_trigger"] = malformed;
            assert!(
                tool_from_descriptor(&descriptor).is_none(),
                "malformed trigger was accepted: {}",
                descriptor["confirmation_trigger"]
            );
        }

        let mut wrong_kind = conditional_descriptor();
        wrong_kind["inputs"][0]["kind"] = json!("value");
        assert!(tool_from_descriptor(&wrong_kind).is_none());

        let mut not_effectful = conditional_descriptor();
        not_effectful["confirmation_required"] = json!(false);
        assert!(tool_from_descriptor(&not_effectful).is_none());
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

    /// Two windows are open and an agent calls a paired tool. Launching a
    /// third is the one thing that must not happen: the machine already cannot
    /// say which instance the call is for, and a new one would not answer that
    /// question either.
    #[test]
    fn two_live_instances_refuse_a_tool_call_instead_of_launching_a_third() {
        let instances = vec![
            "11111111111111111111111111111111".to_owned(),
            "22222222222222222222222222222222".to_owned(),
        ];
        let mut launched = 0usize;
        let failure = ensure_desktop_with(
            Authority::Project,
            false,
            &mut || Ok(DesktopState::Ambiguous(instances.clone())),
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || panic!("a live machine must not enter launch polling"),
        )
        .expect_err("nothing but the caller may choose between two instances");
        assert_eq!(failure.code(), "desktop_ambiguous");
        assert_eq!(launched, 0);
        assert_eq!(
            failure.detail_value().expect("the choices")["instances"],
            json!(instances)
        );
        assert!(
            failure
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("target=desktop:<instance_id>")),
            "the remedy must be the argument that settles it: {:?}",
            failure.remedy_text()
        );
    }

    /// The same refusal after a launch this call did make: an instance that
    /// starts beside one the gate could not see is still an ambiguity, and the
    /// poll stops rather than waiting out its bound.
    #[test]
    fn an_ambiguity_that_appears_after_a_launch_stops_the_poll() {
        let mut observations = 0usize;
        let mut launched = 0usize;
        let failure = ensure_desktop_with(
            Authority::DesktopPairing,
            false,
            &mut || {
                observations += 1;
                Ok(if observations > 1 {
                    DesktopState::Ambiguous(vec!["11111111111111111111111111111111".to_owned()])
                } else {
                    DesktopState::Absent
                })
            },
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || {},
        )
        .expect_err("an ambiguity is answered, not waited out");
        assert_eq!(failure.code(), "desktop_ambiguous");
        assert_eq!((launched, observations), (1, 2));
    }

    /// A tool call that named a runtime — an instance through `target`, or a
    /// descriptor path — is answered about that runtime. It is never quietly
    /// replaced by a launch of some other one.
    #[test]
    fn a_named_instance_is_never_replaced_by_an_automatic_launch() {
        let mut launched = 0usize;
        let failure = ensure_desktop_with(
            Authority::DesktopUser,
            true,
            &mut || Ok(DesktopState::Absent),
            &mut || {
                launched += 1;
                Ok(())
            },
            &mut || panic!("a named runtime must not enter launch polling"),
        )
        .expect_err("the named runtime is not live");
        assert_eq!(failure.code(), "desktop_not_paired");
        assert_eq!(launched, 0);
        assert!(
            failure
                .detail_value()
                .and_then(|detail| detail["mcp_desktop_gate"].as_str())
                .is_some_and(|detail| detail.contains("named runtime")),
            "the refusal says which side of the gate answered: {:?}",
            failure.detail_value()
        );
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
