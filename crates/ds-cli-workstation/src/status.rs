use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

pub static COMMAND: Command = Command {
    id: "workstation.status",
    path: &["workstation", "status"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Detected prerequisite tools, versions, components, and shells.",
    purpose: "Reports local prerequisite discovery without installing, downloading, or changing settings. Shell meanings remain separate: PATH Bash, the active shell, VS Code and Windows Terminal defaults, and DS subprocess execution.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Platform, prerequisite states with paths and versions, governed reference-component receipt state, and separate shell/integration facts.",
    examples: &[Example {
        command: "ds workstation status --output json",
        note: "Safe first call; no package manager or settings file is changed.",
        runnable: true,
    }],
    refusals: &[],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let platform = Platform::current();
    let components = detect::catalog()
        .iter()
        .map(|component| detect::snapshot(component, platform, true))
        .collect::<Vec<_>>();
    Ok(json!({
        "platform": platform.token(),
        "mutated": false,
        "components": components,
        "shells": shell_snapshot(platform),
    }))
}

fn shell_snapshot(platform: Platform) -> Value {
    let bash_names = if platform == Platform::Windows {
        vec!["bash.exe".to_string()]
    } else {
        vec!["bash".to_string()]
    };
    let bash = detect::find_in_directories(
        &bash_names,
        &detect::path_directories(std::env::var_os("PATH")),
    );
    let vscode = if platform == Platform::Windows {
        windows_vscode_default()
    } else {
        json!({"state": "not_applicable", "value": null})
    };
    let terminal = if platform == Platform::Windows {
        windows_terminal_default()
    } else {
        json!({"state": "not_applicable", "value": null})
    };
    json!({
        "path_bash": bash.as_deref().map(|path| path.to_string_lossy().into_owned()),
        "git": executable_snapshot(&[if platform == Platform::Windows { "git.exe" } else { "git" }], "git"),
        "git_bash": detect::component("git-bash").map(|component| detect::snapshot(&component, platform, true)),
        "active": std::env::var("SHELL").ok().or_else(|| std::env::var("COMSPEC").ok()),
        "vscode_default_profile_windows": vscode,
        "windows_terminal_default_profile": terminal,
        "ds_subprocess": {
            "kind": "direct_process_execution",
            "shell": null,
            "note": "ds does not select a general-purpose subprocess shell",
        },
        "remote_ssh": "the remote platform's native shell remains independent of local Windows defaults",
    })
}

fn executable_snapshot(names: &[&str], component: &str) -> Value {
    let names = names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let path =
        detect::find_in_directories(&names, &detect::path_directories(std::env::var_os("PATH")));
    let version = path
        .as_deref()
        .and_then(|path| detect::version(path, component).ok());
    json!({
        "state": if path.is_some() { "installed" } else { "absent" },
        "path": path.as_deref().map(|path| path.to_string_lossy().into_owned()),
        "version": version,
    })
}

fn read_setting(path: Option<PathBuf>, key: &str) -> Value {
    let Some(path) = path else {
        return json!({"state": "unknown", "value": null, "path": null});
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => json!({
            "state": if crate::policy::jsonc_string(&text, key).is_some() { "configured" } else { "default" },
            "value": crate::policy::jsonc_string(&text, key),
            "path": path.to_string_lossy(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            json!({"state": "default", "value": null, "path": path.to_string_lossy()})
        }
        Err(error) => {
            json!({"state": "unreadable", "value": null, "path": path.to_string_lossy(), "reason": error.kind().to_string()})
        }
    }
}

fn windows_vscode_default() -> Value {
    let path = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("Code").join("User").join("settings.json"));
    read_setting(path, "terminal.integrated.defaultProfile.windows")
}

fn windows_terminal_default() -> Value {
    let path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .and_then(|root| {
            [
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
                "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe",
            ]
            .into_iter()
            .map(|package| {
                root.join("Packages")
                    .join(package)
                    .join("LocalState")
                    .join("settings.json")
            })
            .find(|path| path.is_file())
        });
    read_setting(path, "defaultProfile")
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "workstation {} · no changes\n",
        data["platform"].as_str().unwrap_or("?")
    );
    for component in data["components"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<17} {}",
            component["id"].as_str().unwrap_or("?"),
            component["state"].as_str().unwrap_or("unknown")
        ));
        if let Some(version) = component["version"].as_str() {
            out.push_str(&format!(" · {version}"));
        }
        out.push('\n');
    }
    out
}
