//! `ds mcp install` — the host entry that launches `ds mcp serve`.
//!
//! Every MCP host reads the same shape: a named stdio server with a command
//! and arguments. What differs is the file it lives in. This command prints
//! the entry for this executable and, with `--write`, merges it into the
//! host's user-level file (never a workspace file: the server must run on the
//! machine that has DS GridDesign, and a workspace file travels).
//!
//! That target fixes the effect class. A user-level host configuration is not
//! "a file in your workspace" — it changes how every agent session on this
//! machine starts. The command remains `machine_write`; its declared
//! `--write` switch is the centralized confirmation trigger, while the blind
//! proposal path is read-only.

use std::collections::BTreeMap;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};
use toml_edit::{Array, DocumentMut, Item, Table, value};

pub const HOSTS: &[&str] = &[
    "vscode",
    "claude-code",
    "claude-desktop",
    "codex",
    "cursor",
    "gemini-cli",
    "windsurf",
    "github-copilot",
    "generic",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    const fn token(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }

    const fn current() -> Self {
        #[cfg(target_os = "windows")]
        return Self::Windows;
        #[cfg(target_os = "macos")]
        return Self::Macos;
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        Self::Linux
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigRoot {
    Servers,
    McpServers,
    McpServersSnake,
}

impl ConfigRoot {
    const fn token(self) -> &'static str {
        match self {
            Self::Servers => "servers",
            Self::McpServers => "mcpServers",
            Self::McpServersSnake => "mcp_servers",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HostAdapter {
    token: &'static str,
    display_name: &'static str,
    platforms: &'static [Platform],
    root: ConfigRoot,
    automatic_merge: bool,
    restart_requirement: &'static str,
}

const ALL_PLATFORMS: &[Platform] = &[Platform::Windows, Platform::Macos, Platform::Linux];
const WINDOWS_ONLY: &[Platform] = &[Platform::Windows];

const ADAPTERS: &[HostAdapter] = &[
    HostAdapter {
        token: "vscode",
        display_name: "Visual Studio Code",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::Servers,
        automatic_merge: true,
        restart_requirement: "restart VS Code or start a new agent session",
    },
    HostAdapter {
        token: "claude-code",
        display_name: "Claude Code",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServers,
        automatic_merge: true,
        restart_requirement: "restart Claude Code",
    },
    HostAdapter {
        token: "claude-desktop",
        display_name: "Claude Desktop",
        platforms: WINDOWS_ONLY,
        root: ConfigRoot::McpServers,
        automatic_merge: true,
        restart_requirement: "restart Claude Desktop",
    },
    HostAdapter {
        token: "codex",
        display_name: "Codex",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServersSnake,
        automatic_merge: true,
        restart_requirement: "fully quit and restart Codex, then start a new agent session so it reloads the `ds` MCP registration",
    },
    HostAdapter {
        token: "cursor",
        display_name: "Cursor",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServers,
        automatic_merge: true,
        restart_requirement: "restart Cursor or start a new agent session",
    },
    HostAdapter {
        token: "gemini-cli",
        display_name: "Gemini CLI",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServers,
        automatic_merge: true,
        restart_requirement: "restart Gemini CLI",
    },
    HostAdapter {
        token: "windsurf",
        display_name: "Windsurf",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServers,
        automatic_merge: true,
        restart_requirement: "restart Windsurf or start a new agent session",
    },
    HostAdapter {
        token: "github-copilot",
        display_name: "GitHub Copilot CLI",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServers,
        automatic_merge: true,
        restart_requirement: "restart GitHub Copilot CLI",
    },
    HostAdapter {
        token: "generic",
        display_name: "Generic MCP client",
        platforms: ALL_PLATFORMS,
        root: ConfigRoot::McpServers,
        automatic_merge: false,
        restart_requirement: "follow the MCP client's restart requirements",
    },
];

#[derive(Debug, Clone)]
struct ConnectionDescriptor {
    executable: PathBuf,
    args: Vec<String>,
    exposure: crate::surface::Exposure,
    profile: Option<crate::surface::Profile>,
    build: Value,
    skill_bundle_source_sha: Option<String>,
    required_environment: BTreeMap<String, String>,
}

impl ConnectionDescriptor {
    fn json(&self) -> Value {
        json!({
            "server_name": "ds",
            "transport": "stdio",
            "executable": self.executable.display().to_string(),
            "args": self.args,
            "exposure": self.exposure.token(),
            "profile": self.profile.map(crate::surface::Profile::token),
            "build": self.build,
            "skill_bundle_source_sha": self.skill_bundle_source_sha,
            "required_environment": self.required_environment,
        })
    }
}

pub static COMMAND: Command = Command {
    id: "mcp.install",
    path: &["mcp", "install"],
    contract: 5,
    chapter: ds_cli_contract::spec::Chapter::Catalog,
    summary: "Print or write an MCP host entry for this `ds`.",
    purpose: "\
Print this executable's exact stdio entry and user-level target. The default \
is read-only. `--write --yes` atomically merges only `ds`, preserves siblings, \
and refuses a conflicting entry. Workspace configuration is never written.",
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg {
            name: "host",
            kind: ArgKind::Value,
            value: "<host>",
            required: false,
            default: Some("vscode"),
            choices: &[],
            summary: "Target one host listed by the read-only JSON proposal.",
        },
        Arg {
            name: "write",
            kind: ArgKind::Switch,
            value: "",
            required: false,
            default: None,
            choices: &[],
            summary: "Merge the entry into the host's user-level file instead of only printing it.",
        },
        Arg {
            name: "exposure",
            kind: ArgKind::Value,
            value: "<chapters|commands>",
            required: false,
            default: Some("chapters"),
            choices: crate::surface::EXPOSURES,
            summary: "Use chapter routers or typed command tools.",
        },
        Arg {
            name: "profile",
            kind: ArgKind::Value,
            value: "<name>",
            required: false,
            default: None,
            choices: crate::surface::PROFILE_IDS,
            summary: "Choose a typed profile.",
        },
    ],
    output: "Connection descriptor, supported hosts, selected entry/target, build identity, change state and restart handoff.",
    examples: &[
        Example {
            command: "ds mcp install --output json",
            note: "Print supported hosts and the default VS Code proposal.",
            runnable: true,
        },
        Example {
            command: "ds mcp install --host vscode --write --yes",
            note: "Merge `ds` into the VS Code user mcp.json.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::HOST_UNKNOWN,
        crate::HOST_OS_MISMATCH,
        crate::HOST_WRITE_UNSUPPORTED,
        crate::CONFIG_UNWRITABLE,
        crate::CONFIG_CONFLICT,
        crate::PROFILE_EXPOSURE_INVALID,
        crate::CAPABILITIES_UNAVAILABLE,
        Refusal {
            code: "confirmation_required",
            when: "--write was requested without --yes",
            remedy: "inspect the proposal, then re-run with --write --yes",
        },
    ],
    reference: Some("docs/reference/mcp.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let host = inputs.value("host").unwrap_or("vscode");
    let adapter = adapter(host).ok_or_else(|| unknown_host(host))?;
    let executable = std::env::current_exe().map_err(|error| {
        Failure::failed("mcp_config_unwritable", format!("could not resolve this executable's path: {error}"))
            .remedy("read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand")
    })?;
    if !executable.is_absolute() {
        return Err(Failure::failed(
            "mcp_config_unwritable",
            "the running ds executable did not resolve to an absolute path",
        )
        .remedy("run the installed ds by its absolute path, then retry MCP installation"));
    }
    let build = crate::tools::build_identity(&executable)?;
    let doctor = crate::tools::doctor_identity(&executable)?;
    let exposure =
        crate::surface::Exposure::from_token(inputs.value("exposure").unwrap_or("chapters"))
            .expect("the command parser enforces exposure choices");
    let profile = inputs.value("profile").map(|value| {
        crate::surface::Profile::from_token(value)
            .expect("the command parser enforces profile choices")
    });
    if profile.is_some() && exposure != crate::surface::Exposure::Commands {
        return Err(Failure::invalid(
            "mcp_profile_exposure_invalid",
            "specialized profiles publish typed command tools and require `--exposure commands`",
        )
        .remedy("pass `--exposure commands --profile <name>`, or omit `--profile`"));
    }
    let descriptor = connection_descriptor(
        executable,
        build,
        doctor["skills"]["source_sha"].as_str().map(str::to_owned),
        exposure,
        profile,
    );
    validate_host_platform(adapter, &descriptor)?;
    let entry = server_entry(adapter, &descriptor);
    let file = config_file(adapter);
    if adapter.automatic_merge && file.is_none() {
        return Err(Failure::failed(
            "mcp_config_unwritable",
            format!("host `{host}` has no resolvable user-level configuration target"),
        )
        .remedy(
            "restore the platform's HOME/USERPROFILE/APPDATA environment and retry from the host machine",
        ));
    }
    if let Some(path) = &file
        && !path.is_absolute()
    {
        return Err(Failure::failed(
            "mcp_config_unwritable",
            format!(
                "host `{host}` resolved a non-absolute user-level target: {}",
                path.display()
            ),
        )
        .remedy(
            "restore an absolute HOME/USERPROFILE/APPDATA value and retry from the host machine",
        ));
    }
    let writing = inputs.switch("write");
    let (written, change, changed) = if writing {
        if !adapter.automatic_merge {
            return Err(Failure::invalid(
                "mcp_host_write_unsupported",
                format!(
                    "host `{host}` has no verified automatic merge; the proposed entry was not written"
                ),
            )
            .remedy(format!(
                "copy only the `ds` entry under `{}` in the reported user-level configuration",
                adapter.root.token()
            ))
            .next(format!("ds mcp install --host {host} --output json"))
            .detail(json!({ "host": host, "supported_hosts": HOSTS })));
        }
        let Some(path) = file.as_ref() else {
            return Err(Failure::failed(
                "mcp_config_unwritable",
                format!("host `{host}` has no resolvable user-level configuration path"),
            )
            .remedy(
                "restore the platform's user profile environment and retry from that host machine",
            ));
        };
        if host == "codex" {
            let change = merge_codex_into_file(path, &entry)?;
            let token = change.token(true);
            let changed = change.changed();
            (true, token, changed)
        } else if guarded_json_host(host) {
            let change = merge_guarded_json_into_file(path, host, &entry)?;
            let token = change.token(true);
            let changed = change.changed();
            (true, token, changed)
        } else {
            merge_into_file(path, host, &entry)?;
            (true, "merged", true)
        }
    } else if host == "codex" || guarded_json_host(host) {
        let Some(path) = file.as_ref() else {
            return Err(Failure::failed(
                "mcp_config_unwritable",
                format!("host `{host}` has no resolvable user-level configuration path"),
            )
            .remedy("restore an absolute HOME or USERPROFILE and retry"));
        };
        let change = if host == "codex" {
            plan_codex_file(path, &entry)?
        } else {
            plan_guarded_json_file(path, host, &entry)?
        };
        (false, change.token(false), false)
    } else {
        (false, "planned", false)
    };
    let restart_required = written && changed;
    let restart_handoff = if restart_required {
        adapter.restart_requirement.to_string()
    } else if written {
        "no restart is required because the installed entry already exactly matched".to_string()
    } else {
        format!("after writing, {}", adapter.restart_requirement)
    };
    let descriptor_json = descriptor.json();
    let file_path = file.as_ref().map(|path| path.display().to_string());
    Ok(json!({
        "host": host,
        "server_name": "ds",
        "entry": entry,
        "file": file_path,
        "path": file_path,
        "written": written,
        "changed": changed,
        "change": change,
        "restart_required": restart_required,
        "restart_handoff": restart_handoff,
        "executable": descriptor.executable.display().to_string(),
        "source_sha": descriptor.build["source_sha"],
        "dirty": descriptor.build["dirty"],
        "exposure": exposure.token(),
        "profile": profile.map(crate::surface::Profile::token),
        "transport": "stdio",
        "build": descriptor.build,
        "skill_bundle_source_sha": descriptor.skill_bundle_source_sha,
        "required_environment": descriptor.required_environment,
        "connection": descriptor_json,
        "supported_hosts": supported_hosts(),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!("host: {}\n", data["host"].as_str().unwrap_or("?")));
    if let Some(file) = data["file"].as_str() {
        out.push_str(&format!(
            "file: {file}{}\n",
            if data["written"].as_bool().unwrap_or(false) {
                "  (written)"
            } else {
                ""
            }
        ));
    }
    if let Some(change) = data["change"].as_str() {
        out.push_str(&format!("change: {change}\n"));
    }
    if let Some(handoff) = data["restart_handoff"].as_str() {
        out.push_str(&format!("restart: {handoff}\n"));
    }
    out.push_str("entry:\n");
    out.push_str(&serde_json::to_string_pretty(&data["entry"]).unwrap_or_default());
    out.push('\n');
    out
}

fn connection_descriptor(
    executable: PathBuf,
    build: Value,
    skill_bundle_source_sha: Option<String>,
    exposure: crate::surface::Exposure,
    profile: Option<crate::surface::Profile>,
) -> ConnectionDescriptor {
    let mut args = vec![
        "mcp".to_string(),
        "serve".to_string(),
        "--exposure".to_string(),
        exposure.token().to_string(),
    ];
    if let Some(profile) = profile {
        args.extend(["--profile".to_string(), profile.token().to_string()]);
    }
    ConnectionDescriptor {
        executable,
        args,
        exposure,
        profile,
        build,
        skill_bundle_source_sha,
        required_environment: BTreeMap::new(),
    }
}

/// Transform the one canonical descriptor into a host's verified dialect.
fn server_entry(adapter: &HostAdapter, descriptor: &ConnectionDescriptor) -> Value {
    let command = descriptor.executable.display().to_string();
    let server = if adapter.token == "github-copilot" {
        json!({
            "type": "local",
            "command": command,
            "args": descriptor.args,
            "env": {},
            "tools": ["*"],
        })
    } else if adapter.root == ConfigRoot::Servers {
        json!({ "type": "stdio", "command": command, "args": descriptor.args })
    } else {
        json!({ "command": command, "args": descriptor.args })
    };
    json!({ adapter.root.token(): { "ds": server } })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexServerEntry {
    command: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegistrationChange {
    Create(Vec<u8>),
    Merge(Vec<u8>),
    Unchanged,
}

impl RegistrationChange {
    const fn token(&self, writing: bool) -> &'static str {
        match (self, writing) {
            (Self::Create(_), false) => "would_create",
            (Self::Merge(_), false) => "would_merge",
            (Self::Create(_), true) => "created",
            (Self::Merge(_), true) => "merged",
            (Self::Unchanged, _) => "unchanged",
        }
    }

    const fn changed(&self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Create(bytes) | Self::Merge(bytes) => Some(bytes),
            Self::Unchanged => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CodexTomlIssue {
    Malformed(String),
    Conflict { existing: String, proposed: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonRegistrationIssue {
    Malformed(String),
    Conflict { existing: String, proposed: String },
}

fn merge_guarded_json(
    existing: Option<&[u8]>,
    entry: &Value,
) -> Result<RegistrationChange, JsonRegistrationIssue> {
    let document: Value = match existing {
        None => json!({}),
        Some(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => json!({}),
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| JsonRegistrationIssue::Malformed(error.to_string()))?,
    };
    let root = document
        .as_object()
        .ok_or_else(|| JsonRegistrationIssue::Malformed("root is not an object".to_string()))?;
    let entry_root = entry
        .as_object()
        .filter(|root| root.len() == 1)
        .ok_or_else(|| {
            JsonRegistrationIssue::Malformed(
                "proposal has more than one configuration root".to_string(),
            )
        })?;
    let (section, proposed_servers) = entry_root.iter().next().expect("one proposal root");
    let proposed = proposed_servers
        .get("ds")
        .ok_or_else(|| JsonRegistrationIssue::Malformed("proposal has no ds server".to_string()))?;
    if let Some(existing_root) = root.get(section) {
        let existing_servers =
            existing_root
                .as_object()
                .ok_or_else(|| JsonRegistrationIssue::Conflict {
                    existing: existing_root.to_string(),
                    proposed: proposed.to_string(),
                })?;
        if let Some(current) = existing_servers.get("ds") {
            return if current == proposed {
                Ok(RegistrationChange::Unchanged)
            } else {
                Err(JsonRegistrationIssue::Conflict {
                    existing: serde_json::to_string_pretty(current)
                        .unwrap_or_else(|_| current.to_string()),
                    proposed: serde_json::to_string_pretty(proposed)
                        .unwrap_or_else(|_| proposed.to_string()),
                })
            };
        }
    }
    let merged = merge(&document, entry).ok_or_else(|| {
        JsonRegistrationIssue::Malformed("configuration root is not mergeable".to_string())
    })?;
    let mut bytes = serde_json::to_vec_pretty(&merged)
        .map_err(|error| JsonRegistrationIssue::Malformed(error.to_string()))?;
    bytes.push(b'\n');
    Ok(if existing.is_none() {
        RegistrationChange::Create(bytes)
    } else {
        RegistrationChange::Merge(bytes)
    })
}

fn codex_server_entry(entry: &Value) -> Option<CodexServerEntry> {
    let server = entry.get("mcp_servers")?.get("ds")?;
    let command = server.get("command")?.as_str()?.to_string();
    let args = server
        .get("args")?
        .as_array()?
        .iter()
        .map(|arg| arg.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(CodexServerEntry { command, args })
}

fn codex_entry_preview(entry: &CodexServerEntry) -> String {
    let mut document = DocumentMut::new();
    let mut server = Table::new();
    server.insert("command", value(&entry.command));
    let mut args = Array::new();
    for arg in &entry.args {
        args.push(arg);
    }
    server.insert("args", value(args));
    let mut servers = Table::new();
    servers.insert("ds", Item::Table(server));
    document.insert("mcp_servers", Item::Table(servers));
    document.to_string()
}

fn existing_codex_server(item: &Item) -> Option<CodexServerEntry> {
    let table = item.as_table()?;
    let command = table.get("command")?.as_str()?.to_string();
    let args = table
        .get("args")?
        .as_array()?
        .iter()
        .map(|arg| arg.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(CodexServerEntry { command, args })
}

/// Losslessly plan the one table DS owns in Codex's TOML document. Existing
/// sibling tables, comments, whitespace, and key formatting remain under
/// `toml_edit`; a pre-existing non-identical `ds` entry is never overwritten.
fn merge_codex_toml(
    existing: Option<&[u8]>,
    entry: &Value,
) -> Result<RegistrationChange, CodexTomlIssue> {
    let desired = codex_server_entry(entry).ok_or_else(|| {
        CodexTomlIssue::Malformed(
            "the internal Codex proposal is not one command/args entry".to_string(),
        )
    })?;
    let text = match existing {
        None => String::new(),
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|error| {
                CodexTomlIssue::Malformed(format!("configuration is not UTF-8: {error}"))
            })?
            .to_string(),
    };
    let mut document = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|error| CodexTomlIssue::Malformed(error.to_string()))?
    };

    if let Some(servers) = document.get("mcp_servers") {
        let servers = servers.as_table().ok_or_else(|| CodexTomlIssue::Conflict {
            existing: servers.to_string(),
            proposed: codex_entry_preview(&desired),
        })?;
        if let Some(current) = servers.get("ds") {
            return match existing_codex_server(current) {
                Some(current) if current == desired => Ok(RegistrationChange::Unchanged),
                _ => Err(CodexTomlIssue::Conflict {
                    existing: current.to_string(),
                    proposed: codex_entry_preview(&desired),
                }),
            };
        }
    }

    if !document.contains_key("mcp_servers") {
        document.insert("mcp_servers", Item::Table(Table::new()));
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .expect("the checked or inserted MCP root is a table");
    let mut server = Table::new();
    server.insert("command", value(&desired.command));
    let mut args = Array::new();
    for arg in &desired.args {
        args.push(arg);
    }
    server.insert("args", value(args));
    servers.insert("ds", Item::Table(server));
    let bytes = document.to_string().into_bytes();
    Ok(if existing.is_none() || text.trim().is_empty() {
        RegistrationChange::Create(bytes)
    } else {
        RegistrationChange::Merge(bytes)
    })
}

fn adapter(host: &str) -> Option<&'static HostAdapter> {
    ADAPTERS.iter().find(|adapter| adapter.token == host)
}

fn guarded_json_host(host: &str) -> bool {
    matches!(host, "gemini-cli" | "windsurf" | "github-copilot")
}

fn unknown_host(host: &str) -> Failure {
    let tokens = HOSTS.join(", ");
    Failure::invalid(
        "mcp_host_unknown",
        format!("no configuration recipe for host `{host}`"),
    )
    .remedy(format!("pass one of these supported host tokens: {tokens}"))
    .next(format!(
        "ds mcp install --host <{}> --output json",
        HOSTS.join("|")
    ))
    .detail(json!({ "supported_hosts": HOSTS }))
}

/// The user-level file for the host on this platform. Generic output has no
/// target, and Codex remains print-only because its on-disk format is TOML.
fn config_file(adapter: &HostAdapter) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    config_file_for(
        adapter,
        Platform::current(),
        home.as_deref(),
        std::env::var_os("APPDATA").as_deref().map(Path::new),
    )
}

fn config_file_for(
    adapter: &HostAdapter,
    platform: Platform,
    home: Option<&Path>,
    appdata: Option<&Path>,
) -> Option<PathBuf> {
    if !adapter.platforms.contains(&platform) {
        return None;
    }
    match adapter.token {
        "vscode" => Some(if platform == Platform::Windows {
            appdata?.join("Code").join("User").join("mcp.json")
        } else if platform == Platform::Macos {
            home?
                .join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("mcp.json")
        } else {
            home?
                .join(".config")
                .join("Code")
                .join("User")
                .join("mcp.json")
        }),
        "claude-code" => Some(home?.join(".claude.json")),
        "claude-desktop" => Some(appdata?.join("Claude").join("claude_desktop_config.json")),
        "codex" => Some(home?.join(".codex").join("config.toml")),
        "cursor" => Some(home?.join(".cursor").join("mcp.json")),
        "gemini-cli" => Some(home?.join(".gemini").join("settings.json")),
        "windsurf" => Some(
            home?
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
        ),
        "github-copilot" => Some(home?.join(".copilot").join("mcp-config.json")),
        _ => None,
    }
}

fn supported_hosts() -> Vec<Value> {
    let current = Platform::current();
    ADAPTERS
        .iter()
        .map(|adapter| {
            let resolved = config_file(adapter);
            let discoverable = resolved.as_ref().is_some_and(|path| {
                path.exists() || path.parent().is_some_and(Path::exists)
            });
            json!({
                "token": adapter.token,
                "display_name": adapter.display_name,
                "supported_platforms": adapter.platforms.iter().map(|platform| platform.token()).collect::<Vec<_>>(),
                "supported_on_this_platform": adapter.platforms.contains(&current),
                "configuration_path": resolved.map(|path| path.display().to_string()),
                "configuration_paths": configuration_paths(adapter),
                "configuration_root": adapter.root.token(),
                "supported_transports": ["stdio"],
                "automatic_merge": adapter.automatic_merge,
                "discoverable": discoverable,
                "appears_installed_or_discoverable": discoverable,
                "restart_requirement": adapter.restart_requirement,
            })
        })
        .collect()
}

fn configuration_paths(adapter: &HostAdapter) -> Vec<Value> {
    match adapter.token {
        "vscode" => vec![
            json!({ "platform": "windows", "path": r"%APPDATA%\Code\User\mcp.json" }),
            json!({ "platform": "macos", "path": "~/Library/Application Support/Code/User/mcp.json" }),
            json!({ "platform": "linux", "path": "~/.config/Code/User/mcp.json" }),
        ],
        "claude-code" => ALL_PLATFORMS
            .iter()
            .map(|platform| json!({ "platform": platform.token(), "path": "~/.claude.json" }))
            .collect(),
        "claude-desktop" => vec![json!({
            "platform": "windows",
            "path": r"%APPDATA%\Claude\claude_desktop_config.json",
        })],
        "codex" => ALL_PLATFORMS
            .iter()
            .map(|platform| json!({ "platform": platform.token(), "path": "~/.codex/config.toml" }))
            .collect(),
        "cursor" => ALL_PLATFORMS
            .iter()
            .map(|platform| json!({ "platform": platform.token(), "path": "~/.cursor/mcp.json" }))
            .collect(),
        "gemini-cli" => ALL_PLATFORMS
            .iter()
            .map(|platform| json!({ "platform": platform.token(), "path": "~/.gemini/settings.json" }))
            .collect(),
        "windsurf" => ALL_PLATFORMS
            .iter()
            .map(|platform| json!({ "platform": platform.token(), "path": "~/.codeium/windsurf/mcp_config.json" }))
            .collect(),
        "github-copilot" => ALL_PLATFORMS
            .iter()
            .map(|platform| json!({ "platform": platform.token(), "path": "~/.copilot/mcp-config.json" }))
            .collect(),
        _ => Vec::new(),
    }
}

fn validate_host_platform(
    adapter: &HostAdapter,
    descriptor: &ConnectionDescriptor,
) -> Result<(), Failure> {
    let current = Platform::current();
    let executable_platform = executable_platform(&descriptor.executable).map_err(|reason| {
        Failure::failed(
            "mcp_host_os_mismatch",
            format!("the selected ds executable format could not be verified: {reason}"),
        )
        .remedy("run MCP installation from a native installed ds executable on the host machine")
        .next(format!(
            "ds mcp install --host {} --output json",
            adapter.token
        ))
    })?;
    if executable_platform == current && adapter.platforms.contains(&current) {
        return Ok(());
    }
    let supported = adapter
        .platforms
        .iter()
        .map(|platform| platform.token())
        .collect::<Vec<_>>()
        .join(", ");
    let profile = descriptor.build["profile"].as_str().unwrap_or("unknown");
    Err(Failure::invalid(
        "mcp_host_os_mismatch",
        format!(
            "{} cannot locally spawn {} from this {} process",
            adapter.display_name,
            descriptor.executable.display(),
            current.token()
        ),
    )
    .remedy(format!(
        "run installation on a {supported} machine where {} is installed, using that machine's ds executable; selected executable {} is {} profile {profile}",
        adapter.display_name,
        descriptor.executable.display(),
        executable_platform.token(),
    ))
    .next(format!(
        "ds mcp install --host {} --output json",
        adapter.token
    ))
    .detail(json!({
        "host": adapter.token,
        "host_platforms": adapter.platforms.iter().map(|platform| platform.token()).collect::<Vec<_>>(),
        "current_platform": current.token(),
        "executable_platform": executable_platform.token(),
        "executable": descriptor.executable.display().to_string(),
        "build_profile": profile,
    })))
}

fn executable_platform(path: &Path) -> Result<Platform, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("path is not a regular file".to_string());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|error| error.to_string())?;
    if magic[..2] == *b"MZ" {
        return Ok(Platform::Windows);
    }
    if magic == [0x7f, b'E', b'L', b'F'] {
        return Ok(Platform::Linux);
    }
    if matches!(
        magic,
        [0xfe, 0xed, 0xfa, 0xce]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xcf, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xbe, 0xba, 0xfe, 0xca]
    ) {
        return Ok(Platform::Macos);
    }
    Err("file has no supported PE, ELF, or Mach-O signature".to_string())
}

/// Merge `entry` (one top-level map with one server) into the JSON at `path`,
/// keeping every other key and server. A malformed file is refused, never
/// overwritten.
///
/// The target-file policy is deliberately conservative: a target, its direct
/// parent, the lock, the staging files, and the backup must all be ordinary
/// files/directories, never symlinks or Windows reparse points. In particular
/// this command never follows a target symlink and replaces its referent.
///
/// A bounded, same-directory exclusive lock serializes cooperating DS writers.
/// We also re-read the target immediately before replacement, so an external
/// editor that did not take that lock is preserved and the install is refused
/// instead of silently discarding its change. The stage and backup are synced
/// before rename, and the parent directory is synced where the platform
/// supports directory fsync.
pub fn merge_into_file(path: &Path, host: &str, entry: &Value) -> Result<(), Failure> {
    merge_into_file_with_hook(path, host, entry, || {})
}

fn codex_issue(path: &Path, issue: CodexTomlIssue) -> Failure {
    match issue {
        CodexTomlIssue::Malformed(reason) => Failure::failed(
            "mcp_config_unwritable",
            format!("{} is not valid Codex TOML: {reason}", path.display()),
        )
        .remedy("fix the malformed TOML without removing unrelated settings, then re-run the read-only proposal")
        .next("ds mcp install --host codex --output json")
        .detail(json!({ "file": path.display().to_string(), "host": "codex" })),
        CodexTomlIssue::Conflict { existing, proposed } => Failure::invalid(
            "mcp_config_conflict",
            format!(
                "{} already contains a non-identical `mcp_servers.ds` entry; it was left untouched",
                path.display()
            ),
        )
        .remedy("inspect the previews, then remove or rename the existing `[mcp_servers.ds]` entry and re-run the read-only proposal; DS never overwrites it")
        .next("ds mcp install --host codex --output json")
        .detail(json!({
            "file": path.display().to_string(),
            "host": "codex",
            "existing": existing,
            "proposed": proposed,
        })),
    }
}

fn plan_codex_file(path: &Path, entry: &Value) -> Result<RegistrationChange, Failure> {
    let existing = read_regular_file(path).map_err(|error| {
        Failure::failed(
            "mcp_config_unwritable",
            format!("could not read {}: {error}", path.display()),
        )
        .remedy("read the reported path; replace links or special files with an ordinary Codex config, then retry")
        .detail(json!({ "file": path.display().to_string(), "host": "codex" }))
    })?;
    merge_codex_toml(existing.as_ref().map(|file| file.bytes.as_slice()), entry)
        .map_err(|issue| codex_issue(path, issue))
}

fn merge_codex_into_file(path: &Path, entry: &Value) -> Result<RegistrationChange, Failure> {
    let unwritable = |message: String| {
        Failure::failed("mcp_config_unwritable", message)
            .remedy("read the reported path; fix or remove malformed TOML, then re-run the read-only proposal")
            .detail(json!({ "file": path.display().to_string(), "host": "codex" }))
    };
    let parent = prepare_parent(path)
        .map_err(|error| unwritable(format!("could not prepare {}: {error}", path.display())))?;
    let _lock = InstallLock::acquire(path)
        .map_err(|error| unwritable(format!("could not lock {}: {error}", path.display())))?;
    let existing = read_regular_file(path)
        .map_err(|error| unwritable(format!("could not read {}: {error}", path.display())))?;
    let change = merge_codex_toml(existing.as_ref().map(|file| file.bytes.as_slice()), entry)
        .map_err(|issue| codex_issue(path, issue))?;
    let Some(bytes) = change.bytes() else {
        return Ok(change);
    };

    let mut staged = StagedFile::create(path, "stage").map_err(|error| {
        unwritable(format!(
            "could not create private stage for {}: {error}",
            path.display()
        ))
    })?;
    staged
        .write_synced(bytes, existing.as_ref().map(|file| &file.metadata))
        .map_err(|error| {
            unwritable(format!(
                "could not write {}: {error}",
                staged.path.display()
            ))
        })?;
    if let Some(previous) = &existing {
        write_backup(path, previous).map_err(|error| {
            unwritable(format!(
                "could not preserve backup for {}: {error}",
                path.display()
            ))
        })?;
    }
    let current = read_regular_file(path).map_err(|error| {
        unwritable(format!(
            "could not re-check {} before replacement: {error}",
            path.display()
        ))
    })?;
    if !same_contents(existing.as_ref(), current.as_ref()) {
        return Err(unwritable(format!(
            "{} changed while its replacement was being prepared; it was left untouched",
            path.display()
        )));
    }
    staged
        .replace(path)
        .map_err(|error| unwritable(format!("could not replace {}: {error}", path.display())))?;
    sync_parent(parent).map_err(|error| {
        unwritable(format!(
            "could not make {} durable: {error}",
            parent.display()
        ))
    })?;
    Ok(change)
}

fn guarded_json_issue(path: &Path, host: &str, issue: JsonRegistrationIssue) -> Failure {
    match issue {
        JsonRegistrationIssue::Malformed(reason) => Failure::failed(
            "mcp_config_unwritable",
            format!("{} is not valid host JSON: {reason}", path.display()),
        )
        .remedy("fix the malformed JSON without removing unrelated settings, then re-run the read-only proposal")
        .next(format!("ds mcp install --host {host} --output json"))
        .detail(json!({ "file": path.display().to_string(), "host": host })),
        JsonRegistrationIssue::Conflict { existing, proposed } => Failure::invalid(
            "mcp_config_conflict",
            format!(
                "{} already contains a non-identical MCP `ds` entry; it was left untouched",
                path.display()
            ),
        )
        .remedy("inspect the previews, then remove or rename the existing `ds` entry and re-run the read-only proposal; DS never overwrites it")
        .next(format!("ds mcp install --host {host} --output json"))
        .detail(json!({
            "file": path.display().to_string(),
            "host": host,
            "existing": existing,
            "proposed": proposed,
        })),
    }
}

fn plan_guarded_json_file(
    path: &Path,
    host: &str,
    entry: &Value,
) -> Result<RegistrationChange, Failure> {
    let existing = read_regular_file(path).map_err(|error| {
        Failure::failed(
            "mcp_config_unwritable",
            format!("could not read {}: {error}", path.display()),
        )
        .remedy("read the reported path; replace links or special files with an ordinary host config, then retry")
        .detail(json!({ "file": path.display().to_string(), "host": host }))
    })?;
    merge_guarded_json(existing.as_ref().map(|file| file.bytes.as_slice()), entry)
        .map_err(|issue| guarded_json_issue(path, host, issue))
}

fn merge_guarded_json_into_file(
    path: &Path,
    host: &str,
    entry: &Value,
) -> Result<RegistrationChange, Failure> {
    let unwritable = |message: String| {
        Failure::failed("mcp_config_unwritable", message)
            .remedy("read the reported path; fix or remove malformed JSON, then re-run the read-only proposal")
            .detail(json!({ "file": path.display().to_string(), "host": host }))
    };
    let parent = prepare_parent(path)
        .map_err(|error| unwritable(format!("could not prepare {}: {error}", path.display())))?;
    let _lock = InstallLock::acquire(path)
        .map_err(|error| unwritable(format!("could not lock {}: {error}", path.display())))?;
    let existing = read_regular_file(path)
        .map_err(|error| unwritable(format!("could not read {}: {error}", path.display())))?;
    let change = merge_guarded_json(existing.as_ref().map(|file| file.bytes.as_slice()), entry)
        .map_err(|issue| guarded_json_issue(path, host, issue))?;
    let Some(bytes) = change.bytes() else {
        return Ok(change);
    };

    let mut staged = StagedFile::create(path, "stage").map_err(|error| {
        unwritable(format!(
            "could not create private stage for {}: {error}",
            path.display()
        ))
    })?;
    staged
        .write_synced(bytes, existing.as_ref().map(|file| &file.metadata))
        .map_err(|error| {
            unwritable(format!(
                "could not write {}: {error}",
                staged.path.display()
            ))
        })?;
    if let Some(previous) = &existing {
        write_backup(path, previous).map_err(|error| {
            unwritable(format!(
                "could not preserve backup for {}: {error}",
                path.display()
            ))
        })?;
    }
    let current = read_regular_file(path).map_err(|error| {
        unwritable(format!(
            "could not re-check {} before replacement: {error}",
            path.display()
        ))
    })?;
    if !same_contents(existing.as_ref(), current.as_ref()) {
        return Err(unwritable(format!(
            "{} changed while its replacement was being prepared; it was left untouched",
            path.display()
        )));
    }
    staged
        .replace(path)
        .map_err(|error| unwritable(format!("could not replace {}: {error}", path.display())))?;
    sync_parent(parent).map_err(|error| {
        unwritable(format!(
            "could not make {} durable: {error}",
            parent.display()
        ))
    })?;
    Ok(change)
}

fn merge_into_file_with_hook(
    path: &Path,
    host: &str,
    entry: &Value,
    before_replace: impl FnOnce(),
) -> Result<(), Failure> {
    let unwritable = |message: String| {
        Failure::failed("mcp_config_unwritable", message)
            .remedy("read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand")
            .detail(json!({ "file": path.display().to_string(), "host": host }))
    };
    let parent = prepare_parent(path)
        .map_err(|error| unwritable(format!("could not prepare {}: {error}", path.display())))?;
    let _lock = InstallLock::acquire(path)
        .map_err(|error| unwritable(format!("could not lock {}: {error}", path.display())))?;

    let existing = read_regular_file(path)
        .map_err(|error| unwritable(format!("could not read {}: {error}", path.display())))?;
    let document: Value = match existing.as_ref().map(|existing| existing.bytes.as_slice()) {
        None => json!({}),
        Some(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => json!({}),
        Some(bytes) => serde_json::from_slice(bytes).map_err(|error| {
            unwritable(format!("{} is not valid JSON: {error}", path.display()))
        })?,
    };
    let document = merge(&document, entry).ok_or_else(|| {
        unwritable(format!(
            "{} does not hold an object at its root",
            path.display()
        ))
    })?;
    let mut bytes =
        serde_json::to_vec_pretty(&document).map_err(|error| unwritable(error.to_string()))?;
    bytes.push(b'\n');

    // Same directory, so replacement cannot cross a filesystem boundary and
    // degrade into a copy. `create_new` makes a hostile pre-created stage a
    // refusal rather than a truncation or a followed symlink.
    let mut staged = StagedFile::create(path, "stage").map_err(|error| {
        unwritable(format!(
            "could not create private stage for {}: {error}",
            path.display()
        ))
    })?;
    staged
        .write_synced(&bytes, existing.as_ref().map(|existing| &existing.metadata))
        .map_err(|error| {
            unwritable(format!(
                "could not write {}: {error}",
                staged.path.display()
            ))
        })?;

    if let Some(previous) = &existing {
        write_backup(path, previous).map_err(|error| {
            unwritable(format!(
                "could not preserve backup for {}: {error}",
                path.display()
            ))
        })?;
    }

    // The lock protects DS installers. This check covers an editor or another
    // program that changes the path without joining that protocol.
    before_replace();
    let current = read_regular_file(path).map_err(|error| {
        unwritable(format!(
            "could not re-check {} before replacement: {error}",
            path.display()
        ))
    })?;
    if !same_contents(existing.as_ref(), current.as_ref()) {
        return Err(unwritable(format!(
            "{} changed while its replacement was being prepared; it was left untouched",
            path.display()
        )));
    }

    staged
        .replace(path)
        .map_err(|error| unwritable(format!("could not replace {}: {error}", path.display())))?;
    sync_parent(parent).map_err(|error| {
        unwritable(format!(
            "could not make {} durable: {error}",
            parent.display()
        ))
    })
}

const LOCK_ATTEMPTS: u32 = 100;
const LOCK_WAIT: Duration = Duration::from_millis(20);
static PRIVATE_PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn sibling_path(path: &Path, kind: &str) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    let sequence = PRIVATE_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(
        ".{name}.ds-{kind}-{}-{sequence}.tmp",
        std::process::id()
    ))
}

fn lock_path(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!(".{name}.ds.lock"))
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{name}.bak"))
}

struct ExistingFile {
    bytes: Vec<u8>,
    metadata: Metadata,
}

fn same_contents(left: Option<&ExistingFile>, right: Option<&ExistingFile>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.bytes == right.bytes,
        _ => false,
    }
}

fn prepare_parent(path: &Path) -> std::io::Result<&Path> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let metadata = std::fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(unsafe_path_error(parent));
    }
    Ok(parent)
}

fn read_regular_file(path: &Path) -> std::io::Result<Option<ExistingFile>> {
    let checked = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if checked.file_type().is_symlink() || is_reparse_point(&checked) || !checked.is_file() {
        return Err(unsafe_path_error(path));
    }
    let mut file = open_read_no_follow(path)?;
    let opened = file.metadata()?;
    if is_reparse_point(&opened) || !opened.is_file() {
        return Err(unsafe_path_error(path));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(ExistingFile {
        bytes,
        // Preserve the policy of the inode actually opened, not a pathname
        // observation that might have changed before O_NOFOLLOW opened it.
        metadata: opened,
    }))
}

fn unsafe_path_error(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "{} is not an ordinary non-link file or directory",
            path.display()
        ),
    )
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    // FILE_FLAG_OPEN_REPARSE_POINT makes CreateFile open the link object rather
    // than its referent; reading it then fails, which is the desired refusal.
    OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

fn create_exclusive(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(0x0020_0000)
            .open(path)
    }
    #[cfg(not(any(unix, windows)))]
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn preserve_file_policy(file: &File, source: Option<&Metadata>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd as _;
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if let Some(source) = source {
            // Rename replaces ownership with that of the staging file. Preserve
            // owner/group when the platform lets this process do so, rather than
            // silently changing the access policy of a secret-bearing config.
            if unsafe { libc::fchown(file.as_raw_fd(), source.uid(), source.gid()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            return file.set_permissions(std::fs::Permissions::from_mode(source.mode() & 0o7777));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        if let Some(source) = source {
            return file.set_permissions(source.permissions());
        }
        Ok(())
    }
}

fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        // Windows does not expose a portable directory handle sync through
        // std. The staged file itself is flushed before replacement.
        let _ = parent;
        Ok(())
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

struct StagedFile {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl StagedFile {
    fn create(target: &Path, kind: &str) -> std::io::Result<Self> {
        for _ in 0..32 {
            let path = sibling_path(target, kind);
            match create_exclusive(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        committed: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not reserve a private staging path",
        ))
    }

    fn write_synced(&mut self, bytes: &[u8], policy: Option<&Metadata>) -> std::io::Result<()> {
        let file = self.file.as_mut().expect("stage is open until replacement");
        file.write_all(bytes)?;
        preserve_file_policy(file, policy)?;
        file.sync_all()
    }

    fn replace(mut self, destination: &Path) -> std::io::Result<()> {
        drop(self.file.take());
        atomic_replace(&self.path, destination)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn write_backup(path: &Path, previous: &ExistingFile) -> std::io::Result<()> {
    let backup = backup_path(path);
    if let Some(metadata) = read_path_metadata(&backup)?
        && (metadata.file_type().is_symlink() || is_reparse_point(&metadata) || !metadata.is_file())
    {
        return Err(unsafe_path_error(&backup));
    }
    let parent = backup.parent().ok_or_else(|| unsafe_path_error(&backup))?;
    let mut staged = StagedFile::create(&backup, "backup")?;
    staged.write_synced(&previous.bytes, Some(&previous.metadata))?;
    staged.replace(&backup)?;
    sync_parent(parent)
}

fn read_path_metadata(path: &Path) -> std::io::Result<Option<Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

struct InstallLock {
    path: PathBuf,
    token: Vec<u8>,
    _file: File,
}

impl InstallLock {
    fn acquire(target: &Path) -> std::io::Result<Self> {
        Self::acquire_with_attempts(target, LOCK_ATTEMPTS)
    }

    fn acquire_with_attempts(target: &Path, attempts: u32) -> std::io::Result<Self> {
        let path = lock_path(target);
        for attempt in 0..attempts {
            match create_exclusive(&path) {
                Ok(mut file) => {
                    let token = format!(
                        "ds mcp install {} {}\n",
                        std::process::id(),
                        PRIVATE_PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed)
                    )
                    .into_bytes();
                    file.write_all(&token)?;
                    file.sync_all()?;
                    return Ok(Self {
                        path,
                        token,
                        _file: file,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = read_path_metadata(&path)?
                        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::WouldBlock))?;
                    if metadata.file_type().is_symlink()
                        || is_reparse_point(&metadata)
                        || !metadata.is_file()
                    {
                        return Err(unsafe_path_error(&path));
                    }
                    if attempt + 1 == attempts {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            "another DS installer is still updating this file; retry after it finishes",
                        ));
                    }
                    std::thread::sleep(LOCK_WAIT);
                }
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "lock attempts must be non-zero",
        ))
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        // Never remove a hostile replacement. The contents check is also what
        // makes a left-over lock inspectable and safely recoverable by a user.
        if let Ok(Some(metadata)) = read_path_metadata(&self.path)
            && metadata.is_file()
            && !metadata.file_type().is_symlink()
            && !is_reparse_point(&metadata)
            && std::fs::read(&self.path).ok().as_deref() == Some(self.token.as_slice())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Pure merge: the entry's single top-level map key is merged into the
/// document's map of the same name, replacing only the `ds` server.
pub fn merge(document: &Value, entry: &Value) -> Option<Value> {
    let mut root = document.as_object()?.clone();
    let entry_root = entry.as_object()?;
    if entry_root.len() != 1 {
        return None;
    }
    let (section, servers) = entry_root.iter().next()?;
    let servers = servers.as_object()?;
    if servers.len() != 1 || !servers.contains_key("ds") {
        return None;
    }
    let mut existing = match root.get(section) {
        Some(value) => value.as_object()?.clone(),
        None => Map::new(),
    };
    existing.insert("ds".to_string(), servers["ds"].clone());
    root.insert(section.clone(), Value::Object(existing));
    Some(Value::Object(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_entry(host: &str, executable: &std::path::Path) -> Value {
        let descriptor = connection_descriptor(
            executable.to_path_buf(),
            json!({
                "source_sha": "0123456789012345678901234567890123456789",
                "dirty": false,
                "profile": "debug",
                "target": "x86_64-unknown-linux-gnu",
            }),
            Some("0123456789012345678901234567890123456789".to_string()),
            crate::surface::Exposure::Chapters,
            None,
        );
        server_entry(adapter(host).expect("adapter"), &descriptor)
    }

    #[test]
    fn vscode_entry_is_a_stdio_server_pointing_at_this_executable() {
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        assert_eq!(entry["servers"]["ds"]["type"], "stdio");
        assert_eq!(entry["servers"]["ds"]["command"], "/usr/bin/ds");
        assert_eq!(
            entry["servers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "chapters"])
        );
    }

    #[test]
    fn typed_profile_entry_names_both_exposure_and_profile() {
        let descriptor = connection_descriptor(
            std::path::PathBuf::from("/usr/bin/ds"),
            json!({ "source_sha": "a", "dirty": false, "profile": "release" }),
            Some("a".to_string()),
            crate::surface::Exposure::Commands,
            Some(crate::surface::Profile::Pls),
        );
        let entry = server_entry(adapter("claude-code").unwrap(), &descriptor);
        assert_eq!(
            entry["mcpServers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "commands", "--profile", "pls"])
        );
    }

    #[test]
    fn every_adapter_is_table_driven_and_claude_desktop_uses_its_verified_root() {
        assert_eq!(
            ADAPTERS
                .iter()
                .map(|adapter| adapter.token)
                .collect::<Vec<_>>(),
            HOSTS
        );
        let entry = default_entry(
            "claude-desktop",
            std::path::Path::new(r"C:\Program Files\DS GridDesign\ds.exe"),
        );
        assert_eq!(
            entry["mcpServers"]["ds"]["command"],
            r"C:\Program Files\DS GridDesign\ds.exe"
        );
        assert_eq!(
            entry["mcpServers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "chapters"])
        );
        assert_eq!(
            configuration_paths(adapter("claude-desktop").unwrap()),
            vec![json!({
                "platform": "windows",
                "path": r"%APPDATA%\Claude\claude_desktop_config.json",
            })]
        );
    }

    #[test]
    fn windows_paths_are_verified_user_level_targets() {
        let home = Path::new(r"C:\Users\operator");
        let appdata = Path::new(r"C:\Users\operator\AppData\Roaming");
        assert_eq!(
            config_file_for(
                adapter("claude-desktop").unwrap(),
                Platform::Windows,
                Some(home),
                Some(appdata),
            ),
            Some(appdata.join("Claude").join("claude_desktop_config.json"))
        );
        assert_eq!(
            config_file_for(
                adapter("vscode").unwrap(),
                Platform::Windows,
                Some(home),
                Some(appdata),
            ),
            Some(appdata.join("Code").join("User").join("mcp.json"))
        );
        assert_eq!(
            config_file_for(
                adapter("codex").unwrap(),
                Platform::Windows,
                Some(home),
                Some(appdata),
            ),
            Some(home.join(".codex").join("config.toml"))
        );
        assert_eq!(
            config_file_for(
                adapter("codex").unwrap(),
                Platform::Linux,
                Some(Path::new("/home/operator")),
                None,
            ),
            Some(Path::new("/home/operator/.codex/config.toml").to_path_buf())
        );
    }

    #[test]
    fn claude_desktop_is_refused_off_windows_and_generic_has_no_guessed_path() {
        assert_eq!(
            config_file_for(
                adapter("claude-desktop").unwrap(),
                Platform::Linux,
                Some(Path::new("/home/operator")),
                None,
            ),
            None
        );
        assert_eq!(
            config_file_for(
                adapter("generic").unwrap(),
                Platform::Linux,
                Some(Path::new("/home/operator")),
                None,
            ),
            None
        );
    }

    #[test]
    fn canonical_descriptor_keeps_host_neutral_identity_and_required_environment() {
        let descriptor = connection_descriptor(
            PathBuf::from("/opt/ds/bin/ds"),
            json!({ "source_sha": "abc", "dirty": false, "profile": "release" }),
            Some("abc".to_string()),
            crate::surface::Exposure::Commands,
            Some(crate::surface::Profile::SurveyProjects),
        );
        let value = descriptor.json();
        assert_eq!(value["server_name"], "ds");
        assert_eq!(value["transport"], "stdio");
        assert_eq!(value["executable"], "/opt/ds/bin/ds");
        assert_eq!(value["build"]["source_sha"], "abc");
        assert_eq!(value["skill_bundle_source_sha"], "abc");
        assert_eq!(value["required_environment"], json!({}));
        assert_eq!(value["profile"], "survey-projects");
    }

    #[test]
    fn codex_entry_is_the_verified_stdio_table_shape() {
        let entry = default_entry("codex", Path::new("/opt/ds/bin/ds"));
        assert_eq!(entry["mcp_servers"]["ds"]["command"], "/opt/ds/bin/ds");
        assert_eq!(
            entry["mcp_servers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "chapters"])
        );
        assert!(adapter("codex").unwrap().automatic_merge);
    }

    #[test]
    fn added_json_hosts_use_their_verified_dialects_and_user_paths() {
        let home = Path::new("/home/operator");
        for (host, relative) in [
            ("gemini-cli", ".gemini/settings.json"),
            ("windsurf", ".codeium/windsurf/mcp_config.json"),
            ("github-copilot", ".copilot/mcp-config.json"),
        ] {
            for platform in ALL_PLATFORMS {
                assert_eq!(
                    config_file_for(adapter(host).unwrap(), *platform, Some(home), None),
                    Some(home.join(relative)),
                    "{host} on {}",
                    platform.token()
                );
            }
        }

        for host in ["gemini-cli", "windsurf"] {
            let entry = default_entry(host, Path::new("/opt/ds/bin/ds"));
            assert_eq!(entry["mcpServers"]["ds"]["command"], "/opt/ds/bin/ds");
            assert_eq!(
                entry["mcpServers"]["ds"]["args"],
                json!(["mcp", "serve", "--exposure", "chapters"])
            );
            assert!(entry["mcpServers"]["ds"].get("type").is_none());
        }

        let copilot = default_entry("github-copilot", Path::new("/opt/ds/bin/ds"));
        assert_eq!(copilot["mcpServers"]["ds"]["type"], "local");
        assert_eq!(copilot["mcpServers"]["ds"]["env"], json!({}));
        assert_eq!(copilot["mcpServers"]["ds"]["tools"], json!(["*"]));
    }

    #[test]
    fn added_json_hosts_preserve_siblings_and_refuse_ds_conflicts() {
        let entry = default_entry("gemini-cli", Path::new("/opt/ds/bin/ds"));
        let existing = br#"{
  "theme": "dark",
  "mcpServers": { "other": { "command": "keep" } }
}"#;
        let merged = merge_guarded_json(Some(existing), &entry).expect("merge");
        let bytes = merged.bytes().expect("merged bytes");
        let document: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(document["theme"], "dark");
        assert_eq!(document["mcpServers"]["other"]["command"], "keep");
        assert_eq!(document["mcpServers"]["ds"]["command"], "/opt/ds/bin/ds");
        assert_eq!(
            merge_guarded_json(Some(bytes), &entry).unwrap(),
            RegistrationChange::Unchanged
        );

        let conflicting = br#"{"mcpServers":{"ds":{"command":"other","args":[]}}}"#;
        assert!(matches!(
            merge_guarded_json(Some(conflicting), &entry),
            Err(JsonRegistrationIssue::Conflict { .. })
        ));
        assert!(matches!(
            merge_guarded_json(Some(b"{broken"), &entry),
            Err(JsonRegistrationIssue::Malformed(_))
        ));
    }

    #[test]
    fn codex_empty_config_creates_one_exact_table_and_is_idempotent() {
        let entry = default_entry("codex", Path::new("/opt/ds/bin/ds"));
        let first = merge_codex_toml(None, &entry).expect("create");
        assert_eq!(first.token(false), "would_create");
        let bytes = first.bytes().expect("created bytes");
        let text = std::str::from_utf8(bytes).unwrap();
        assert!(text.contains("[mcp_servers.ds]"));
        assert!(text.contains("command = \"/opt/ds/bin/ds\""));
        assert!(text.contains("args = [\"mcp\", \"serve\", \"--exposure\", \"chapters\"]"));
        assert_eq!(
            merge_codex_toml(Some(bytes), &entry).unwrap(),
            RegistrationChange::Unchanged
        );
    }

    #[test]
    fn codex_merge_preserves_unrelated_tables_comments_and_formatting() {
        let entry = default_entry("codex", Path::new("/opt/ds/bin/ds"));
        let existing = b"# operator comment\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand='other' # keep format\nargs = []\n";
        let merged = merge_codex_toml(Some(existing), &entry).expect("merge");
        assert_eq!(merged.token(false), "would_merge");
        let text = std::str::from_utf8(merged.bytes().unwrap()).unwrap();
        assert!(text.starts_with("# operator comment\nmodel = \"gpt-5\""));
        assert!(text.contains("command='other' # keep format"));
        assert!(text.contains("[mcp_servers.ds]"));
    }

    #[test]
    fn codex_conflict_and_malformed_toml_are_bounded_refusals() {
        let entry = default_entry("codex", Path::new("/opt/ds/bin/ds"));
        let conflict = merge_codex_toml(
            Some(b"[mcp_servers.ds]\ncommand = \"another-ds\"\nargs = []\n"),
            &entry,
        )
        .unwrap_err();
        match conflict {
            CodexTomlIssue::Conflict { existing, proposed } => {
                assert!(existing.contains("another-ds"));
                assert!(proposed.contains("/opt/ds/bin/ds"));
            }
            other => panic!("wrong issue: {other:?}"),
        }
        assert!(matches!(
            merge_codex_toml(Some(b"[mcp_servers.ds\ncommand ="), &entry),
            Err(CodexTomlIssue::Malformed(_))
        ));
    }

    #[test]
    fn supported_host_records_expose_every_discovery_field() {
        let hosts = supported_hosts();
        assert_eq!(hosts.len(), HOSTS.len());
        for (record, token) in hosts.iter().zip(HOSTS) {
            assert_eq!(record["token"], *token);
            assert!(record["display_name"].is_string());
            assert!(record["supported_platforms"].is_array());
            assert!(record["configuration_paths"].is_array());
            assert!(record["configuration_root"].is_string());
            assert_eq!(record["supported_transports"], json!(["stdio"]));
            assert!(record["automatic_merge"].is_boolean());
            assert!(record["appears_installed_or_discoverable"].is_boolean());
            assert!(record["restart_requirement"].is_string());
        }
    }

    #[test]
    fn unknown_host_remedy_and_next_enumerate_every_token() {
        let failure = unknown_host("invented");
        for token in HOSTS {
            assert!(failure.remedy_text().unwrap().contains(token));
            assert!(failure.next_commands()[0].contains(token));
        }
    }

    #[test]
    fn executable_formats_are_checked_without_guessing_from_the_filename() {
        let dir = scratch("formats");
        std::fs::create_dir_all(&dir).unwrap();
        let pe = dir.join("pe.bin");
        let elf = dir.join("elf.exe");
        let macho = dir.join("macho.bin");
        let unknown = dir.join("unknown.bin");
        std::fs::write(&pe, b"MZ\0\0").unwrap();
        std::fs::write(&elf, b"\x7fELF").unwrap();
        std::fs::write(&macho, [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        std::fs::write(&unknown, b"text").unwrap();
        assert_eq!(executable_platform(&pe), Ok(Platform::Windows));
        assert_eq!(executable_platform(&elf), Ok(Platform::Linux));
        assert_eq!(executable_platform(&macho), Ok(Platform::Macos));
        assert!(executable_platform(&unknown).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_keeps_other_servers_and_keys() {
        let existing = json!({ "inputs": [], "servers": { "other": { "type": "stdio", "command": "x" }, "ds": { "command": "old" } } });
        let entry = default_entry("vscode", std::path::Path::new("/new/ds"));
        let merged = merge(&existing, &entry).expect("merged");
        assert_eq!(merged["inputs"], json!([]));
        assert_eq!(merged["servers"]["other"]["command"], "x");
        assert_eq!(merged["servers"]["ds"]["command"], "/new/ds");
    }

    #[test]
    fn claude_desktop_merge_preserves_siblings_and_unrelated_settings() {
        let existing = json!({
            "preferences": { "theme": "dark" },
            "mcpServers": {
                "other": { "command": "other.exe" },
                "ds": { "command": "old.exe" },
            },
        });
        let entry = default_entry(
            "claude-desktop",
            Path::new(r"C:\Program Files\DS GridDesign\ds.exe"),
        );
        let merged = merge(&existing, &entry).unwrap();
        assert_eq!(merged["preferences"]["theme"], "dark");
        assert_eq!(merged["mcpServers"]["other"]["command"], "other.exe");
        assert_eq!(
            merged["mcpServers"]["ds"]["command"],
            r"C:\Program Files\DS GridDesign\ds.exe"
        );
    }

    #[test]
    fn merge_is_idempotent_and_refuses_malformed_or_overbroad_sections() {
        let entry = default_entry("vscode", std::path::Path::new("/new/ds"));
        let existing = json!({ "inputs": [], "servers": { "other": { "command": "x" } } });
        let once = merge(&existing, &entry).unwrap();
        let twice = merge(&once, &entry).unwrap();
        assert_eq!(once, twice);
        assert!(merge(&json!({ "servers": "not-an-object" }), &entry).is_none());
        assert!(
            merge(
                &existing,
                &json!({ "servers": { "ds": {}, "foreign": {} } })
            )
            .is_none()
        );
        assert!(
            merge(
                &existing,
                &json!({ "servers": { "ds": {} }, "otherRoot": {} })
            )
            .is_none()
        );
    }

    #[test]
    fn a_non_object_root_is_refused_not_replaced() {
        assert!(
            merge(
                &json!([1, 2]),
                &default_entry("vscode", std::path::Path::new("ds"))
            )
            .is_none()
        );
    }

    /// An isolated directory under the system temp root. Never a real host
    /// configuration path: these tests write, and a user's `mcp.json` is not
    /// a fixture.
    fn scratch(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ds-mcp-install-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn writing_creates_the_file_and_a_malformed_one_is_left_alone() {
        let dir = scratch("malformed");
        let path = dir.join("User").join("mcp.json");
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).expect("write");
        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written["servers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "chapters"])
        );
        std::fs::write(&path, b"{ not json").unwrap();
        let refused = merge_into_file(&path, "vscode", &entry).unwrap_err();
        assert_eq!(refused.code(), "mcp_config_unwritable");
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn codex_write_is_atomic_idempotent_and_never_writes_on_refusal() {
        let dir = scratch("codex-write");
        let path = dir.join(".codex").join("config.toml");
        let entry = default_entry("codex", Path::new("/usr/bin/ds"));

        let created = merge_codex_into_file(&path, &entry).expect("create");
        assert_eq!(created.token(true), "created");
        let installed = std::fs::read(&path).unwrap();
        assert!(
            std::str::from_utf8(&installed)
                .unwrap()
                .contains("[mcp_servers.ds]")
        );

        let unchanged = merge_codex_into_file(&path, &entry).expect("idempotent");
        assert_eq!(unchanged, RegistrationChange::Unchanged);
        assert_eq!(std::fs::read(&path).unwrap(), installed);

        let conflicting = b"# keep me\n[mcp_servers.ds]\ncommand = \"other\"\nargs = []\n";
        std::fs::write(&path, conflicting).unwrap();
        let refused = merge_codex_into_file(&path, &entry).unwrap_err();
        assert_eq!(refused.code(), "mcp_config_conflict");
        assert_eq!(std::fs::read(&path).unwrap(), conflicting);

        let malformed = b"[mcp_servers.ds\ncommand =";
        std::fs::write(&path, malformed).unwrap();
        let refused = merge_codex_into_file(&path, &entry).unwrap_err();
        assert_eq!(refused.code(), "mcp_config_unwritable");
        assert_eq!(std::fs::read(&path).unwrap(), malformed);
        assert_no_private_stages(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guarded_json_host_never_writes_a_conflicting_registration() {
        let dir = scratch("guarded-json-conflict");
        let path = dir
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let before = br#"{"mcpServers":{"ds":{"command":"operator-owned","args":[]}},"keep":true}"#;
        std::fs::write(&path, before).unwrap();
        let entry = default_entry("windsurf", Path::new("/usr/bin/ds"));

        let refused = merge_guarded_json_into_file(&path, "windsurf", &entry).unwrap_err();
        assert_eq!(refused.code(), "mcp_config_conflict");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(!backup_path(&path).exists());
        assert_no_private_stages(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replacing_a_host_file_keeps_the_previous_document_as_a_backup() {
        let dir = scratch("backup");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        let before = b"{\n  \"servers\": { \"other\": { \"command\": \"x\" } }\n}\n";
        std::fs::write(&path, before).unwrap();

        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).expect("write");

        let backup = super::backup_path(&path);
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            before,
            "the operator's previous host configuration must be recoverable"
        );
        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["servers"]["other"]["command"], "x");
        assert_eq!(written["servers"]["ds"]["command"], "/usr/bin/ds");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_successful_write_leaves_no_staging_file_behind() {
        let dir = scratch("staging");
        let path = dir.join("mcp.json");
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).expect("write");

        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files survived: {leftovers:?}"
        );

        // Every private sibling shares the target's directory, so the rename
        // can never cross a filesystem boundary and silently become a copy.
        assert_eq!(super::backup_path(&path).parent(), path.parent());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_external_edit_between_read_and_replace_is_preserved_and_refused() {
        let dir = scratch("external-edit");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        let before = b"{\"servers\": {\"other\": {\"command\": \"x\"}}}";
        let external = b"{\"servers\": {\"external\": {\"command\": \"keep\"}}}";
        std::fs::write(&path, before).unwrap();
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));

        let refused = merge_into_file_with_hook(&path, "vscode", &entry, || {
            std::fs::write(&path, external).unwrap();
        })
        .unwrap_err();
        assert_eq!(refused.code(), "mcp_config_unwritable");
        assert_eq!(std::fs::read(&path).unwrap(), external);
        assert_eq!(std::fs::read(backup_path(&path)).unwrap(), before);
        assert_no_private_stages(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_installers_serialize_their_merges() {
        let dir = scratch("concurrent");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{\"keep\": true}").unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let first_path = path.clone();
        let first_start = start.clone();
        let first = std::thread::spawn(move || {
            first_start.wait();
            merge_into_file(
                &first_path,
                "vscode",
                &json!({ "servers": { "ds": { "command": "first" } } }),
            )
        });
        let second_path = path.clone();
        let second_start = start.clone();
        let second = std::thread::spawn(move || {
            second_start.wait();
            merge_into_file(
                &second_path,
                "vscode",
                &json!({ "mcpServers": { "ds": { "command": "second" } } }),
            )
        });
        start.wait();
        first.join().unwrap().expect("first installer succeeds");
        second.join().unwrap().expect("second installer succeeds");

        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["keep"], true);
        assert_eq!(written["servers"]["ds"]["command"], "first");
        assert_eq!(written["mcpServers"]["ds"]["command"], "second");
        assert!(!lock_path(&path).exists(), "our lock must be cleaned up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn assert_no_private_stages(dir: &Path) {
        let leftovers: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.contains(".ds-stage-") || name.contains(".ds-backup-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "private stages survived: {leftovers:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hostile_target_backup_stage_and_lock_links_are_never_followed() {
        use std::os::unix::fs::symlink;

        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));

        let target_dir = scratch("target-link");
        std::fs::create_dir_all(&target_dir).unwrap();
        let victim = target_dir.join("victim.json");
        std::fs::write(&victim, b"target secret").unwrap();
        let target = target_dir.join("mcp.json");
        symlink(&victim, &target).unwrap();
        assert!(merge_into_file(&target, "vscode", &entry).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"target secret");
        assert!(
            std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = std::fs::remove_dir_all(&target_dir);

        let backup_dir = scratch("backup-link");
        std::fs::create_dir_all(&backup_dir).unwrap();
        let backup_target = backup_dir.join("mcp.json");
        let before = b"{\"servers\": {\"other\": {\"command\": \"x\"}}}";
        std::fs::write(&backup_target, before).unwrap();
        let backup_victim = backup_dir.join("backup-victim.json");
        std::fs::write(&backup_victim, b"backup secret").unwrap();
        symlink(&backup_victim, backup_path(&backup_target)).unwrap();
        assert!(merge_into_file(&backup_target, "vscode", &entry).is_err());
        assert_eq!(std::fs::read(&backup_target).unwrap(), before);
        assert_eq!(std::fs::read(&backup_victim).unwrap(), b"backup secret");
        assert_no_private_stages(&backup_dir);
        let _ = std::fs::remove_dir_all(&backup_dir);

        let lock_dir = scratch("lock-link");
        std::fs::create_dir_all(&lock_dir).unwrap();
        let lock_target = lock_dir.join("mcp.json");
        let lock_victim = lock_dir.join("lock-victim");
        std::fs::write(&lock_victim, b"lock secret").unwrap();
        symlink(&lock_victim, lock_path(&lock_target)).unwrap();
        assert!(merge_into_file(&lock_target, "vscode", &entry).is_err());
        assert_eq!(std::fs::read(&lock_victim).unwrap(), b"lock secret");
        assert!(
            std::fs::symlink_metadata(lock_path(&lock_target))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = std::fs::remove_dir_all(&lock_dir);

        // A pre-created stage is rejected by exclusive creation; it is neither
        // followed nor truncated, even though a hostile name resembles ours.
        let stage_dir = scratch("stage-link");
        std::fs::create_dir_all(&stage_dir).unwrap();
        let stage_victim = stage_dir.join("stage-victim");
        std::fs::write(&stage_victim, b"stage secret").unwrap();
        let hostile_stage = stage_dir.join(".mcp.json.ds-stage-attacker.tmp");
        symlink(&stage_victim, &hostile_stage).unwrap();
        assert!(create_exclusive(&hostile_stage).is_err());
        assert_eq!(std::fs::read(&stage_victim).unwrap(), b"stage secret");
        let hostile_regular_stage = stage_dir.join(".mcp.json.ds-stage-foreign.tmp");
        std::fs::write(&hostile_regular_stage, b"foreign stage").unwrap();
        assert!(create_exclusive(&hostile_regular_stage).is_err());
        assert_eq!(
            std::fs::read(&hostile_regular_stage).unwrap(),
            b"foreign stage"
        );
        let _ = std::fs::remove_dir_all(&stage_dir);
    }

    #[test]
    fn a_precreated_regular_lock_is_not_overwritten_or_removed() {
        let dir = scratch("foreign-lock");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("mcp.json");
        let lock = lock_path(&target);
        std::fs::write(&lock, b"foreign lock").unwrap();
        assert!(InstallLock::acquire_with_attempts(&target, 1).is_err());
        assert_eq!(std::fs::read(&lock).unwrap(), b"foreign lock");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn target_and_backup_keep_restrictive_modes() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch("modes");
        let path = dir.join("mcp.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{\"servers\": {}}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let entry = default_entry("vscode", std::path::Path::new("/usr/bin/ds"));
        merge_into_file(&path, "vscode", &entry).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(backup_path(&path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_declared_effect_and_write_trigger_keep_confirmation_discoverable() {
        assert_eq!(COMMAND.effect, Effect::MachineWrite);
        assert!(COMMAND.effect.needs_confirmation());
        assert!(COMMAND.arg("write").is_some());
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == "confirmation_required"),
            "a gate a caller cannot discover from help is not a contract"
        );
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == "mcp_capabilities_unavailable"),
            "`build_identity` can emit this on the way to writing the entry"
        );
    }
}
