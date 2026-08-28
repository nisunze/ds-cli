//! `ds mcp install` — the host entry that launches `ds mcp serve`.
//!
//! Every MCP host reads the same shape: a named stdio server with a command
//! and arguments. What differs is the file it lives in. This command prints
//! the entry for this executable and, with `--write`, merges it into the
//! host's user-level file (never a workspace file: the server must run on the
//! machine that has DS GridDesign, and a workspace file travels).
//!
//! That target is what fixes the effect class. A user-level host
//! configuration is not "a file in your workspace" — it changes how every
//! agent session on this machine starts, and it survives the workspace being
//! deleted. So the command is `machine_write` and dispatch requires `--yes`,
//! the same gate `ds workstation install` passes through. It was
//! `local_file_write` once, which is not in the confirmation set, so the
//! `--yes` this command's own help asked for was decorative.

use std::io::Write as _;

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

pub const HOSTS: &[&str] = &["vscode", "claude-code", "codex", "cursor", "generic"];

pub static COMMAND: Command = Command {
    id: "mcp.install",
    path: &["mcp", "install"],
    contract: 2,
    chapter: ds_cli_contract::spec::Chapter::Catalog,
    summary: "Print or write the MCP host entry that launches this `ds`.",
    purpose: "\
Prints this executable's stdio server entry and its user-level host file. With \
`--write`, merges only the `ds` entry and preserves other servers, staging the \
merged document beside the target and renaming it into place. It never writes \
workspace configuration. Changing a user-level host file changes how agent \
sessions start on this machine, so every invocation needs `--yes`.",
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg {
            name: "host",
            kind: ArgKind::Value,
            value: "<vscode|claude-code|codex|cursor|generic>",
            required: false,
            default: Some("vscode"),
            choices: &["vscode", "claude-code", "codex", "cursor", "generic"],
            summary: "Which host's configuration shape and file to target.",
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
            summary: "Publish compact chapter routers or typed command tools.",
        },
        Arg {
            name: "profile",
            kind: ArgKind::Value,
            value: "<name>",
            required: false,
            default: None,
            choices: crate::surface::PROFILE_IDS,
            summary: "Install one typed operator-workflow profile.",
        },
    ],
    output: "\
The host, the server entry as JSON, the file it belongs in, and whether it \
was written.",
    examples: &[
        Example {
            command: "ds mcp install --output json --yes",
            note: "Print the VS Code entry and its file path; without `--write` nothing is written.",
            runnable: true,
        },
        Example {
            command: "ds mcp install --host vscode --write --yes",
            note: "Merge the `ds` server into the VS Code user profile's mcp.json.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::HOST_UNKNOWN,
        crate::CONFIG_UNWRITABLE,
        crate::PROFILE_EXPOSURE_INVALID,
        crate::CAPABILITIES_UNAVAILABLE,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that can change this machine's host configuration",
            remedy: "re-run with --yes; without --write the entry is only printed",
        },
    ],
    reference: Some("docs/reference/mcp.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let host = inputs.value("host").unwrap_or("vscode");
    if !HOSTS.contains(&host) {
        return Err(Failure::invalid("mcp_host_unknown", format!("no configuration recipe for host `{host}`"))
            .remedy("pass one of the hosts listed in `ds mcp install --help`, or omit --host to print the generic entry")
            .detail(json!({ "hosts": HOSTS })));
    }
    let executable = std::env::current_exe().map_err(|error| {
        Failure::failed("mcp_config_unwritable", format!("could not resolve this executable's path: {error}"))
            .remedy("read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand")
    })?;
    let build = crate::tools::build_identity(&executable)?;
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
    let entry = server_entry(host, &executable, exposure, profile);
    let file = config_file(host);
    let written = if inputs.switch("write") {
        let Some(path) = file.as_ref() else {
            return Err(Failure::invalid("mcp_host_unknown", format!("host `{host}` has no user-level file to write; copy the printed entry into your host's MCP settings"))
                .remedy("pass one of the hosts listed in `ds mcp install --help`, or omit --host to print the generic entry"));
        };
        merge_into_file(path, host, &entry)?;
        true
    } else {
        false
    };
    Ok(json!({
        "host": host,
        "server_name": "ds",
        "entry": entry,
        "file": file.map(|path| path.display().to_string()),
        "written": written,
        "executable": executable.display().to_string(),
        "source_sha": build["source_sha"],
        "dirty": build["dirty"],
        "exposure": exposure.token(),
        "profile": profile.map(crate::surface::Profile::token),
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
    out.push_str("entry:\n");
    out.push_str(&serde_json::to_string_pretty(&data["entry"]).unwrap_or_default());
    out.push('\n');
    out
}

/// The entry in the host's own dialect. Every host launches the same thing.
pub fn server_entry(
    host: &str,
    executable: &std::path::Path,
    exposure: crate::surface::Exposure,
    profile: Option<crate::surface::Profile>,
) -> Value {
    let command = executable.display().to_string();
    let mut args = vec!["mcp", "serve", "--exposure", exposure.token()];
    if let Some(profile) = profile {
        args.extend(["--profile", profile.token()]);
    }
    match host {
        // VS Code: `servers` map with an explicit `type`.
        "vscode" => {
            json!({ "servers": { "ds": { "type": "stdio", "command": command, "args": args } } })
        }
        // Claude Code, Cursor: `mcpServers` map.
        "claude-code" | "cursor" | "generic" => {
            json!({ "mcpServers": { "ds": { "command": command, "args": args } } })
        }
        // Codex: TOML on disk; the JSON here is the same data for the caller to translate.
        "codex" => {
            json!({ "mcp_servers": { "ds": { "command": command, "args": args } } })
        }
        _ => json!({ "mcpServers": { "ds": { "command": command, "args": args } } }),
    }
}

/// The user-level file for the host on this platform; `None` when the host
/// keeps no JSON file this command should touch (Codex uses TOML).
pub fn config_file(host: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    match host {
        "vscode" => Some(if cfg!(target_os = "windows") {
            PathBuf::from(std::env::var_os("APPDATA")?)
                .join("Code")
                .join("User")
                .join("mcp.json")
        } else if cfg!(target_os = "macos") {
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("mcp.json")
        } else {
            home.join(".config")
                .join("Code")
                .join("User")
                .join("mcp.json")
        }),
        "claude-code" => Some(home.join(".claude.json")),
        "cursor" => Some(home.join(".cursor").join("mcp.json")),
        _ => None,
    }
}

/// Merge `entry` (one top-level map with one server) into the JSON at `path`,
/// keeping every other key and server. A malformed file is refused, never
/// overwritten.
///
/// The replacement is atomic and reversible. The merged document is written
/// to a sibling temporary file, flushed, and renamed over the target, so a
/// host that reads the file concurrently sees either the old document or the
/// new one and never a half-written prefix. The previous contents are kept
/// beside it as `<file>.bak`, matching what `scripts/install-skills.sh`
/// already does for the skills bundle: this is somebody's editor
/// configuration, and it should be recoverable without git.
pub fn merge_into_file(path: &std::path::Path, host: &str, entry: &Value) -> Result<(), Failure> {
    let unwritable = |message: String| {
        Failure::failed("mcp_config_unwritable", message)
            .remedy("read the reported path; fix or remove a malformed file, then re-run, or copy the printed entry by hand")
            .detail(json!({ "file": path.display().to_string(), "host": host }))
    };
    let existing: Option<Vec<u8>> = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(unwritable(format!(
                "could not read {}: {error}",
                path.display()
            )));
        }
    };
    let document: Value = match existing.as_deref() {
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            unwritable(format!("could not create {}: {error}", parent.display()))
        })?;
    }
    let mut bytes =
        serde_json::to_vec_pretty(&document).map_err(|error| unwritable(error.to_string()))?;
    bytes.push(b'\n');

    // Same directory, so the rename below cannot cross a filesystem boundary
    // and degrade into a copy.
    let staged = staging_path(path);
    let stage = |message: String| {
        let _ = std::fs::remove_file(&staged);
        unwritable(message)
    };
    let mut handle = std::fs::File::create(&staged)
        .map_err(|error| unwritable(format!("could not create {}: {error}", staged.display())))?;
    handle
        .write_all(&bytes)
        .and_then(|()| handle.sync_all())
        .map_err(|error| stage(format!("could not write {}: {error}", staged.display())))?;
    drop(handle);

    if let Some(previous) = existing {
        let backup = backup_path(path);
        std::fs::write(&backup, previous)
            .map_err(|error| stage(format!("could not write {}: {error}", backup.display())))?;
    }
    std::fs::rename(&staged, path)
        .map_err(|error| stage(format!("could not replace {}: {error}", path.display())))
}

/// The sibling this process stages into. Named after the running process so
/// two `ds` invocations cannot stage over each other.
fn staging_path(path: &std::path::Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!(".{name}.ds-{}.tmp", std::process::id()))
}

fn backup_path(path: &std::path::Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || String::from("mcp.json"),
        |name| name.to_string_lossy().into_owned(),
    );
    path.with_file_name(format!("{name}.bak"))
}

/// Pure merge: the entry's single top-level map key is merged into the
/// document's map of the same name, replacing only the `ds` server.
pub fn merge(document: &Value, entry: &Value) -> Option<Value> {
    let mut root = document.as_object()?.clone();
    for (section, servers) in entry.as_object()? {
        let mut existing = root
            .get(section)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(Map::new);
        for (name, server) in servers.as_object()? {
            existing.insert(name.clone(), server.clone());
        }
        root.insert(section.clone(), Value::Object(existing));
    }
    Some(Value::Object(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_entry(host: &str, executable: &std::path::Path) -> Value {
        server_entry(host, executable, crate::surface::Exposure::Chapters, None)
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
        let entry = server_entry(
            "claude-code",
            std::path::Path::new("/usr/bin/ds"),
            crate::surface::Exposure::Commands,
            Some(crate::surface::Profile::Pls),
        );
        assert_eq!(
            entry["mcpServers"]["ds"]["args"],
            json!(["mcp", "serve", "--exposure", "commands", "--profile", "pls"])
        );
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
        assert!(!super::staging_path(&path).exists());

        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files survived: {leftovers:?}"
        );

        // The staged sibling shares the target's directory, so the rename can
        // never cross a filesystem boundary and silently become a copy.
        assert_eq!(super::staging_path(&path).parent(), path.parent());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_declared_effect_makes_the_documented_yes_real() {
        // F7: this command writes a *user-level host* file, not a workspace
        // file. `local_file_write` is not in the confirmation set, so the
        // `--write --yes` its own help and `docs/reference/mcp.md` ask for was
        // decorative. Reverting either half of this fails here.
        assert_eq!(COMMAND.effect, Effect::MachineWrite);
        assert!(COMMAND.effect.needs_confirmation());
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
